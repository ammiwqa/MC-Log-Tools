use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use progress::{ProgressReporter, StageKind, StageStats};

use log_parser::ErrorStats;
use rayon::prelude::*;
use read_base_info::{LatestInfo, PathEntry, TimeInfo, save_log};
use xxhash_rust::xxh3::Xxh3;

const HASH_BUFFER_SIZE: usize = 1024 * 1024;
const DEBUG_HASH_EVERY: usize = 100;
const DEBUG: bool = false;

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

fn debug_stage(message: &str) {
    if DEBUG {
        eprintln!("[create_base][{:?}] {}", Instant::now(), message);
    }
}

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .to_string()
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn skip_gzip_string(file: &mut File) -> io::Result<()> {
    let mut byte = [0u8; 1];

    loop {
        file.read_exact(&mut byte)?;

        if byte[0] == 0 {
            return Ok(());
        }
    }
}

fn compute_gzip_payload_hash(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();

    let mut header = [0u8; 10];
    file.read_exact(&mut header)?;

    if header[0] != 0x1f || header[1] != 0x8b || header[2] != 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid gzip header",
        ));
    }

    let flags = header[3];

    if flags & 0xE0 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Reserved gzip flags are set",
        ));
    }

    if flags & 0x04 != 0 {
        let mut xlen = [0u8; 2];
        file.read_exact(&mut xlen)?;

        let xlen = u16::from_le_bytes(xlen) as i64;
        file.seek(SeekFrom::Current(xlen))?;
    }

    if flags & 0x08 != 0 {
        skip_gzip_string(&mut file)?;
    }

    if flags & 0x10 != 0 {
        skip_gzip_string(&mut file)?;
    }

    if flags & 0x02 != 0 {
        file.seek(SeekFrom::Current(2))?;
    }

    let payload_start = file.stream_position()?;

    if file_len < payload_start + 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Gzip file is too short: file_len={}, payload_start={}, remaining={}",
                file_len,
                payload_start,
                file_len.saturating_sub(payload_start)
            ),
        ));
    }

    let payload_len = file_len - payload_start - 8;

    let mut hasher = Xxh3::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    let mut remaining = payload_len;

    while remaining > 0 {
        let to_read = remaining.min(buffer.len() as u64) as usize;

        file.read_exact(&mut buffer[..to_read])?;
        hasher.update(&buffer[..to_read]);

        remaining -= to_read as u64;
    }

    Ok(hasher.digest())
}

pub fn create_base<P: ProgressReporter + ?Sized>(
    paths: Vec<String>,
    name: String,
    progress: &P,
) -> Result<CreateResult, Box<dyn std::error::Error>> {
    let total_start = Instant::now();

    debug_stage(&format!(
        "START: name='{}', root directories={}",
        name,
        paths.len()
    ));

    let mut all_logs: Vec<String> = Vec::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();

    let mut entries_by_path: HashMap<String, PathEntry> = HashMap::new();

    let mut seen_full_paths: HashSet<String> = HashSet::new();
    let mut seen_gz_hashes: HashSet<u64> = HashSet::new();
    let mut seen_latest_files: HashSet<String> = HashSet::new();

    let mut duplicate_files = 0usize;

    debug_stage("STEP 1: discovering .log.gz files");

    progress.set_message("Scanning gzip directories");
    progress.set_total(paths.len() as u64);

    let mut files_by_path: Vec<(String, Vec<String>)> = Vec::with_capacity(paths.len());

    let mut total_gz_candidates = 0usize;

    for path in &paths {
        let full_files = get_logfiles::find_log_files(path, true, ".log.gz", false);

        total_gz_candidates += full_files.len();

        debug_stage(&format!(
            "Found {} gzip files in '{}'",
            full_files.len(),
            path
        ));

        files_by_path.push((path.clone(), full_files));
        progress.inc(1);
    }

    debug_stage(&format!(
        "Gzip discovery complete: {} candidate files",
        total_gz_candidates
    ));

    progress.set_message("Preparing gzip files");

    let mut candidates: Vec<(String, String, String)> = Vec::with_capacity(total_gz_candidates);

    for (path, full_files) in files_by_path {
        let entry = entries_by_path.entry(path.clone()).or_default();
        entry.path = path.clone();

        for full_path in full_files {
            let normalized_path = PathBuf::from(&full_path).to_string_lossy().into_owned();

            let short = file_name_of(&normalized_path);

            if !seen_full_paths.insert(normalized_path.clone()) {
                duplicate_files += 1;

                if !entry.ignored.contains(&short) {
                    entry.ignored.push(short);
                }

                continue;
            }

            candidates.push((path.clone(), normalized_path, short));
        }
    }

    let hash_total = candidates.len();

    debug_stage(&format!(
        "Prepared {} files for parallel hashing",
        hash_total
    ));

    progress.set_message("Hashing gzip files");
    progress.set_total(hash_total as u64);

    let hash_start = Instant::now();

    let (tx, rx) = mpsc::channel::<(usize, io::Result<u64>)>();

    let mut hash_results: Vec<Option<io::Result<u64>>> =
        std::iter::repeat_with(|| None).take(hash_total).collect();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            candidates
                .par_iter()
                .enumerate()
                .for_each_with(tx, |sender, (index, candidate)| {
                    let result = compute_gzip_payload_hash(Path::new(&candidate.1));

                    let _ = sender.send((index, result));
                });
        });

        for (completed, (index, result)) in rx.into_iter().enumerate() {
            hash_results[index] = Some(result);

            progress.inc(1);

            if DEBUG && ((completed + 1) % DEBUG_HASH_EVERY == 0 || completed + 1 == hash_total) {
                debug_stage(&format!("Hashed {}/{} files", completed + 1, hash_total));
            }
        }
    });

    debug_stage(&format!(
        "Parallel hashing complete: {} files, elapsed={:?}",
        hash_total,
        hash_start.elapsed()
    ));


    progress.set_message("Deduplicating gzip files");

    for ((path, normalized_path, short), hash_result) in
        candidates.into_iter().zip(hash_results.into_iter())
    {
        let entry = entries_by_path.entry(path).or_default();

        let hash_result = match hash_result {
            Some(result) => result,
            None => {
                eprintln!(
                    "[create_base][WARNING] Missing hash result for '{}'. Keeping file.",
                    normalized_path
                );

                if !entry.files.contains(&short) {
                    entry.files.push(short);
                }

                all_logs.push(normalized_path);
                continue;
            }
        };

        match hash_result {
            Ok(hash) => {
                if !seen_gz_hashes.insert(hash) {
                    duplicate_files += 1;

                    if !entry.ignored.contains(&short) {
                        entry.ignored.push(short);
                    }

                    debug_stage(&format!("DUPLICATE gzip: {}", normalized_path));

                    continue;
                }
            }
            Err(err) => {
                eprintln!(
                    "[create_base][WARNING] Failed to hash '{}': {}. Keeping file.",
                    normalized_path, err
                );
            }
        }

        if !entry.files.contains(&short) {
            entry.files.push(short);
        }

        all_logs.push(normalized_path);
    }

    let gz_files_count = all_logs.len();

    progress.stage_completed(StageStats {
        stage: StageKind::Dedup,
        files: 0,
        lines: 0,
        file_errors: 0,
        parse_errors: 0,
        duplicate_files,
    });

    debug_stage(&format!(
        "Gzip deduplication complete: unique={}, duplicates={}",
        gz_files_count, duplicate_files
    ));

    debug_stage("STEP 5: scanning for latest.log");

    progress.set_message("Scanning latest.log files");
    progress.set_total(paths.len() as u64);

    for (dir_index, path) in paths.iter().enumerate() {
        progress.set_message(&format!(
            "Scanning latest.log {}/{}",
            dir_index + 1,
            paths.len()
        ));

        let latest_files = get_logfiles::find_log_files(path, true, "latest.log", true);

        debug_stage(&format!(
            "Found {} latest.log files in '{}'",
            latest_files.len(),
            path
        ));

        for full_path in latest_files {
            let normalized_path = PathBuf::from(&full_path).to_string_lossy().into_owned();

            if !seen_latest_files.insert(normalized_path.clone()) {
                duplicate_files += 1;

                debug_stage(&format!("DUPLICATE latest.log path: {}", normalized_path));

                continue;
            }

            latest_params.push((normalized_path, 0));
        }

        progress.inc(1);
    }

    let latest_params_len = latest_params.len();

    debug_stage(&format!(
        "STEP 5 COMPLETE: latest.log files={}",
        latest_params_len
    ));

    debug_stage(&format!("STEP 6: parsing {} gzip files", all_logs.len()));

    progress.set_message("Parsing gzip logs");
    progress.set_total(all_logs.len() as u64);

    let parse_gz_start = Instant::now();

    let (mut logs, errors) = log_parser::parse_logs(all_logs, progress)?;

    progress.finish();

    let parsed_gz_lines = logs.len();

    progress.stage_completed(StageStats {
        stage: StageKind::GzipParsing,
        files: gz_files_count,
        lines: parsed_gz_lines,
        file_errors: errors.file_errors,
        parse_errors: errors.parse_errors,
        duplicate_files: 0,
    });

    debug_stage(&format!(
        "STEP 6 COMPLETE: parsed gzip lines={}, elapsed={:?}",
        parsed_gz_lines,
        parse_gz_start.elapsed()
    ));

    debug_stage(&format!(
        "STEP 7: parsing {} latest.log files",
        latest_params_len
    ));

    progress.set_message("Parsing latest logs");
    progress.set_total(latest_params_len as u64);

    let parse_latest_start = Instant::now();

    let (logs_latest, params, latest_errors) = log_parser::parse_latest(latest_params, progress)?;

    progress.finish();

    let parsed_latest_lines = logs_latest.len();

    progress.stage_completed(StageStats {
        stage: StageKind::LatestParsing,
        files: latest_params_len,
        lines: parsed_latest_lines,
        file_errors: latest_errors.file_errors,
        parse_errors: latest_errors.parse_errors,
        duplicate_files: 0,
    });

    debug_stage(&format!(
        "STEP 7 COMPLETE: parsed latest lines={}, elapsed={:?}",
        parsed_latest_lines,
        parse_latest_start.elapsed()
    ));

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
            entry.latest = serde_json::to_value(LatestInfo::default()).unwrap_or_default();
        }
    }

    logs.extend(logs_latest);
    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();
    let now = system_time_secs();

    let cfg = read_base_info::BaseConfig {
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

    progress.set_message("Writing logs");
    progress.set_total(lines as u64);

    save_log(&name, &cfg)?;

    let zst_path = read_base_info::get_zst_path(&name)?;

    // let write_start = Instant::now();

    zip_writer::write_logs_to_zstd_with_progress(&logs, &zst_path, progress)?;

    progress.finish();

    debug_stage(&format!(
        "SUCCESS: total elapsed={:?}, gzip files={}, gzip lines={}, \
         latest files={}, latest lines={}, total lines={}, duplicates={}",
        total_start.elapsed(),
        gz_files_count,
        parsed_gz_lines,
        latest_params_len,
        parsed_latest_lines,
        lines,
        duplicate_files
    ));

    Ok(CreateResult {
        gz_files: gz_files_count,
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
