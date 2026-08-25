use log_parser::{parse_latest, parse_logs, ErrorStats};
use progress::ProgressReporter;
use read_base_info::{get_zst_path, load_log, save_log, LatestInfo};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
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
    let mut cfg = load_log(name)?;

    let mut known_gz_files: HashSet<String> = HashSet::new();
    let mut known_latest_files: HashSet<String> = HashSet::new();

    for entry in &cfg.paths {
        for f in entry.files.iter().chain(entry.ignored.iter()) {
            known_gz_files.insert(f.clone());
        }
    }

    let mut all_new_gz_logs: Vec<String> = Vec::new();
    let mut all_new_latest_logs: Vec<(String, usize)> = Vec::new();

    for entry in &mut cfg.paths {
        let source_path = entry.path.clone();

        let current_gz_short = get_logfiles::find_log_files(&source_path, false, ".log.gz", false);

        let mut new_gz_files: Vec<String> = Vec::new();
        for f in current_gz_short {
            if known_gz_files.insert(f.clone()) {
                new_gz_files.push(f);
            }
        }

        if !new_gz_files.is_empty() {
            let full: Vec<String> = new_gz_files
                .iter()
                .map(|f| {
                    PathBuf::from(&source_path)
                        .join(f)
                        .to_string_lossy()
                        .to_string()
                })
                .collect();
            all_new_gz_logs.extend(full);
            entry.files.extend(new_gz_files);
        }

        let stored = entry.latest_flat();

        for latest_file in get_logfiles::find_log_files(&source_path, true, "latest.log", true) {
            if !known_latest_files.insert(latest_file.clone()) {
                continue;
            }

            let current_hash = match compute_file_hash(&latest_file) {
                Ok(h) => h,
                Err(_) => continue,
            };

            if current_hash != stored.hash {
                all_new_latest_logs.push((latest_file, stored.last_line as usize));
            }
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
            total_lines: cfg.lines as usize,
            no_new_files: true,
        });
    }

    progress.set_total(all_new_gz_logs.len() as u64);
    let (new_logs, gz_errors) = match parse_logs(std::mem::take(&mut all_new_gz_logs), progress) {
        Ok(v) => v,
        Err(e) => {
            progress.finish();
            return Err(Box::new(e));
        }
    };
    progress.finish();

    progress.set_total(all_new_latest_logs.len() as u64);
    let (logs_latest, latest_params, latest_errors) =
        match parse_latest(std::mem::take(&mut all_new_latest_logs), progress) {
            Ok(v) => v,
            Err(e) => {
                progress.finish();
                return Err(Box::new(e));
            }
        };
    progress.finish();

    let parsed_gz_lines = new_logs.len();
    let parsed_latest_lines = logs_latest.len();

    let mut latest_updates: HashMap<String, LatestInfo> = HashMap::new();
    for (path, last_line, hash) in latest_params {
        latest_updates.insert(
            get_dir(&path),
            LatestInfo {
                hash,
                last_line: last_line as u64,
            },
        );
    }

    for entry in &mut cfg.paths {
        if let Some(info) = latest_updates.get(&entry.path) {
            entry.set_latest_flat(info.clone());
        } else if entry.latest_flat().hash.is_empty() && !latest_updates.is_empty() {
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

    let now = system_time_secs().to_string();

    cfg.lines = total_lines as u64;
    cfg.time.edit_unix = now.clone();
    if cfg.time.creation_unix.is_empty() {
        cfg.time.creation_unix = now.clone();
    }

    let first_log_time = all_logs
        .first()
        .map(|(ts, _)| ts.to_string())
        .unwrap_or_default();
    let last_log_time = all_logs
        .last()
        .map(|(ts, _)| ts.to_string())
        .unwrap_or_default();

    if first_log_time.len() == 12 {
        cfg.time.fld_unix = first_log_time;
    }
    if last_log_time.len() == 12 {
        cfg.time.lld_unix = last_log_time;
    }

    save_log(name, &cfg)?;

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

fn system_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_existing_logs(zst_path: &str) -> std::io::Result<Vec<(i64, String)>> {
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
