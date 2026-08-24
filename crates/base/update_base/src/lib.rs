use log_parser::{parse_latest, parse_logs, ErrorStats};
use progress::ProgressReporter;
use read_base_info::{get_zst_path, load_log};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use zip_writer::write_logs_to_zstd_with_progress;

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .to_string()
}

fn compute_file_hash(file_path: &str) -> std::io::Result<String> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        File::options()
            .read(true)
            .share_mode(0x00000001 | 0x00000002 | 0x00000004)
            .open(file_path)?
    };
    #[cfg(not(windows))]
    let file = File::open(file_path)?;

    use std::io::Read;
    let mut file = file;
    let mut file_bytes = Vec::new();
    file.read_to_end(&mut file_bytes)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug)]
pub struct UpdateResult {
    pub new_gz_files: usize,
    pub new_latest_files: usize,
    pub parsed_gz_lines: usize,
    pub parsed_latest_lines: usize,
    pub gz_errors: ErrorStats,
    pub latest_errors: ErrorStats,
    pub total_new_lines: usize,
    pub total_lines: usize,
    pub no_new_files: bool,
}

pub fn update_base<P: ProgressReporter + ?Sized>(
    name: &str,
    progress: &P,
) -> Result<UpdateResult, Box<dyn std::error::Error>> {
    let base_data = load_log(name)?;

    let paths_value = base_data
        .get("paths")
        .and_then(|p| p.as_array())
        .ok_or("Invalid base config: missing paths")?;

    let mut all_new_gz_logs: Vec<String> = Vec::new();
    let mut all_new_latest_logs: Vec<(String, usize)> = Vec::new();
    let mut updated_paths: Vec<Value> = Vec::new();

    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut ignored_snapshot: HashMap<String, Vec<String>> = HashMap::new();
    let mut latest_map: HashMap<String, Value> = HashMap::new();

    let mut known_gz_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut known_latest_files: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for path_entry in paths_value {
        for (_source_path, path_data) in path_entry
            .as_object()
            .ok_or("Invalid base config: path entry is not an object")?
        {
            for key in ["files", "ignored"] {
                if let Some(files) = path_data.get(key).and_then(|f| f.as_array()) {
                    for v in files {
                        if let Some(s) = v.as_str() {
                            known_gz_files.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }

    for path_entry in paths_value {
        for (source_path, path_data) in path_entry
            .as_object()
            .ok_or("Invalid base config: path entry is not an object")?
        {
            let latest_info = path_data.get("latest").cloned().unwrap_or(json!({}));
            let existing_files: Vec<String> = path_data
                .get("files")
                .and_then(|f| f.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let existing_ignored: Vec<String> = path_data
                .get("ignored")
                .and_then(|f| f.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let _current_gz_files =
                get_logfiles::find_log_files(source_path, true, ".log.gz", false);
            let current_gz_short =
                get_logfiles::find_log_files(source_path, false, ".log.gz", false);

            let mut new_gz_files: Vec<String> = Vec::new();
            for f in current_gz_short {
                if known_gz_files.insert(f.clone()) {
                    new_gz_files.push(f);
                }
            }

            let new_gz_full: Vec<String> = new_gz_files
                .iter()
                .map(|f| {
                    let mut p = PathBuf::from(source_path);
                    p.push(f);
                    p.to_string_lossy().to_string()
                })
                .collect();

            all_new_gz_logs.extend(new_gz_full.clone());

            let snapshot_entry = files_snapshot.entry(source_path.clone()).or_default();
            for f in &existing_files {
                if !snapshot_entry.contains(f) {
                    snapshot_entry.push(f.clone());
                }
            }
            snapshot_entry.extend(new_gz_files);

            let ignored_entry = ignored_snapshot.entry(source_path.clone()).or_default();
            for f in existing_ignored {
                if !ignored_entry.contains(&f) {
                    ignored_entry.push(f);
                }
            }

            let latest_files = get_logfiles::find_log_files(source_path, true, "latest.log", true);

            let mut latest_params = Vec::new();
            for latest_file in latest_files {
                if !known_latest_files.insert(latest_file.clone()) {
                    continue;
                }
                let stored_latest = latest_info
                    .get("hash")
                    .and_then(|_| Some(latest_info.clone()))
                    .unwrap_or_else(|| latest_info.get(&source_path).cloned().unwrap_or(json!({})));

                let stored_hash = stored_latest
                    .get("hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                let stored_last_line = stored_latest
                    .get("last_line")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0) as usize;

                let current_hash = match compute_file_hash(&latest_file) {
                    Ok(h) => h,
                    Err(_) => continue,
                };

                if current_hash != stored_hash {
                    latest_params.push((latest_file, stored_last_line));
                }
            }

            all_new_latest_logs.extend(latest_params.clone());

            // Start from the stored latest info so an untouched latest.log keeps
            // its hash in the rewritten config (otherwise it would be re-parsed
            // as "changed" on the NEXT update).
            latest_map.insert(source_path.clone(), latest_info);
        }
    }

    let new_gz_count = all_new_gz_logs.len();
    let new_latest_count = all_new_latest_logs.len();
    let total_new_files = new_gz_count + new_latest_count;

    if total_new_files == 0 {
        progress.finish();
        return Ok(UpdateResult {
            new_gz_files: 0,
            new_latest_files: 0,
            parsed_gz_lines: 0,
            parsed_latest_lines: 0,
            gz_errors: ErrorStats::default(),
            latest_errors: ErrorStats::default(),
            total_new_lines: 0,
            total_lines: 0,
            no_new_files: true,
        });
    }

    progress.set_total(all_new_gz_logs.len() as u64);
    let (new_logs, gz_errors) = match parse_logs(all_new_gz_logs, progress) {
        Ok(v) => v,
        Err(e) => {
            progress.finish();
            return Err(Box::new(e));
        }
    };
    progress.finish();

    progress.set_total(all_new_latest_logs.len() as u64);
    let (logs_latest, latest_params, latest_errors) =
        match parse_latest(all_new_latest_logs.clone(), progress) {
            Ok(v) => v,
            Err(e) => {
                progress.finish();
                return Err(Box::new(e));
            }
        };
    progress.finish();

    let parsed_gz_lines = new_logs.len();
    let parsed_latest_lines = logs_latest.len();

    for (path, last_line, hash) in latest_params {
        let dir = get_dir(&path);
        if let Some(latest) = latest_map.get_mut(&dir) {
            *latest = json!({
                "hash": hash,
                "last_line": last_line
            });
        }
    }

    let mut written_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path_entry in paths_value {
        for (source_path, _path_data) in path_entry
            .as_object()
            .ok_or("Invalid base config: path entry is not an object")?
        {
            if !written_paths.insert(source_path.clone()) {
                continue;
            }

            let files = files_snapshot.get(source_path).cloned().unwrap_or_default();
            let ignored = ignored_snapshot
                .get(source_path)
                .cloned()
                .unwrap_or_default();
            let latest = latest_map.get(source_path).cloned().unwrap_or(json!({}));

            updated_paths.push(json!({
                source_path: {
                    "latest": latest,
                    "files": files,
                    "ignored": ignored
                }
            }));
        }
    }

    let zst_path = get_zst_path(name)?;
    let existing_logs = read_existing_logs(&zst_path)?;

    let existing_count = existing_logs.len();
    let mut all_logs = existing_logs;
    all_logs.extend(new_logs);
    all_logs.extend(logs_latest);
    all_logs.sort_unstable_by_key(|x| x.0);

    let new_lines_count = all_logs.len().saturating_sub(existing_count);
    let total_lines = all_logs.len();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let first_log_time = all_logs
        .first()
        .map(|(ts, _)| ts.to_string())
        .unwrap_or_default();
    let last_log_time = all_logs
        .last()
        .map(|(ts, _)| ts.to_string())
        .unwrap_or_default();

    let old_time = base_data.get("time").cloned().unwrap_or(json!({
        "creation_unix": now.clone(),
        "edit_unix": "",
        "FLD_unix": "",
        "LLD_unix": ""
    }));

    let updated_json = json!({
        "name": base_data.get("name").unwrap_or(&json!(name)),
        "lines": total_lines,
        "hash": base_data.get("hash").unwrap_or(&json!("")),
        "tags": base_data.get("tags").unwrap_or(&json!([])),
        "custom": base_data.get("custom").unwrap_or(&json!("custom")),
        "time": {
            "creation_unix": old_time.get("creation_unix").and_then(|v| v.as_str()).unwrap_or(&now),
            "edit_unix": now,
            "FLD_unix": first_log_time,
            "LLD_unix": last_log_time
        },
        "paths": updated_paths
    });

    let appdata = env::var("APPDATA")?;
    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools");
    base_path.push("bases");
    base_path.push(name);

    let file_path = base_path.join(format!("{}.json", name));
    fs::write(&file_path, serde_json::to_string_pretty(&updated_json)?)?;

    progress.set_total(total_lines as u64);
    progress.set_message("Writing logs");
    write_logs_to_zstd_with_progress(&all_logs, &zst_path, progress)?;
    progress.finish();

    Ok(UpdateResult {
        new_gz_files: new_gz_count,
        new_latest_files: new_latest_count,
        parsed_gz_lines,
        parsed_latest_lines,
        gz_errors,
        latest_errors,
        total_new_lines: new_lines_count,
        total_lines,
        no_new_files: false,
    })
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
