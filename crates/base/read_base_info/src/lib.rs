use std::{collections::HashMap, env, fs, path::PathBuf};

use serde_json::Value;

pub fn load_log(name: &str) -> Result<HashMap<String, Value>, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA not found.".to_string())?;

    let mut path = PathBuf::from(appdata);
    path.push("LogTools");
    path.push("bases");
    path.push(name);
    path.push(format!("{}.json", name));

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("Error! Wrong BataBase name!");
            std::process::exit(1);
        }
    };

    let json: HashMap<String, Value> = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(_) => {
            eprintln!("Broken JSON DataBase file!");
            std::process::exit(1);
        }
    };
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
