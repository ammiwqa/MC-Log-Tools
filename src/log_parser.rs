use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufReader, BufRead, Read};
use std::path::Path;
use sha2::{Sha256, Digest};
use encoding_rs::WINDOWS_1251;
use memchr::{memchr, memmem};
use indicatif::ProgressBar;
use std::sync::Arc;
use std::collections::HashMap;




const BUF_SIZE: usize = 256 * 1024;

type PlainInput = HashMap<String, usize>;

#[derive(Clone, Debug)]
pub struct FileMeta {
    path: String,
    line: usize,
    hash: String,
}

enum Processed {
    Gz {
        path: String,
        data: Vec<(i64, String)>,
    },
    Plain {
        path: String,
        data: Vec<(i64, String)>,
        line: usize,
        hash: String,
    },
}

// =======================================================
// PARSE LOGS
// =======================================================
pub fn parse_logs(
    files: Vec<String>,
    plain_map: PlainInput,
    pb: &Arc<ProgressBar>,
) -> std::io::Result<(Vec<(i64, String)>, Vec<FileMeta>)> {

    let results: std::io::Result<Vec<Processed>> = files
        .par_iter()
        .map(|f| {
            let res = if f.ends_with(".log.gz") {
                process_file_gz(f)
            } else if f.ends_with(".log") {
                let start_line = 0; // временно

                process_file_plain(f, start_line)
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unsupported file type: {}", f),
                ));
            };

            pb.inc(1);
            res
        })
        .collect();

    let results = results?;

    let mut all = Vec::new();
    let mut latest = Vec::new();

    for r in results {
        match r {
            Processed::Gz { path: _, mut data } => {
                all.append(&mut data);
            }

            Processed::Plain { path, data, line, hash } => {
                all.extend(data);

                latest.push(FileMeta {
                    path,
                    line,
                    hash,
                });
            }
        }
    }

    all.sort_unstable_by_key(|x| x.0);

    // DEBUG OUTPUT
    for item in &latest {
        println!(
            "{:<40} {:<6} {}",
            item.path,
            item.line,
            item.hash
        );
    }

    println!("latest count: {}", latest.len());

    Ok((all, latest))
}

// =======================================================
// GZ PROCESSING
// =======================================================
pub fn process_file_gz(file_path: &str) -> std::io::Result<Processed> {
    let file = File::open(file_path)?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let filename = Path::new(file_path)
        .file_name()
        .unwrap()
        .to_string_lossy();

    let (y, m, d) = parse_file_date(&filename);
    let base_day = to_unix_days(y, m, d);

    let mut force_ansi = false;

    let data = process_reader(reader, base_day, &mut force_ansi)?;

    Ok(Processed::Gz {
        path: file_path.to_string(),
        data,
    })
}

// =======================================================
// PLAIN PROCESSING
// =======================================================
pub fn process_file_plain(
    file_path: &str,
    start_line: usize,
) -> std::io::Result<Processed> {

    let file_bytes = std::fs::read(file_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let hash = format!("{:x}", hasher.finalize());

    let file = File::open(file_path)?;
    let mut reader = BufReader::with_capacity(BUF_SIZE, file);

    let filename = Path::new(file_path)
        .file_name()
        .unwrap()
        .to_string_lossy();

    let (y, m, d) = parse_file_date(&filename);
    let base_day = to_unix_days(y, m, d);

    let mut force_ansi = false;

    let mut line = String::new();
    let mut current = 0;

    for _ in 0..start_line {
        if reader.read_line(&mut line)? == 0 {
            return Ok(Processed::Plain {
                path: file_path.to_string(),
                data: vec![],
                line: current,
                hash,
            });
        }
        current += 1;
        line.clear();
    }

    let data = process_reader(reader, base_day, &mut force_ansi)?;

    Ok(Processed::Plain {
        path: file_path.to_string(),
        data,
        line: start_line,
        hash,
    })
}



pub fn process_reader<R: Read>(
    mut reader: BufReader<R>,
    base_day: i64,
    force_ansi: &mut bool,
) -> std::io::Result<Vec<(i64, String)>> {

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
            process_buffer(&leftover, base_day, force_ansi, &mut result);
            leftover.clear();
        } else {
            process_buffer(data, base_day, force_ansi, &mut result);
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

            let msg_bytes = match extract_chat_message(line) {
                Some(m) => m,
                None => continue,
            };

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