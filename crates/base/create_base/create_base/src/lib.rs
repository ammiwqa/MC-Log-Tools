use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use log_parser::ErrorStats;
use progress::ProgressReporter;

#[derive(Debug)]
pub struct CreateResult {
    pub gz_files: usize,
    pub parsed_gz_lines: usize,
    pub latest_files: usize,
    pub parsed_latest_lines: usize,
    pub gz_errors: ErrorStats,
    pub latest_errors: ErrorStats,
    pub total_lines: usize,
    pub duplicate_files: usize,
}

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .to_string()
}

fn file_name_of(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

pub fn create_base<P: ProgressReporter + ?Sized>(
    paths: Vec<String>,
    name: String,
    progress: &P,
) -> Result<CreateResult, Box<dyn std::error::Error>> {
    let mut all_logs: Vec<String> = Vec::new();
    let mut paths_json: Vec<serde_json::Value> = Vec::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();

    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();

    let mut seen_gz_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_latest_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut duplicate_files = 0usize;

    for path in &paths {
        let np = get_logfiles::find_log_files(path, true, ".log.gz", false);
        let np_short_list = get_logfiles::find_log_files(path, false, ".log.gz", false);

        let mut unique_short_list: Vec<String> = Vec::with_capacity(np_short_list.len());
        for short in np_short_list {
            if seen_gz_files.contains(&short) {
                continue;
            }
            seen_gz_files.insert(short.clone());
            unique_short_list.push(short);
        }

        duplicate_files += np.len().saturating_sub(unique_short_list.len());

        let unique_short_set: std::collections::HashSet<_> =
            unique_short_list.iter().cloned().collect();
        let unique_full: Vec<String> = np
            .into_iter()
            .filter(|full| unique_short_set.contains(&file_name_of(full)))
            .collect();

        let latest_files: Vec<String> =
            get_logfiles::find_log_files(path, true, "latest.log", true)
                .into_iter()
                .filter(|full| {
                    if !seen_latest_files.insert(full.clone()) {
                        duplicate_files += 1;
                        return false;
                    }
                    true
                })
                .collect();

        files_snapshot.insert(path.clone(), unique_short_list);

        latest_params.extend(latest_files.into_iter().map(|f| (f, 0)));

        all_logs.extend(unique_full);
    }

    let all_logs_len = all_logs.len();

    // =========================
    // PARSE GZ LOGS
    // =========================
    progress.set_total(all_logs_len as u64);

    let (mut logs, errors) = log_parser::parse_logs(all_logs, progress)?;

    progress.finish();

    // =========================
    // PARSE LATEST LOGS
    // =========================
    let latest_params_len = latest_params.len();

    progress.set_total(latest_params_len as u64);

    let (logs_latest, params, latest_errors) = log_parser::parse_latest(latest_params, progress)?;

    progress.finish();

    // =========================
    // LATEST JSON GEN
    // =========================
    let mut latest_map: HashMap<String, serde_json::Value> = HashMap::new();

    for (path, lines, hash) in params {
        let dir = get_dir(&path);

        latest_map.insert(
            dir,
            json!({
                "hash": hash,
                "last_line": lines
            }),
        );
    }

    // =========================
    // BUILD JSON
    // =========================
    for path in &paths {
        let latest = latest_map.get(path).cloned().unwrap_or(json!({}));

        let files = files_snapshot.get(path).cloned().unwrap_or_default();

        paths_json.push(json!({
            path: {
                "latest": latest,
                "files": files
            }
        }));
    }

    // =========================
    // FINAL MERGE
    // =========================
    let parsed_gz_lines = logs.len();
    let parsed_latest_lines = logs_latest.len();

    logs.extend(logs_latest);
    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();

    let first_log_time = logs.first().map(|(ts, _)| ts.to_string()).unwrap_or_default();
    let last_log_time = logs.last().map(|(ts, _)| ts.to_string()).unwrap_or_default();

    // =========================
    // BASE DIR
    // =========================
    let appdata = env::var("APPDATA")?;

    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools");
    base_path.push("bases");
    base_path.push(&name);

    fs::create_dir_all(&base_path)?;

    // =========================
    // WRITE LOGS
    // =========================
    progress.set_total(lines as u64);
    progress.set_message("Writing logs");

    let zst_path = base_path
        .join(format!("{}.zst", &name))
        .display()
        .to_string();
    zip_writer::write_logs_to_zstd_with_progress(&logs, &zst_path, progress)?;

    progress.finish();

    // =========================
    // WRITE LT3 BASE CONF
    // =========================
    let json_value = json!({
        "name": name,
        "lines": lines,
        "hash": "",

        "tags": [],
        "custom": "custom",

        "time": {
            "creation_unix": now,
            "edit_unix": now,
            "FLD_unix": first_log_time,
            "LLD_unix": last_log_time
        },

        "paths": paths_json
    });

    let file_path = base_path.join(format!("{}.json", &name));
    fs::write(&file_path, serde_json::to_string_pretty(&json_value)?)?;

    Ok(CreateResult {
        gz_files: all_logs_len,
        parsed_gz_lines,
        latest_files: latest_params_len,
        parsed_latest_lines,
        gz_errors: errors,
        latest_errors,
        total_lines: lines,
        duplicate_files,
    })
}
