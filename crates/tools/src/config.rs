use serde_json::Value;
use std::fs;

use crate::paths;

pub fn config_path() -> std::path::PathBuf {
    paths::config_path()
}

/// Load user config file.
pub fn load_config() -> serde_json::Map<String, Value> {
    let content = match fs::read_to_string(config_path()) {
        Ok(c) => c,
        Err(_) => return serde_json::Map::new(),
    };

    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => map,
        _ => {
            eprintln!(
                "Warning: broken config file, ignoring ({})",
                config_path().display()
            );
            serde_json::Map::new()
        }
    }
}

/// Save user config file.
pub fn save_config(map: &serde_json::Map<String, Value>) -> Result<(), std::io::Error> {
    if let Some(parent) = config_path().parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        config_path(),
        serde_json::to_string_pretty(&Value::Object(map.clone()))?,
    )
}

/// Get a config par as a string.
pub fn get_config_param(param: &str) -> Option<String> {
    load_config()
        .get(param)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Set a config par to a string value.
pub fn save_config_param(param: &str, value: &str) -> Result<(), std::io::Error> {
    let mut map = load_config();
    map.insert(param.to_string(), Value::String(value.to_string()));
    save_config(&map)
}

/// Names of the config pars that are supported
pub const KNOWN_PARAMS: &[&str] = &[
    "everything_dll",
    "confirm_delete",
    "chunk_lines",
    "sort_run_lines",
];

/// Check whether a parm name is a known config parm.
pub fn is_known_param(param: &str) -> bool {
    KNOWN_PARAMS.contains(&param)
}
