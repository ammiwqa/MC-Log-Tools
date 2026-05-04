use std::fs;
use std::path::Path;

/// Находит все .log.gz файлы в указанной директории (без рекурсивного обхода)
///
/// # ARGS
/// * `dir_path` - путь к директории для поиска

pub fn find_log_gz_files(dir_path: &str, full_path: bool) -> Result<Vec<String>, std::io::Error> {
    let path = Path::new(dir_path);

    // Проверяем, существует ли директория
    if !path.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Directory '{}' not found", dir_path),
        ));
    }

    let mut result = Vec::new();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_path = entry.path();

        if file_path.is_file() {
            if let Some(file_name) = file_path.file_name() {
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.ends_with(".log.gz") {
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
    }

    Ok(result)
}