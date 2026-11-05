use serde_json::Value;
use std::fs;

use crate::paths;

pub fn state_path() -> std::path::PathBuf {
    paths::state_path()
}

pub fn load_state() -> serde_json::Map<String, Value> {
    let content = match fs::read_to_string(state_path()) {
        Ok(c) => c,
        Err(_) => return serde_json::Map::new(),
    };

    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => map,
        _ => {
            eprintln!(
                "Warning: broken state file, ignoring ({})",
                state_path().display()
            );
            serde_json::Map::new()
        }
    }
}

pub fn save_state(map: &serde_json::Map<String, Value>) -> Result<(), std::io::Error> {
    if let Some(parent) = state_path().parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        state_path(),
        serde_json::to_string_pretty(&Value::Object(map.clone()))?,
    )
}

pub fn get_state_param(param: &str) -> Option<String> {
    load_state()
        .get(param)
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn save_state_param(param: &str, value: &str) -> Result<(), std::io::Error> {
    let mut map = load_state();
    map.insert(param.to_string(), Value::String(value.to_string()));
    save_state(&map)
}

pub const STATE_PARAMS: &[&str] = &["last_search_base", "last_search_text"];

pub fn is_state_param(param: &str) -> bool {
    STATE_PARAMS.contains(&param)
}

pub fn migrate_from_config() -> Result<(), std::io::Error> {
    let mut config = crate::config::load_config();
    let mut state = load_state();
    let mut config_changed = false;
    let mut state_changed = false;

    for key in STATE_PARAMS {
        if let Some(value) = config.remove(*key) {
            state.insert(key.to_string(), value);
            config_changed = true;
            state_changed = true;
        }
    }

    if config_changed {
        crate::config::save_config(&config)?;
    }
    if state_changed {
        save_state(&state)?;
    }

    Ok(())
}
