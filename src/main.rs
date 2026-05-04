use std::env;
mod log_parser;
mod get_logfiles;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <command> <arguments>", args[0]);
        return Ok(());
    }

    let command = &args[1];

    if command == "cb" {
        if args.len() >= 3 {
            let path = &args[2];

            let files = match get_logfiles::find_log_gz_files(&path) {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("Error finding log files in '{}': {}", path, e);
                    return Err(e);
                }
            };

            let logs = log_parser::parse_logs(files)?;

            return Ok(());

        } else {
            eprintln!("Usage: {} cb <path>", args[0]);
            return Ok(());
        }
    }

    Ok(())
}