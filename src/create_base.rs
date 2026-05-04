use std::sync::Arc;
use indicatif::{ProgressBar, ProgressStyle};

use serde_json::json;
use std::fs;

use crate::log_parser;
use crate::get_logfiles;
use crate::zip_writer;


pub fn create_base(
    paths: Vec<String>,
    name: String,
)   {


    let mut all_logs: Vec<String> = Vec::new();
    let mut paths_json: Vec<serde_json::Value> = Vec::new();

    for path in &paths {
        let np = get_logfiles::find_log_gz_files(path, true).unwrap();
        let np_short_list = get_logfiles::find_log_gz_files(path, false).unwrap();

        let json_path = json!({path: {"latest": [{"hash": "", "last_line": "",}], "files": np_short_list}});

        paths_json.push(json_path);
        all_logs.extend(np);
    }

    let pb = Arc::new(ProgressBar::new(all_logs.len() as u64));

    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {wide_bar} {pos}/{len} ({eta})")
            .unwrap(),
    );

    let logs = log_parser::parse_logs(all_logs, pb).unwrap();
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

    fs::write("data.json", serde_json::to_string_pretty(&json_value).unwrap()).unwrap();

    let _ = zip_writer::write_logs_to_zstd(&logs, "output.log.gz");
}