use clap::{Parser, Subcommand};
use create_base;
use std::fs;
use std::path::Path;
use tools;

use console::Style;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "lt3", version = "1.0", about = "MC Log-toolss")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(aliases = ["cb", "create"])]
    CreateBase {
        #[arg(value_name = "PATH")]
        paths: Vec<String>,

        #[arg(short, long, value_name = "FILE")]
        from_file: Option<String>,

        #[arg(required = true, value_name = "NAME")]
        name: String,
    },

    #[command(aliases = ["search", "find", "s"])]
    Search {
        #[arg(required = true, value_name = "BASE")]
        base_name: String,

        #[arg(required = true, value_name = "TEXT")]
        text: String,
    },
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
                            create_base::create_base(all_paths, name);
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
                    let (progress, handle) = search::search_async(&base_name, &text);

                    let max_lines = progress.get_max_progress();

                    let bright_cyan_style = Style::new().cyan().bold();

                    let pb = Arc::new(ProgressBar::new(max_lines as u64));
                    pb.set_style(
                        ProgressStyle::default_bar()
                            .template(
                                "   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}]",
                            )
                            .unwrap()
                            .progress_chars("=>-"),
                    );

                    pb.set_prefix(format!("{}", bright_cyan_style.apply_to("Searching")));

                    loop {
                        let done = progress.get_progress();

                        pb.set_position(done as u64);

                        if handle.is_finished() {
                            break;
                        }

                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }

                    let results = handle.join().unwrap();

                    pb.set_position(max_lines as u64);
                    pb.finish_and_clear();

                    println!(
                        "   {} {} -> {} founds",
                        Style::new().green().bold().apply_to("Searching"),
                        max_lines,
                        results.len()
                    );
                }
            }
        }
    }
}
