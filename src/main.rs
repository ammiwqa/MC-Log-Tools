use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use encoding_rs::WINDOWS_1251;
use memchr::{memchr, memmem};

const BUF_SIZE: usize = 256 * 1024;
const TS_WIDTH: usize = 12;

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

    // 🔥 читаем параллельно
    let results: Vec<_> = files
        .par_iter()
        .map(|f| process_file(f))
        .collect();

    let mut all: Vec<(i64, String)> = Vec::new();

    for res in results {
        if let Ok(mut v) = res {
            all.append(&mut v);
        }
    }

    // 🔥 сортировка
    all.sort_unstable_by_key(|x| x.0);

    // 🔥 запись
    let mut writer = BufWriter::new(File::create("final.txt")?);

    for (ts, line) in all {
        write_fixed_ts(&mut writer, ts)?;
        writer.write_all(b" ")?;
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}

fn process_file(file_path: &str) -> std::io::Result<Vec<(i64, String)>> {
    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let mut reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let filename = Path::new(file_path)
        .file_name()
        .unwrap()
        .to_string_lossy();

    let (y, m, d) = parse_file_date(&filename);
    let base_day = to_unix_days(y, m, d);

    let mut force_ansi = false;
    let mut result = Vec::with_capacity(10000);

    let mut buf = vec![0u8; BUF_SIZE];
    let mut leftover = Vec::new();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        let data = &buf[..n];

        if !leftover.is_empty() {
            leftover.extend_from_slice(data);
            process_buffer(&leftover, base_day, &mut force_ansi, &mut result);
            leftover.clear();
        } else {
            process_buffer(data, base_day, &mut force_ansi, &mut result);
        }

        if let Some(pos) = data.iter().rposition(|&b| b == b'\n') {
            leftover.clear();
            leftover.extend_from_slice(&data[pos + 1..]);
        }
    }

    Ok(result)
}

#[inline(always)]
fn process_buffer(
    data: &[u8],
    base_day: i64,
    force_ansi: &mut bool,
    result: &mut Vec<(i64, String)>,
) {
    let mut start = 0;

    for i in 0..data.len() {
        if data[i] == b'\n' {
            let line = &data[start..i];
            start = i + 1;

            if memchr(b'H', line).is_none() {
                continue;
            }

            if memmem::find(line, b"[CHAT]").is_none() {
                continue;
            }

            // --- parse time ---
            let parsed = if line.len() > 10 && line.get(3) == Some(&b':') {
                parse_hms(line)
            } else {
                parse_hms_long(line)
            };

            let (h, m, s) = match parsed {
                Some(v) => v,
                None => continue,
            };

            let unix_time = base_day * 86400 + (h * 3600 + m * 60 + s) as i64;

            // --- decode ---
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

            result.push((unix_time, text.into_owned()));
        }
    }
}

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

    let h = (line[1] - b'0') as u32 * 10 + (line[2] - b'0') as u32;
    let m = (line[4] - b'0') as u32 * 10 + (line[5] - b'0') as u32;
    let s = (line[7] - b'0') as u32 * 10 + (line[8] - b'0') as u32;

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

    let h = (line[i + 1] - b'0') as u32 * 10 + (line[i + 2] - b'0') as u32;
    let m = (line[i + 4] - b'0') as u32 * 10 + (line[i + 5] - b'0') as u32;
    let s = (line[i + 7] - b'0') as u32 * 10 + (line[i + 8] - b'0') as u32;

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