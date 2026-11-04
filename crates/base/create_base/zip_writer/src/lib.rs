use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use zstd::stream::read::Decoder;
use zstd::stream::write::Encoder;

use progress::ProgressReporter;

const TS_WIDTH: usize = 12;
const OUT_BUF_SIZE: usize = 16 * 1024 * 1024;

/// Streaming zstd writer for chunk files.
///
/// Unlike [`write_logs_to_zstd`], this keeps only the current output buffer in
/// memory and can be fed one parsed line at a time. It is used by the external
/// sort merge so a chunk never has to be materialized as a second `Vec`.
pub struct ZstdLogWriter {
    encoder: Option<Encoder<'static, BufWriter<File>>>,
    buf: Vec<u8>,
}

impl ZstdLogWriter {
    pub fn create(path: &str) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let buf_writer = BufWriter::with_capacity(OUT_BUF_SIZE, file);

        let mut encoder = Encoder::new(buf_writer, 1)?;
        encoder.multithread(num_cpus::get() as u32)?;

        Ok(Self {
            encoder: Some(encoder),
            buf: Vec::with_capacity(OUT_BUF_SIZE),
        })
    }

    /// Append one `(timestamp, message)` line to the chunk.
    pub fn write_line(&mut self, ts: i64, msg: &str) -> std::io::Result<()> {
        write_fixed_ts(&mut self.buf, ts);
        self.buf.push(b' ');
        self.buf.extend_from_slice(msg.as_bytes());
        self.buf.push(b'\n');

        if self.buf.len() >= OUT_BUF_SIZE {
            self.flush_buf()?;
        }

        Ok(())
    }

    fn flush_buf(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            if let Some(encoder) = self.encoder.as_mut() {
                encoder.write_all(&self.buf)?;
            }
            self.buf.clear();
        }

        Ok(())
    }

    /// Flush all buffered lines and finish the zstd frame.
    pub fn finish(mut self) -> std::io::Result<()> {
        self.flush_buf()?;

        if let Some(encoder) = self.encoder.take() {
            let mut writer = encoder.finish()?;
            writer.flush()?;
        }

        Ok(())
    }
}

impl Drop for ZstdLogWriter {
    fn drop(&mut self) {
        if self.encoder.is_some() {
            let _ = self.flush_buf();

            if let Some(encoder) = self.encoder.take() {
                if let Ok(mut writer) = encoder.finish() {
                    let _ = writer.flush();
                }
            }
        }
    }
}

pub fn write_logs_to_zstd_with_progress<P: ProgressReporter + ?Sized>(
    logs: &[(i64, String)],
    path: &str,
    progress: &P,
) -> std::io::Result<()> {
    write_logs_to_zstd(logs, path, progress)
}

pub fn write_logs_to_zstd<P: ProgressReporter + ?Sized>(
    logs: &[(i64, String)],
    path: &str,
    progress: &P,
) -> std::io::Result<()> {
    let mut writer = ZstdLogWriter::create(path)?;

    for (ts, msg) in logs {
        progress.inc(1);
        writer.write_line(*ts, msg)?;
    }

    writer.finish()
}

pub fn read_logs_from_zstd(path: &str) -> std::io::Result<Vec<(i64, String)>> {
    let file = File::open(path)?;

    let mut magic = [0u8; 4];
    let mut probe = File::open(path)?;
    let _ = probe.read(&mut magic)?;

    if magic != [0x28, 0xB5, 0x2F, 0xFD] {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Not a zstd file: {}", path),
        ));
    }

    let decoder = Decoder::new(file)?;
    let reader = BufReader::new(decoder);

    let mut logs = Vec::new();

    for line in reader.lines() {
        let line = line?;

        if line.len() < 13 {
            continue;
        }

        if let Ok(ts) = line[..12].parse::<i64>() {
            logs.push((ts, line[13..].to_owned()));
        }
    }

    Ok(logs)
}

#[inline(always)]
fn write_fixed_ts(buf: &mut Vec<u8>, ts: i64) {
    let mut tmp = [b'0'; TS_WIDTH];
    let mut n = ts;
    let mut i = TS_WIDTH;

    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    buf.extend_from_slice(&tmp);
}
