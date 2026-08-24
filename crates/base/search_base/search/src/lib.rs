use crossbeam_channel::{Receiver, bounded};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::thread;
use zstd::stream::read::Decoder as ZstdDecoder;

use progress::ParseProgress;

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

            _ => Err(format!(
                "Bad query: unexpected token at position {}",
                i
            )),
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

    let decoder =
        ZstdDecoder::new(file).map_err(|e| format!("Broken zstd base file: {}", e))?;

    Ok(Box::new(BufReader::new(decoder)))
}

fn worker(
    rx: Receiver<Vec<String>>,
    expr: Arc<Expr>,
    progress: Arc<ParseProgress>,
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

            let text = line[13..].to_lowercase();

            if matches(&expr, &text) {
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
) -> Result<(Arc<ParseProgress>, thread::JoinHandle<Result<SearchResult, String>>), String> {
    compile_query(query)?;

    let config = read_base_info::load_log(&name)?;
    let path = read_base_info::get_zst_path(&name)?;

    let query = query.to_string();
    let progress = Arc::new(ParseProgress::new());

    let max_lines: u64 = config.get("lines").and_then(|v| v.as_u64()).unwrap_or(0);

    const MAX_CAPACITY: usize = 10_000;

    progress.set_max_progress(max_lines);

    let handle = {
        let progress = progress.clone();

        thread::spawn(move || -> Result<SearchResult, String> {
            let reader = open_zstd(&path)?;

            let tokens = tokenize(&query);
            let expr = Arc::new(parse(&tokens)?);

            let (tx, rx) = bounded::<Vec<String>>(num_cpus::get() * 2);

            let threads = num_cpus::get();

            let handles: Vec<std::thread::JoinHandle<Vec<(u64, String)>>> = (0..threads)
                .map(|_| {
                    let rx = rx.clone();
                    let expr = expr.clone();
                    let progress = progress.clone();

                    thread::spawn(move || worker(rx, expr, progress))
                })
                .collect();

            let mut batch = Vec::with_capacity(MAX_CAPACITY);

            for line in reader.lines() {
                if let Ok(line) = line {
                    batch.push(line);

                    if batch.len() >= MAX_CAPACITY {
                        if tx.send(batch).is_err() {
                            return Err("Search workers terminated unexpectedly".to_string());
                        }
                        batch = Vec::with_capacity(MAX_CAPACITY);
                    }
                }
            }

            if !batch.is_empty() {
                let _ = tx.send(batch);
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
