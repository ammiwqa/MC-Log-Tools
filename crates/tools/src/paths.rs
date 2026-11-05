// Data directory for LogTools config and state.
// On Windows: %APPDATA%\LogTools
// On macOS: ~/Library/Application Support/LogTools
// On Linux: ~/.config/logtools (respecting XDG_CONFIG_HOME)

use std::env;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = env::var("LOGTOOLS_DATA_DIR") {
        return PathBuf::from(dir);
    }

    #[cfg(windows)]
    {
        let appdata = env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("LogTools")
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("LogTools")
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("logtools")
        } else {
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config").join("logtools")
        }
    }
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn state_path() -> PathBuf {
    data_dir().join("state.json")
}
