use crossbeam_channel::{Receiver, bounded};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::thread;
use zstd::stream::read::Decoder as ZstdDecoder;

use progress::ParseProgress;

use base_chunks::read_chunk;

#[derive(Clone)]
enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Pattern(Pattern),
}

#[derive(Clone)]
struct Pattern {
    parts: Vec<String>,
}

#[inline]
fn compile_pattern(s: &str) -> Pattern {
    Pattern {
        parts: s.split('*').map(|x| x.to_lowercase()).collect(),
    }
}

#[inline]
fn match_pattern(p: &Pattern, text: &str) -> bool {
    if p.parts.is_empty() {
        return true;
    }
    let text = text.to_lowercase();

    let mut pos = 0;

    for part in &p.parts {
        if part.is_empty() {
            continue;
        }

        match text[pos..].find(part) {
            Some(idx) => pos += idx + part.len(),
            None => return false,
        }
    }

    true
}

#[inline]
fn matches(expr: &Expr, text: &str) -> bool {
    match expr {
        Expr::And(a, b) => matches(a, text) && matches(b, text),
        Expr::Or(a, b) => matches(a, text) || matches(b, text),
        Expr::Not(a) => !matches(a, text),
        Expr::Pattern(p) => match_pattern(p, text),
    }
}

#[derive(Clone)]
enum Token {
    And,
    Or,
    Not,
    LParen,
    RParen,
    Pattern(String),
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '&' => tokens.push(Token::And),
            '|' => tokens.push(Token::Or),
            '-' => tokens.push(Token::Not),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            '!' => tokens.push(Token::Not),

            '<' => {
                let mut s = String::new();
                while let Some(ch) = chars.next() {
                    if ch == '>' {
                        break;
                    }
                    s.push(ch);
                }
                if !s.is_empty() {
                    tokens.push(Token::Pattern(s));
                }
            }

            ' ' => {}

            _ => {
                let mut s = String::new();
                s.push(c);

                while let Some(&ch) = chars.peek() {
                    if ch.is_whitespace() || "&|-()<>".contains(ch) {
                        break;
                    }
                    s.push(ch);
                    chars.next();
                }

                if !s.is_empty() {
                    tokens.push(Token::Pattern(s));
                }
            }
        }
    }

    tokens
}

fn parse(tokens: &[Token]) -> Result<Expr, String> {
    if tokens.is_empty() {
        return Ok(Expr::Pattern(Pattern { parts: Vec::new() }));
    }
    fn expr(tokens: &[Token], i: &mut usize) -> Result<Expr, String> {
        let mut node = term(tokens, i)?;

        while *i < tokens.len() {
            match tokens[*i] {
                Token::Or => {
                    *i += 1;
                    node = Expr::Or(Box::new(node), Box::new(term(tokens, i)?));
                }
                _ => break,
            }
        }

        Ok(node)
    }

    fn term(tokens: &[Token], i: &mut usize) -> Result<Expr, String> {
        let mut node = factor(tokens, i)?;

        while *i < tokens.len() {
            match tokens[*i] {
                Token::And => {
                    *i += 1;
                    node = Expr::And(Box::new(node), Box::new(factor(tokens, i)?));
                }
                _ => break,
            }
        }

        Ok(node)
    }

    fn factor(tokens: &[Token], i: &mut usize) -> Result<Expr, String> {
        match &tokens[*i] {
            Token::Not => {
                *i += 1;
                Ok(Expr::Not(Box::new(factor(tokens, i)?)))
            }

            Token::LParen => {
                *i += 1;
                let node = expr(tokens, i)?;
                *i += 1;
                Ok(node)
            }

            Token::Pattern(s) => {
                *i += 1;
                Ok(Expr::Pattern(compile_pattern(s)))
            }

            _ => Err(format!("Bad query: unexpected token at position {}", i)),
        }
    }

    let mut i = 0;
    expr(tokens, &mut i)
}

pub fn compile_query(query: &str) -> Result<(), String> {
    let tokens = tokenize(query);
    parse(&tokens).map(|_| ())
}

fn open_zstd(path: &str) -> Result<Box<dyn BufRead>, String> {
    let mut file = File::open(path).map_err(|e| format!("Cannot open base file: {}", e))?;

    let mut magic = [0u8; 4];
    let _n = file
        .read(&mut magic)
        .map_err(|e| format!("Cannot read base file: {}", e))?;

    if magic != [0x28, 0xB5, 0x2F, 0xFD] {
        return Err(format!("Not a zstd file: {}", path));
    }

    let file = File::open(path).map_err(|e| format!("Cannot open base file: {}", e))?;

    let decoder = ZstdDecoder::new(file).map_err(|e| format!("Broken zstd base file: {}", e))?;

    Ok(Box::new(BufReader::new(decoder)))
}

fn worker(
    rx: Receiver<Vec<String>>,
    expr: Arc<Expr>,
    progress: Arc<ParseProgress>,
    from_unix: Option<u64>,
    to_unix: Option<u64>,
) -> Vec<(u64, String)> {
    let mut out = Vec::new();

    while let Ok(batch) = rx.recv() {
        let mut local_count = 0;

        for line in batch {
            if line.len() <= 13 {
                continue;
            }

            let unix = match line[0..12].parse::<u64>() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(f) = from_unix {
                if unix < f {
                    continue;
                }
            }
            if let Some(t) = to_unix {
                if unix > t {
                    continue;
                }
            }

            let text = &line[13..];

            if matches(&expr, text) {
                out.push((unix, line));
            }

            local_count += 1;

            if local_count == 32 {
                progress.inc_progress(32);
                local_count = 0;
            }
        }

        if local_count > 0 {
            progress.inc_progress(local_count);
        }
    }
    out
}

pub type SearchResult = Vec<(u64, String)>;

pub fn search_async(
    name: &str,
    query: &str,
) -> Result<
    (
        Arc<ParseProgress>,
        thread::JoinHandle<Result<SearchResult, String>>,
    ),
    String,
> {
    compile_query(query)?;

    let config = read_base_info::load_log(&name)?;
    let chunks = read_base_info::chunk_files(name, &config)?;

    let query = query.to_string();
    let progress = Arc::new(ParseProgress::new());

    let max_lines: u64 = config.lines;

    const MAX_CAPACITY: usize = 10_000;

    progress.set_max_progress(max_lines);

    let handle = {
        let progress = progress.clone();

        thread::spawn(move || -> Result<SearchResult, String> {
            let tokens = tokenize(&query);
            let expr = Arc::new(parse(&tokens)?);

            let (tx, rx) = bounded::<Vec<String>>(num_cpus::get() * 2);

            let threads = num_cpus::get();

            let handles: Vec<std::thread::JoinHandle<Vec<(u64, String)>>> = (0..threads)
                .map(|_| {
                    let rx = rx.clone();
                    let expr = expr.clone();
                    let progress = progress.clone();

                    thread::spawn(move || worker(rx, expr, progress, None, None))
                })
                .collect();

            for (path, _first_ts, _last_ts) in &chunks {
                let reader = open_zstd(path)?;

                let mut batch = Vec::with_capacity(MAX_CAPACITY);

                for line in reader.lines() {
                    let Ok(line) = line else {
                        continue;
                    };

                    batch.push(line);

                    if batch.len() >= MAX_CAPACITY {
                        if tx.send(batch).is_err() {
                            return Err(" workers terminated unexpectedly".to_string());
                        }
                        batch = Vec::with_capacity(MAX_CAPACITY);
                    }
                }

                if !batch.is_empty() {
                    if tx.send(batch).is_err() {
                        return Err(" workers terminated unexpectedly".to_string());
                    }
                }
            }

            drop(tx);

            let mut results = Vec::new();

            for h in handles {
                match h.join() {
                    Ok(mut r) => results.append(&mut r),
                    Err(_) => return Err("A search worker panicked".to_string()),
                }
            }

            results.sort_unstable_by_key(|x| x.0);

            Ok(results)
        })
    };

    Ok((progress, handle))
}

/// Search with an optional unix-timestamp date range.
///
/// Chunks are selected with a binary search over their `[first_ts, last_ts]`
/// bounds, and inside each chunk lines are located with a binary search since
/// they are stored sorted by timestamp. When `use_date_filtering` is `false`
/// the whole base is scanned (same as [`search_async`]).
pub fn search_async_date_range(
    name: &str,
    query: &str,
    date_from: Option<i64>,
    date_to: Option<i64>,
    use_date_filtering: bool,
) -> Result<
    (
        Arc<ParseProgress>,
        thread::JoinHandle<Result<SearchResult, String>>,
    ),
    String,
> {
    compile_query(query)?;

    let config = read_base_info::load_log(&name)?;
    let chunks = read_base_info::chunk_files(name, &config)?;

    let query = query.to_string();
    let progress = Arc::new(ParseProgress::new());

    let max_lines: u64 = config.lines;
    const MAX_CAPACITY: usize = 10_000;

    progress.set_max_progress(max_lines);

    let from_u64 = if use_date_filtering {
        date_from.map(|v| if v < 0 { 0 } else { v as u64 })
    } else {
        None
    };
    let to_u64 = if use_date_filtering {
        date_to.map(|v| if v < 0 { 0 } else { v as u64 })
    } else {
        None
    };

    let handle = {
        let progress = progress.clone();
        let from_u64 = from_u64;
        let to_u64 = to_u64;

        thread::spawn(move || -> Result<SearchResult, String> {
            let tokens = tokenize(&query);
            let expr = Arc::new(parse(&tokens)?);

            let (tx, rx) = bounded::<Vec<String>>(num_cpus::get() * 2);
            let threads = num_cpus::get();

            let handles: Vec<std::thread::JoinHandle<Vec<(u64, String)>>> = (0..threads)
                .map(|_| {
                    let rx = rx.clone();
                    let expr = expr.clone();
                    let progress = progress.clone();

                    thread::spawn(move || worker(rx, expr, progress, from_u64, to_u64))
                })
                .collect();

            let range = if use_date_filtering {
                let f = date_from.unwrap_or(i64::MIN);
                let t = date_to.unwrap_or(i64::MAX);
                let first_overlap = chunks.partition_point(|c| c.2 < f);
                let first_past_t = chunks.partition_point(|c| c.1 <= t);
                if first_overlap < first_past_t {
                    Some(first_overlap..first_past_t)
                } else {
                    None
                }
            } else {
                Some(0..chunks.len())
            };

            if let Some(range) = range {
                for idx in range {
                    let (path, _, _) = &chunks[idx];
                    let lines = match read_chunk(path) {
                        Ok(l) => l,
                        Err(e) => return Err(format!("Cannot read chunk '{}': {}", path, e)),
                    };

                    let cstart = if use_date_filtering {
                        date_from
                            .map(|f| lines.partition_point(|l| l.0 < f))
                            .unwrap_or(0)
                    } else {
                        0
                    };

                    let mut batch = Vec::with_capacity(MAX_CAPACITY);

                    for (ts, text) in lines.into_iter().skip(cstart) {
                        if use_date_filtering {
                            if let Some(t) = date_to {
                                if ts > t {
                                    break;
                                }
                            }
                        }

                        batch.push(format!("{:012} {}", ts, text));

                        if batch.len() >= MAX_CAPACITY {
                            if tx.send(batch).is_err() {
                                return Err("workers terminated unexpectedly".to_string());
                            }
                            batch = Vec::with_capacity(MAX_CAPACITY);
                        }
                    }

                    if !batch.is_empty() {
                        if tx.send(batch).is_err() {
                            return Err("workers terminated unexpectedly".to_string());
                        }
                    }
                }
            }

            drop(tx);

            let mut results = Vec::new();
            for h in handles {
                match h.join() {
                    Ok(mut r) => results.append(&mut r),
                    Err(_) => return Err("A search worker panicked".to_string()),
                }
            }

            results.sort_unstable_by_key(|x| x.0);

            Ok(results)
        })
    };

    Ok((progress, handle))
}

pub fn read_context_around(
    name: &str,
    target_ts: i64,
    n_before: usize,
    n_after: usize,
) -> Result<Vec<(i64, String)>, String> {
    let config = read_base_info::load_log(&name)?;
    let chunks = read_base_info::chunk_files(name, &config)?;

    let cand = chunks.partition_point(|c| c.1 <= target_ts).saturating_sub(1);
    if cand >= chunks.len() {
        return Ok(Vec::new());
    }
    if chunks[cand].2 < target_ts {
        return Ok(Vec::new());
    }

    let (path, _, _) = &chunks[cand];
    let lines = read_chunk(path)
        .map_err(|e| format!("Cannot read chunk '{}': {}", path, e))?;

    let pos = lines.partition_point(|l| l.0 < target_ts);
    let center = pos.min(lines.len().saturating_sub(1));
    let start = center.saturating_sub(n_before);
    let end = (center + 1 + n_after).min(lines.len());

    Ok(lines[start..end].to_vec())
}
