use clap::{Parser, Subcommand};
use std::fs;
use std::path::Path;
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

/// Drives a `CoreEvent`
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
    }
}
