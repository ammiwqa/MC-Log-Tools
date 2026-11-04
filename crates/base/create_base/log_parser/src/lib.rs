pub use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use tools;

use hex::encode;

use encoding_rs::WINDOWS_1251;
use memchr::{memchr, memchr_iter, memmem};

use progress::ProgressReporter;

const BUF_SIZE: usize = 1024 * 1024;
const COMP_BUF_SIZE: usize = 256 * 1024;

#[derive(Default, Debug, Clone)]
pub struct ErrorStats {
    pub file_errors: usize,
    pub parse_errors: usize,
}

pub fn parse_logs<P: ProgressReporter + Sync + ?Sized>(
    files: Vec<String>,
    progress: &P,
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats)> {
    let (all, stats) = files
        .par_iter()
        .fold(
            || (Vec::with_capacity(4096), ErrorStats::default()),
            |mut acc, f| {
                match process_file_gz(f) {
                    Ok((v, s)) => {
                        acc.0.extend(v);
                        acc.1.parse_errors += s.parse_errors;
                    }
                    Err(_e) => {
                        acc.1.file_errors += 1;
                    }
                }
                progress.inc(1);
                acc
            },
        )
        .reduce(
            || (Vec::new(), ErrorStats::default()),
            |mut a, b| {
                a.0.extend(b.0);
                a.1.parse_errors += b.1.parse_errors;
                a.1.file_errors += b.1.file_errors;
                a
            },
        );

    Ok((all, stats))
}

const PARSE_BATCH_LINES: usize = 8_192;

enum ParseMessage {
    Lines(Vec<(i64, String)>),
    FileDone(ErrorStats),
    FileFailed,
}

pub fn parse_logs_streaming<P, F>(
    files: Vec<String>,
    progress: &P,
    mut sink: F,
) -> std::io::Result<ErrorStats>
where
    P: ProgressReporter + Sync + ?Sized,
    F: FnMut(Vec<(i64, String)>) -> std::io::Result<()>,
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<ParseMessage>(4);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            files.par_iter().for_each_with(tx, |sender, file| {
                let mut batch: Vec<(i64, String)> = Vec::with_capacity(PARSE_BATCH_LINES);

                let result = process_file_gz_with(file, |line| {
                    batch.push(line);

                    if batch.len() >= PARSE_BATCH_LINES {
                        let out = std::mem::replace(
                            &mut batch,
                            Vec::with_capacity(PARSE_BATCH_LINES),
                        );

                        sender.send(ParseMessage::Lines(out)).map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::BrokenPipe,
                                "sort sink was dropped",
                            )
                        })?;
                    }

                    Ok(())
                });

                if !batch.is_empty() {
                    let _ = sender.send(ParseMessage::Lines(batch));
                }

                match result {
                    Ok(stats) => {
                        let _ = sender.send(ParseMessage::FileDone(stats));
                    }
                    Err(_) => {
                        let _ = sender.send(ParseMessage::FileFailed);
                    }
                }

                progress.inc(1);
            });
        });

        let mut total = ErrorStats::default();
        let mut sink_error = None;

        for message in rx {
            match message {
                ParseMessage::Lines(lines) => {
                    if sink_error.is_none() {
                        if let Err(err) = sink(lines) {
                            sink_error = Some(err);
                        }
                    }
                }
                ParseMessage::FileDone(stats) => {
                    total.parse_errors += stats.parse_errors;
                }
                ParseMessage::FileFailed => {
                    total.file_errors += 1;
                }
            }
        }

        match sink_error {
            Some(err) => Err(err),
            None => Ok(total),
        }
    })
}

pub fn parse_latest<P: ProgressReporter + Sync + ?Sized>(
    latest: Vec<(String, usize)>,
    progress: &P,
) -> std::io::Result<(Vec<(i64, String)>, Vec<(String, usize, String)>, ErrorStats)> {
    let results: Vec<_> = latest
        .par_iter()
        .map(|(file, value)| {
            let res = process_file_plain(file, *value)
                .map(|(result, stats, line, hash)| (file.clone(), result, stats, line, hash));

            progress.inc(1);
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
    let mut result = Vec::with_capacity(100_000);

    let stats = process_reader_with(
        &mut reader,
        base_day,
        force_ansi,
        |line| {
            result.push(line);
            Ok(())
        },
    )?;

    Ok((result, stats))
}


pub fn process_reader_with<R, F>(
    reader: &mut BufReader<R>,
    base_day: i64,
    force_ansi: &mut bool,
    mut emit: F,
) -> std::io::Result<ErrorStats>
where
    R: Read,
    F: FnMut((i64, String)) -> std::io::Result<()>,
{
    let mut stats = ErrorStats::default();

    let mut buf = vec![0u8; BUF_SIZE];
    let mut carry: Vec<u8> = Vec::new();
    let mut scratch: Vec<(i64, String)> = Vec::new();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        if carry.is_empty() {
            match buf[..n].iter().rposition(|&b| b == b'\n') {
                Some(pos) => {
                    process_and_emit(
                        &buf[..=pos],
                        base_day,
                        force_ansi,
                        &mut scratch,
                        &mut stats,
                        &mut emit,
                    )?;

                    let tail = &buf[pos + 1..n];
                    if !tail.is_empty() {
                        carry.extend_from_slice(tail);
                    }
                }
                None => carry.extend_from_slice(&buf[..n]),
            }
        } else {
            carry.extend_from_slice(&buf[..n]);

            if let Some(pos) = carry.iter().rposition(|&b| b == b'\n') {
                process_and_emit(
                    &carry[..=pos],
                    base_day,
                    force_ansi,
                    &mut scratch,
                    &mut stats,
                    &mut emit,
                )?;

                carry.drain(..=pos);
            }
        }
    }

    Ok(stats)
}

fn process_and_emit<F>(
    data: &[u8],
    base_day: i64,
    force_ansi: &mut bool,
    scratch: &mut Vec<(i64, String)>,
    stats: &mut ErrorStats,
    emit: &mut F,
) -> std::io::Result<()>
where
    F: FnMut((i64, String)) -> std::io::Result<()>,
{
    scratch.clear();
    process_buffer(data, base_day, force_ansi, scratch, stats);

    for line in scratch.drain(..) {
        emit(line)?;
    }

    Ok(())
}

type GzReader = BufReader<GzDecoder<BufReader<File>>>;

fn open_gz_reader(file_path: &str) -> std::io::Result<(GzReader, i64)> {
    let file = File::open(file_path)?;

    let raw = BufReader::with_capacity(COMP_BUF_SIZE, file);
    let decoder = GzDecoder::new(raw);
    let reader = BufReader::with_capacity(BUF_SIZE, decoder);

    let filename = match Path::new(file_path).file_name().and_then(|s| s.to_str()) {
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

    Ok((reader, to_unix_days(y, m, d)))
}

pub fn process_file_gz(file_path: &str) -> std::io::Result<(Vec<(i64, String)>, ErrorStats)> {
    let (reader, base_day) = open_gz_reader(file_path)?;
    let mut force_ansi = false;

    process_reader(reader, base_day, &mut force_ansi)
}

pub fn process_file_gz_with<F>(
    file_path: &str,
    emit: F,
) -> std::io::Result<ErrorStats>
where
    F: FnMut((i64, String)) -> std::io::Result<()>,
{
    let (mut reader, base_day) = open_gz_reader(file_path)?;
    let mut force_ansi = false;

    process_reader_with(&mut reader, base_day, &mut force_ansi, emit)
}

pub fn process_file_plain(
    file_path: &str,
    start_line: usize,
) -> std::io::Result<(Vec<(i64, String)>, ErrorStats, usize, String)> {
    let file_bytes = std::fs::read(file_path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("Failed to read file: {}", e)))?;

    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let file_hash = encode(hasher.finalize());

    let base_day = match tools::file_modified_days_local(file_path) {
        Ok(d) => d,
        Err(e) => {
            return Err(std::io::Error::new(
                e.kind(),
                format!("Failed to get file date: {}", e),
            ));
        }
    };

    let mut force_ansi = false;
    let mut result = Vec::with_capacity(100_000);
    let mut stats = ErrorStats::default();

    process_buffer(
        &file_bytes,
        base_day,
        &mut force_ansi,
        &mut result,
        &mut stats,
    );

    let total_parsed = result.len();

    if start_line >= total_parsed {
        return Ok((Vec::new(), stats, total_parsed, file_hash));
    }

    result.drain(0..start_line);

    Ok((result, stats, total_parsed, file_hash))
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

    for i in memchr_iter(b'\n', data) {
        let line = &data[start..i];
        start = i + 1;

        if memchr(b'H', line).is_none() {
            continue;
        }

        let msg_bytes = match extract_chat_message(line) {
            Some(m) => m,
            None => continue,
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

        let owned;
        let text: &str = if *force_ansi {
            owned = WINDOWS_1251.decode(msg_bytes).0.into_owned();
            &owned
        } else {
            match std::str::from_utf8(msg_bytes) {
                Ok(s) => s,
                Err(_) => {
                    *force_ansi = true;
                    owned = WINDOWS_1251.decode(msg_bytes).0.into_owned();
                    &owned
                }
            }
        };

        let text = text.trim();

        if text.is_empty() {
            continue;
        }

        result.push((unix_time, text.to_owned()));
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
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }

    let to_digit = |c: u8| (c as char).to_digit(10);

    let year =
        to_digit(b[0])? * 1000 + to_digit(b[1])? * 100 + to_digit(b[2])? * 10 + to_digit(b[3])?;

    let month = to_digit(b[5])? * 10 + to_digit(b[6])?;
    let day = to_digit(b[8])? * 10 + to_digit(b[9])?;

    Some((year as i32, month, day))
}

#[inline(always)]
fn parse_hms(line: &[u8]) -> Option<(u32, u32, u32)> {
    if line.len() < 9 {
        return None;
    }

    let h = (line[1].wrapping_sub(b'0')) as u32 * 10 + (line[2].wrapping_sub(b'0')) as u32;
    let m = (line[4].wrapping_sub(b'0')) as u32 * 10 + (line[5].wrapping_sub(b'0')) as u32;
    let s = (line[7].wrapping_sub(b'0')) as u32 * 10 + (line[8].wrapping_sub(b'0')) as u32;

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

    // HH:MM:SS
    let h1 = line.get(i + 1)?;
    let h2 = line.get(i + 2)?;
    let m1 = line.get(i + 4)?;
    let m2 = line.get(i + 5)?;
    let s1 = line.get(i + 7)?;
    let s2 = line.get(i + 8)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn base_day() -> i64 {
        to_unix_days(2024, 1, 15)
    }

    fn unix_for(h: u32, m: u32, s: u32, base_day: i64) -> i64 {
        base_day * 86400 + (h as i64) * 3600 + (m as i64) * 60 + (s as i64)
    }

    fn run_process(data: &[u8], base_day: i64) -> (Vec<(i64, String)>, ErrorStats, bool) {
        let mut force_ansi = false;
        let reader = std::io::BufReader::new(Cursor::new(data));
        let (lines, stats) = process_reader(reader, base_day, &mut force_ansi).unwrap();
        (lines, stats, force_ansi)
    }

    // ----------------------------------------------------------------
    // extract_chat_message
    // ----------------------------------------------------------------
    #[test]
    fn extract_chat_message_returns_text_after_chat_tag() {
        let line = b"[12:34:56] [CHAT] hello world";
        assert_eq!(extract_chat_message(line), Some(b"hello world".as_slice()));
    }

    #[test]
    fn extract_chat_message_skips_multiple_spaces() {
        let line = b"[CHAT]   spaced   out";
        assert_eq!(extract_chat_message(line), Some(b"spaced   out".as_slice()));
    }

    #[test]
    fn extract_chat_message_nothing_after_tag_is_none() {
        let line = b"[CHAT]";
        assert_eq!(extract_chat_message(line), None);
    }

    #[test]
    fn extract_chat_message_only_spaces_after_tag_is_none() {
        let line = b"[CHAT]   ";
        assert_eq!(extract_chat_message(line), None);
    }

    #[test]
    fn extract_chat_message_missing_tag_is_none() {
        let line = b"[12:34:56] hello world";
        assert_eq!(extract_chat_message(line), None);
    }

    #[test]
    fn extract_chat_message_double_tag_returns_rest_after_first() {
        let line = b"[CHAT][CHAT] second";
        assert_eq!(extract_chat_message(line), Some(b"[CHAT] second".as_slice()));
    }

    // ----------------------------------------------------------------
    // parse_hms  (format [HH:MM:SS])
    // ----------------------------------------------------------------
    #[test]
    fn parse_hms_valid_full_range() {
        assert_eq!(parse_hms(b"[23:59:59]"), Some((23, 59, 59)));
        assert_eq!(parse_hms(b"[00:00:00]"), Some((0, 0, 0)));
        assert_eq!(parse_hms(b"[12:34:56]"), Some((12, 34, 56)));
    }

    #[test]
    fn parse_hms_hour_out_of_range() {
        assert_eq!(parse_hms(b"[24:00:00]"), None);
        assert_eq!(parse_hms(b"[99:00:00]"), None);
    }

    #[test]
    fn parse_hms_minute_out_of_range() {
        assert_eq!(parse_hms(b"[12:60:00]"), None);
    }

    #[test]
    fn parse_hms_second_out_of_range() {
        assert_eq!(parse_hms(b"[12:34:60]"), None);
    }

    #[test]
    fn parse_hms_too_short() {
        assert_eq!(parse_hms(b"[12:34"), None);
        assert_eq!(parse_hms(b"[12"), None);
        assert_eq!(parse_hms(b"[]"), None);
    }

    #[test]
    fn parse_hms_non_digits() {
        assert_eq!(parse_hms(b"[ab:cd:ef]"), None);
    }

    // ----------------------------------------------------------------
    // parse_hms_long  (text HH:MM:SS ...)
    // ----------------------------------------------------------------
    #[test]
    fn parse_hms_long_valid() {
        assert_eq!(parse_hms_long(b"2024-01-15 12:34:56 [CHAT] hi"), Some((12, 34, 56)));
        assert_eq!(parse_hms_long(b"[12:34:56] x"), None); // no space before time
    }

    #[test]
    fn parse_hms_long_time_at_end() {
        assert_eq!(parse_hms_long(b"prefix 00:00:01"), Some((0, 0, 1)));
    }

    #[test]
    fn parse_hms_long_invalid_ranges() {
        assert_eq!(parse_hms_long(b"x 24:00:00"), None);
        assert_eq!(parse_hms_long(b"x 12:60:00"), None);
        assert_eq!(parse_hms_long(b"x 12:34:60"), None);
    }

    #[test]
    fn parse_hms_long_too_short_after_space() {
        assert_eq!(parse_hms_long(b"x 12:34:5"), None);
        assert_eq!(parse_hms_long(b"x "), None);
    }

    #[test]
    fn parse_hms_long_non_digits_in_time() {
        assert_eq!(parse_hms_long(b"x ab:cd:ef"), None);
    }

    #[test]
    fn parse_hms_long_no_space_at_all() {
        assert_eq!(parse_hms_long(b"nospace12:34:56"), None);
    }

    // ----------------------------------------------------------------
    // parse_file_date  (YYYY-MM-DD inside the gz name)
    // ----------------------------------------------------------------
    #[test]
    fn parse_file_date_valid() {
        assert_eq!(parse_file_date("2024-01-15.log.gz"), Some((2024, 1, 15)));
        assert_eq!(parse_file_date("2020-12-31.gz"), Some((2020, 12, 31)));
        assert_eq!(parse_file_date("2024-01-15"), Some((2024, 1, 15)));
    }

    #[test]
    fn parse_file_date_too_short() {
        assert_eq!(parse_file_date("24-01-15"), None);
        assert_eq!(parse_file_date("2024-01-1"), None);
        assert_eq!(parse_file_date(""), None);
    }

    #[test]
    fn parse_file_date_wrong_separators() {
        assert_eq!(parse_file_date("2024/01/15.log.gz"), None);
        assert_eq!(parse_file_date("2024.01.15.log.gz"), None);
    }

    #[test]
    fn parse_file_date_non_digits() {
        assert_eq!(parse_file_date("abcd-ef-gh.log.gz"), None);
        assert_eq!(parse_file_date("2024-1a-15.log.gz"), None);
    }

    // ----------------------------------------------------------------
    // process_reader / process_buffer
    // ----------------------------------------------------------------
    #[test]
    fn process_reader_basic_chat_line() {
        let base_day = base_day();
        let data = b"[12:34:56] [CHAT] hello world\n";
        let (lines, stats, _ansi) = run_process(data, base_day);
        assert_eq!(stats.parse_errors, 0);
        assert_eq!(stats.file_errors, 0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, unix_for(12, 34, 56, base_day));
        assert_eq!(lines[0].1, "hello world");
    }

    #[test]
    fn process_reader_multiple_lines_preserve_order() {
        let base_day = base_day();
        let data = b"[01:02:03] [CHAT] first\n[23:59:59] [CHAT] second\n";
        let (lines, _stats, _ansi) = run_process(data, base_day);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].1, "first");
        assert_eq!(lines[1].1, "second");
        assert!(lines[0].0 < lines[1].0);
    }

    #[test]
    fn process_reader_skips_line_without_h() {
        let base_day = base_day();
        // no 'H' anywhere -> skipped entirely
        let data = b"[12:34:56] [cat] no h here\n";
        let (lines, stats, _ansi) = run_process(data, base_day);
        assert_eq!(lines.len(), 0);
        assert_eq!(stats.parse_errors, 0);
    }

    #[test]
    fn process_reader_skips_line_without_chat_tag() {
        let base_day = base_day();
        let data = b"[12:34:56] hello no chat tag\n";
        let (lines, stats, _ansi) = run_process(data, base_day);
        assert_eq!(lines.len(), 0);
        assert_eq!(stats.parse_errors, 0);
    }

    #[test]
    fn process_reader_bad_timestamp_counts_parse_error() {
        let base_day = base_day();
        let data = b"[99:99:99] [CHAT] bad time\n";
        let (_lines, stats, _ansi) = run_process(data, base_day);
        assert_eq!(stats.parse_errors, 1);
        assert_eq!(_lines.len(), 0);
    }

    #[test]
    fn process_reader_empty_message_is_skipped() {
        let base_day = base_day();
        let data = b"[12:34:56] [CHAT]   \n";
        let (lines, _stats, _ansi) = run_process(data, base_day);
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn process_reader_line_split_across_buffers() {
        let base_day = base_day();
        // feed in tiny chunks to exercise the carry/buffer logic
        let data = b"[12:34:56] [CHAT] part one\n[13:34:56] [CHAT] part two\n";
        let (lines, stats, _ansi) = run_process(data, base_day);
        assert_eq!(stats.parse_errors, 0);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].1, "part one");
        assert_eq!(lines[1].1, "part two");
    }

    #[test]
    fn process_reader_trailing_line_without_newline_is_ignored() {
        let base_day = base_day();
        let data = b"[12:34:56] [CHAT] complete\n[13:00:00] [CHAT] no newline at end";
        let (lines, _stats, _ansi) = run_process(data, base_day);
        // The last line has no trailing newline, so it is not emitted
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, "complete");
    }

    #[test]
    fn process_reader_non_utf8_forces_windows1251() {
        let base_day = base_day();
        // invalid UTF-8 byte inside the chat message
        let data: Vec<u8> = b"[12:34:56] [CHAT] privet \xff \x00\n".to_vec();
        let (_lines, stats, force_ansi) = run_process(&data, base_day);
        assert_eq!(stats.parse_errors, 0);
        assert!(force_ansi, "invalid UTF-8 should flip force_ansi to true");
    }

    #[test]
    fn process_reader_windows1251_cyrillic_decoded() {
        let base_day = base_day();
        // "привет" in Windows-1251: п=0xEF р=0xF0 и=0xE8 в=0xE2 е=0xE5 т=0xF2
        let msg: Vec<u8> = vec![0xEF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        let mut data = b"[12:34:56] [CHAT] ".to_vec();
        data.extend_from_slice(&msg);
        data.push(b'\n');
        let (lines, stats, force_ansi) = run_process(&data, base_day);
        assert_eq!(stats.parse_errors, 0);
        assert!(force_ansi);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, "привет");
    }

    #[test]
    fn error_stats_default_is_zero() {
        let s = ErrorStats::default();
        assert_eq!(s.file_errors, 0);
        assert_eq!(s.parse_errors, 0);
    }

    // ----------------------------------------------------------------
    // process_file_plain  (latest.log)
    // ----------------------------------------------------------------
    #[test]
    fn process_file_plain_parses_latest_log() {
        let dir = std::env::temp_dir().join(format!(
            "lt-log-parser-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest.log");
        std::fs::write(&path, "[10:00:00] [CHAT] line one\n[11:00:00] [CHAT] line two\n").unwrap();

        let (lines, stats, total_lines, _hash) =
            process_file_plain(path.to_str().unwrap(), 0).unwrap();
        assert_eq!(stats.parse_errors, 0);
        assert_eq!(lines.len(), 2);
        assert_eq!(total_lines, 2);
        assert_eq!(lines[0].1, "line one");
        assert_eq!(lines[1].1, "line two");
        assert!(!_hash.is_empty());
    }

    #[test]
    fn process_file_plain_start_line_skips_prefix() {
        let dir = std::env::temp_dir().join(format!(
            "lt-log-parser-start-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest.log");
        std::fs::write(&path, "[10:00:00] [CHAT] a\n[11:00:00] [CHAT] b\n[12:00:00] [CHAT] c\n").unwrap();

        let (lines, _stats, total_lines, _hash) = process_file_plain(path.to_str().unwrap(), 1).unwrap();
        assert_eq!(total_lines, 3);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].1, "b");
        assert_eq!(lines[1].1, "c");
    }

    #[test]
    fn process_file_plain_start_line_beyond_end_is_empty() {
        let dir = std::env::temp_dir().join(format!(
            "lt-log-parser-beyond-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest.log");
        std::fs::write(&path, "[10:00:00] [CHAT] a\n").unwrap();

        let (lines, _stats, total_lines, _hash) =
            process_file_plain(path.to_str().unwrap(), 999).unwrap();
        assert_eq!(total_lines, 1);
        assert!(lines.is_empty());
    }

    #[test]
    fn process_file_plain_nonexistent_file_is_error() {
        let err = process_file_plain("/nonexistent/path/latest.log", 0).unwrap_err();
        assert!(err.to_string().contains("Failed to read file"));
    }
}
