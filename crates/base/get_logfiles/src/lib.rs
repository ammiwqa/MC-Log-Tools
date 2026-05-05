use std::fs;
use std::path::Path;

/// Ищет файлы в директории по заданным критериям
///
/// # Аргументы
/// * `path` - путь к директории
/// * `full_path` - если true, возвращает полный путь, иначе только имя файла
/// * `file_end` - строка для поиска (имя файла или окончание)
/// * `mode` - если true, ищет точное совпадение имени файла, иначе проверяет окончание
///
/// # Возвращает
/// * `Result<Vec<String>, std::io::Error>` - вектор найденных файлов или ошибку
pub fn find_log_files(
    path: &str,
    full_path: bool,
    file_end: &str,
    mode: bool,
) -> Result<Vec<String>, std::io::Error> {
    let dir_path = Path::new(path);

    if !dir_path.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Directory '{}' not found", path),
        ));
    }

    let mut result = Vec::new();

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let file_path = entry.path();

        if file_path.is_file() {
            if let Some(file_name) = file_path.file_name() {
                let file_name_str = file_name.to_string_lossy();

                let matches = if mode {
                    // Точное совпадение имени файла
                    file_name_str.as_ref() == file_end
                } else {
                    // Проверка окончания файла
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
    }

    Ok(result)
}