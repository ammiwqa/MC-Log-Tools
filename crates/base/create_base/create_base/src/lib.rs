use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log_parser::ErrorStats;
use progress::ProgressReporter;
use read_base_info::{LatestInfo, PathEntry, TimeInfo, save_log};

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
    let mut latest_params: Vec<(String, usize)> = Vec::new();

    let mut entries_by_path: HashMap<String, PathEntry> = HashMap::new();

    let mut seen_gz_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_latest_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut duplicate_files = 0usize;

    for path in &paths {
        let np = get_logfiles::find_log_files(path, true, ".log.gz", false);
        let np_short_list = get_logfiles::find_log_files(path, false, ".log.gz", false);

        let entry = entries_by_path.entry(path.clone()).or_default();
        entry.path = path.clone();

        for short in np_short_list {
            if !seen_gz_files.insert(short.clone()) {
                duplicate_files += 1;
                if !entry.ignored.contains(&short) {
                    entry.ignored.push(short);
                }
                continue;
            }
            if !entry.files.contains(&short) {
                entry.files.push(short);
            }
        }

        let unique_short_set: std::collections::HashSet<_> = entry.files.iter().cloned().collect();
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
    // LATEST STATE GEN
    // =========================
    let mut latest_map: HashMap<String, LatestInfo> = HashMap::new();

    for (path, lines, hash) in params {
        latest_map.insert(
            get_dir(&path),
            LatestInfo {
                hash,
                last_line: lines as u64,
            },
        );
    }

    for (path, entry) in entries_by_path.iter_mut() {
        if let Some(info) = latest_map.get(path) {
            entry.set_latest_flat(info.clone());
        } else {
            entry.latest =
                serde_json::to_value(LatestInfo::default()).unwrap_or(Default::default());
        }
    }

    // =========================
    // FINAL MERGE
    // =========================
    let parsed_gz_lines = logs.len();
    let parsed_latest_lines = logs_latest.len();

    logs.extend(logs_latest);
    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();

    let now = system_time_secs();

    // =========================
    // BUILD CONFIG
    // =========================
    let mut cfg = read_base_info::BaseConfig {
        name: name.clone(),
        lines: lines as u64,
        hash: String::new(),
        tags: Vec::new(),
        custom: "custom".to_string(),
        time: TimeInfo {
            creation_unix: now.to_string(),
            edit_unix: now.to_string(),
            fld_unix: logs
                .first()
                .map(|(ts, _)| ts.to_string())
                .unwrap_or_default(),
            lld_unix: logs
                .last()
                .map(|(ts, _)| ts.to_string())
                .unwrap_or_default(),
        },
        paths: read_base_info::normalize_paths(entries_by_path.into_values().collect()),
    };

    // =========================
    // BASE DIR + WRITE LOGS
    // =========================
    progress.set_total(lines as u64);
    progress.set_message("Writing logs");

    save_log(&name, &cfg)?;

    let zst_path = read_base_info::get_zst_path(&name)?;
    zip_writer::write_logs_to_zstd_with_progress(&logs, &zst_path, progress)?;

    progress.finish();

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

fn system_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
