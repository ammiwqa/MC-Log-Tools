use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use encoding_rs::WINDOWS_1251;
use memchr::{memchr, memmem};

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
    errors: usize,
}

fn main() -> std::io::Result<()> {
    let files = vec![
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-4.log.gz".to_string(),
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-13-5.log.gz".to_string(),
        r"D:\Archives\Minecraft\logs\full-logs2\slFull\2023-04-20-2.log.gz".to_string(),
        r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-05-01-1.log.gz".to_string(),
        r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-29-1.log.gz".to_string(),
        r"C:\Users\Wqaya\AppData\Roaming\CheatBreaker\downloads\logs\1.8.9\2026-04-30-2.log.gz".to_string(),
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
            total.errors += s.errors;
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
    println!("errors: {}", total.errors);

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

    // 🔥 парсим дату файла один раз
    let (y, m, d) = parse_file_date(&filename);
    let base_day = to_unix_days(y, m, d);

    let thread_id = format!("{:?}", std::thread::current().id());
    let mut out_path = PathBuf::from(out_dir);
    out_path.push(format!("{}_{}.txt", filename, thread_id));

    let out_file = File::create(out_path)?;
    let mut writer = BufWriter::with_capacity(BUF_SIZE, out_file);

    let mut stats = Stats::default();
    let mut local_buf = Vec::with_capacity(LOCAL_BUF_SIZE);

    let mut force_ansi = false;

    let mut buf = vec![0u8; BUF_SIZE];
    let mut leftover = Vec::new();

    loop {
        let read_start = Instant::now();
        let n = reader.read(&mut buf)?;
        stats.read += read_start.elapsed();

        if n == 0 {
            break;
        }

        let data = &buf[..n];

        if !leftover.is_empty() {
            leftover.extend_from_slice(data);
            process_buffer(
                &leftover,
                base_day,
                &mut force_ansi,
                &mut stats,
                &mut local_buf,
            );
            leftover.clear();
        } else {
            process_buffer(
                data,
                base_day,
                &mut force_ansi,
                &mut stats,
                &mut local_buf,
            );
        }

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
        "[{}] total: {:?}, read: {:?}, filter: {:?}, decode: {:?}, write: {:?}, lines: {}, matched: {}, errors: {}",
        filename,
        stats.total,
        stats.read,
        stats.filter,
        stats.decode,
        stats.write,
        stats.lines,
        stats.matched,
        stats.errors
    );

    Ok(stats)
}

#[inline(always)]
fn process_buffer(
    data: &[u8],
    base_day: i64,
    force_ansi: &mut bool,
    stats: &mut Stats,
    local_buf: &mut Vec<u8>,
) {
    let mut start = 0;

    for i in 0..data.len() {
        if data[i] == b'\n' {
            let line = &data[start..i];
            start = i + 1;

            stats.lines += 1;

            if memchr(b'H', line).is_none() {
                continue;
            }

            let filter_start = Instant::now();
            let matched = memmem::find(line, b"[CHAT]").is_some();
            stats.filter += filter_start.elapsed();

            if !matched {
                continue;
            }

            stats.matched += 1;

            // 🔥 PARSE TIME
            let parsed = if line.len() > 10 && line.get(3) == Some(&b':') {
                parse_hms(line)
            } else {
                parse_hms_long(line)
            };

            let (h, m, s) = match parsed {
                Some(v) => v,
                None => {
                    stats.errors += 1;
                    continue;
                }
            };

            let unix_time = base_day * 86400 + (h * 3600 + m * 60 + s) as i64;

            let decode_start = Instant::now();

            let text: std::borrow::Cow<str> = if *force_ansi {
                WINDOWS_1251.decode(line).0
            } else {
                match std::str::from_utf8(line) {
                    Ok(s) => std::borrow::Cow::Borrowed(s),
                    Err(_) => {
                        *force_ansi = true;
                        WINDOWS_1251.decode(line).0
                    }
                }
            };

            stats.decode += decode_start.elapsed();

            write_line(local_buf, unix_time, text.as_ref());
        }
    }
}

#[inline(always)]
fn write_line(buf: &mut Vec<u8>, ts: i64, text: &str) {
    buf.extend_from_slice(ts.to_string().as_bytes());
    buf.push(b' ');

    let bytes = text.as_bytes();
    let end = if bytes.ends_with(b"\r") {
        bytes.len() - 1
    } else {
        bytes.len()
    };

    buf.extend_from_slice(&bytes[..end]);
    buf.push(b'\n');
}

#[inline(always)]
fn parse_file_date(name: &str) -> (i32, u32, u32) {
    let b = name.as_bytes();

    let year =
        (b[0] - b'0') as i32 * 1000 +
            (b[1] - b'0') as i32 * 100 +
            (b[2] - b'0') as i32 * 10 +
            (b[3] - b'0') as i32;

    let month = (b[5] - b'0') as u32 * 10 + (b[6] - b'0') as u32;
    let day = (b[8] - b'0') as u32 * 10 + (b[9] - b'0') as u32;

    (year, month, day)
}

#[inline(always)]
fn parse_hms(line: &[u8]) -> Option<(u32, u32, u32)> {
    if line.len() < 9 {
        return None;
    }

    let h = (line[1].wrapping_sub(b'0') as u32) * 10
        + line[2].wrapping_sub(b'0') as u32;
    let m = (line[4].wrapping_sub(b'0') as u32) * 10
        + line[5].wrapping_sub(b'0') as u32;
    let s = (line[7].wrapping_sub(b'0') as u32) * 10
        + line[8].wrapping_sub(b'0') as u32;

    if h < 24 && m < 60 && s < 60 {
        Some((h, m, s))
    } else {
        None
    }
}

#[inline(always)]
fn parse_hms_long(line: &[u8]) -> Option<(u32, u32, u32)> {
    let mut i = 0;

    while i < line.len() && line[i] != b' ' {
        i += 1;
    }

    if i + 9 >= line.len() {
        return None;
    }

    let h = (line[i + 1].wrapping_sub(b'0') as u32) * 10
        + line[i + 2].wrapping_sub(b'0') as u32;
    let m = (line[i + 4].wrapping_sub(b'0') as u32) * 10
        + line[i + 5].wrapping_sub(b'0') as u32;
    let s = (line[i + 7].wrapping_sub(b'0') as u32) * 10
        + line[i + 8].wrapping_sub(b'0') as u32;

    if h < 24 && m < 60 && s < 60 {
        Some((h, m, s))
    } else {
        None
    }
}

#[inline(always)]
fn to_unix_days(y: i32, m: u32, d: u32) -> i64 {
    let mut y = y as i64;
    let mut m = m as i64;
    let d = d as i64;

    if m <= 2 {
        y -= 1;
        m += 12;
    }

    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m - 3) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;

    era * 146097 + doe - 719468
}