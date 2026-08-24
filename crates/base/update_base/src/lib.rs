use console::style;
use log_parser::{parse_latest_with_progress, parse_logs_with_progress, ErrorStats};
use progress::{ProgressBarWrapper, ProgressReporter, WriteProgress};
use read_base_info::{get_zst_path, load_log};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zip_writer::write_logs_to_zstd_with_progress;

fn format_error_stats(stats: &ErrorStats) -> String {
    match (stats.file_errors, stats.parse_errors) {
        (0, 0) => String::new(),
        (f, 0) => format!("({} file errors)", f),
        (0, p) => format!("({} parse errors)", p),
        (f, p) => format!("({} file errors, {} parse errors)", f, p),
    }
}

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .to_string()
}

fn compute_file_hash(file_path: &str) -> std::io::Result<String> {
    let file_bytes = fs::read(file_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn update_base(name: &str, progress: &ProgressBarWrapper) -> Result<(), Box<dyn std::error::Error>> {
    let base_data = load_log(name)?;
    
    let paths_value = base_data.get("paths")
        .and_then(|p| p.as_array())
        .ok_or("Invalid base config: missing paths")?;

    let mut all_new_gz_logs: Vec<String> = Vec::new();
    let mut all_new_latest_logs: Vec<(String, usize)> = Vec::new();
    let mut updated_paths: Vec<Value> = Vec::new();
    
    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut latest_map: HashMap<String, Value> = HashMap::new();

    for path_entry in paths_value {
        for (source_path, path_data) in path_entry.as_object().unwrap() {
            let latest_info = path_data.get("latest").cloned().unwrap_or(json!({}));
            let existing_files: Vec<String> = path_data
                .get("files")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let _current_gz_files = get_logfiles::find_log_files(source_path, true, ".log.gz", false);
            let current_gz_short = get_logfiles::find_log_files(source_path, false, ".log.gz", false);
            
            let existing_set: std::collections::HashSet<_> = existing_files.iter().cloned().collect();
            let new_gz_files: Vec<String> = current_gz_short
                .iter()
                .filter(|f| !existing_set.contains(*f))
                .cloned()
                .collect();

            let new_gz_full: Vec<String> = new_gz_files
                .iter()
                .map(|f| {
                    let mut p = PathBuf::from(source_path);
                    p.push(f);
                    p.to_string_lossy().to_string()
                })
                .collect();

            all_new_gz_logs.extend(new_gz_full.clone());
            files_snapshot.insert(source_path.clone(), current_gz_short);

            let latest_files = get_logfiles::find_log_files(source_path, true, "latest.log", true);
            
            let mut latest_params = Vec::new();
            for latest_file in latest_files {
                let dir = get_dir(&latest_file);
                let stored_latest = latest_info.get(&dir).cloned().unwrap_or(json!({}));
                
                let stored_hash = stored_latest.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                let stored_last_line = stored_latest.get("last_line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;

                let current_hash = compute_file_hash(&latest_file).unwrap_or_default();
                
                if current_hash != stored_hash {
                    latest_params.push((latest_file, stored_last_line));
                }
            }

            all_new_latest_logs.extend(latest_params.clone());

            let mut updated_latest = latest_info;
            for (latest_file, _) in &latest_params {
                let dir = get_dir(latest_file);
                let current_hash = compute_file_hash(latest_file).unwrap_or_default();
                updated_latest[dir] = json!({
                    "hash": current_hash,
                    "last_line": 0
                });
            }
            latest_map.insert(source_path.clone(), updated_latest);
        }
    }

    if all_new_gz_logs.is_empty() && all_new_latest_logs.is_empty() {
        println!("   {} No new logs to parse", style("Update").green().bold());
        return Ok(());
    }

    progress.set_total(all_new_gz_logs.len() as u64);
    let (new_logs, gz_errors) = parse_logs_with_progress(all_new_gz_logs, progress).unwrap();
    progress.finish_and_clear();

    println!(
        "   {} {} new gz files -> {} lines {}",
        style("Parsing").green().bold(),
        new_logs.len(),
        new_logs.len(),
        format_error_stats(&gz_errors)
    );

    progress.set_total(all_new_latest_logs.len() as u64);
    let (logs_latest, latest_params, latest_errors) = 
        parse_latest_with_progress(all_new_latest_logs.clone(), progress).unwrap();
    progress.finish_and_clear();

    println!(
        "   {} {} latest files -> {} lines {}",
        style("Parsing latest").green().bold(),
        all_new_latest_logs.len(),
        logs_latest.len(),
        format_error_stats(&latest_errors)
    );

    for (path, last_line, hash) in latest_params {
        let dir = get_dir(&path);
        if let Some(latest) = latest_map.get_mut(&dir) {
            latest[dir] = json!({
                "hash": hash,
                "last_line": last_line
            });
        }
    }

    for path_entry in paths_value {
        for (source_path, _path_data) in path_entry.as_object().unwrap() {
            let files = files_snapshot.get(source_path).cloned().unwrap_or_default();
            let latest = latest_map.get(source_path).cloned().unwrap_or(json!({}));

            updated_paths.push(json!({
                source_path: {
                    "latest": latest,
                    "files": files
                }
            }));
        }
    }

    let zst_path = get_zst_path(name)?;
    let existing_logs = read_existing_logs(&zst_path)?;
    
    let mut all_logs = existing_logs;
    all_logs.extend(new_logs);
    all_logs.extend(logs_latest);
    all_logs.sort_unstable_by_key(|x| x.0);
    all_logs.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let total_lines = all_logs.len();

    let updated_json = json!({
        "name": base_data.get("name").unwrap_or(&json!(name)),
        "lines": total_lines,
        "hash": base_data.get("hash").unwrap_or(&json!("")),
        "tags": base_data.get("tags").unwrap_or(&json!([])),
        "custom": base_data.get("custom").unwrap_or(&json!("custom")),
        "time": base_data.get("time").unwrap_or(&json!({
            "creation_unix": "",
            "edit_unix": "",
            "FLD_unix": "",
            "LLD_unix": ""
        })),
        "paths": updated_paths
    });

    let appdata = env::var("APPDATA").expect("No APPDATA env variable");
    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools");
    base_path.push("bases");
    base_path.push(name);

    let file_path = base_path.join(format!("{}.json", name));
    fs::write(&file_path, serde_json::to_string_pretty(&updated_json)?)?;

    progress.set_total(total_lines as u64);
    let write_progress = Arc::new(WriteProgress::new());
    write_logs_to_zstd_with_progress(&all_logs, &zst_path, &write_progress)?;
    progress.finish_and_clear();

    println!(
        "   {} {} total lines -> {}",
        style("Writing").green().bold(),
        total_lines,
        name
    );

    Ok(())
}

pub fn update_base_with_progress(name: &str, progress: &ProgressBarWrapper) -> (usize, usize) {
    let base_data = match load_log(name) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error loading base: {}", e);
            return (0, 0);
        }
    };
    let paths_value = match base_data.get("paths").and_then(|p| p.as_array()) {
        Some(v) => v,
        None => {
            eprintln!("Invalid base config: missing paths");
            return (0, 0);
        }
    };

    let mut all_new_gz_logs: Vec<String> = Vec::new();
    let mut all_new_latest_logs: Vec<(String, usize)> = Vec::new();
    let mut updated_paths: Vec<Value> = Vec::new();
    
    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut latest_map: HashMap<String, Value> = HashMap::new();

    for path_entry in paths_value {
        for (source_path, path_data) in path_entry.as_object().unwrap() {
            let latest_info = path_data.get("latest").cloned().unwrap_or(json!({}));
            let existing_files: Vec<String> = path_data
                .get("files")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let _current_gz_files = get_logfiles::find_log_files(source_path, true, ".log.gz", false);
            let current_gz_short = get_logfiles::find_log_files(source_path, false, ".log.gz", false);
            
            let existing_set: std::collections::HashSet<_> = existing_files.iter().cloned().collect();
            let new_gz_files: Vec<String> = current_gz_short
                .iter()
                .filter(|f| !existing_set.contains(*f))
                .cloned()
                .collect();

            let new_gz_full: Vec<String> = new_gz_files
                .iter()
                .map(|f| {
                    let mut p = PathBuf::from(source_path);
                    p.push(f);
                    p.to_string_lossy().to_string()
                })
                .collect();

            all_new_gz_logs.extend(new_gz_full.clone());
            files_snapshot.insert(source_path.clone(), current_gz_short);

            let latest_files = get_logfiles::find_log_files(source_path, true, "latest.log", true);
            
            let mut latest_params = Vec::new();
            for latest_file in latest_files {
                let dir = get_dir(&latest_file);
                let stored_latest = latest_info.get(&dir).cloned().unwrap_or(json!({}));
                
                let stored_hash = stored_latest.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                let stored_last_line = stored_latest.get("last_line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;

                let current_hash = match compute_file_hash(&latest_file) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                
                if current_hash != stored_hash {
                    latest_params.push((latest_file, stored_last_line));
                }
            }

            all_new_latest_logs.extend(latest_params.clone());

            let mut updated_latest = latest_info;
            for (latest_file, _) in &latest_params {
                let dir = get_dir(latest_file);
                let current_hash = match compute_file_hash(latest_file) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                updated_latest[dir] = json!({
                    "hash": current_hash,
                    "last_line": 0
                });
            }
            latest_map.insert(source_path.clone(), updated_latest);
        }
    }

    let new_gz_count = all_new_gz_logs.len();
    let new_latest_count = all_new_latest_logs.len();
    let total_new_files = new_gz_count + new_latest_count;

    if total_new_files == 0 {
        println!("   {} No new logs to parse", style("Update").green().bold());
        return (0, 0);
    }

    progress.set_total(all_new_gz_logs.len() as u64);
    let (new_logs, gz_errors) = match parse_logs_with_progress(all_new_gz_logs, progress) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error parsing gz logs: {}", e);
            return (0, 0);
        }
    };
    progress.finish_and_clear();

    println!(
        "   {} {} new gz files -> {} lines {}",
        style("Parsing").green().bold(),
        new_gz_count,
        new_logs.len(),
        format_error_stats(&gz_errors)
    );

    progress.set_total(all_new_latest_logs.len() as u64);
    let (logs_latest, latest_params, latest_errors) = match parse_latest_with_progress(all_new_latest_logs.clone(), progress) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error parsing latest logs: {}", e);
            return (0, 0);
        }
    };
    progress.finish_and_clear();

    println!(
        "   {} {} latest files -> {} lines {}",
        style("Parsing latest").green().bold(),
        new_latest_count,
        logs_latest.len(),
        format_error_stats(&latest_errors)
    );

    for (path, last_line, hash) in latest_params {
        let dir = get_dir(&path);
        if let Some(latest) = latest_map.get_mut(&dir) {
            latest[dir] = json!({
                "hash": hash,
                "last_line": last_line
            });
        }
    }

    for path_entry in paths_value {
        for (source_path, _path_data) in path_entry.as_object().unwrap() {
            let files = files_snapshot.get(source_path).cloned().unwrap_or_default();
            let latest = latest_map.get(source_path).cloned().unwrap_or(json!({}));

            updated_paths.push(json!({
                source_path: {
                    "latest": latest,
                    "files": files
                }
            }));
        }
    }

    let zst_path = match get_zst_path(name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error getting zst path: {}", e);
            return (0, 0);
        }
    };
    
    let existing_logs = match read_existing_logs(&zst_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error reading existing logs: {}", e);
            return (0, 0);
        }
    };
    
    let existing_count = existing_logs.len();
    let mut all_logs = existing_logs;
    all_logs.extend(new_logs);
    all_logs.extend(logs_latest);
    all_logs.sort_unstable_by_key(|x| x.0);
    all_logs.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let new_lines_count = all_logs.len().saturating_sub(existing_count);
    let total_lines = all_logs.len();

    let updated_json = json!({
        "name": base_data.get("name").unwrap_or(&json!(name)),
        "lines": total_lines,
        "hash": base_data.get("hash").unwrap_or(&json!("")),
        "tags": base_data.get("tags").unwrap_or(&json!([])),
        "custom": base_data.get("custom").unwrap_or(&json!("custom")),
        "time": base_data.get("time").unwrap_or(&json!({
            "creation_unix": "",
            "edit_unix": "",
            "FLD_unix": "",
            "LLD_unix": ""
        })),
        "paths": updated_paths
    });

    let appdata = match env::var("APPDATA") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("No APPDATA env variable");
            return (0, 0);
        }
    };
    
    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools");
    base_path.push("bases");
    base_path.push(name);

    let file_path = base_path.join(format!("{}.json", name));
    if let Err(e) = fs::write(&file_path, serde_json::to_string_pretty(&updated_json).unwrap()) {
        eprintln!("Error writing config: {}", e);
        return (0, 0);
    }

    progress.set_total(total_lines as u64);
    let write_progress = Arc::new(WriteProgress::new());
    if let Err(e) = write_logs_to_zstd_with_progress(&all_logs, &zst_path, &write_progress) {
        eprintln!("Error writing logs: {}", e);
        return (0, 0);
    }
    progress.finish_and_clear();

    println!(
        "   {} {} total lines -> {}",
        style("Writing").green().bold(),
        total_lines,
        name
    );

    (total_new_files, new_lines_count)
}

fn read_existing_logs(zst_path: &str) -> std::io::Result<Vec<(i64, String)>> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use zstd::stream::read::Decoder;

    let file = File::open(zst_path)?;
    let decoder = Decoder::new(file)?;
    let reader = BufReader::new(decoder);

    let mut logs = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.len() >= 13 {
            let ts_str = &line[..12];
            if let Ok(ts) = ts_str.parse::<i64>() {
                let msg = line[13..].to_string();
                logs.push((ts, msg));
            }
        }
    }
    Ok(logs)
}