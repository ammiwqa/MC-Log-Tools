use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use console::{Style, style};
use log_parser::ErrorStats;

fn format_error_stats(stats: &ErrorStats) -> String {
    match (stats.file_errors, stats.parse_errors) {
        (0, 0) => String::new(),
        (f, 0) => format!("({} file errors)", f),
        (0, p) => format!("({} parse errors)", p),
        (f, p) => format!("({} file errors, {} parse errors)", f, p),
    }
}

fn get_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .to_string()
}

pub fn create_base(paths: Vec<String>, name: String) {
    let mut all_logs: Vec<String> = Vec::new();
    let mut paths_json: Vec<serde_json::Value> = Vec::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();

    // =========================
    // SNAPSHOT FOR JSON
    // =========================
    let mut files_snapshot: HashMap<String, Vec<String>> = HashMap::new();

    for path in &paths {
        let np = get_logfiles::find_log_files(path, true, ".log.gz", false);
        let np_short_list = get_logfiles::find_log_files(path, false, ".log.gz", false);

        let latest_files = get_logfiles::find_log_files(path, true, "latest.log", true);

        files_snapshot.insert(path.clone(), np_short_list);

        latest_params.extend(latest_files.into_iter().map(|f| (f, 0)));

        all_logs.extend(np);
    }

    let all_logs_len = all_logs.len();

    // =========================
    // PROGRESS BAR (logs)
    // =========================
    let bright_cyan_style = Style::new().cyan().bold();

    let pb = Arc::new(ProgressBar::new(all_logs_len as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
            )
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Parsing")));

    let (mut logs, errors) = log_parser::parse_logs(all_logs, &pb).unwrap();

    pb.finish_and_clear();

    println!(
        "{} {} files -> {} lines {}",
        style("   Parsing").green().bold(),
        all_logs_len,
        logs.len(),
        format_error_stats(&errors)
    );

    // =========================
    // PROGRESS BAR (latest logs)
    // =========================
    let latest_params_len = latest_params.len();

    let pb_latest = Arc::new(ProgressBar::new(latest_params_len as u64));
    pb_latest.set_style(
        ProgressStyle::default_bar()
            .template(
                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
            )
            .unwrap()
            .progress_chars("=>-"),
    );
    pb_latest.set_prefix(format!("{}", bright_cyan_style.apply_to("Parsing latest")));

    let (logs_latest, params, latest_errors) =
        log_parser::parse_latest(latest_params, &pb_latest).unwrap();

    pb_latest.finish_and_clear();

    println!(
        "{} {} files -> {} lines {}",
        style("   Parsing latest").green().bold(),
        latest_params_len,
        logs_latest.len(),
        format_error_stats(&latest_errors)
    );

    // =========================
    // latest json gen
    // =========================
    let mut latest_map: HashMap<String, serde_json::Value> = HashMap::new();

    for (path, lines, hash) in params {
        let dir = get_dir(&path);

        latest_map.insert(
            dir,
            json!({
                "hash": hash,
                "last_line": lines
            }),
        );
    }

    // =========================
    // BUILD JSON
    // =========================
    for path in &paths {
        let latest = latest_map.get(path).cloned().unwrap_or(json!({}));

        let files = files_snapshot.get(path).cloned().unwrap_or_default();

        paths_json.push(json!({
            path: {
                "latest": latest,
                "files": files
            }
        }));
    }

    // =========================
    // FINAL MERGE
    // =========================
    logs.extend(logs_latest);
    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();

    let json_value = json!({
        "name": name,
        "lines": lines,
        "hash": "",

        "tags": [],
        "custom": "custom",

        "time": {
            "creation_unix": "",
            "edit_unix": "",
            "FLD_unix": "",
            "LLD_unix": ""
        },

        "paths": paths_json
    });

    // =========================
    // WRITE LT3 BASE CONF
    // =========================
    let appdata = env::var("APPDATA").expect("No APPDATA env variable");

    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools");
    base_path.push("bases");
    base_path.push(&name);

    fs::create_dir_all(&base_path).unwrap();

    let file_path = base_path.join(format!("{}.json", &name));
    fs::write(
        file_path,
        serde_json::to_string_pretty(&json_value).unwrap(),
    )
    .unwrap();

    // =========================
    // WRITE LOGS
    // =========================
    let pb_writing = Arc::new(ProgressBar::new(logs.len() as u64));

    pb_writing.set_style(
        ProgressStyle::default_bar()
            .template(
                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
            )
            .unwrap()
            .progress_chars("=>-"),
    );

    pb_writing.set_prefix(format!("{}", bright_cyan_style.apply_to("Writing")));

    let file_path = base_path
        .join(format!("{}.zst", &name))
        .display()
        .to_string();
    let _ = zip_writer::write_logs_to_zstd(&logs, &file_path, &pb_writing);

    pb_writing.finish_and_clear();

    println!(
        "{} {} lines -> {}",
        style("   Writing").green().bold(),
        lines,
        name
    );
}
