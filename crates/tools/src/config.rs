use serde_json::Value;
use std::fs;
use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    let mut path = PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string()));
    path.push("LogTools");
    path.push("config.json");
    path
}

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

pub fn save_config(map: &serde_json::Map<String, Value>) -> Result<(), std::io::Error> {
    if let Some(parent) = config_path().parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        config_path(),
        serde_json::to_string_pretty(&Value::Object(map.clone()))?,
    )
}

pub fn get_config_param(param: &str) -> Option<String> {
    load_config()
        .get(param)
        .and_then(|v| v.as_str())
        .map(String::from)
}
