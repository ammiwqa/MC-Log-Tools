use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use encoding_rs::WINDOWS_1251;

const BATCH_SIZE: usize = 1024 * 64;

fn main() -> std::io::Result<()> {
    let files = vec![
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-4.log.gz".to_string(),
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-5.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-20-2.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-05-01-1.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-29-1.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-30-2.log.gz".to_string(),
    ];

    process_logs(files, "output.txt")?;

    Ok(())
}

pub fn process_logs(files: Vec<String>, output_path: &str) -> std::io::Result<()> {
    let out_file = File::create(output_path)?;
    let writer = Arc::new(Mutex::new(BufWriter::new(out_file)));

    files.par_iter().for_each(|file_path| {
        if let Err(e) = process_one(file_path, &writer) {
            eprintln!("Error processing {}: {}", file_path, e);
        }
    });

    Ok(())
}

fn process_one(
    file_path: &str,
    writer: &Arc<Mutex<BufWriter<File>>>,
) -> std::io::Result<()> {
    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::with_capacity(1024 * 64, decoder);

    let mut local_buffer = Vec::with_capacity(BATCH_SIZE);

    for line in reader.split(b'\n') {
        let line = line?;

		let mut owned; // будет жить вне match

		let text: &str = match std::str::from_utf8(&line) {
			Ok(s) => s,
			Err(_) => {
				let (cow, _, _) = WINDOWS_1251.decode(&line);
				owned = cow.into_owned(); // теперь строка живёт
				&owned
			}
		};

        if text.contains("[CHAT]") {
            let formatted = format!(
                "{} {}\n",
                Path::new(file_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                text.trim_end()
            );

            local_buffer.extend_from_slice(formatted.as_bytes());
        }

        if local_buffer.len() >= BATCH_SIZE {
            flush_buffer(&local_buffer, writer);
            local_buffer.clear();
        }
    }

    if !local_buffer.is_empty() {
        flush_buffer(&local_buffer, writer);
    }

    Ok(())
}

fn flush_buffer(buffer: &[u8], writer: &Arc<Mutex<BufWriter<File>>>) {
    let mut w = writer.lock().unwrap();
    let _ = w.write_all(buffer);
}