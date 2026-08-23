use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use log_parser;
use progress::{ProgressBarWrapper, ProgressReporter, WriteProgress};
use read_base_info;

pub fn update_base(name: &str) -> (Arc<WriteProgress>, thread::JoinHandle<Vec<(u64, String)>>) {
    let config = read_base_info::load_log(name).unwrap();
    let zst_path = read_base_info::get_zst_path(name).unwrap();

    let paths_value = config.get("paths").unwrap();
    let paths_array = paths_value.as_array().unwrap();

    let mut all_logs: Vec<String> = Vec::new();
    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();
    let mut paths_json: Vec<Value> = Vec::new();

    for path_obj in paths_array {
        for (path_str, path_value) in path_obj.as_object().unwrap() {
            let path_logs = get_logfiles::find_log_files(path_str, true, ".log.gz", false);
            let path_logs_short = get_logfiles::find_log_files(path_str, false, ".log.gz", false);

            files_snapshot.insert(path_str.clone(), path_logs_short);

            all_logs.extend(path_logs);

            let latest_files = get_logfiles::find_log_files(path_str, true, "latest.log", true);

            for latest_path in &latest_files {
                let latest_dir = Path::new(latest_path)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                let old_hash = config
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.iter().find(|p| p.get(path_str).is_some()))
                    .and_then(|p| p.get(path_str))
                    .and_then(|pv| pv.get("latest"))
                    .and_then(|l| l.get(&latest_dir))
                    .and_then(|ld| ld.get("hash"))
                    .and_then(|h| h.as_str());

                let old_last_line = config
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.iter().find(|p| p.get(path_str).is_some()))
                    .and_then(|p| p.get(path_str))
                    .and_then(|pv| pv.get("latest"))
                    .and_then(|l| l.get(&latest_dir))
                    .and_then(|ld| ld.get("last_line"))
                    .and_then(|l| l.as_u64());

                let start_line = old_last_line.unwrap_or(0) as usize;

                if let Ok((_, stats, total_lines, hash)) =
                    log_parser::process_file_plain(latest_path, start_line)
                {
                    if old_hash != Some(hash.as_str())
                        || stats.parse_errors > 0
                        || total_lines > start_line
                    {
                        latest_params.push((latest_path.clone(), start_line));
                    }
                }
            }

            let mut latest_map: HashMap<String, Value> = HashMap::new();
            for latest_path in &latest_files {
                let latest_dir = Path::new(latest_path)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                if let Ok((_, _, total_lines, hash)) =
                    log_parser::process_file_plain(latest_path, 0)
                {
                    latest_map.insert(
                        latest_dir,
                        json!({
                            "hash": hash,
                            "last_line": total_lines
                        }),
                    );
                }
            }

            let files = files_snapshot.get(path_str).cloned().unwrap_or_default();

            let mut path_entry = Value::Object(serde_json::Map::new());
            path_entry.as_object_mut().unwrap().insert(
                path_str.clone(),
                json!({
                    "latest": latest_map,
                    "files": files
                }),
            );

            paths_json.push(path_entry);
        }
    }

    // =========================
    // PROGRESS BAR (logs)
    // =========================
    let progress = Arc::new(WriteProgress::new());
    let progress_clone = Arc::clone(&progress);

    let all_logs_len = all_logs.len();
    progress_clone.set_total(all_logs_len as u64);

    let (logs, logs_errors) = match log_parser::parse_logs(all_logs, &progress_clone) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error parsing logs: {}", e);
            (Vec::new(), log_parser::ErrorStats::default())
        }
    };

    println!(
        "   Parsing {} files -> {} lines ({})",
        all_logs_len,
        logs.len(),
        format_error_stats(&logs_errors)
    );

    // =========================
    // PROGRESS BAR (latest logs)
    // =========================
    let progress_latest = Arc::clone(&progress);
    let latest_params_len = latest_params.len();
    progress_latest.set_total(latest_params_len as u64);

    let (logs_latest, _, latest_errors) =
        match log_parser::parse_latest(latest_params, &progress_latest) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("Error parsing latest logs: {}", e);
                (Vec::new(), Vec::new(), log_parser::ErrorStats::default())
            }
        };

    println!(
        "   Parsing latest {} files -> {} lines ({})",
        latest_params_len,
        logs_latest.len(),
        format_error_stats(&latest_errors)
    );

    // =========================
    // FINAL MERGE
    // =========================
    let mut all_logs_result = logs;
    all_logs_result.extend(logs_latest);
    all_logs_result.sort_unstable_by_key(|x| x.0);

    let lines = all_logs_result.len();

    // =========================
    // BUILD JSON
    // =========================
    let json_value = json!({
        "name": name,
        "lines": lines,
        "hash": "",
        "tags": [],
        "custom": "custom",
        "time": {
            "creation_unix": "",
            "edit_unix": "",
            "FLD_unix": "",
            "LLD_unix": ""
        },
        "paths": paths_json
    });

    // =========================
    // WRITE LT3 BASE CONF
    // =========================
    let appdata = std::env::var("APPDATA").expect("No APPDATA env variable");

    let mut base_path = Path::new(&appdata)
        .join("LogTools")
        .join("bases")
        .join(name);

    fs::create_dir_all(&base_path).unwrap();

    let file_path = base_path.join(format!("{}.json", name));
    fs::write(
        file_path,
        serde_json::to_string_pretty(&json_value).unwrap(),
    )
    .unwrap();

    // =========================
    // WRITE LOGS
    // =========================
    let progress_write = Arc::clone(&progress);
    progress_write.set_total(lines as u64);

    let _ = log_parser::write_logs_to_zstd(&all_logs_result, &zst_path, &progress_write);

    println!("   Writing {} lines -> {}", lines, name);

    let handle = thread::spawn(|| {
        all_logs_result
            .into_iter()
            .map(|(ts, msg)| (ts as u64, msg))
            .collect()
    });

    (progress, handle)
}

pub fn update_base_with_progress(name: &str, _progress: &ProgressBarWrapper) -> (u64, u64) {
    let config = read_base_info::load_log(name).unwrap();
    let zst_path = read_base_info::get_zst_path(name).unwrap();

    let paths_value = config.get("paths").unwrap();
    let paths_array = paths_value.as_array().unwrap();

    let mut all_logs: Vec<String> = Vec::new();
    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();
    let mut paths_json: Vec<Value> = Vec::new();
    let mut total_new_lines: u64 = 0;

    for path_obj in paths_array {
        for (path_str, _path_value) in path_obj.as_object().unwrap() {
            let path_logs = get_logfiles::find_log_files(path_str, true, ".log.gz", false);
            let path_logs_short = get_logfiles::find_log_files(path_str, false, ".log.gz", false);

            files_snapshot.insert(path_str.clone(), path_logs_short);

            all_logs.extend(path_logs);

            let latest_files = get_logfiles::find_log_files(path_str, true, "latest.log", true);

            for latest_path in &latest_files {
                let latest_dir = Path::new(latest_path)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                let old_hash = config
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.iter().find(|p| p.get(path_str).is_some()))
                    .and_then(|p| p.get(path_str))
                    .and_then(|pv| pv.get("latest"))
                    .and_then(|l| l.get(&latest_dir))
                    .and_then(|ld| ld.get("hash"))
                    .and_then(|h| h.as_str());

                let old_last_line = config
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.iter().find(|p| p.get(path_str).is_some()))
                    .and_then(|p| p.get(path_str))
                    .and_then(|pv| pv.get("latest"))
                    .and_then(|l| l.get(&latest_dir))
                    .and_then(|ld| ld.get("last_line"))
                    .and_then(|l| l.as_u64());

                let start_line = old_last_line.unwrap_or(0) as usize;

                if let Ok((_, stats, total_lines, hash)) =
                    log_parser::process_file_plain(latest_path, start_line)
                {
                    if old_hash != Some(hash.as_str())
                        || stats.parse_errors > 0
                        || total_lines > start_line
                    {
                        latest_params.push((latest_path.clone(), start_line));
                    }
                    if total_lines > start_line {
                        total_new_lines += (total_lines - start_line) as u64;
                    }
                }
            }

            let mut latest_map: HashMap<String, Value> = HashMap::new();
            for latest_path in &latest_files {
                let latest_dir = Path::new(latest_path)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                if let Ok((_, _, total_lines, hash)) =
                    log_parser::process_file_plain(latest_path, 0)
                {
                    latest_map.insert(
                        latest_dir,
                        json!({
                            "hash": hash,
                            "last_line": total_lines
                        }),
                    );
                }
            }

            let files = files_snapshot.get(path_str).cloned().unwrap_or_default();

            let mut path_entry = Value::Object(serde_json::Map::new());
            path_entry.as_object_mut().unwrap().insert(
                path_str.clone(),
                json!({
                    "latest": latest_map,
                    "files": files
                }),
            );

            paths_json.push(path_entry);
        }
    }

    // =========================
    // PROGRESS BAR (logs)
    // =========================
    let all_logs_len = all_logs.len();

    let (logs, logs_errors) = match log_parser::parse_logs(all_logs, _progress) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error parsing logs: {}", e);
            (Vec::new(), log_parser::ErrorStats::default())
        }
    };

    total_new_lines += logs.len() as u64;

    println!(
        "   Parsing {} files -> {} lines ({})",
        all_logs_len,
        logs.len(),
        format_error_stats(&logs_errors)
    );

    // =========================
    // PROGRESS BAR (latest logs)
    // =========================
    let latest_params_len = latest_params.len();

    let (logs_latest, _, latest_errors) = match log_parser::parse_latest(latest_params, _progress) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error parsing latest logs: {}", e);
            (Vec::new(), Vec::new(), log_parser::ErrorStats::default())
        }
    };

    total_new_lines += logs_latest.len() as u64;

    println!(
        "   Parsing latest {} files -> {} lines ({})",
        latest_params_len,
        logs_latest.len(),
        format_error_stats(&latest_errors)
    );

    // =========================
    // FINAL MERGE
    // =========================
    let mut all_logs_result = logs;
    all_logs_result.extend(logs_latest);
    all_logs_result.sort_unstable_by_key(|x| x.0);

    let lines = all_logs_result.len();

    // =========================
    // BUILD JSON
    // =========================
    let json_value = json!({
        "name": name,
        "lines": lines,
        "hash": "",
        "tags": [],
        "custom": "custom",
        "time": {
            "creation_unix": "",
            "edit_unix": "",
            "FLD_unix": "",
            "LLD_unix": ""
        },
        "paths": paths_json
    });

    // =========================
    // WRITE LT3 BASE CONF
    // =========================
    let appdata = std::env::var("APPDATA").expect("No APPDATA env variable");

    let base_path = Path::new(&appdata)
        .join("LogTools")
        .join("bases")
        .join(name);

    fs::create_dir_all(&base_path).unwrap();

    let file_path = base_path.join(format!("{}.json", name));
    fs::write(
        file_path,
        serde_json::to_string_pretty(&json_value).unwrap(),
    )
    .unwrap();

    // =========================
    // WRITE LOGS
    // =========================
    let _ = log_parser::write_logs_to_zstd(&all_logs_result, &zst_path, _progress);

    println!("   Writing {} lines -> {}", lines, name);

    (all_logs_len as u64, total_new_lines)
}

fn format_error_stats(stats: &log_parser::ErrorStats) -> String {
    match (stats.file_errors, stats.parse_errors) {
        (0, 0) => String::new(),
        (f, 0) => format!("({} file errors)", f),
        (0, p) => format!("({} parse errors)", p),
        (f, p) => format!("({} file errors, {} parse errors)", f, p),
    }
}
