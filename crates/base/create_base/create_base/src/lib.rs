use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

use log_parser::ErrorStats;
use progress::ProgressReporter;
use read_base_info::{LatestInfo, PathEntry, TimeInfo, save_log};
use sha2::{Digest, Sha256};

const HASH_BUFFER_SIZE: usize = 64 * 1024;
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

fn compute_gzip_payload_hash(path: &Path) -> io::Result<String> {
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
            "Gzip file is too short",
        ));
    }

    let payload_len = file_len - payload_start - 8;

    let mut hasher = Sha256::new();
    let mut remaining = payload_len;
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];

    while remaining > 0 {
        let to_read = remaining.min(buffer.len() as u64) as usize;

        file.read_exact(&mut buffer[..to_read])?;
        hasher.update(&buffer[..to_read]);

        remaining -= to_read as u64;
    }

    let hash = hex::encode(hasher.finalize());

    if DEBUG {
        eprintln!(
            "[GZIP HASH] Hashed {} bytes: {}",
            payload_len,
            path.display()
        );
    }

    Ok(hash)
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
    let mut seen_gz_hashes: HashSet<String> = HashSet::new();
    let mut seen_latest_files: HashSet<String> = HashSet::new();

    let mut duplicate_files = 0usize;

    debug_stage("STEP 1: discovering .log.gz files");

    progress.set_message("Scanning gzip directories");
    progress.set_total(paths.len() as u64);

    let mut files_by_path: Vec<(String, Vec<String>)> = Vec::with_capacity(paths.len());

    let mut total_gz_candidates = 0usize;

    for path in &paths {
        debug_stage(&format!("Discovering gzip files in '{}'", path));

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

    progress.set_message("Hashing gzip files");
    progress.set_total(total_gz_candidates as u64);

    let mut processed_gz_candidates = 0usize;

    for (path, full_files) in files_by_path {
        let entry = entries_by_path.entry(path.clone()).or_default();
        entry.path = path.clone();

        for full_path in full_files {
            processed_gz_candidates += 1;

            let normalized_path = PathBuf::from(&full_path).to_string_lossy().into_owned();

            let short = file_name_of(&normalized_path);

            if !seen_full_paths.insert(normalized_path.clone()) {
                duplicate_files += 1;

                if !entry.ignored.contains(&short) {
                    entry.ignored.push(short);
                }

                progress.inc(1);
                continue;
            }

            if DEBUG
                && (processed_gz_candidates % DEBUG_HASH_EVERY == 0 || processed_gz_candidates == 1)
            {
                debug_stage(&format!(
                    "Hashing file {}/{}: {}",
                    processed_gz_candidates, total_gz_candidates, normalized_path
                ));
            }

            let hash_result = compute_gzip_payload_hash(Path::new(&normalized_path));

            match hash_result {
                Ok(hash) => {
                    if !seen_gz_hashes.insert(hash) {
                        duplicate_files += 1;

                        if !entry.ignored.contains(&short) {
                            entry.ignored.push(short);
                        }

                        debug_stage(&format!("DUPLICATE gzip: {}", normalized_path));

                        progress.inc(1);
                        continue;
                    }
                }
                Err(err) => {
                    eprintln!(
                        "[create_base][WARNING] Failed to hash '{}': {}. \
                         Keeping file.",
                        normalized_path, err
                    );
                }
            }

            if !entry.files.contains(&short) {
                entry.files.push(short);
            }

            all_logs.push(normalized_path);

            progress.inc(1);
        }

        debug_stage(&format!(
            "Finished directory '{}'; unique gzip files so far: {}, \
             duplicates: {}",
            path,
            all_logs.len(),
            duplicate_files
        ));
    }

    progress.set_message("Gzip deduplication completed");

    let gz_files_count = all_logs.len();

    debug_stage(&format!(
        "Deduplication complete: unique gzip files={}, duplicates={}",
        gz_files_count, duplicate_files
    ));

    let gz_files_count = all_logs.len();

    debug_stage(&format!(
        "STEP 2 COMPLETE: unique gzip files={}, duplicates={}",
        gz_files_count, duplicate_files
    ));

    debug_stage("STEP 3: scanning for latest.log");

    progress.set_message("Scanning latest.log files");
    progress.set_total(paths.len() as u64);

    for (dir_index, path) in paths.iter().enumerate() {
        progress.set_message(&format!(
            "Scanning latest.log {}/{}",
            dir_index + 1,
            paths.len()
        ));

        debug_stage(&format!(
            "Scanning latest.log in directory {}/{}: {}",
            dir_index + 1,
            paths.len(),
            path
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
        "STEP 3 COMPLETE: latest.log files={}",
        latest_params_len
    ));

    debug_stage(&format!("STEP 4: parsing {} gzip files", all_logs.len()));

    progress.set_message("Parsing gzip logs");
    progress.set_total(all_logs.len() as u64);

    let parse_gz_start = Instant::now();

    let (mut logs, errors) = log_parser::parse_logs(all_logs, progress)?;

    progress.finish();

    let parsed_gz_lines = logs.len();

    debug_stage(&format!(
        "STEP 4 COMPLETE: parsed gzip lines={}, elapsed={:?}",
        parsed_gz_lines,
        parse_gz_start.elapsed()
    ));

    debug_stage(&format!(
        "STEP 5: parsing {} latest.log files",
        latest_params_len
    ));

    progress.set_message("Parsing latest logs");
    progress.set_total(latest_params_len as u64);

    let parse_latest_start = Instant::now();

    let (logs_latest, params, latest_errors) = log_parser::parse_latest(latest_params, progress)?;

    progress.finish();

    let parsed_latest_lines = logs_latest.len();

    debug_stage(&format!(
        "STEP 5 COMPLETE: parsed latest lines={}, elapsed={:?}",
        parsed_latest_lines,
        parse_latest_start.elapsed()
    ));

    debug_stage(&format!(
        "STEP 6: building latest state from {} parameter records",
        params.len()
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

    debug_stage(&format!(
        "STEP 6 COMPLETE: latest state entries={}",
        latest_map.len()
    ));

    debug_stage(&format!(
        "STEP 7: merging {} gzip lines and {} latest lines",
        logs.len(),
        logs_latest.len()
    ));

    logs.extend(logs_latest);

    debug_stage(&format!("Sorting {} total log records", logs.len()));

    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();
    let now = system_time_secs();

    debug_stage(&format!("STEP 7 COMPLETE: total records={}", lines));

    debug_stage("STEP 8: building base configuration");

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

    debug_stage("STEP 8 COMPLETE: base configuration built");

    debug_stage("STEP 9: saving base metadata");

    progress.set_message("Writing logs");
    progress.set_total(lines as u64);

    save_log(&name, &cfg)?;

    debug_stage("Base metadata saved");

    let zst_path = read_base_info::get_zst_path(&name)?;

    debug_stage(&format!("Writing {} records to '{}'", logs.len(), zst_path));

    let write_start = Instant::now();

    zip_writer::write_logs_to_zstd_with_progress(&logs, &zst_path, progress)?;

    progress.finish();

    debug_stage(&format!(
        "STEP 9 COMPLETE: writing elapsed={:?}",
        write_start.elapsed()
    ));

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
