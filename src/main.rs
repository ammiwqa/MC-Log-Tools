use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

mod log_parser;
mod get_logfiles;
mod create_base;

const TS_WIDTH: usize = 12;

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

            let pb = Arc::new(ProgressBar::new(files.len() as u64));

            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {wide_bar} {pos}/{len} ({eta})")
                    .unwrap(),
            );

            let logs = log_parser::parse_logs(files, pb.clone())?;

            // 🔥 запись в файл
            let out_file = File::create("output.txt")?;
            let mut writer = BufWriter::with_capacity(1024 * 1024, out_file);

            for (ts, msg) in logs {
                write_fixed_ts(&mut writer, ts)?;
                writer.write_all(b" ")?;
                writer.write_all(msg.as_bytes())?;
                writer.write_all(b"\n")?;
            }

            writer.flush()?;

            println!("Done. Output written to output.txt");

            return Ok(());
        } else {
            eprintln!("Usage: {} cb <path>", args[0]);
            return Ok(());
        }
    }

    Ok(())
}

// =============================
// helper: фиксированный unix
// =============================

#[inline(always)]
fn write_fixed_ts<W: Write>(w: &mut W, ts: i64) -> std::io::Result<()> {
    let mut buf = [b'0'; TS_WIDTH];
    let mut n = ts;

    let mut i = TS_WIDTH;

    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    w.write_all(&buf)
}