//! Integration tests for create_base.

use create_base::{create_base, CreateOptions, CreateResult};
use flate2::write::GzEncoder;
use progress::ParseProgress;
use read_base_info::load_log;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
        Self {
            _guard: guard,
            previous,
        }
    }
}

impl Drop for AppDataScope {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => {
                let _ = unsafe { std::env::set_var("APPDATA", v) };
            }
            None => {
                let _ = unsafe { std::env::remove_var("APPDATA") };
            }
        }
    }
}

fn temp_root() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lt-createbase-{}-{}",
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

fn make_latest_log(root: &Path, lines: &[String]) -> PathBuf {
    let path = root.join("latest.log");
    let mut file = fs::File::create(&path).unwrap();
    for line in lines {
        writeln!(file, "{}", line).unwrap();
    }
    path
}

fn make_corrupt_gz(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, "this is definitely not gzip data").unwrap();
    path
}

fn make_bad_date_gz(root: &Path) -> PathBuf {
    let path = root.join("not-a-date.log.gz");
    let file = fs::File::create(&path).unwrap();
    let mut encoder = GzEncoder::new(file, flate2::Compression::default());
    writeln!(encoder, "[12:00:00] [CHAT] should be skipped").unwrap();
    encoder.finish().unwrap();
    path
}

fn chat(ts: &str, msg: &str) -> String {
    format!("{} [CHAT] {}", ts, msg)
}

fn list_zst(base_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(base_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|e| e == "zst").unwrap_or(false) {
                out.push(p.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }
    out
}

#[test]
fn create_base_basic_single_folder() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    make_gz_log(&root, "2024-01-15", &[chat("[12:34:56]", "hello world"), chat("[13:00:00]", "second line")]);
    make_latest_log(&root, &[chat("[09:00:00]", "latest first")]);

    let result: CreateResult = create_base(vec![root.to_string_lossy().to_string()], "base_basic".to_string(), &progress()).unwrap();

    assert_eq!(result.gz_files, 1);
    assert_eq!(result.parsed_gz_lines, 2);
    assert_eq!(result.latest_files, 1);
    assert_eq!(result.parsed_latest_lines, 1);
    assert_eq!(result.input_lines, 3);
    assert_eq!(result.total_lines, 3);
    assert_eq!(result.duplicate_files, 0);
    assert_eq!(result.gz_errors.file_errors, 0);
    assert_eq!(result.gz_errors.parse_errors, 0);
    assert_eq!(result.latest_errors.file_errors, 0);
    assert_eq!(result.latest_errors.parse_errors, 0);

    let base_dir = read_base_info::base_dir("base_basic").unwrap();
    let zsts = list_zst(&base_dir);
    assert!(!zsts.is_empty(), "at least one chunk should be written");

    let cfg = load_log("base_basic").unwrap();
    assert_eq!(cfg.name, "base_basic");
    assert_eq!(cfg.lines, 3);
    assert_eq!(cfg.paths.len(), 1);
    assert_eq!(cfg.paths[0].path, root.to_string_lossy());
    assert!(!cfg.hash.is_empty());
}

#[test]
fn create_base_multiple_folders_multiple_latest_logs() {
    let root_a = temp_root().join("folder_a");
    let root_b = temp_root().join("folder_b");
    let _scope = AppDataScope::enter(&temp_root().join("controller"));
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    make_gz_log(&root_a, "2024-01-15", &[chat("[10:00:00]", "a one")]);
    make_latest_log(&root_a, &[chat("[08:00:00]", "a latest")]);
    make_gz_log(&root_b, "2024-01-16", &[chat("[11:00:00]", "b one"), chat("[12:00:00]", "b two")]);
    make_latest_log(&root_b, &[chat("[09:00:00]", "b latest")]);

    let result: CreateResult = create_base(
        vec![
            root_a.to_string_lossy().to_string(),
            root_b.to_string_lossy().to_string(),
        ],
        "base_multi".to_string(),
        &progress(),
    )
    .unwrap();

    assert_eq!(result.gz_files, 2);
    assert_eq!(result.parsed_gz_lines, 3);
    assert_eq!(result.latest_files, 2);
    assert_eq!(result.parsed_latest_lines, 2);
    assert_eq!(result.total_lines, 5);

    let cfg = load_log("base_multi").unwrap();
    assert_eq!(cfg.lines, 5);
    assert_eq!(cfg.paths.len(), 2);
}

#[test]
fn create_base_duplicate_gz_by_content_is_ignored() {
    let root_a = temp_root().join("folder_a");
    let root_b = temp_root().join("folder_b");
    let _scope = AppDataScope::enter(&temp_root().join("controller"));
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    let lines = &[chat("[10:00:00]", "duplicate content")];
    make_gz_log(&root_a, "2024-01-15", lines);
    make_gz_log(&root_b, "2024-01-15", lines);

    let result: CreateResult = create_base(
        vec![
            root_a.to_string_lossy().to_string(),
            root_b.to_string_lossy().to_string(),
        ],
        "base_dup_gz".to_string(),
        &progress(),
    )
    .unwrap();

    assert_eq!(result.gz_files, 1);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.duplicate_files, 1);

    let cfg = load_log("base_dup_gz").unwrap();
    assert_eq!(cfg.lines, 1);
}

#[test]
fn create_base_duplicate_latest_path_is_ignored() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    make_gz_log(&root, "2024-01-15", &[chat("[10:00:00]", "only once")]);
    make_latest_log(&root, &[chat("[08:00:00]", "latest")]);

    let result: CreateResult = create_base(
        vec![
            root.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
        ],
        "base_dup_latest".to_string(),
        &progress(),
    )
    .unwrap();

    assert_eq!(result.latest_files, 1);
    assert_eq!(result.parsed_latest_lines, 1);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.duplicate_files, 2);
    assert_eq!(result.total_lines, 2);
}

#[test]
fn create_base_empty_folders_produce_empty_base() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    fs::create_dir_all(root.join("empty1")).unwrap();
    fs::create_dir_all(root.join("empty2")).unwrap();

    let result: CreateResult =
        create_base(vec![root.to_string_lossy().to_string()], "base_empty".to_string(), &progress()).unwrap();

    assert_eq!(result.gz_files, 0);
    assert_eq!(result.latest_files, 0);
    assert_eq!(result.total_lines, 0);
    assert_eq!(result.input_lines, 0);

    let cfg = load_log("base_empty").unwrap();
    assert_eq!(cfg.lines, 0);
    assert_eq!(cfg.chunks.len(), 0);
}

#[test]
fn create_base_corrupt_gz_counts_file_error_and_continues() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    make_gz_log(&root, "2024-01-15", &[chat("[10:00:00]", "good line")]);
    make_corrupt_gz(&root, "corrupt.log.gz");

    let result: CreateResult =
        create_base(vec![root.to_string_lossy().to_string()], "base_corrupt".to_string(), &progress()).unwrap();

    assert_eq!(result.gz_files, 2);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.gz_errors.file_errors, 1, "corrupt gz should be a file error");
    assert_eq!(result.total_lines, 1);

    let cfg = load_log("base_corrupt").unwrap();
    assert_eq!(cfg.lines, 1);
}

#[test]
fn create_base_gz_with_bad_date_name_counts_file_error() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    make_gz_log(&root, "2024-01-15", &[chat("[10:00:00]", "good line")]);
    make_bad_date_gz(&root);

    let result: CreateResult =
        create_base(vec![root.to_string_lossy().to_string()], "base_bad_date".to_string(), &progress()).unwrap();

    assert_eq!(result.gz_files, 2);
    assert_eq!(result.parsed_gz_lines, 1);
    assert_eq!(result.gz_errors.file_errors, 1, "unparseable date name should error");
}

#[test]
fn create_base_unpredictable_symbols_in_raw_logs_do_not_panic() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    let path = root.join("2024-01-15.log.gz");
    let file = fs::File::create(&path).unwrap();
    let mut encoder = GzEncoder::new(file, flate2::Compression::default());
    writeln!(encoder, "{}", chat("[10:00:00]", "valid line")).unwrap();
    writeln!(encoder, "{}", "[10:00:02] no chat tag here".to_string()).unwrap();
    writeln!(encoder, "{}", format!("{} [CHAT] \x00\x01 control chars", "[10:00:03]")).unwrap();
    writeln!(encoder, "{}", chat("[10:00:04]", "utf-8 \u{0440}\u{0438}\u{0432}\u{0435}\u{0442}")).unwrap();
    encoder
        .write_all(b"[10:00:05] [CHAT] \x01\xff\x02 unpredictable\n")
        .unwrap();
    encoder.finish().unwrap();

    let result: CreateResult = create_base(
        vec![root.to_string_lossy().to_string()],
        "base_bad_symbols".to_string(),
        &progress(),
    )
    .unwrap();

    assert!(result.parsed_gz_lines >= 1);
    assert_eq!(result.gz_errors.file_errors, 0);

    let cfg = load_log("base_bad_symbols").unwrap();
    assert!(cfg.lines >= 1);
}

#[test]
fn create_base_options_sanitized_replaces_zero_with_defaults() {
    let root = temp_root();
    let _scope = AppDataScope::enter(&root);

    make_gz_log(&root, "2024-01-15", &[chat("[10:00:00]", "sanitized")]);
    make_latest_log(&root, &[chat("[08:00:00]", "latest")]);

    let options = CreateOptions {
        chunk_lines: 0,   // -> default
        sort_run_lines: 0, // -> default
    }
    .sanitized();
    assert!(options.chunk_lines > 0);
    assert!(options.sort_run_lines > 0);

    let result: CreateResult =
        create_base_with_options(vec![root.to_string_lossy().to_string()], "base_sanitized".to_string(), options, &progress()).unwrap();

    assert_eq!(result.total_lines, 2);
}

#[test]
fn create_base_nonexistent_root_yields_empty_base() {
    let _scope = AppDataScope::enter(&temp_root());

    let result: CreateResult = create_base(
        vec!["/definitely/does/not/exist".to_string()],
        "base_nonexistent".to_string(),
        &progress(),
    )
    .unwrap();

    assert_eq!(result.gz_files, 0);
    assert_eq!(result.latest_files, 0);
    assert_eq!(result.total_lines, 0);

    let cfg = load_log("base_nonexistent").unwrap();
    assert_eq!(cfg.lines, 0);
}

use create_base::create_base_with_options;
