use chrono::{DateTime, Local};
use std::fs;
use thiserror::Error;

use std::env;
use std::path::PathBuf;

#[derive(Error, Debug, PartialEq)]
pub enum FilenameError {
    #[error("DB name cant be empty")]
    Empty,

    #[error("DB name cant contain char: '{0}'")]
    ForbiddenChar(char),

    #[error("BD name cant end with a space or a period")]
    EndsWithSpaceOrDot,

    #[error("DB name '{0}' is a reserved Windows device name")]
    ReservedName(String),

    #[error("DB name too long (max {max} characters, received {actual})")]
    TooLong { max: usize, actual: usize },
}

pub fn validate_windows_filename(filename: &str) -> Result<(), FilenameError> {
    if filename.is_empty() {
        return Err(FilenameError::Empty);
    }

    if filename.len() > 255 {
        return Err(FilenameError::TooLong {
            max: 255,
            actual: filename.len(),
        });
    }

    let forbidden_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

    if let Some(bad_char) = filename.chars().find(|c| forbidden_chars.contains(c)) {
        return Err(FilenameError::ForbiddenChar(bad_char));
    }

    if filename.ends_with(' ') || filename.ends_with('.') {
        return Err(FilenameError::EndsWithSpaceOrDot);
    }

    let reserved_names = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let name_without_ext = filename.split('.').next().unwrap_or("");
    let upper_name = name_without_ext.to_uppercase();

    if reserved_names.contains(&upper_name.as_str()) {
        return Err(FilenameError::ReservedName(name_without_ext.to_string()));
    }

    Ok(())
}

pub fn format_filename_error(filename: &str, error: &FilenameError) -> String {
    match error {
        FilenameError::Empty => "DB name cant be empty".to_string(),

        FilenameError::ForbiddenChar(c) => format!(
            "DB name '{}' contains forbidden char '{}'.\n\
                     You can use: letters, numbers, spaces, hyphens, underscores and dots.\n\
                     Forbidden: < > : \" / \\ | ? *",
            filename, c
        ),

        FilenameError::EndsWithSpaceOrDot => {
            format!("BD name '{}' cannot end with a space or a dot", filename)
        }

        FilenameError::ReservedName(name) => format!(
            "DB name '{}' is a reserved Windows device name.\n\
					Reserved names: CON, PRN, AUX, NUL, COM1-COM9, LPT1-LPT9",
            name
        ),

        FilenameError::TooLong { max, actual } => format!(
            "DB name is too long ({} characters).\n\
					Maximum length: {} characters",
            actual, max
        ),
    }
}

pub fn file_modified_days_local(path: &str) -> std::io::Result<i64> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;

    let dt: DateTime<Local> = modified.into();

    let date = dt.date_naive();

    Ok(date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() / 86_400)
}

pub fn get_valid_file_name(input: &str) -> String {
    format!(
        "{}.txt",
        input
            .replace("|", "OR")
            .replace("*", "...")
            .replace("?", ".")
            .replace(":", " ")
            .replace("/", " ")
            .replace("\\", " ")
            .replace("\"", " ")
            .replace("<", "str(")
            .replace(">", ")")
    )
}

pub fn get_results_file_path(file_name: &str) -> String {
    let appdata = env::var("APPDATA").expect("APPDATA not found.");

    let mut path = PathBuf::from(appdata);
    path.push("logtools");
    path.push("results");

    fs::create_dir_all(&path).unwrap();

    path.push(file_name);

    let path_string = path.to_string_lossy().to_string();

    path_string
}
