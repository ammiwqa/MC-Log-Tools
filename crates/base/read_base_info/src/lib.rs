use std::{collections::HashMap, env, fs, path::PathBuf};

use serde_json::Value;

pub fn load_log(name: &str) -> Result<HashMap<String, Value>, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA not found.".to_string())?;

    let mut path = PathBuf::from(appdata);
    path.push("LogTools");
    path.push("bases");
    path.push(name);
    path.push(format!("{}.json", name));

    let content = fs::read_to_string(&path)
        .map_err(|_| format!("Base '{}' not found: {}", name, path.display()))?;

    let json: HashMap<String, Value> =
        serde_json::from_str(&content).map_err(|e| format!("Broken JSON base file: {}", e))?;
    Ok(json)
}

pub fn get_zst_path(name: &str) -> Result<String, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA not found.".to_string())?;

    let mut path = PathBuf::from(appdata);
    path.push("LogTools");
    path.push("bases");
    path.push(name);
    path.push(format!("{}.zst", name));

    Ok(path.to_string_lossy().to_string())
}

#[derive(Debug, Clone)]
pub struct BaseInfo {
    pub name: String,
    pub lines: usize,
    pub paths_count: usize,
    pub creation_time: String,
    pub last_edit_time: String,
}

pub fn list_bases() -> Result<Vec<BaseInfo>, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA not found.".to_string())?;

    let mut bases_path = PathBuf::from(appdata);
    bases_path.push("LogTools");
    bases_path.push("bases");

    if !bases_path.exists() {
        return Ok(Vec::new());
    }

    let mut bases = Vec::new();

    let entries = fs::read_dir(&bases_path).map_err(|e| format!("Failed to read bases dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let json_path = path.join(format!("{}.json", name));

            if json_path.exists() {
                if let Ok(content) = fs::read_to_string(&json_path) {
                    if let Ok(json) = serde_json::from_str::<Value>(&content) {
                        let lines = json.get("lines").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        
                        let paths_count = json.get("paths")
                            .and_then(|p| p.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);

                        let creation_time = json.get("time")
                            .and_then(|t| t.get("creation_unix"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let last_edit_time = json.get("time")
                            .and_then(|t| t.get("edit_unix"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        bases.push(BaseInfo {
                            name,
                            lines,
                            paths_count,
                            creation_time,
                            last_edit_time,
                        });
                    }
                }
            }
        }
    }

    bases.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(bases)
}

fn format_unix_time(ts: &str) -> String {
    if ts.is_empty() {
        return "—".to_string();
    }
    match ts.parse::<i64>() {
        Ok(unix_ts) => {
            use chrono::{DateTime, Local, TimeZone};
            let dt: DateTime<Local> = Local.timestamp_opt(unix_ts, 0).single().unwrap_or_default();
            dt.format("%Y-%m-%d %H:%M").to_string()
        }
        Err(_) => "—".to_string(),
    }
}

pub fn print_bases_table(bases: &[BaseInfo]) {
    if bases.is_empty() {
        println!("No bases found.");
        return;
    }

    const NAME_W: usize = 25;
    const LINES_W: usize = 12;
    const PATHS_W: usize = 8;
    const TIME_W: usize = 20;

    let header = format!(
        "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
        "NAME", "LINES", "PATHS", "CREATED", "EDITED",
        NAME_W = NAME_W, LINES_W = LINES_W, PATHS_W = PATHS_W, TIME_W = TIME_W
    );

    let separator = "─".repeat(header.len());

    println!("{}", header);
    println!("{}", separator);

    for base in bases {
        let name = if base.name.len() > NAME_W {
            format!("{}…", &base.name[..NAME_W - 1])
        } else {
            base.name.clone()
        };

        let created = format_unix_time(&base.creation_time);
        let edited = format_unix_time(&base.last_edit_time);

        println!(
            "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
            name,
            base.lines,
            base.paths_count,
            created,
            edited,
            NAME_W = NAME_W, LINES_W = LINES_W, PATHS_W = PATHS_W, TIME_W = TIME_W
        );
    }

    println!("{}", separator);
    println!("Total: {} base(s)", bases.len());
}
