use log_parser::{parse_latest, parse_logs, ErrorStats};
use progress::ProgressReporter;
use rayon::prelude::*;
use read_base_info::{get_zst_path, load_log, save_log, LatestInfo};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use zip_writer::write_logs_to_zstd_with_progress;

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .into_owned()
}

fn compute_file_hash(path: &str) -> io::Result<String> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;

        File::options()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
            .open(path)?
    };

    #[cfg(not(windows))]
    let file = File::open(path)?;

    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];

    loop {
        let bytes_read = reader.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

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

    let mut known_gz_files = HashSet::<String>::new();
    let mut known_latest_files = HashSet::<String>::new();

    for entry in &cfg.paths {
        known_gz_files.extend(entry.files.iter().cloned());
        known_gz_files.extend(entry.ignored.iter().cloned());
    }

    let mut all_new_gz_logs = Vec::<String>::new();
    let mut latest_checks = Vec::<(String, String, usize)>::new();

    if !cfg.paths.is_empty() {
        progress.set_message("Scanning");
        progress.set_total(cfg.paths.len() as u64);
    }

    let discovered: Vec<(Vec<String>, Vec<String>)> = cfg
        .paths
        .par_iter()
        .map(|entry| {
            let gz =
                get_logfiles::find_log_files(&entry.path, false, ".log.gz", false);
            let latest =
                get_logfiles::find_log_files(&entry.path, true, "latest.log", true);
            progress.inc(1);
            (gz, latest)
        })
        .collect();

    for (entry, (current_gz_files, latest_files)) in cfg.paths.iter_mut().zip(discovered) {
        let source_path = entry.path.clone();

        let new_gz_files: Vec<String> = current_gz_files
            .into_iter()
            .filter(|file| known_gz_files.insert(file.clone()))
            .collect();

        if !new_gz_files.is_empty() {
            all_new_gz_logs.extend(new_gz_files.iter().map(|file| {
                Path::new(&source_path)
                    .join(file)
                    .to_string_lossy()
                    .into_owned()
            }));

            entry.files.extend(new_gz_files);
        }

        let stored = entry.latest_flat();

        for latest_file in latest_files {
            if known_latest_files.insert(latest_file.clone()) {
                latest_checks.push((latest_file, stored.hash.clone(), stored.last_line as usize));
            }
        }
    }

    if !cfg.paths.is_empty() {
        progress.finish();
    }

    if !latest_checks.is_empty() {
        progress.set_message("Checking hashes");
        progress.set_total(latest_checks.len() as u64);
    }

    let latest_results: Vec<(String, usize)> = latest_checks
        .par_iter()
        .filter_map(
            |(path, stored_hash, last_line)| {
                let result = match compute_file_hash(path) {
                    Ok(current_hash) if current_hash != *stored_hash => {
                        Some((path.clone(), *last_line))
                    }

                    Ok(_) => None,

                    Err(err) => {
                        eprintln!("Failed to hash '{}': {}", path, err);
                        None
                    }
                };
                progress.inc(1);
                result
            },
        )
        .collect();

    if !latest_checks.is_empty() {
        progress.set_message("");
        progress.finish();
    }

    let new_gz_count = all_new_gz_logs.len();
    let new_latest_count = latest_results.len();
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

    progress.set_total(new_gz_count as u64);

    let (new_logs, gz_errors) = match parse_logs(std::mem::take(&mut all_new_gz_logs), progress) {
        Ok(value) => value,
        Err(err) => {
            progress.finish();
            return Err(Box::new(err));
        }
    };

    progress.finish();

    progress.set_total(new_latest_count as u64);

    let (logs_latest, latest_params, latest_errors) = match parse_latest(latest_results, progress) {
        Ok(value) => value,
        Err(err) => {
            progress.finish();
            return Err(Box::new(err));
        }
    };

    progress.finish();

    let parsed_gz_lines = new_logs.len();
    let parsed_latest_lines = logs_latest.len();

    let latest_updates: HashMap<String, LatestInfo> = latest_params
        .into_iter()
        .map(|(path, last_line, hash)| {
            (
                get_dir(&path),
                LatestInfo {
                    hash,
                    last_line: last_line as u64,
                },
            )
        })
        .collect();

    for entry in &mut cfg.paths {
        if let Some(info) = latest_updates.get(&entry.path) {
            entry.set_latest_flat(info.clone());
        }
    }

    let zst_path = get_zst_path(name)?;

    progress.set_message("Reading existing");
    progress.set_total(cfg.lines);

    let existing_logs = read_existing_logs_with_progress(&zst_path, progress)?;

    progress.finish();

    let existing_count = existing_logs.len();

    let mut all_logs = existing_logs;
    all_logs.extend(new_logs);
    all_logs.extend(logs_latest);

    all_logs.sort_unstable_by_key(|log| log.0);

    let total_lines = all_logs.len();
    let new_lines_count = total_lines.saturating_sub(existing_count);

    let now = system_time_secs().to_string();

    cfg.lines = total_lines as u64;
    cfg.time.edit_unix = now.clone();

    if cfg.time.creation_unix.is_empty() {
        cfg.time.creation_unix = now;
    }

    if let Some((ts, _)) = all_logs.first() {
        let ts = ts.to_string();

        if ts.len() == 12 {
            cfg.time.fld_unix = ts;
        }
    }

    if let Some((ts, _)) = all_logs.last() {
        let ts = ts.to_string();

        if ts.len() == 12 {
            cfg.time.lld_unix = ts;
        }
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

fn read_existing_logs_with_progress<P: ProgressReporter + ?Sized>(
    zst_path: &str,
    progress: &P,
) -> io::Result<Vec<(i64, String)>> {
    use std::io::{BufRead, BufReader};
    use zstd::stream::read::Decoder;

    let file = File::open(zst_path)?;
    let decoder = Decoder::new(file)?;
    let reader = BufReader::new(decoder);

    let mut logs = Vec::new();

    for line in reader.lines() {
        let line = line?;

        if line.len() < 13 {
            continue;
        }

        let ts_str = &line[..12];

        if let Ok(ts) = ts_str.parse::<i64>() {
            logs.push((ts, line[13..].to_owned()));
        }

        progress.inc(1);
    }

    Ok(logs)
}
