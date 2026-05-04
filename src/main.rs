use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::{create_dir_all, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::io::Read;

use encoding_rs::WINDOWS_1251;
use memchr::memmem;

const BUF_SIZE: usize = 256 * 1024;
const LOCAL_BUF_SIZE: usize = 512 * 1024;

#[derive(Default)]
struct Stats {
    total: Duration,
    read: Duration,
    filter: Duration,
    decode: Duration,
    write: Duration,
    lines: usize,
    matched: usize,
}

fn main() -> std::io::Result<()> {
    let files = vec![
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-4.log.gz".to_string(),
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-5.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-20-2.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-05-01-1.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-29-1.log.gz".to_string(),
		r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-30-2.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-24-1.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-28-2.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-07-23-6.log.gz".to_string(),
		r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-08-21-5.log.gz".to_string(),
		r"D:\Data\blcMC\.minecraft\logs\blclient\minecraft\2025-08-04-3.log.gz".to_string(),
		r"D:\Data\blcMC\.minecraft\logs\blclient\minecraft\2025-07-25-2.log.gz".to_string(),
    ];

    let out_dir = "output_parts";
    create_dir_all(out_dir)?;

    rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get_physical())
        .build_global()
        .unwrap();

    let results: Vec<_> = files
        .par_iter()
        .map(|file| process_file(file, out_dir))
        .collect();

    let mut total = Stats::default();

    for res in results {
        if let Ok(s) = res {
            total.total += s.total;
            total.read += s.read;
            total.filter += s.filter;
            total.decode += s.decode;
            total.write += s.write;
            total.lines += s.lines;
            total.matched += s.matched;
        }
    }

    println!("\n=== TOTAL ===");
    println!("total:   {:?}", total.total);
    println!("read:    {:?}", total.read);
    println!("filter:  {:?}", total.filter);
    println!("decode:  {:?}", total.decode);
    println!("write:   {:?}", total.write);
    println!("lines:   {}", total.lines);
    println!("matched: {}", total.matched);

    Ok(())
}

fn process_file(file_path: &str, out_dir: &str) -> std::io::Result<Stats> {
    let total_start = Instant::now();

    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let mut reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let filename = Path::new(file_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let thread_id = format!("{:?}", std::thread::current().id());
    let mut out_path = PathBuf::from(out_dir);
    out_path.push(format!("{}_{}.txt", filename, thread_id));

    let out_file = File::create(out_path)?;
    let mut writer = BufWriter::with_capacity(BUF_SIZE, out_file);

    // --- detect encoding ---
    let detect_start = Instant::now();

    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let mut probe_reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let mut probe = Vec::with_capacity(4096);
    let mut tmp = Vec::new();

    for _ in 0..10 {
        tmp.clear();
        if probe_reader.read_until(b'\n', &mut tmp)? == 0 {
            break;
        }
        probe.extend_from_slice(&tmp);
        if probe.len() > 4096 {
            break;
        }
    }

    let is_utf8 = std::str::from_utf8(&probe).is_ok();

    let mut stats = Stats::default();
    stats.read += detect_start.elapsed();

    let mut local_buf = Vec::with_capacity(LOCAL_BUF_SIZE);

    // ===============================
    // 🔥 STREAMING PARSER (NO SPLIT)
    // ===============================
    let mut buf = vec![0u8; BUF_SIZE];
    let mut leftover = Vec::new();

    loop {
        let read_start = Instant::now();
        let n = reader.read(&mut buf)?;
        stats.read += read_start.elapsed();

        if n == 0 {
            break;
        }

        let mut start = 0;
        let data = &buf[..n];

        // объединяем с остатком
        if !leftover.is_empty() {
            leftover.extend_from_slice(data);
            process_buffer(
                &leftover,
                &filename,
                is_utf8,
                &mut stats,
                &mut local_buf,
            );
            leftover.clear();
        } else {
            process_buffer(
                data,
                &filename,
                is_utf8,
                &mut stats,
                &mut local_buf,
            );
        }

        // если строка оборвалась — сохраняем хвост
        if let Some(pos) = data.iter().rposition(|&b| b == b'\n') {
            leftover.clear();
            leftover.extend_from_slice(&data[pos + 1..]);
        }

        if local_buf.len() >= LOCAL_BUF_SIZE {
            let write_start = Instant::now();
            writer.write_all(&local_buf)?;
            stats.write += write_start.elapsed();
            local_buf.clear();
        }
    }

    if !local_buf.is_empty() {
        let write_start = Instant::now();
        writer.write_all(&local_buf)?;
        stats.write += write_start.elapsed();
    }

    stats.total = total_start.elapsed();

    println!(
        "[{}] total: {:?}, read: {:?}, filter: {:?}, decode: {:?}, write: {:?}, lines: {}, matched: {}",
        filename,
        stats.total,
        stats.read,
        stats.filter,
        stats.decode,
        stats.write,
        stats.lines,
        stats.matched
    );

    Ok(stats)
}

#[inline(always)]
fn process_buffer(
    data: &[u8],
    filename: &str,
    is_utf8: bool,
    stats: &mut Stats,
    local_buf: &mut Vec<u8>,
) {
    let mut start = 0;

    for i in 0..data.len() {
        if data[i] == b'\n' {
            let line = &data[start..i];
            start = i + 1;

            stats.lines += 1;

            // 🔥 FAST FILTER (cheap prefilter)
            if memchr::memchr(b'H', line).is_none() {
                continue;
            }

            let filter_start = Instant::now();
            let matched = memmem::find(line, b"[CHAT]").is_some();
            stats.filter += filter_start.elapsed();

            if !matched {
                continue;
            }

            stats.matched += 1;

            let decode_start = Instant::now();

            if is_utf8 {
                let text = unsafe { std::str::from_utf8_unchecked(line) };
                stats.decode += decode_start.elapsed();
                write_line(local_buf, filename, text);
            } else {
                let (cow, _, _) = WINDOWS_1251.decode(line);
                stats.decode += decode_start.elapsed();
                write_line(local_buf, filename, &cow);
            }
        }
    }
}

#[inline(always)]
fn write_line(buf: &mut Vec<u8>, filename: &str, text: &str) {
    let bytes = text.as_bytes();

    let end = if bytes.ends_with(b"\r") {
        bytes.len() - 1
    } else {
        bytes.len()
    };

    buf.extend_from_slice(filename.as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(&bytes[..end]);
    buf.push(b'\n');
}