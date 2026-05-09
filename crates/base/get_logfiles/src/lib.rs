use std::fs;
use std::path::Path;

pub fn find_log_files(
    path: &str,
    full_path: bool,
    file_end: &str,
    mode: bool,
) -> Vec<String> {
    let dir_path = Path::new(path);

    // если папки нет — просто пустой результат
    if !dir_path.is_dir() {
        return Vec::new();
    }

    let mut result = Vec::new();

    let Ok(entries) = fs::read_dir(dir_path) else {
        return Vec::new();
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };

        let file_path = entry.path();

        if file_path.is_file() {
            let Some(file_name) = file_path.file_name() else {
                continue;
            };

            let file_name_str = file_name.to_string_lossy();

            let matches = if mode {
                file_name_str.as_ref() == file_end
            } else {
                file_name_str.ends_with(file_end)
            };

            if matches {
                if full_path {
                    if let Some(path_str) = file_path.to_str() {
                        result.push(path_str.to_string());
                    }
                } else {
                    result.push(file_name_str.to_string());
                }
            }
        }
    }

    result
}