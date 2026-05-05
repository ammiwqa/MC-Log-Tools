use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use sha2::{Sha256, Digest};

use tools;

use hex::encode;

use encoding_rs::WINDOWS_1251;
use memchr::{memchr, memmem};

const BUF_SIZE: usize = 256 * 1024;

use indicatif::ProgressBar;
use std::sync::Arc;

#[derive(Default, Debug, Clone)]
pub struct ErrorStats {
    pub file_errors: usize,
    pub parse_errors: usize,
}

pub fn parse_logs(
    files: Vec<String>,
    pb: &Arc<ProgressBar>,
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats)> {

    let results: Vec<_> = files
        .par_iter()
        .map(|f| {
            let res = process_file_gz(f);
            pb.inc(1);
            res
        })
        .collect();

    let mut all = Vec::new();
    let mut stats = ErrorStats::default();

    for res in results {
        match res {
            Ok((mut v, s)) => {
                all.append(&mut v);
                stats.parse_errors += s.parse_errors;
            }
            Err(_e) => {
                stats.file_errors += 1;
                //eprintln!("Error: {}", e);
            }
        }
    }

    Ok((all, stats))
}

pub fn parse_latest(
    latest: Vec<(String, usize)>,
    pb: &Arc<ProgressBar>
) -> std::io::Result<(
    Vec<(i64, String)>,
    Vec<(String, usize, String)>,
    ErrorStats
)> {

    let results: Vec<_> = latest
        .par_iter()
        .map(|(file, value)| {
            let res = process_file_plain(file, *value)
                .map(|(result, stats, line, hash)| {
                    (file.clone(), result, stats, line, hash)
                });

            pb.inc(1);
            res
        })
        .collect();

    let mut all = Vec::new();
    let mut meta = Vec::new();
    let mut stats = ErrorStats::default();

    for res in results {
        match res {
            Ok((path, mut result, s, line, hash)) => {
                all.append(&mut result);
                meta.push((path, line, hash));
                stats.parse_errors += s.parse_errors;
            }
            Err(_e) => {
                stats.file_errors += 1;
                //eprintln!("Error: {}", e);
            }
        }
    }

    Ok((all, meta, stats))
}

pub fn process_reader<R: Read>(
    mut reader: BufReader<R>,
    base_day: i64,
    force_ansi: &mut bool,
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats)> {

    let mut result = Vec::with_capacity(10000);
    let mut stats = ErrorStats::default();

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
            process_buffer(&leftover, base_day, force_ansi, &mut result, &mut stats);
            leftover.clear();
        } else {
            process_buffer(data, base_day, force_ansi, &mut result, &mut stats);
        }

        if let Some(pos) = data.iter().rposition(|&b| b == b'\n') {
            leftover.clear();
            leftover.extend_from_slice(&data[pos + 1..]);
        }
    }

    Ok((result, stats))
}

pub fn process_file_gz(
    file_path: &str
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats)> {

    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let filename = match Path::new(file_path)
        .file_name()
        .and_then(|s| s.to_str())
    {
        Some(f) => f,
        None => {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
    };

    let (y, m, d) = match parse_file_date(filename) {
        Some(v) => v,
        None => {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
    };

    let base_day = to_unix_days(y, m, d);

    let mut force_ansi = false;

    process_reader(reader, base_day, &mut force_ansi)
}

pub fn process_file_plain(
    file_path: &str,
    start_line: usize,
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats, usize, String)> {

    let file_bytes = std::fs::read(file_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let file_hash = encode(hasher.finalize());

    let file = File::open(file_path)?;
    let reader = BufReader::with_capacity(BUF_SIZE, file);

    let base_day = tools::file_modified_days_local(file_path)?;

    let mut force_ansi = false;

    let (mut result, stats) = process_reader(reader, base_day, &mut force_ansi)?;

    let total_lines = std::fs::read_to_string(file_path)?.lines().count();

    if start_line >= total_lines {
        return Ok((Vec::new(), stats, total_lines, file_hash));
    }

    result.drain(0..start_line);

    Ok((result, stats, total_lines, file_hash))
}

#[inline(always)]
fn process_buffer(
    data: &[u8],
    base_day: i64,
    force_ansi: &mut bool,
    result: &mut Vec<(i64, String)>,
    stats: &mut ErrorStats,
) {
    let mut start = 0;

    for i in 0..data.len() {
        if data[i] == b'\n' {
            let line = &data[start..i];
            start = i + 1;

            if memchr(b'H', line).is_none() {
                continue;
            }

            let msg_bytes = match extract_chat_message(line) {
                Some(m) => m,
                None => continue, // ✔ просто пропускаем
            };

            let parsed = if line.len() >= 9 && line.get(3) == Some(&b':') {
                parse_hms(line)
            } else {
                parse_hms_long(line)
            };

            let (h, m, s) = match parsed {
                Some(v) => v,
                None => {
                    stats.parse_errors += 1;
                    continue;
                }
            };

            let unix_time = base_day * 86400 + (h * 3600 + m * 60 + s) as i64;

            let text: std::borrow::Cow<str> = if *force_ansi {
                WINDOWS_1251.decode(msg_bytes).0
            } else {
                match std::str::from_utf8(msg_bytes) {
                    Ok(s) => std::borrow::Cow::Borrowed(s),
                    Err(_) => {
                        *force_ansi = true;
                        WINDOWS_1251.decode(msg_bytes).0
                    }
                }
            };

            result.push((unix_time, text.into_owned()));
        }
    }
}

// HELPERS

#[inline(always)]
fn extract_chat_message(line: &[u8]) -> Option<&[u8]> {
    let pos = memmem::find(line, b"[CHAT]")?;

    let mut start = pos + 6;

    while start < line.len() && line[start] == b' ' {
        start += 1;
    }

    if start >= line.len() {
        return None;
    }

    Some(&line[start..])
}

#[inline(always)]
fn parse_file_date(name: &str) -> Option<(i32, u32, u32)> {
    let b = name.as_bytes();

    if b.len() < 10 {
        return None;
    }

    let to_digit = |c: u8| (c as char).to_digit(10);

    let year = to_digit(b[0])? * 1000
        + to_digit(b[1])? * 100
        + to_digit(b[2])? * 10
        + to_digit(b[3])?;

    let month = to_digit(b[5])? * 10 + to_digit(b[6])?;
    let day = to_digit(b[8])? * 10 + to_digit(b[9])?;

    Some((year as i32, month, day))
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

    if i + 8 >= line.len() {
        return None;
    }

    // ожидаем формат HH:MM:SS
    let h1 = line.get(i + 1)?;
    let h2 = line.get(i + 2)?;
    let m1 = line.get(i + 4)?;
    let m2 = line.get(i + 5)?;
    let s1 = line.get(i + 7)?;
    let s2 = line.get(i + 8)?;

    // 🔥 защита от паники
    if !h1.is_ascii_digit()
        || !h2.is_ascii_digit()
        || !m1.is_ascii_digit()
        || !m2.is_ascii_digit()
        || !s1.is_ascii_digit()
        || !s2.is_ascii_digit()
    {
        return None;
    }

    let h = (h1 - b'0') as u32 * 10 + (h2 - b'0') as u32;
    let m = (m1 - b'0') as u32 * 10 + (m2 - b'0') as u32;
    let s = (s1 - b'0') as u32 * 10 + (s2 - b'0') as u32;

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