use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lt_core::AppError;
use lt_core::discovery::{self, DiscoverSource};
use lt_core::events::CoreEvent;
pub use lt_core::operations;
pub use lt_core::types::BaseInfo;

// =========================
// PRESENTER
// =========================

pub trait Presenter {
    fn progress(&mut self, pos: u64, total: u64);

    fn message(&mut self, msg: &str);

    fn log(&mut self, line: &str);

    fn error(&mut self, msg: &str);

    fn confirm(&mut self, prompt: &str) -> bool;

    fn open_file(&mut self, path: &PathBuf);

    fn search_completed(&mut self, path: &PathBuf) {
        // Default behaviour: open the results file in the system app.
        // GUI overrides this to not auto-open (results are shown in a dedicated window instead).
        self.open_file(path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Ok,
    Err(String),
    ExitCode(i32),
}

const POLL_INTERVAL: Duration = Duration::from_millis(10);

// =========================
// Helpers
// =========================

fn normalize_path_arg(raw: &str) -> &str {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(trimmed);
    trimmed.trim()
}

pub fn read_paths_from_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<String>, std::io::Error> {
    let raw = file_path.as_ref().to_string_lossy().to_string();
    let cleaned = normalize_path_arg(&raw).to_string();
    let content = fs::read_to_string(&cleaned)?;

    let paths: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect();

    Ok(paths)
}

fn format_error_counts(file_errors: usize, parse_errors: usize) -> String {
    let mut parts = Vec::new();

    if file_errors > 0 {
        parts.push(format!("{} file errors", file_errors));
    }

    if parse_errors > 0 {
        parts.push(format!("{} parse errors", parse_errors));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("({})", parts.join(", "))
    }
}

fn format_error_stats(stats: &lt_core::ErrorStats) -> String {
    format_error_counts(stats.file_errors, stats.parse_errors)
}

fn format_unix_time(ts: &str) -> String {
    if ts.is_empty() {
        return "—".to_string();
    }
    match ts.parse::<i64>() {
        Ok(unix_ts) => {
            use chrono::{DateTime, Local, TimeZone};
            let dt: DateTime<Local> = Local.timestamp_opt(unix_ts, 0).single().unwrap_or_default();
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        Err(_) => "—".to_string(),
    }
}

pub fn format_bases_table(bases: &[BaseInfo]) -> String {
    if bases.is_empty() {
        return "No bases found.".to_string();
    }

    const NAME_W: usize = 25;
    const LINES_W: usize = 12;
    const PATHS_W: usize = 8;
    const TIME_W: usize = 20;

    let header = format!(
        "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
        "NAME",
        "LINES",
        "PATHS",
        "CREATED",
        "EDITED",
        NAME_W = NAME_W,
        LINES_W = LINES_W,
        PATHS_W = PATHS_W,
        TIME_W = TIME_W
    );

    let separator: String = std::iter::repeat('─')
        .take(header.chars().count())
        .collect();
    let mut out = Vec::new();

    out.push(header.clone());
    out.push(separator.clone());

    for base in bases {
        let name = if base.name.chars().count() > NAME_W {
            let short: String = base.name.chars().take(NAME_W - 1).collect();
            format!("{}…", short)
        } else {
            base.name.clone()
        };

        out.push(format!(
            "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
            name,
            base.lines,
            base.paths_count,
            format_unix_time(&base.creation_time),
            format_unix_time(&base.last_edit_time),
            NAME_W = NAME_W,
            LINES_W = LINES_W,
            PATHS_W = PATHS_W,
            TIME_W = TIME_W
        ));
    }

    out.push(separator);
    out.push(format!("Total: {} base(s)", bases.len()));

    out.join("\n")
}

fn pump_events(rx: &std::sync::mpsc::Receiver<CoreEvent>, presenter: &mut dyn Presenter) {
    while let Ok(ev) = rx.try_recv() {
        match ev {
            CoreEvent::Progress { pos, total } => presenter.progress(pos, total),
            CoreEvent::Message(msg) => presenter.message(&msg),
            CoreEvent::DedupCompleted { duplicate_files } => {
                if duplicate_files > 0 {
                    presenter.log(&format!(
                        "   Dedup {} duplicate files skipped",
                        duplicate_files
                    ));
                }
            }
            CoreEvent::GzipParsingCompleted {
                files,
                lines,
                file_errors,
                parse_errors,
            } => {
                if files > 0 {
                    presenter.log(&format!(
                        "   Parsing {} files -> {} lines {}",
                        files,
                        lines,
                        format_error_counts(file_errors, parse_errors)
                    ));
                }
            }
            CoreEvent::LatestParsingCompleted {
                files,
                lines,
                file_errors,
                parse_errors,
            } => {
                if files > 0 {
                    presenter.log(&format!(
                        "   Parsing latest {} files -> {} lines {}",
                        files,
                        lines,
                        format_error_counts(file_errors, parse_errors)
                    ));
                }
            }
            CoreEvent::WritingCompleted { lines } => {
                presenter.log(&format!("   Writing {} lines written", lines));
            }
        }
    }
}

// =========================
// Commands
// =========================

pub fn run_create_base(
    paths: Vec<String>,
    from_file: Option<String>,
    name: String,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    let options = operations::default_create_options();
    run_create_base_with_options(paths, from_file, name, options, presenter)
}

/// Resolves create options from optional CLI overrides plus the config file.
pub fn resolve_create_options(
    chunk_lines: Option<u64>,
    sort_run_lines: Option<u64>,
) -> operations::CreateOptions {
    let mut options = operations::default_create_options();

    if let Some(value) = chunk_lines.filter(|value| *value > 0) {
        options.chunk_lines = value;
    }

    if let Some(value) = sort_run_lines.filter(|value| *value > 0) {
        options.sort_run_lines = value;
    }

    options.sanitized()
}

pub fn run_create_base_with_options(
    paths: Vec<String>,
    from_file: Option<String>,
    name: String,
    options: operations::CreateOptions,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    let mut all_paths = paths;

    if let Some(file_path) = from_file {
        match read_paths_from_file(&file_path) {
            Ok(file_paths) => all_paths.extend(file_paths),
            Err(e) => {
                presenter.error(&format!("Ошибка чтения файла '{}': {}", file_path, e));
                return CommandResult::ExitCode(1);
            }
        }
    }

    if all_paths.is_empty() {
        presenter.error("No paths");
        return CommandResult::ExitCode(1);
    }

    if name.is_empty() {
        return CommandResult::Ok;
    }

    if let Err(e) = tools::validate_windows_filename(&name) {
        presenter.log(&format!("\n{}", tools::format_filename_error(&name, &e)));
        return CommandResult::Ok;
    }

    presenter.message("Parsing");
    let (tx, rx) = operations::event_channel();
    let name_clone = name.clone();

    let handle =
        std::thread::spawn(move || operations::create_base_with_options(all_paths, name_clone, options, tx));

    while !handle.is_finished() {
        pump_events(&rx, presenter);
        std::thread::sleep(POLL_INTERVAL);
    }

    pump_events(&rx, presenter);

    match handle
        .join()
        .unwrap_or_else(|_| Err(AppError::Other("create_base thread panicked".to_string())))
    {
        Ok(result) => {
            presenter.log(&format!(
                "   Writing {} lines -> {}",
                result.total_lines, name
            ));

            if let Some(bytes) = result.peak_memory_bytes {
                presenter.log(&format!(
                    "   Peak RAM {:.1} MiB",
                    bytes as f64 / (1024.0 * 1024.0)
                ));
            }

            CommandResult::Ok
        }
        Err(e) => {
            let msg = format!("Error creating base: {}", e);
            presenter.error(&msg);
            CommandResult::Err(msg)
        }
    }
}

pub fn run_search(base_name: String, text: String, presenter: &mut dyn Presenter) -> CommandResult {
    run_search_date_range(base_name, text, None, None, false, presenter)
}

pub fn run_search_date_range(
    base_name: String,
    text: String,
    date_from: Option<i64>,
    date_to: Option<i64>,
    use_date_filtering: bool,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    if base_name.is_empty() || text.is_empty() {
        return CommandResult::Ok;
    }

    let exit_file_name = tools::get_valid_file_name_with_timestamp(&text);
    let exit_file_path = tools::get_results_file_path(&exit_file_name);

    let search_handle = match operations::start_search_date_range(
        &base_name,
        &text,
        date_from,
        date_to,
        use_date_filtering,
    ) {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("Error starting search: {}", e);
            presenter.error(&msg);
            return CommandResult::Err(msg);
        }
    };

    let max_lines = search_handle.progress.get_max_progress();
    presenter.message("Searching");

    loop {
        presenter.progress(search_handle.progress.get_progress(), max_lines);

        if search_handle.handle.is_finished() {
            break;
        }

        std::thread::sleep(POLL_INTERVAL);
    }

    let results = match operations::wait_search(search_handle) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Search failed: {}", e);
            presenter.error(&msg);
            return CommandResult::Err(msg);
        }
    };

    let results_len = results.len();

    presenter.progress(max_lines, max_lines);
    presenter.log(&format!(
        "   Searching {} lines -> {} founds",
        max_lines, results_len
    ));

    let write_job = write_result::write_results_async(results, exit_file_path.clone());
    presenter.message("Writing");

    let write_total = write_job.progress.get_total();

    loop {
        presenter.progress(write_job.progress.get_written(), write_total);

        if write_job.handle.is_finished() {
            break;
        }

        std::thread::sleep(POLL_INTERVAL);
    }

    if let Err(e) = write_job.handle.join() {
        let msg = format!("Error writing results: {:?}", e);
        presenter.error(&msg);
        return CommandResult::Err(msg);
    }

    presenter.progress(write_total, write_total);
    presenter.log(&format!(
        "   Writing {} founds -> ({})",
        results_len, exit_file_path
    ));
    presenter.log(&format!("   Results file: {}", exit_file_path));

    let path = PathBuf::from(&exit_file_path);
    presenter.search_completed(&path);

    CommandResult::Ok
}

pub fn read_context_around(
    name: &str,
    target_ts: i64,
    n_before: usize,
    n_after: usize,
) -> Result<Vec<(i64, String)>, String> {
    operations::read_context_around(name, target_ts, n_before, n_after).map_err(|e| e.to_string())
}

pub fn run_update_base(name: String, presenter: &mut dyn Presenter) -> CommandResult {
    if name.is_empty() {
        return CommandResult::Ok;
    }

    presenter.message("Updating");
    let (tx, rx) = operations::event_channel();
    let name_clone = name.clone();

    let handle = std::thread::spawn(move || operations::update_base(&name_clone, tx));

    while !handle.is_finished() {
        pump_events(&rx, presenter);
        std::thread::sleep(POLL_INTERVAL);
    }

    pump_events(&rx, presenter);

    match handle
        .join()
        .unwrap_or_else(|_| Err(AppError::Other("update_base thread panicked".to_string())))
    {
        Ok(result) => {
            if result.no_new_files {
                presenter.log("   Update No new logs to parse");
                return CommandResult::Ok;
            }

            if result.new_gz_files > 0 {
                presenter.log(&format!(
                    "   Parsing {} new gz files -> {} lines {}",
                    result.new_gz_files,
                    result.parsed_gz_lines,
                    format_error_stats(&result.gz_errors)
                ));
            }

            if result.new_latest_files > 0 {
                presenter.log(&format!(
                    "   Parsing latest {} latest files -> {} lines {}",
                    result.new_latest_files,
                    result.parsed_latest_lines,
                    format_error_stats(&result.latest_errors)
                ));
            }

            presenter.log(&format!(
                "   Writing {} total lines -> {}",
                result.total_lines, name
            ));
            CommandResult::Ok
        }
        Err(e) => {
            let msg = format!("Error updating base: {}", e);
            presenter.error(&msg);
            CommandResult::Err(msg)
        }
    }
}

pub fn run_list_bases(presenter: &mut dyn Presenter) -> CommandResult {
    match operations::list_bases() {
        Ok(bases) => {
            presenter.log(&format_bases_table(&bases));
            CommandResult::Ok
        }
        Err(e) => {
            let msg = format!("Error listing bases: {}", e);
            presenter.error(&msg);
            CommandResult::Err(msg)
        }
    }
}

pub fn run_config(
    param: Option<String>,
    var: Option<String>,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    match (param, var) {
        (None, _) => {
            let map = tools::config::load_config();

            if map.is_empty() {
                presenter.log("No config parameters set.");
            } else {
                for (k, v) in &map {
                    presenter.log(&format!(
                        "{:<20} {}",
                        k,
                        v.as_str().unwrap_or(&v.to_string())
                    ));
                }
            }
            CommandResult::Ok
        }

        (Some(param), None) => match tools::config::get_config_param(&param) {
            Some(value) => {
                presenter.log(&value);
                CommandResult::Ok
            }
            None => {
                let msg = format!("Parameter '{}' is not set", param);
                presenter.error(&msg);
                CommandResult::Err(msg)
            }
        },

        (Some(param), Some(var)) => {
            let mut map = tools::config::load_config();

            map.insert(
                param.clone(),
                serde_json::Value::String(var.trim().to_string()),
            );

            match tools::config::save_config(&map) {
                Ok(()) => {
                    presenter.log(&format!("   Config {} -> \"{}\"", param, var));
                    CommandResult::Ok
                }
                Err(e) => {
                    let msg = format!("Error saving config: {}", e);
                    presenter.error(&msg);
                    CommandResult::ExitCode(1)
                }
            }
        }
    }
}

pub fn run_delete(name: String, force_yes: bool, presenter: &mut dyn Presenter) -> CommandResult {
    let confirm_enabled = tools::config::get_config_param("confirm_delete")
        .map(|v| v != "false" && v != "0" && v != "no")
        .unwrap_or(true);

    if confirm_enabled && !force_yes {
        let prompt = format!("   Delete base '{}' (config + logs)? [y/N]", name);

        if !presenter.confirm(&prompt) {
            presenter.log("   Cancelled");
            return CommandResult::Ok;
        }
    }

    match operations::delete_base(&name) {
        Ok(()) => {
            presenter.log(&format!("   Deleted {}", name));
            CommandResult::Ok
        }
        Err(e) => {
            let msg = format!("Error deleting base: {}", e);
            presenter.error(&msg);
            CommandResult::Err(msg)
        }
    }
}

pub fn run_discover(
    root: Option<String>,
    out: Option<String>,
    es: Option<String>,
    no_everything: bool,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    let out_path = out.unwrap_or_else(|| {
        tools::get_results_file_path(&tools::get_valid_file_name("mc_log_dirs"))
    });

    let start = std::time::Instant::now();
    presenter.message("Discovery");
    let mut discovery_result = None;

    if !no_everything {
        match discovery::discover_everything(es.as_ref()) {
            Ok(d) => {
                let source_str = match d.source {
                    DiscoverSource::Sdk => "Everything SDK".to_string(),
                    DiscoverSource::Cli => "Everything CLI".to_string(),
                    DiscoverSource::Walk => unreachable!(),
                };

                presenter.log(&format!("   Scanning {}", source_str));
                discovery_result = Some(d.dirs);
            }
            Err(e) => {
                if es.is_some() {
                    presenter.log(&format!("   Everything unavailable: {}", e));
                }
            }
        }
    }

    let found = match discovery_result {
        Some(mut dirs) => {
            if let Some(root_str) = root {
                if !root_str.trim().is_empty() {
                    let root = PathBuf::from(&root_str);
                    if root.is_dir() {
                        dirs.retain(|d| {
                            let dir = PathBuf::from(d);
                            dir.starts_with(&root) || dir == root
                        });
                    }
                }
            }
            dirs
        }

        None => {
            let scan_root = match root {
                Some(r) => PathBuf::from(r),
                None => match std::env::var("USERPROFILE") {
                    Ok(home) => PathBuf::from(home),
                    Err(_) => {
                        presenter.error("Cannot determine user home directory");
                        return CommandResult::ExitCode(1);
                    }
                },
            };

            if !scan_root.is_dir() {
                let msg = format!("Not a directory: {}", scan_root.display());
                presenter.error(&msg);
                return CommandResult::ExitCode(1);
            }

            presenter.log(&format!("   Scanning {}", scan_root.display()));

            if !no_everything {
                presenter.log(
                    "   Tip: install Everything (https://www.voidtools.com/) and Everything SDK for an instant whole-PC scan.\n   Then set the DLL or es.exe path: logtools config everything_dll \"<path to Everything64.dll or es.exe>\"",
                );
            }

            discovery::discover_walk(&scan_root)
        }
    };

    if let Err(e) = fs::write(&out_path, found.join("\n")) {
        let msg = format!("Error writing output file: {}", e);
        presenter.error(&msg);
        return CommandResult::ExitCode(1);
    }

    presenter.log(&format!(
        "   Discovered {} log directories -> ({}) in {:.1}s",
        found.len(),
        out_path,
        start.elapsed().as_secs_f32()
    ));

    presenter.log(&format!("   Results file: {}", out_path));
    presenter.log(&format!(
        "   Create a global base: logtools cb <NAME> --from-file \"{}\"",
        out_path
    ));

    CommandResult::Ok
}
