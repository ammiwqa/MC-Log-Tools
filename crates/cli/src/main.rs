use clap::{Parser, Subcommand};
use create_base;
use log_parser::ErrorStats;
use read_base_info;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tools;
use update_base;
use write_result;

use std::process::Command;

use console::Style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use progress::ProgressBarWrapper;

fn format_error_stats(stats: &ErrorStats) -> String {
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

fn main() {
    let cli = Cli::parse();

    let bright_cyan_style = Style::new().cyan().bold();
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

            if !all_paths.is_empty() {
                if !name.is_empty() {
                    match tools::validate_windows_filename(&name) {
                        Ok(()) => {
                            let bright_cyan_style = Style::new().cyan().bold();
                            let parsing_pb = ProgressBar::new(1);
                            parsing_pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));
                            parsing_pb.set_style(
                                ProgressStyle::default_bar()
                                    .template(
                                        "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
                                    )
                                    .unwrap()
                                    .progress_chars("=>-"),
                            );
                            parsing_pb
                                .set_prefix(format!("{}", bright_cyan_style.apply_to("Parsing")));
                            parsing_pb.enable_steady_tick(std::time::Duration::from_millis(100));

                            let wrapper = progress::ProgressBarWrapper::new(parsing_pb);
                            if let Err(e) = create_base::create_base(all_paths, name, &wrapper) {
                                eprintln!("Error creating base: {}", e);
                            }
                        }
                        Err(e) => {
                            println!("\n{}", tools::format_filename_error(&name, &e));
                        }
                    }
                }
            } else {
                eprintln!("No paths");
                std::process::exit(1);
            }
        }

        Commands::Search { base_name, text } => {
            if !base_name.is_empty() {
                if !text.is_empty() {
                    let exit_file_name = tools::get_valid_file_name_with_timestamp(&text);
                    let exit_file_path = tools::get_results_file_path(&exit_file_name);

                    let (progress, handle) = search::search_async(&base_name, &text);
                    let max_lines = progress.get_max_progress();

                    let search_pb = Arc::new(ProgressBar::new(max_lines as u64));
                    search_pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));

                    search_pb.set_style(
                        ProgressStyle::default_bar()
                            .template(
                                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}]",
                            )
                            .unwrap()
                            .progress_chars("=>-"),
                    );

                    search_pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Searching")));

                    loop {
                        let done = progress.get_progress();

                        search_pb.set_position(done);

                        if handle.is_finished() {
                            break;
                        }

                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }

                    let results = handle.join().unwrap();
                    let results_len = results.len();

                    search_pb.set_position(max_lines as u64);
                    search_pb.finish_and_clear();

                    println!(
                        "   {} {} lines -> {} founds",
                        bright_green_style.apply_to("Searching"),
                        max_lines,
                        results_len
                    );

                    let write_job =
                        write_result::write_results_async(results, exit_file_path.clone());

                    let write_pb = ProgressBar::new(write_job.progress.get_total());
                    write_pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));

                    write_pb.set_style(
                        ProgressStyle::default_bar()
                            .template(
                                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}]",
                            )
                            .unwrap()
                            .progress_chars("=>-"),
                    );

                    write_pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Writing")));

                    loop {
                        write_pb.set_position(write_job.progress.get_written());

                        if write_job.handle.is_finished() {
                            break;
                        }

                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }

                    write_job.handle.join().unwrap();

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
            }
        }

        Commands::UpdateBase { name } => {
            if !name.is_empty() {
                let bright_cyan_style = Style::new().cyan().bold();
                let bright_green_style = Style::new().green().bold();
                let update_pb = ProgressBar::new(1);
                update_pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));
                update_pb.set_style(
                    ProgressStyle::default_bar()
                        .template(
                            "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}] {msg:.white}",
                        )
                        .unwrap()
                        .progress_chars("=>-"),
                );
                update_pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Updating")));
                update_pb.enable_steady_tick(std::time::Duration::from_millis(100));

                let wrapper = ProgressBarWrapper::new(update_pb);
                match update_base::update_base(&name, &wrapper) {
                    Ok(result) => {
                        if result.no_new_files {
                            println!("   {} No new logs to parse", bright_green_style.apply_to("Update"));
                        } else {
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
                    }
                    Err(e) => {
                        eprintln!("Error updating base: {}", e);
                    }
                }
            }
        }

        Commands::ListBases => {
            match read_base_info::list_bases() {
                Ok(bases) => {
                    read_base_info::print_bases_table(&bases);
                }
                Err(e) => {
                    eprintln!("Error listing bases: {}", e);
                }
            }
        }
    }
}
