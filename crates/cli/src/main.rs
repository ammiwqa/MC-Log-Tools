use clap::{Parser, Subcommand};
use create_base;
use tools;


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
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<String>,

        #[arg(value_name = "NAME")]
        name: String,
    }


    //#[command(aliases = ["s", "find"])]
    //Search {
    //    #[arg(value_name = "BASE")]
    //    base: String,

    //    #[arg(value_name = "SEARCH TEXT")]
    //    text: String,
    //},
}

fn main() {
    let cli = Cli::parse();

    match cli.command {

        Commands::CreateBase { paths, name } => {

            if paths.len() > 0 {
                if !name.is_empty() {

                    match tools::validate_windows_filename(&name) {
                        Ok(()) => {
                           create_base::create_base(paths, name);
                        }
                        Err(e) => {
                            println!("\n{}", tools::format_filename_error(&name, &e));
                        }
                    }

                    //
                } else { }
            } else { }
        }

        //Commands::Search { base, text } => { }
    }
}