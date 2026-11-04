use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hash_utils::compute_gzip_payload_hash;
use progress::{ProgressReporter, StageKind, StageStats};

use log_parser::ErrorStats;
use rayon::prelude::*;
use read_base_info::{LatestInfo, PathEntry, TimeInfo, save_log};

mod resources;

pub use resources::peak_rss_bytes;

const DEBUG: bool = false;

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub chunk_lines: u64,
    pub sort_run_lines: u64,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            chunk_lines: read_base_info::DEFAULT_CHUNK_LINES,
            sort_run_lines: read_base_info::DEFAULT_SORT_RUN_LINES,
        }
    }
}

impl CreateOptions {
    pub fn sanitized(self) -> Self {
        Self {
            chunk_lines: if self.chunk_lines == 0 {
                read_base_info::DEFAULT_CHUNK_LINES
            } else {
                self.chunk_lines
            },
            sort_run_lines: if self.sort_run_lines == 0 {
                read_base_info::DEFAULT_SORT_RUN_LINES
            } else {
                self.sort_run_lines
            },
        }
    }
}

#[derive(Debug)]
pub struct CreateResult {
    pub gz_files: usize,
    pub parsed_gz_lines: usize,
    pub latest_files: usize,
    pub parsed_latest_lines: usize,
    pub input_lines: usize,
    pub gz_errors: ErrorStats,
    pub latest_errors: ErrorStats,
    pub total_lines: usize,
    pub duplicate_files: usize,
    pub peak_memory_bytes: Option<u64>,
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

pub fn create_base<P: ProgressReporter + ?Sized>(
    paths: Vec<String>,
    name: String,
    progress: &P,
) -> Result<CreateResult, Box<dyn std::error::Error>> {
    create_base_with_options(paths, name, CreateOptions::default(), progress)
}

pub fn create_base_with_options<P: ProgressReporter + ?Sized>(
    paths: Vec<String>,
    name: String,
    options: CreateOptions,
    progress: &P,
) -> Result<CreateResult, Box<dyn std::error::Error>> {
    let options = options.sanitized();
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

    let mut sized_candidates: Vec<(String, String, String, u64)> =
        Vec::with_capacity(candidates.len());

    for (path, normalized_path, short) in candidates {
        let size = fs::metadata(Path::new(&normalized_path))
            .map(|m| m.len())
            .unwrap_or(0);
        sized_candidates.push((path, normalized_path, short, size));
    }

    let mut size_groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, (_, _, _, size)) in sized_candidates.iter().enumerate() {
        size_groups.entry(*size).or_default().push(index);
    }

    let mut to_hash: Vec<(usize, String, String, String)> = Vec::new();
    for (index, (path, normalized_path, short, _)) in sized_candidates.iter().enumerate() {
        let group_len = size_groups
            .get(&sized_candidates[index].3)
            .map(|v| v.len())
            .unwrap_or(1);
        if group_len > 1 {
            to_hash.push((index, path.clone(), normalized_path.clone(), short.clone()));
        }
    }

    let hash_total = to_hash.len();

    let mut hash_results: Vec<Option<io::Result<u64>>> =
        (0..sized_candidates.len()).map(|_| None).collect();

    if hash_total > 0 {
        debug_stage(&format!(
            "Files to hash (size groups > 1): {} of {} total",
            hash_total,
            sized_candidates.len()
        ));

        progress.set_message("Hashing gzip files");
        progress.set_total(hash_total as u64);

        let hash_start = Instant::now();

        let results: Vec<(usize, io::Result<u64>)> = to_hash
            .par_iter()
            .map(|(index, _path, normalized_path, _short)| {
                let result = compute_gzip_payload_hash(Path::new(normalized_path));
                progress.inc(1);
                (*index, result)
            })
            .collect();

        for (index, result) in results {
            hash_results[index] = Some(result);
        }

        debug_stage(&format!(
            "Parallel hashing complete: {} files, elapsed={:?}",
            hash_total,
            hash_start.elapsed()
        ));
    } else {
        debug_stage("No files need hashing (all files have unique sizes)");
    }

    progress.set_message("Deduplicating gzip files");

    for (index, (path, normalized_path, short, size)) in sized_candidates.into_iter().enumerate() {
        let entry = entries_by_path.entry(path).or_default();
        
        let group_len = size_groups.get(&size).map(|v| v.len()).unwrap_or(1);
        if group_len <= 1 {
            if !entry.files.contains(&short) {
                entry.files.push(short);
            }
            all_logs.push(normalized_path);
            continue;
        }

        let hash_result = match hash_results[index].take() {
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

    let base_dir = read_base_info::base_dir(&name)?;
    let sort_dir = base_dir.join(".sort-tmp");

    let mut sorter = base_chunks::external_sort::ExternalSorter::new(
        &sort_dir,
        options.sort_run_lines as usize,
    )?;

    progress.set_message("Parsing gzip logs");
    progress.set_total(all_logs.len() as u64);

    let parse_gz_start = Instant::now();
    let mut parsed_gz_lines = 0usize;

    let errors = log_parser::parse_logs_streaming(all_logs, progress, |batch| {
        parsed_gz_lines += batch.len();
        sorter.push_batch(batch)
    })?;

    progress.finish();

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

    sorter.push_batch(logs_latest)?;

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

    let expected_lines = parsed_gz_lines + parsed_latest_lines;
    let now = system_time_secs();

    progress.set_message("Writing chunks");
    progress.set_total(expected_lines as u64);

    let sorted = sorter.finish()?;

    let chunks = base_chunks::write_chunks_streaming(
        &name,
        sorted,
        options.chunk_lines as usize,
        progress,
    )?;

    progress.finish();

    let lines: usize = chunks.iter().map(|chunk| chunk.lines as usize).sum();
    let input_lines = parsed_gz_lines + parsed_latest_lines;

    if lines > input_lines {
        eprintln!(
            "[create_base][WARNING] total lines in base ({}) exceeds parsed input lines ({}); \
             likely cause: latest.log overlaps with gz files (same lines appear in both sources)",
            lines, input_lines
        );
    }

    let cfg = read_base_info::BaseConfig {
        name: name.clone(),
        lines: lines as u64,
        hash: base_chunks::base_hash(&chunks),
        tags: Vec::new(),
        custom: "custom".to_string(),
        time: TimeInfo {
            creation_unix: now.to_string(),
            edit_unix: now.to_string(),
            fld_unix: chunks
                .first()
                .map(|chunk| chunk.first_ts.to_string())
                .unwrap_or_default(),
            lld_unix: chunks
                .last()
                .map(|chunk| chunk.last_ts.to_string())
                .unwrap_or_default(),
        },
        paths: read_base_info::normalize_paths(entries_by_path.into_values().collect()),
        chunks,
        chunk_lines: options.chunk_lines,
    };

    save_log(&name, &cfg)?;

    let peak_memory_bytes = peak_rss_bytes();

    debug_stage(&format!(
        "SUCCESS: total elapsed={:?}, gzip files={}, gzip lines={}, \
         latest files={}, latest lines={}, total lines={}, duplicates={}, \
         peak_rss={:?}",
        total_start.elapsed(),
        gz_files_count,
        parsed_gz_lines,
        latest_params_len,
        parsed_latest_lines,
        lines,
        duplicate_files,
        peak_memory_bytes
    ));

    Ok(CreateResult {
        gz_files: gz_files_count,
        parsed_gz_lines,
        latest_files: latest_params_len,
        parsed_latest_lines,
        input_lines,
        gz_errors: errors,
        latest_errors,
        total_lines: lines,
        duplicate_files,
        peak_memory_bytes,
    })
}

fn system_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
