use clap::{Parser, Subcommand};
use create_base;
use tools;
use std::fs;
use std::path::Path;

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
    }
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
        Commands::CreateBase { paths, from_file, name } => {
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
    }
}