//! Integration tests for [`search`].
//!
//! Bases are built directly with [`base_chunks::write_chunks_with_size`] and
//! [`read_base_info::save_log`], so no `.log.gz` parsing is needed here — we
//! test the query language, result ordering, date filtering and context
//! reading on already-stored chunks.
//!
//! `APPDATA` is overridden to a temp directory for the whole test body.

use base_chunks::write_chunks_with_size;
use progress::ParseProgress;
use read_base_info::{BaseConfig, TimeInfo};
use search::{read_context_around, search_async};
use std::fs;
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
        "lt-search-{}-{}",
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

/// Writes `logs` (one chunk per line for full date-range control) as a base
/// called `name`, saves its config, and returns the total line count.
fn make_base(name: &str, logs: &[(i64, &str)]) -> usize {
    let mut sorted: Vec<(i64, String)> = logs.iter().map(|(t, s)| (*t, s.to_string())).collect();
    sorted.sort_unstable_by_key(|x| x.0);

    // chunk_lines = 1 -> one chunk per line, giving distinct ts ranges.
    let chunks = write_chunks_with_size(name, &sorted, 1, &progress()).unwrap();

    let total_lines: usize = chunks.iter().map(|c| c.lines as usize).sum();

    let cfg = BaseConfig {
        name: name.to_string(),
        lines: total_lines as u64,
        hash: base_chunks::base_hash(&chunks),
        tags: Vec::new(),
        custom: "custom".to_string(),
        time: TimeInfo {
            creation_unix: "1000".to_string(),
            edit_unix: "1000".to_string(),
            fld_unix: chunks.first().map(|c| c.first_ts.to_string()).unwrap_or_default(),
            lld_unix: chunks.last().map(|c| c.last_ts.to_string()).unwrap_or_default(),
        },
        paths: Vec::new(),
        chunks,
        chunk_lines: read_base_info::DEFAULT_CHUNK_LINES,
    };
    read_base_info::save_log(name, &cfg).unwrap();
    total_lines
}

fn ts(hour: u64) -> i64 {
    // 2024-01-15 00:00:00 UTC + hour hours.
    1705276800 + (hour * 3600) as i64
}

fn run_search(name: &str, query: &str) -> Vec<(u64, String)> {
    let (_progress, handle) = search_async(name, query).expect("query should compile");
    handle.join().expect("search thread should not panic").expect("search should succeed")
}

#[test]
fn search_simple_pattern_matches() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("pat", &[
        (ts(10), "player1 said hello"),
        (ts(11), "player2 said bye"),
        (ts(12), "server started"),
    ]);

    let res = run_search("pat", "*hello*");
    assert_eq!(res.len(), 1);
    assert!(res[0].1.contains("hello"));
}

#[test]
fn search_pattern_case_insensitive() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("case", &[(ts(10), "Hello World")]);

    let lower = run_search("case", "*hello*");
    let upper = run_search("case", "*HELLO*");
    let mixed = run_search("case", "*HeLlO*");
    assert_eq!(lower.len(), 1);
    assert_eq!(upper.len(), 1);
    assert_eq!(mixed.len(), 1);
}

#[test]
fn search_pattern_no_match_returns_empty() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("nomatch", &[(ts(10), "player1 said hello")]);

    let res = run_search("nomatch", "*zzzznomatch*");
    assert!(res.is_empty());
}

#[test]
fn search_and_requires_both_parts() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("and", &[
        (ts(10), "player1 said hello"),
        (ts(11), "player2 said hello"),
        (ts(12), "player1 said bye"),
    ]);

    let res = run_search("and", "*player1*&*hello*");
    assert_eq!(res.len(), 1);
    assert!(res[0].1.contains("player1"));
    assert!(res[0].1.contains("hello"));
}

#[test]
fn search_or_matches_either_side() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("or", &[
        (ts(10), "alpha message"),
        (ts(11), "beta message"),
        (ts(12), "gamma message"),
    ]);

    let res = run_search("or", "*alpha*|*gamma*");
    assert_eq!(res.len(), 2);
    let texts: Vec<&str> = res.iter().map(|(_, t)| t.as_str()).collect();
    assert!(texts.iter().any(|t| t.contains("alpha")));
    assert!(texts.iter().any(|t| t.contains("gamma")));
    assert!(!texts.iter().any(|t| t.contains("beta")));
}

#[test]
fn search_not_excludes_matching_lines() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("not", &[
        (ts(10), "player1 said hello"),
        (ts(11), "player2 said hello"),
        (ts(12), "player1 said bye"),
    ]);

    // NOT only binds as a unary prefix after `&` in this grammar:
    // `*hello* & - *player1*` = hello AND NOT player1 -> only player2 hello.
    let res = run_search("not", "*hello* & - *player1*");
    assert_eq!(res.len(), 1);
    assert!(res[0].1.contains("player2"));

    let res2 = run_search("not", "*hello* & ! *player1*");
    assert_eq!(res2.len(), 1);
    assert!(res2[0].1.contains("player2"));

    // A bare trailing `-` operator is not a valid infix operator here; the
    // query is still accepted (trailing tokens are ignored) and matches all
    // lines with "hello".
    let res3 = run_search("not", "*hello* - *player1*");
    assert_eq!(res3.len(), 2);
}

#[test]
fn search_grouping_changes_precedence() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("group", &[
        (ts(10), "player1 said hello"),
        (ts(11), "player2 said hello"),
        (ts(12), "player1 said bye"),
        (ts(13), "player3 said bye"),
    ]);

    // (player1 OR player2) AND hello  -> player1 hello AND player2 hello
    let res = run_search("group", "(*player1*|*player2*) & *hello*");
    assert_eq!(res.len(), 2);
    assert!(res.iter().any(|(_, t)| t.contains("player1")));
    assert!(res.iter().any(|(_, t)| t.contains("player2")));

    // player1 OR (player2 AND hello) -> player1 hello + player1 bye + player2 hello
    let res2 = run_search("group", "*player1* | *player2* & *hello*");
    assert_eq!(res2.len(), 3);
}

#[test]
fn search_angle_bracket_pattern() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("angle", &[(ts(10), "some [CHAT] text here")]);

    let res = run_search("angle", "<[CHAT]>");
    assert_eq!(res.len(), 1);

    let res2 = run_search("angle", "<nomatch>");
    assert!(res2.is_empty());
}

#[test]
fn search_empty_query_matches_everything() {
    let _scope = AppDataScope::enter(&temp_root());
    let n = make_base("empty", &[
        (ts(10), "a"),
        (ts(11), "b"),
        (ts(12), "c"),
    ]);

    let res = run_search("empty", "");
    assert_eq!(res.len(), n);
}

#[test]
fn search_invalid_query_is_rejected() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("badq", &[(ts(10), "x")]);

    for bad in ["&", "|", "-", "(", ")", "*player1* &", "(&)"] {
        let err = match search_async("badq", bad) {
            Err(e) => e,
            Ok(_) => panic!("query {:?} should be invalid", bad),
        };
        assert!(!err.is_empty(), "query {:?} should be invalid", bad);
    }
}

#[test]
fn search_results_are_sorted_by_timestamp() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("sorted", &[
        (ts(12), "c late"),
        (ts(10), "a early"),
        (ts(11), "b middle"),
    ]);

    let res = run_search("sorted", "*");
    assert_eq!(res.len(), 3);
    assert_eq!(res[0].0, ts(10) as u64);
    assert_eq!(res[1].0, ts(11) as u64);
    assert_eq!(res[2].0, ts(12) as u64);
    assert!(res[0].1.contains("a early"));
    assert!(res[1].1.contains("b middle"));
    assert!(res[2].1.contains("c late"));
}

#[test]
fn search_date_range_filters_by_timestamp() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("dates", &[
        (ts(10), "ten o clock"),
        (ts(11), "eleven o clock"),
        (ts(12), "twelve o clock"),
        (ts(13), "thirteen o clock"),
    ]);

    let from = ts(11);
    let to = ts(12);
    let (_progress, handle) = search_async_date_range(
        "dates", "*", Some(from), Some(to), true,
    ).unwrap();
    let res = handle.join().unwrap().unwrap();
    assert_eq!(res.len(), 2);
    assert!(res.iter().all(|(t, _)| *t >= from as u64 && *t <= to as u64));
}

#[test]
fn search_date_range_without_filtering_scans_all() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("dates2", &[
        (ts(10), "ten"),
        (ts(20), "twenty"),
    ]);

    let (_progress, handle) = search_async_date_range("dates2", "*", Some(ts(15)), Some(ts(16)), false).unwrap();
    let res = handle.join().unwrap().unwrap();
    // use_date_filtering=false ignores the range entirely.
    assert_eq!(res.len(), 2);
}

#[test]
fn search_date_range_no_overlap_returns_empty() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("dates3", &[(ts(10), "only line")]);

    let (_progress, handle) = search_async_date_range("dates3", "*", Some(ts(20)), Some(ts(30)), true).unwrap();
    let res = handle.join().unwrap().unwrap();
    assert!(res.is_empty());
}

#[test]
fn search_date_range_from_only() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("fromonly", &[(ts(10), "a"), (ts(11), "b"), (ts(12), "c")]);

    let (_progress, handle) = search_async_date_range("fromonly", "*", Some(ts(11)), None, true).unwrap();
    let res = handle.join().unwrap().unwrap();
    assert_eq!(res.len(), 2);
    assert!(res.iter().all(|(t, _)| *t >= ts(11) as u64));
}

#[test]
fn search_context_around_returns_neighbors() {
    let _scope = AppDataScope::enter(&temp_root());

    // All four lines in a single chunk so context can span neighbours.
    let sorted: Vec<(i64, String)> = vec![
        (ts(10), "first".to_string()),
        (ts(11), "second".to_string()),
        (ts(12), "third".to_string()),
        (ts(13), "fourth".to_string()),
    ];
    let chunks = base_chunks::write_chunks("ctx", &sorted, &progress()).unwrap();
    let total_lines: u64 = chunks.iter().map(|c| c.lines).sum();
    let cfg = BaseConfig {
        name: "ctx".to_string(),
        lines: total_lines,
        hash: base_chunks::base_hash(&chunks),
        tags: Vec::new(),
        custom: "custom".to_string(),
        time: TimeInfo {
            creation_unix: "1000".to_string(),
            edit_unix: "1000".to_string(),
            fld_unix: chunks.first().map(|c| c.first_ts.to_string()).unwrap_or_default(),
            lld_unix: chunks.last().map(|c| c.last_ts.to_string()).unwrap_or_default(),
        },
        paths: Vec::new(),
        chunks,
        chunk_lines: read_base_info::DEFAULT_CHUNK_LINES,
    };
    read_base_info::save_log("ctx", &cfg).unwrap();

    let ctx = read_context_around("ctx", ts(12), 1, 1).unwrap();
    assert_eq!(ctx.len(), 3);
    assert_eq!(ctx[0].1, "second");
    assert_eq!(ctx[1].1, "third");
    assert_eq!(ctx[2].1, "fourth");
}

#[test]
fn search_context_around_missing_timestamp_is_empty() {
    let _scope = AppDataScope::enter(&temp_root());
    let _n = make_base("ctx2", &[(ts(10), "only")]);

    let ctx = read_context_around("ctx2", ts(99), 2, 2).unwrap();
    assert!(ctx.is_empty());
}

// `search_async_date_range` is defined in the same crate; import it here so it
// can be used before its `use` declaration point (use items are resolved
// module-wide in Rust).
use search::search_async_date_range;
