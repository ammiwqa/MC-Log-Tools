use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

pub fn load_log(name: &str) -> Result<HashMap<String, Value>, String> {
    let appdata = env::var("APPDATA")
        .map_err(|_| "APPDATA not found.".to_string())?;

    let mut path = PathBuf::from(appdata);
    path.push("logtools3");
    path.push("bases");
    path.push(name);
    path.push(format!("{}.json", name));

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Err file: {}", e))?;

    let json: HashMap<String, Value> = serde_json::from_str(&content)
        .map_err(|e| format!("Parse JSON Err: {}", e))?;

    Ok(json)
}


pub fn get_zst_path(name: &str) -> Result<String, String> {
    let appdata = env::var("APPDATA")
        .map_err(|_| "APPDATA not found.".to_string())?;

    let mut path = PathBuf::from(appdata);
    path.push("logtools3");
    path.push("bases");
    path.push(name);
    path.push(format!("{}.zst", name));

    Ok(path.to_string_lossy().to_string())
}