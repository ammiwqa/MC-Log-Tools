use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverSource {
    Sdk,
    Cli,
    Walk,
}

const DISCOVER_SKIP_DIRS: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "windows",
    "program files",
    "program files (x86)",
    "node_modules",
    ".git",
    "cache",
    "caches",
    "libraries",
    "versions",
    "assets",
];

pub fn has_mc_logs(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.ends_with(".log.gz") || name == "latest.log" {
                        return true;
                    }
                }
            }
            false
        }
        Err(_) => false,
    }
}

fn collect_dir(dir: &Path, found: &mut Vec<String>) {
    if has_mc_logs(dir) {
        found.push(dir.to_string_lossy().to_string());
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_lowercase();

        if DISCOVER_SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        subdirs.push(entry.path());
    }

    for subdir in subdirs {
        collect_dir(&subdir, found);
    }
}

pub fn discover_walk(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect_dir(root, &mut found);
    found
}

// =========================
// Everything CLI ...
// =========================

pub fn find_es(explicit: Option<&String>) -> Option<String> {
    if let Some(p) = explicit {
        if Path::new(p).is_file() {
            return Some(p.clone());
        }
    }

    if let Ok(out) = std::process::Command::new("where").arg("es.exe").output() {
        if out.status.success() {
            if let Some(first) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let p = first.trim();
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
    }

    for candidate in ["C:\\Program Files\\Everything\\es.exe"] {
        if Path::new(candidate).is_file() {
            return Some(candidate.to_string());
        }
    }

    None
}

fn dirs_from_paths<I: IntoIterator<Item = String>>(paths: I) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for line in paths {
        let path = Path::new(&line);

        let Some(parent) = path.parent() else {
            continue;
        };

        if !seen.insert(parent.to_path_buf()) || !parent.is_dir() {
            continue;
        }

        if has_mc_logs(parent) {
            dirs.push(parent.to_string_lossy().to_string());
        }
    }

    dirs
}
pub fn discover_es(es: &str) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();

    for query in ["*.log.gz", "latest.log"] {
        let res = std::process::Command::new(es).arg(query).output()?;

        if !res.status.success() {
            return Err(std::io::Error::other(format!(
                "es.exe failed with status {}",
                res.status
            )));
        }

        out.extend(dirs_from_paths(
            String::from_utf8_lossy(&res.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty()),
        ));
    }

    Ok(out)
}

// =========================
// Everything SDK DLL
// =========================

#[cfg(windows)]
pub mod everything_sdk {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub struct Everything {
        set_search_w: unsafe extern "system" fn(*const u16),
        query_w: unsafe extern "system" fn(i32) -> i32,
        get_num_results: unsafe extern "system" fn() -> u32,
        get_result_full_path_name_w: unsafe extern "system" fn(u32, *mut u16, u32) -> u32,
        get_last_error: unsafe extern "system" fn() -> u32,
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn from_wide(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    impl Everything {
        pub fn load(dll_path: &Path) -> Option<Self> {
            let wide = to_wide(&dll_path.to_string_lossy());
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                eprintln!("Failed to load {}", dll_path.display());
                return None;
            }

            macro_rules! proc {
                ($name:expr) => {{
                    let sym = concat!($name, "\0");
                    let addr = unsafe { GetProcAddress(module, sym.as_ptr()) };
                    if addr.is_null() {
                        eprintln!("Symbol {} missing in Everything64.dll", $name);
                        return None;
                    }
                    unsafe { std::mem::transmute(addr) }
                }};
            }

            Some(Self {
                set_search_w: proc!("Everything_SetSearchW"),
                query_w: proc!("Everything_QueryW"),
                get_num_results: proc!("Everything_GetNumResults"),
                get_result_full_path_name_w: proc!("Everything_GetResultFullPathNameW"),
                get_last_error: proc!("Everything_GetLastError"),
            })
        }

        pub fn query_paths(&self, search: &str) -> std::io::Result<Vec<String>> {
            unsafe {
                (self.set_search_w)(to_wide(search).as_ptr());

                if (self.query_w)(1) == 0 {
                    let err = (self.get_last_error)();
                    return Err(std::io::Error::other(format!(
                        "Everything query failed (error code {})",
                        err
                    )));
                }

                let count = (self.get_num_results)();
                let mut out = Vec::with_capacity(count as usize);
                let mut buf = vec![0u16; 32768];

                for i in 0..count {
                    let n =
                        (self.get_result_full_path_name_w)(i, buf.as_mut_ptr(), buf.len() as u32);
                    if n > 0 && (n as usize) < buf.len() {
                        out.push(from_wide(&buf[..n as usize]));
                    }
                }

                Ok(out)
            }
        }
    }

    pub fn find_dll(explicit: Option<&String>) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Some(configured) = tools::config::get_config_param("everything_dll") {
            if !configured.trim().is_empty() {
                candidates.push(PathBuf::from(configured));
            }
        }

        if let Some(p) = explicit {
            candidates.push(PathBuf::from(p));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("Everything64.dll"));
            }
        }

        candidates.push(PathBuf::from(
            r"C:\Program Files\Everything\Everything64.dll",
        ));

        candidates.into_iter().find(|p| p.is_file())
    }
}

#[cfg(windows)]
pub fn discover_sdk(dll: &Path) -> std::io::Result<Vec<String>> {
    let ev = everything_sdk::Everything::load(dll).ok_or_else(|| {
        std::io::Error::other(format!(
            "Cannot load Everything64.dll from {}",
            dll.display()
        ))
    })?;

    let mut out = Vec::new();

    for query in ["*.log.gz", "latest.log"] {
        out.extend(dirs_from_paths(ev.query_paths(query)?));
    }

    Ok(out)
}

pub struct Discovery {
    pub dirs: Vec<String>,
    pub source: DiscoverSource,
}

pub fn discover_everything(es_arg: Option<&String>) -> Result<Discovery, String> {
    #[cfg(windows)]
    {
        if let Some(dll) = everything_sdk::find_dll(es_arg) {
            match discover_sdk(&dll) {
                Ok(dirs) => {
                    return Ok(Discovery {
                        dirs,
                        source: DiscoverSource::Sdk,
                    });
                }
                Err(e) => return Err(format!("Everything SDK ({}) failed: {}", dll.display(), e)),
            }
        }
    }

    if let Some(es_path) = find_es(es_arg) {
        match discover_es(&es_path) {
            Ok(dirs) => {
                return Ok(Discovery {
                    dirs,
                    source: DiscoverSource::Cli,
                });
            }
            Err(e) => return Err(format!("es.exe ({}) failed: {}", es_path, e)),
        }
    }

    Err("Everything is not available".to_string())
}
