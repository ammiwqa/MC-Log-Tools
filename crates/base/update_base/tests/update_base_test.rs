//! Integration tests for [`update_base`].
//!
//! These tests simulate the lifecycle of a base: create it once, then run
//! `update_base` repeatedly while adding new `.log.gz` files and modifying
//! `latest.log`. They cover: no-op updates, newly discovered gzip files,
//! unchanged-vs-changed `latest.log` (hash comparison), corrupt gzip files,
//! and base hash / line count evolution.
//!
//! `APPDATA` is overridden to a temp directory for the whole test body.

use flate2::write::GzEncoder;
use hash_utils::compute_file_hash;
use progress::ParseProgress;
use read_base_info::{BaseConfig, LatestInfo, PathEntry};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use update_base::update_base;

static APPDATA_MUTEX: Mutex<()> = Mutex::new(());

struct AppDataScope {
    _guard: std::sync::MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl AppDataScope {
    fn enter(temp_root: &Path) -> Self {
        let guard: std::sync::MutexGuard<'static, ()> =
            APPDATA_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("APPDATA").ok();
        unsafe { std::env::set_var("APPDATA", temp_root) };
        Self { _guard: guard, previous }
    }
}

impl Drop for AppDataScope {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => { let _ = unsafe { std::env::set_var("APPDATA", v) }; }
            None => { let _ = unsafe { std::env::remove_var("APPDATA") }; }
        }
    }
}

fn temp_root() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lt-updatebase-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::create_dir_all(&path);
    path
}

fn progress() -> ParseProgress {
    ParseProgress::new()
}

fn chat(ts: &str, msg: &str) -> String {
    format!("{} [CHAT] {}", ts, msg)
}

fn make_gz_log(root: &Path, date: &str, lines: &[String]) -> PathBuf {
    let path = root.join(format!("{}.log.gz", date));
    let file = fs::File::create(&path).unwrap();
    let mut encoder = GzEncoder::new(file, flate2::Compression::default());
    for line in lines {
        writeln!(encoder, "{}", line).unwrap();
    }
    encoder.finish().unwrap();
    path
}

fn write_latest(root: &Path, lines: &[String]) -> PathBuf {
    let path = root.join("latest.log");
    let mut file = fs::File::create(&path).unwrap();
    for line in lines {
        writeln!(file, "{}", line).unwrap();
    }
    path
}

/// Creates an initial base with a few stored chunks and a `latest.log`
/// whose hash/last_line are already recorded, so the first update can be a
/// no-op when nothing changed.
fn create_initial_base(
    name: &str,
    root: &Path,
    latest_lines: &[String],
    initial_lines: &[(i64, &str)],
) -> (u64, u64) {
    let latest_path = write_latest(root, latest_lines);
    let latest_hash = compute_file_hash(&latest_path).unwrap();
    let last_line = latest_lines.len() as u64;

    let mut sorted: Vec<(i64, String)> =
        initial_lines.iter().map(|(t, s)| (*t, s.to_string())).collect();
    sorted.sort_unstable_by_key(|x| x.0);

    let chunks = base_chunks::write_chunks_with_size(name, &sorted, 500_000, &progress()).unwrap();
    let total_lines: u64 = chunks.iter().map(|c| c.lines).sum();

    let cfg = BaseConfig {
        name: name.to_string(),
        lines: total_lines,
        hash: base_chunks::base_hash(&chunks),
        tags: Vec::new(),
        custom: "custom".to_string(),
        time: read_base_info::TimeInfo {
            creation_unix: "1000".to_string(),
            edit_unix: "1000".to_string(),
            fld_unix: chunks.first().map(|c| c.first_ts.to_string()).unwrap_or_default(),
            lld_unix: chunks.last().map(|c| c.last_ts.to_string()).unwrap_or_default(),
        },
        paths: vec![PathEntry {
            path: root.to_string_lossy().to_string(),
            files: Vec::new(),
            ignored: Vec::new(),
            latest: serde_json::to_value(LatestInfo {
                hash: latest_hash,
                last_line,
            })
            .unwrap(),
        }],
        chunks: chunks.clone(),
        chunk_lines: read_base_info::DEFAULT_CHUNK_LINES,
    };
    read_base_info::save_log(name, &cfg).unwrap();
    (total_lines, last_line)
}

#[test]
fn update_base_no_changes_is_a_noop() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    write_latest(&root, &[chat("[10:00:00]", "stable")]);
    let (_initial, _last) = create_initial_base("upd_noop", &root, &[chat("[10:00:00]", "stable")], &[]);

    let result = update_base("upd_noop", &progress()).unwrap();

    assert!(result.no_new_files);
    assert_eq!(result.new_gz_files, 0);
    assert_eq!(result.new_latest_files, 0);
    assert_eq!(result.total_new_lines, 0);
    assert_eq!(result.total_lines, 0); // base had no initial chunks
    assert_eq!(result.gz_errors.file_errors, 0);
    assert_eq!(result.latest_errors.file_errors, 0);
}

#[test]
fn update_base_new_gz_file_is_parsed_and_added() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    write_latest(&root, &[chat("[10:00:00]", "stable")]);
    let (initial, _) = create_initial_base("upd_new_gz", &root, &[chat("[10:00:00]", "stable")], &[(1000, "initial")]);

    // Add a brand new gzip file after the base exists.
    make_gz_log(&root, "2024-01-16", &[chat("[11:00:00]", "new gz line")]);

    let result = update_base("upd_new_gz", &progress()).unwrap();

    assert!(!result.no_new_files);
    assert_eq!(result.new_gz_files, 1);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.total_new_lines, 1);
    assert_eq!(result.total_lines, initial as usize + 1);
    assert_eq!(result.gz_errors.file_errors, 0);
}

#[test]
fn update_base_unchanged_latest_is_skipped() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    let latest_lines = &[chat("[10:00:00]", "stable")];
    write_latest(&root, latest_lines);
    let (_initial, _) = create_initial_base("upd_same_latest", &root, latest_lines, &[]);

    // latest.log content identical -> hash identical -> must be skipped.
    let result = update_base("upd_same_latest", &progress()).unwrap();

    assert_eq!(result.new_latest_files, 0);
    assert_eq!(result.parsed_latest_lines, 0);
    assert_eq!(result.latest_errors.file_errors, 0);
}

#[test]
fn update_base_modified_latest_is_reparsed() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    let initial_latest = &[chat("[10:00:00]", "before")];
    write_latest(&root, initial_latest);
    let (_initial, _) = create_initial_base("upd_mod_latest", &root, initial_latest, &[]);

    // Append new lines to latest.log -> hash changes -> re-parsed.
    write_latest(&root, &[chat("[10:00:00]", "before"), chat("[11:00:00]", "after one"), chat("[12:00:00]", "after two")]);

    let result = update_base("upd_mod_latest", &progress()).unwrap();

    assert_eq!(result.new_latest_files, 1);
    assert_eq!(result.parsed_latest_lines, 2, "only the appended lines should be new");
    assert_eq!(result.total_new_lines, 2);
    assert_eq!(result.total_lines, 2);
}

#[test]
fn update_base_corrupt_gz_is_counted_and_does_not_stop() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    write_latest(&root, &[chat("[10:00:00]", "stable")]);
    let (_initial, _) = create_initial_base("upd_corrupt", &root, &[chat("[10:00:00]", "stable")], &[]);

    make_gz_log(&root, "2024-01-16", &[chat("[11:00:00]", "good gz")]);
    // Corrupt gzip next to it (find_log_files scans the top-level directory only).
    fs::write(root.join("bad.log.gz"), "not gzip at all").unwrap();

    let result = update_base("upd_corrupt", &progress()).unwrap();

    assert_eq!(result.new_gz_files, 2, "both discovered gz files are kept (the corrupt one is still parsed)");
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.gz_errors.file_errors, 1, "corrupt gz is a file error");
    assert_eq!(result.total_new_lines, 1);
}

#[test]
fn update_base_duplicate_gz_content_ignored_within_one_update() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    write_latest(&root, &[chat("[10:00:00]", "stable")]);
    let (_initial, _) = create_initial_base("upd_dup_gz", &root, &[chat("[10:00:00]", "stable")], &[]);

    let lines = &[chat("[11:00:00]", "same payload")];
    make_gz_log(&root, "2024-01-16", lines);
    make_gz_log(&root, "2024-01-17", lines); // same size + same payload -> duplicate

    let result = update_base("upd_dup_gz", &progress()).unwrap();

    assert_eq!(result.new_gz_files, 1);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.total_new_lines, 1);
}

#[test]
fn update_base_base_hash_changes_after_adding_data() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    write_latest(&root, &[chat("[10:00:00]", "stable")]);
    let (_initial, _) = create_initial_base("upd_hash", &root, &[chat("[10:00:00]", "stable")], &[(1000, "initial")]);

    let cfg_before = read_base_info::load_log("upd_hash").unwrap();
    let hash_before = cfg_before.hash.clone();

    make_gz_log(&root, "2024-01-16", &[chat("[11:00:00]", "new line")]);
    let _result = update_base("upd_hash", &progress()).unwrap();

    let cfg_after = read_base_info::load_log("upd_hash").unwrap();
    assert_ne!(hash_before, cfg_after.hash, "base hash must change when data is added");
    assert_eq!(cfg_after.lines, 2);
    assert!(!cfg_after.time.edit_unix.is_empty(), "edit time must be set");
}

#[test]
fn update_base_nonexistent_base_name_is_error() {
    let _scope = AppDataScope::enter(&temp_root());

    let err = update_base("definitely_not_a_base", &progress()).unwrap_err();
    assert!(err.to_string().contains("not found"), "error should mention missing base: {err}");
}
