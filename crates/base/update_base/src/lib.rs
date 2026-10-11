use log_parser::{parse_latest, parse_logs, ErrorStats};
use progress::ProgressReporter;
use rayon::prelude::*;
use read_base_info::{get_zst_path, load_log, save_log, LatestInfo};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use xxhash_rust::xxh3::Xxh3;
use zip_writer::write_logs_to_zstd_with_progress;

const HASH_BUFFER_SIZE: usize = 1024 * 1024;

// ------------------------------------------------------------
// Utility functions
// ------------------------------------------------------------

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .into_owned()
}

/// SHA-256 оставлен для latest.log, поскольку сохранённые хеши
/// в LatestInfo могут быть созданы именно этим алгоритмом.
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

    let mut reader = BufReader::with_capacity(HASH_BUFFER_SIZE, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_SIZE];

    loop {
        let bytes_read = reader.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Пропускает строку gzip-заголовка до нулевого терминатора.
///
/// Не допускает смещения позиции чтения за пределы строки.
fn skip_gzip_string(file: &mut File) -> io::Result<()> {
    let mut buffer = [0u8; 256];

    loop {
        let n = file.read(&mut buffer)?;

        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Unterminated gzip header string",
            ));
        }

        if let Some(pos) = buffer[..n].iter().position(|&b| b == 0) {
            let unread = n - pos - 1;

            if unread > 0 {
                file.seek(SeekFrom::Current(-(unread as i64)))?;
            }

            return Ok(());
        }
    }
}

/// Вычисляет XXH3 по gzip payload.
///
/// Исключает заголовок и последние 8 байт trailer.
/// Предполагается один gzip-member на файл.
fn compute_gzip_payload_hash(path: &str) -> io::Result<u64> {
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;

        File::options()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
            .open(path)?
    };

    #[cfg(not(windows))]
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

    // FEXTRA
    if flags & 0x04 != 0 {
        let mut xlen = [0u8; 2];
        file.read_exact(&mut xlen)?;

        let extra_len = u16::from_le_bytes(xlen) as i64;
        file.seek(SeekFrom::Current(extra_len))?;
    }

    // FNAME
    if flags & 0x08 != 0 {
        skip_gzip_string(&mut file)?;
    }

    // FCOMMENT
    if flags & 0x10 != 0 {
        skip_gzip_string(&mut file)?;
    }

    // FHCRC
    if flags & 0x02 != 0 {
        file.seek(SeekFrom::Current(2))?;
    }

    let data_start = file.stream_position()?;

    let data_end = file_len.checked_sub(8).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Gzip file is too short for trailer: {}", path),
        )
    })?;

    if data_start > data_end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Invalid gzip structure: path={}, file_len={}, payload_start={}",
                path, file_len, data_start
            ),
        ));
    }

    let mut hasher = Xxh3::new();
    let mut buffer = [0u8; HASH_BUFFER_SIZE];
    let mut remaining = data_end - data_start;

    while remaining > 0 {
        let to_read = remaining.min(buffer.len() as u64) as usize;

        file.read_exact(&mut buffer[..to_read])?;
        hasher.update(&buffer[..to_read]);

        remaining -= to_read as u64;
    }

    Ok(hasher.digest())
}

// ------------------------------------------------------------
// Result
// ------------------------------------------------------------

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

// ------------------------------------------------------------
// Update base
// ------------------------------------------------------------

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

    // --------------------------------------------------------
    // STEP 1: Scan directories in parallel
    // --------------------------------------------------------

    progress.set_message("Scanning directories");
    progress.set_total(cfg.paths.len() as u64);

    let discovered: Vec<(Vec<String>, Vec<String>)> = cfg
        .paths
        .par_iter()
        .map(|entry| {
            let gz = get_logfiles::find_log_files(&entry.path, false, ".log.gz", false);

            let latest = get_logfiles::find_log_files(&entry.path, true, "latest.log", true);

            (gz, latest)
        })
        .collect();

    // Сканирование закончено; обновляем прогресс в основном потоке.
    for _ in 0..cfg.paths.len() {
        progress.inc(1);
    }

    let mut candidates: Vec<(usize, String, String)> = Vec::new();

    for (index, (entry, (current_gz_files, latest_files))) in
        cfg.paths.iter_mut().zip(discovered).enumerate()
    {
        let source_path = entry.path.clone();

        for file in current_gz_files {
            if known_gz_files.insert(file.clone()) {
                let full_path = Path::new(&source_path)
                    .join(&file)
                    .to_string_lossy()
                    .into_owned();

                candidates.push((index, file, full_path));
            }
        }

        let stored = entry.latest_flat();

        for latest_file in latest_files {
            if known_latest_files.insert(latest_file.clone()) {
                latest_checks.push((latest_file, stored.hash.clone(), stored.last_line as usize));
            }
        }
    }

    progress.finish();

    // Стабильный порядок обработки и выбора оригинального файла.
    candidates.sort_by(|a, b| a.2.cmp(&b.2));

    // --------------------------------------------------------
    // STEP 2: Hash gzip files in parallel with live progress
    // --------------------------------------------------------

    let hash_total = candidates.len();

    progress.set_message("Hashing gzip files");
    progress.set_total(hash_total as u64);

    let (tx, rx) = mpsc::channel::<(usize, io::Result<u64>)>();

    let mut hash_results: Vec<Option<io::Result<u64>>> =
        std::iter::repeat_with(|| None).take(hash_total).collect();

    std::thread::scope(|scope| {
        let candidates_ref = &candidates;

        scope.spawn(move || {
            candidates_ref.par_iter().enumerate().for_each_with(
                tx,
                |sender, (index, candidate)| {
                    let result = compute_gzip_payload_hash(&candidate.2);
                    let _ = sender.send((index, result));
                },
            );
        });

        for (index, result) in rx {
            hash_results[index] = Some(result);
            progress.inc(1);
        }
    });

    progress.finish();

    // --------------------------------------------------------
    // STEP 3: Deduplicate gzip files
    // --------------------------------------------------------

    let mut seen_hashes = HashSet::<u64>::new();

    for (candidate, result) in candidates.into_iter().zip(hash_results) {
        let (index, relative_path, full_path) = candidate;

        let result = match result {
            Some(result) => result,
            None => {
                eprintln!("Missing hash result for '{}'. Keeping file.", full_path);

                all_new_gz_logs.push(full_path);
                cfg.paths[index].files.push(relative_path);
                continue;
            }
        };

        match result {
            Ok(hash) => {
                if seen_hashes.insert(hash) {
                    all_new_gz_logs.push(full_path);
                    cfg.paths[index].files.push(relative_path);
                } else {
                    cfg.paths[index].ignored.push(relative_path);
                }
            }
            Err(err) => {
                eprintln!(
                    "Failed to hash gzip file '{}': {}. Keeping file.",
                    full_path, err
                );

                all_new_gz_logs.push(full_path);
                cfg.paths[index].files.push(relative_path);
            }
        }
    }

    // --------------------------------------------------------
    // STEP 4: Check latest.log hashes in parallel
    // --------------------------------------------------------

    let latest_total = latest_checks.len();

    progress.set_message("Checking latest hashes");
    progress.set_total(latest_total as u64);

    let (tx, rx) = mpsc::channel::<(usize, Option<(String, usize)>)>();

    let mut latest_slots: Vec<Option<(String, usize)>> = vec![None; latest_total];

    std::thread::scope(|scope| {
        let checks_ref = &latest_checks;

        scope.spawn(move || {
            checks_ref.par_iter().enumerate().for_each_with(
                tx,
                |sender, (index, (path, stored_hash, last_line))| {
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

                    let _ = sender.send((index, result));
                },
            );
        });

        for (index, result) in rx {
            latest_slots[index] = result;
            progress.inc(1);
        }
    });

    progress.finish();

    let latest_results: Vec<(String, usize)> = latest_slots.into_iter().flatten().collect();

    // --------------------------------------------------------
    // STEP 5: Return early if nothing changed
    // --------------------------------------------------------

    let new_gz_count = all_new_gz_logs.len();
    let new_latest_count = latest_results.len();

    let total_new_files = new_gz_count + new_latest_count;

    if total_new_files == 0 {
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

    // --------------------------------------------------------
    // STEP 6: Parse new gzip files
    // --------------------------------------------------------

    progress.set_message("Parsing gzip logs");
    progress.set_total(new_gz_count as u64);

    let (new_logs, gz_errors) = match parse_logs(std::mem::take(&mut all_new_gz_logs), progress) {
        Ok(value) => value,
        Err(err) => {
            progress.finish();
            return Err(Box::new(err));
        }
    };

    progress.finish();

    // --------------------------------------------------------
    // STEP 7: Parse changed latest.log files
    // --------------------------------------------------------

    progress.set_message("Parsing latest logs");
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

    // --------------------------------------------------------
    // STEP 8: Update latest metadata
    // --------------------------------------------------------

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

    // --------------------------------------------------------
    // STEP 9: Read existing logs
    // --------------------------------------------------------

    let zst_path = get_zst_path(name)?;

    progress.set_message("Reading existing logs");
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

    // --------------------------------------------------------
    // STEP 10: Update base metadata
    // --------------------------------------------------------

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

    // --------------------------------------------------------
    // STEP 11: Write logs
    // --------------------------------------------------------

    progress.set_message("Writing logs");
    progress.set_total(total_lines as u64);

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

// ------------------------------------------------------------
// Other utilities
// ------------------------------------------------------------

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
    use zstd::stream::read::Decoder;

    let file = File::open(zst_path)?;
    let decoder = Decoder::new(file)?;
    let reader = BufReader::new(decoder);

    let mut logs = Vec::new();

    for line in reader.lines() {
        let line = line?;

        if line.len() < 13 {
            progress.inc(1);
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

