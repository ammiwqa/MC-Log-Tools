use std::sync::Arc;
use std::env;
use indicatif::{ProgressBar, ProgressStyle};

use serde_json::json;
use std::fs;

use console::{Style, style};
use std::path::PathBuf;

use crate::log_parser;
use crate::get_logfiles;
use crate::zip_writer;


pub fn create_base(
    paths: Vec<String>,
    name: String,
)   {


    let mut all_logs: Vec<String> = Vec::new();
    let mut paths_json: Vec<serde_json::Value> = Vec::new();
    let mut latest_params: Vec<(String, usize)> = Vec::new();

    for path in &paths {
        let np = get_logfiles::find_log_files(path, true, ".log.gz".to_string()).unwrap();
        let np_short_list = get_logfiles::find_log_files(path, false, ".log.gz".to_string()).unwrap();

        let json_path = json!({path: {"latest": [{"hash": "", "last_line": "",}], "files": np_short_list}});

        let files = get_logfiles::find_log_files(path, true, ".log".to_string()).unwrap();
        latest_params.extend(
            files.into_iter().map(|f| (f, 0))
        );

        paths_json.push(json_path);
        all_logs.extend(np);
    }

    let all_logs_len = all_logs.len();

    let bright_cyan_style = Style::new().cyan().bold();

    let pb = Arc::new(ProgressBar::new(all_logs_len as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Parsing")));

    let mut logs = log_parser::parse_logs(all_logs, &pb).unwrap();

    pb.finish_and_clear();

    let lines_first = logs.len();
    let success_msg = style("   Parsing").green().bold();
    println!("{} {} files -> {} lines", success_msg, all_logs_len, lines_first);


    let latest_params_len =  latest_params.len();
    let pb_latest = Arc::new(ProgressBar::new(latest_params_len as u64));
    pb_latest.set_style(
        ProgressStyle::default_bar()
            .template("   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb_latest.set_prefix(format!("{}", bright_cyan_style.apply_to("Parsing latest")));

    let (logs_latest, _params) = log_parser::parse_latest(latest_params, &pb_latest).unwrap();
    let lines_latest = logs_latest.len();

    pb_latest.finish_and_clear();

    let success_msg = style("   Parsing latest").green().bold();
    println!("{} {} files -> {} lines", success_msg, latest_params_len, lines_latest);


    logs.extend(logs_latest);
    logs.sort_unstable_by_key(|x| x.0);

    let lines = logs.len();


    let json_value = json!({
        "name":   name,
        "lines":  lines,
        "hash":   "",

        "tags":   [],
        "custom": "custom",

        "time": {
            "creation_unix": "",
            "edit_unix":     "",
            "FLD_unix":      "",
            "LLD_unix":      "",
        },

        "paths": paths_json
    });


    let appdata = env::var("APPDATA").expect("No APPDATA env variable");
    let mut base_path = PathBuf::from(appdata);
    base_path.push("LogTools3");
    base_path.push("bases");
    base_path.push(&name);
    fs::create_dir_all(&base_path).unwrap();


    let file_path = base_path.join(format!("{}.json", &name));
    fs::write(file_path, serde_json::to_string_pretty(&json_value).unwrap()).unwrap();


    let pb2 = Arc::new(ProgressBar::new(logs.len() as u64));

    pb2.set_style(
        ProgressStyle::default_bar()
            .template("   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb2.set_prefix(format!("{}", bright_cyan_style.apply_to("Writing")));



    let file_path = base_path.join(format!("{}.log.gz", &name)).display().to_string();
    let _ = zip_writer::write_logs_to_zstd(&logs, &file_path, pb2);

    pb.finish_and_clear();
    let success_msg = style("   Writing").green().bold();
    println!("{} {} lines -> {}", success_msg, lines, name);
}