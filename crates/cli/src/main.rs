use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use console::Style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use lt_core::events::CoreEvent;
use lt_core::operations;

fn format_error_stats(stats: &lt_core::ErrorStats) -> String {
    match (stats.file_errors, stats.parse_errors) {
        (0, 0) => String::new(),
        (f, 0) => format!("({} file errors)", f),
        (0, p) => format!("({} parse errors)", p),
        (f, p) => format!("({} file errors, {} parse errors)", f, p),
    }
}

#[derive(Parser)]
#[command(name = "lt3", version = "1.0", about = "MC Log-tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(aliases = ["cb", "create"])]
    CreateBase {
        #[arg(required = true, value_name = "NAME")]
        name: String,

        #[arg(value_name = "PATH")]
        paths: Vec<String>,

        #[arg(short, long, value_name = "FILE")]
        from_file: Option<String>,
    },

    #[command(aliases = ["find", "s"])]
    Search {
        #[arg(required = true, value_name = "BASE")]
        base_name: String,

        #[arg(required = true, value_name = "TEXT")]
        text: String,
    },

    #[command(aliases = ["update", "u"])]
    UpdateBase {
        #[arg(required = true, value_name = "BASE")]
        name: String,
    },

    #[command(aliases = ["list", "ls", "l"])]
    ListBases,

    #[command(aliases = ["disc", "d"])]
    Discover {
        /// Root directory to scan (default: user home). Ignored when Everything is used.
        root: Option<String>,

        #[arg(short, long, value_name = "FILE")]
        out: Option<String>,

        /// Path to es.exe if it is not in PATH or the default install location
        #[arg(long, value_name = "EXE")]
        es: Option<String>,

        /// Skip Everything even if it is available
        #[arg(long)]
        no_everything: bool,
    },

    /// Set a config parameter: lt3 config <param> <var>
    /// Show a parameter:  lt3 config <param>
    /// List all:          lt3 config
    Config {
        #[arg(value_name = "PARAM")]
        param: Option<String>,

        #[arg(value_name = "VAR")]
        var: Option<String>,
    },
}

fn config_path() -> PathBuf {
    let mut path = PathBuf::from(
        std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string()),
    );
    path.push("LogTools");
    path.push("config.json");
    path
}

fn load_config() -> serde_json::Map<String, serde_json::Value> {
    let content = match fs::read_to_string(config_path()) {
        Ok(c) => c,
        Err(_) => return serde_json::Map::new(),
    };

    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => {
            eprintln!(
                "Warning: broken config file, ignoring ({})",
                config_path().display()
            );
            serde_json::Map::new()
        }
    }
}

fn save_config(map: &serde_json::Map<String, serde_json::Value>) -> Result<(), std::io::Error> {
    if let Some(parent) = config_path().parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        config_path(),
        serde_json::to_string_pretty(&serde_json::Value::Object(map.clone()))?,
    )
}

fn get_config_param(param: &str) -> Option<String> {
    load_config()
        .get(param)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn read_paths_from_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<String>, std::io::Error> {
    let content = fs::read_to_string(file_path)?;

    let paths: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect();

    Ok(paths)
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

fn has_mc_logs(dir: &Path) -> bool {
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

fn discover_log_dirs(dir: &Path, found: &mut Vec<String>) {
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
        discover_log_dirs(&subdir, found);
    }
}

/// Locates es.exe (Everything command-line interface): --es argument > PATH > install dir.
fn find_es(explicit: Option<&String>) -> Option<String> {
    if let Some(p) = explicit {
        if Path::new(p).is_file() {
            return Some(p.clone());
        }
        eprintln!("es.exe not found at: {}", p);
        return None;
    }

    if let Ok(out) = Command::new("where").arg("es.exe").output() {
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

#[cfg(windows)]
mod everything_sdk {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    const EVERYTHING_OK: u32 = 0;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub struct Everything {
        module: *mut c_void,
        set_search_w: unsafe extern "system" fn(*const u16),
        query_w: unsafe extern "system" fn(i32) -> i32,
        get_num_results: unsafe extern "system" fn() -> u32,
        get_result_full_path_name_w: unsafe extern "system" fn(u32, *mut u16, u32) -> u32,
        get_last_error: unsafe extern "system" fn() -> u32,
    }

    unsafe impl Send for Everything {}

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn from_wide(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    impl Everything {
        /// Loads Everything64.dll from the given path and resolves the functions we need.
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
                module,
                set_search_w: proc!("Everything_SetSearchW"),
                query_w: proc!("Everything_QueryW"),
                get_num_results: proc!("Everything_GetNumResults"),
                get_result_full_path_name_w: proc!("Everything_GetResultFullPathNameW"),
                get_last_error: proc!("Everything_GetLastError"),
            })
        }

        /// Runs a query and returns all full result paths.
        pub fn query_paths(&self, search: &str) -> std::io::Result<Vec<String>> {
            unsafe {
                (self.set_search_w)(to_wide(search).as_ptr());

                // TRUE = wait for results from the Everything window/thread.
                if (self.query_w)(1) == 0 {
                    let err = (self.get_last_error)();
                    return Err(match err {
                        EVERYTHING_OK => std::io::Error::other(
                            "Everything query failed (is Everything running?)",
                        ),
                        code => std::io::Error::other(format!(
                            "Everything query failed (error code {})",
                            code
                        )),
                    });
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

    /// Locates the SDK DLL: --es argument > config (everything_dll) > next to exe > Program Files.
    pub fn find_dll(explicit: Option<&String>) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Some(configured) = crate::get_config_param("everything_dll") {
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

/// Queries Everything via the SDK DLL and returns unique MC log dirs.
#[cfg(windows)]
fn everything_sdk_log_dirs(dll: &Path) -> std::io::Result<Vec<String>> {
    let ev = match everything_sdk::Everything::load(dll) {
        Some(ev) => ev,
        None => {
            return Err(std::io::Error::other(format!(
                "Cannot load Everything64.dll from {}",
                dll.display()
            )))
        }
    };

    let mut dirs = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for query in ["*.log.gz", "latest.log"] {
        for line in ev.query_paths(query)? {
            let path = Path::new(&line);

            let Some(parent) = path.parent() else {
                continue;
            };

            if !seen.insert(parent.to_path_buf()) || !parent.is_dir() {
                continue;
            }

            // Verify it is really an MC log dir (guards against unrelated matches).
            if has_mc_logs(parent) {
                dirs.push(parent.to_string_lossy().to_string());
            }
        }
    }

    Ok(dirs)
}

/// Queries a running Everything instance for MC log files and returns their parent dirs.
fn everything_log_dirs(es: &str) -> std::io::Result<Vec<String>> {
    let mut dirs = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for query in ["*.log.gz", "latest.log"] {
        let out = Command::new(es).arg(query).output()?;

        if !out.status.success() {
            return Err(std::io::Error::other(format!(
                "es.exe failed with status {}",
                out.status
            )));
        }

        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let path = Path::new(line);

            let Some(parent) = path.parent() else {
                continue;
            };

            if !seen.insert(parent.to_path_buf()) || !parent.is_dir() {
                continue;
            }

            // Verify it is really an MC log dir (guards against unrelated matches).
            if has_mc_logs(parent) {
                dirs.push(parent.to_string_lossy().to_string());
            }
        }
    }

    Ok(dirs)
}

fn make_bar(prefix: &str) -> ProgressBar {
    let bright_cyan_style = Style::new().cyan().bold();
    let pb = ProgressBar::new(1);
    pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
            )
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_prefix(format!("{}", bright_cyan_style.apply_to(prefix)));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
    
fn spawn_drawer(pb: ProgressBar, rx: std::sync::mpsc::Receiver<CoreEvent>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            match ev {
                CoreEvent::Progress { pos, total } => {
                    pb.set_length(total);
                    pb.set_position(pos);
                }
                CoreEvent::Message(msg) => pb.set_message(msg),
            }
        }
        pb.finish_and_clear();
    })
}

fn main() {
    let cli = Cli::parse();

    let bright_green_style = Style::new().green().bold();

    match cli.command {
        Commands::CreateBase {
            paths,
            from_file,
            name,
        } => {
            let mut all_paths = paths;

            if let Some(file_path) = from_file {
                match read_paths_from_file(&file_path) {
                    Ok(file_paths) => all_paths.extend(file_paths),
                    Err(e) => {
                        eprintln!("Ошибка чтения файла '{}': {}", file_path, e);
                        std::process::exit(1);
                    }
                }
            }

            if all_paths.is_empty() {
                eprintln!("No paths");
                std::process::exit(1);
            }

            if name.is_empty() {
                return;
            }

            if let Err(e) = tools::validate_windows_filename(&name) {
                println!("\n{}", tools::format_filename_error(&name, &e));
                return;
            }

            let pb = make_bar("Parsing");
            let (tx, rx) = operations::event_channel();
            let drawer = spawn_drawer(pb.clone(), rx);

            match operations::create_base(all_paths, name.clone(), tx.clone()) {
                Ok(result) => {
                    drop(tx);
                    drawer.join().unwrap();

                    if result.duplicate_files > 0 {
                        println!(
                            "   {} {} duplicate files skipped",
                            bright_green_style.apply_to("Dedup"),
                            result.duplicate_files
                        );
                    }
                    if result.gz_files > 0 {
                        println!(
                            "   {} {} files -> {} lines {}",
                            bright_green_style.apply_to("Parsing"),
                            result.gz_files,
                            result.parsed_gz_lines,
                            format_error_stats(&result.gz_errors)
                        );
                    }
                    if result.latest_files > 0 {
                        println!(
                            "   {} {} latest files -> {} lines {}",
                            bright_green_style.apply_to("Parsing latest"),
                            result.latest_files,
                            result.parsed_latest_lines,
                            format_error_stats(&result.latest_errors)
                        );
                    }
                    println!(
                        "   {} {} lines -> {}",
                        bright_green_style.apply_to("Writing"),
                        result.total_lines,
                        name
                    );
                }
                Err(e) => {
                    drop(tx);
                    let _ = drawer.join();
                    eprintln!("Error creating base: {}", e);
                }
            }
        }

        Commands::Search { base_name, text } => {
            if base_name.is_empty() || text.is_empty() {
                return;
            }

            let exit_file_name = tools::get_valid_file_name_with_timestamp(&text);
            let exit_file_path = tools::get_results_file_path(&exit_file_name);

            let search_handle = match operations::start_search(&base_name, &text) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Error starting search: {}", e);
                    return;
                }
            };

            let max_lines = search_handle.progress.get_max_progress();

            let search_pb = Arc::new(make_bar("Searching"));
            search_pb.set_length(max_lines);

            loop {
                search_pb.set_position(search_handle.progress.get_progress());

                if search_handle.handle.is_finished() {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            let results = match operations::wait_search(search_handle) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Search failed: {}", e);
                    return;
                }
            };
            let results_len = results.len();

            search_pb.set_position(max_lines);
            search_pb.finish_and_clear();

            println!(
                "   {} {} lines -> {} founds",
                bright_green_style.apply_to("Searching"),
                max_lines,
                results_len
            );

            let write_job =
                write_result::write_results_async(results, exit_file_path.clone());

            let write_pb = make_bar("Writing");
            write_pb.set_length(write_job.progress.get_total());

            loop {
                write_pb.set_position(write_job.progress.get_written());

                if write_job.handle.is_finished() {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            if let Err(e) = write_job.handle.join() {
                eprintln!("Error writing results: {:?}", e);
                return;
            }

            write_pb.finish_and_clear();
            println!(
                "   {} {} founds -> ({})",
                bright_green_style.apply_to("Writing"),
                results_len,
                exit_file_path
            );

            Command::new("cmd")
                .args(["/C", "start", "", &exit_file_path])
                .spawn()
                .unwrap();
        }

        Commands::UpdateBase { name } => {
            if name.is_empty() {
                return;
            }

            let update_pb = make_bar("Updating");
            let (tx, rx) = operations::event_channel();
            let drawer = spawn_drawer(update_pb.clone(), rx);

            match operations::update_base(&name, tx.clone()) {
                Ok(result) => {
                    drop(tx);
                    drawer.join().unwrap();

                    if result.no_new_files {
                        println!(
                            "   {} No new logs to parse",
                            bright_green_style.apply_to("Update")
                        );
                        return;
                    }

                    if result.new_gz_files > 0 {
                        println!(
                            "   {} {} new gz files -> {} lines {}",
                            bright_green_style.apply_to("Parsing"),
                            result.new_gz_files,
                            result.parsed_gz_lines,
                            format_error_stats(&result.gz_errors)
                        );
                    }
                    if result.new_latest_files > 0 {
                        println!(
                            "   {} {} latest files -> {} lines {}",
                            bright_green_style.apply_to("Parsing latest"),
                            result.new_latest_files,
                            result.parsed_latest_lines,
                            format_error_stats(&result.latest_errors)
                        );
                    }
                    println!(
                        "   {} {} total lines -> {}",
                        bright_green_style.apply_to("Writing"),
                        result.total_lines,
                        name
                    );
                }
                Err(e) => {
                    drop(tx);
                    let _ = drawer.join();
                    eprintln!("Error updating base: {}", e);
                }
            }
        }

        Commands::ListBases => match read_base_info::list_bases() {
            Ok(bases) => {
                read_base_info::print_bases_table(&bases);
            }
            Err(e) => {
                eprintln!("Error listing bases: {}", e);
            }
        },

        Commands::Config { param, var } => match (param, var) {
            (None, _) => {
                let map = load_config();
                if map.is_empty() {
                    println!("No config parameters set.");
                } else {
                    for (k, v) in &map {
                        println!("{:<20} {}", k, v.as_str().unwrap_or(&v.to_string()));
                    }
                }
            }
            (Some(param), None) => match get_config_param(&param) {
                Some(value) => println!("{}", value),
                None => eprintln!("Parameter '{}' is not set", param),
            },
            (Some(param), Some(var)) => {
                let mut map = load_config();
                map.insert(
                    param.clone(),
                    serde_json::Value::String(var.trim().to_string()),
                );

                match save_config(&map) {
                    Ok(()) => {
                        println!(
                            "   {} {} -> \"{}\"",
                            bright_green_style.apply_to("Config"),
                            param,
                            var
                        );
                    }
                    Err(e) => {
                        eprintln!("Error saving config: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },

        Commands::Discover {
            root,
            out,
            es,
            no_everything,
        } => {
            let out_path = out.unwrap_or_else(|| {
                tools::get_results_file_path(&tools::get_valid_file_name("mc_log_dirs"))
            });

            let bright_cyan_style = Style::new().cyan().bold();
            let start = std::time::Instant::now();

            let (found, _used_everything) = if no_everything {
                (None, false)
            } else {
                // 1) Everything SDK DLL (direct IPC)
                #[cfg(windows)]
                {
                    if let Some(dll) = everything_sdk::find_dll(es.as_ref()) {
                        println!(
                            "   {} Everything SDK: {}",
                            bright_cyan_style.apply_to("Scanning"),
                            dll.display()
                        );
                        match everything_sdk_log_dirs(&dll) {
                            Ok(dirs) => (Some(dirs), true),
                            Err(e) => {
                                eprintln!(
                                    "Everything SDK query failed ({}) — trying es.exe / directory scan",
                                    e
                                );
                                (None, false)
                            }
                        }
                    } else {
                        (None, false)
                    }
                }

                #[cfg(not(windows))]
                {
                    (None, false)
                }
            };

            // 2) es.exe fallback
            let (found, _used_everything) = match found {
                Some(f) => (Some(f), true),
                None => match find_es(es.as_ref()) {
                    Some(es_path) => {
                        println!(
                            "   {} Everything CLI: {}",
                            bright_cyan_style.apply_to("Scanning"),
                            es_path
                        );
                        match everything_log_dirs(&es_path) {
                            Ok(dirs) => (Some(dirs), true),
                            Err(e) => {
                                eprintln!(
                                    "Everything query failed ({}) — falling back to directory scan",
                                    e
                                );
                                (None, false)
                            }
                        }
                    }
                    None => (None, false),
                },
            };

            let found = found;

            let found = match found {
                Some(f) => f,
                None => {
                    let scan_root = match root {
                        Some(r) => PathBuf::from(r),
                        None => match std::env::var("USERPROFILE") {
                            Ok(home) => PathBuf::from(home),
                            Err(_) => {
                                eprintln!("Cannot determine user home directory");
                                std::process::exit(1);
                            }
                        },
                    };

                    if !scan_root.is_dir() {
                        eprintln!("Not a directory: {}", scan_root.display());
                        std::process::exit(1);
                    }

                    println!(
                        "   {} {}",
                        bright_cyan_style.apply_to("Scanning"),
                        scan_root.display()
                    );

                    if !no_everything {
                        eprintln!(
                            "   Tip: install Everything (https://www.voidtools.com/) for an instant whole-PC scan.\n   Then set the DLL path: lt3 config everything_dll \"<path to Everything64.dll>\""
                        );
                    }

                    let mut found = Vec::new();
                    discover_log_dirs(&scan_root, &mut found);
                    found
                }
            };

            fs::write(&out_path, found.join("\n")).unwrap_or_else(|e| {
                eprintln!("Error writing output file: {}", e);
                std::process::exit(1);
            });

            println!(
                "   {} {} log directories -> ({}) in {:.1}s",
                bright_green_style.apply_to("Discovered"),
                found.len(),
                out_path,
                start.elapsed().as_secs_f32()
            );
            println!(
                "   Create a global base: lt3 cb <NAME> --from-file \"{}\"",
                out_path
            );
        }
    }
}
