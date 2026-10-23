use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use hex::encode;
use sha2::{Digest, Sha256};
use xxhash_rust::xxh3::Xxh3;

pub const HASH_BUFFER_SIZE: usize = 256 * 1024;

#[cfg(windows)]
fn open_file_read(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    File::options()
        .read(true)
        .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
        .open(path)
}

#[cfg(not(windows))]
fn open_file_read(path: &Path) -> io::Result<File> {
    File::open(path)
}

pub fn compute_file_hash(path: &Path) -> io::Result<String> {
    let file = open_file_read(path)?;
    let mut reader = io::BufReader::with_capacity(HASH_BUFFER_SIZE, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];

    loop {
        let bytes_read = reader.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(encode(hasher.finalize()))
}

fn skip_gzip_string(file: &mut File) -> io::Result<()> {
    let mut buffer = [0u8; 256];

    loop {
        let n = file.read(&mut buffer)?;

        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Unterminated gzip header string",
            ));
        }

        if let Some(pos) = buffer[..n].iter().position(|&b| b == 0) {
            let unread = n - pos - 1;

            if unread > 0 {
                file.seek(SeekFrom::Current(-(unread as i64)))?;
            }

            return Ok(());
        }
    }
}

pub fn compute_gzip_payload_hash(path: &Path) -> io::Result<u64> {
    let mut file = open_file_read(path)?;
    let file_len = file.metadata()?.len();

    let mut header = [0u8; 10];
    file.read_exact(&mut header)?;

    if header[0] != 0x1f || header[1] != 0x8b || header[2] != 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid gzip header",
        ));
    }

    let flags = header[3];

    if flags & 0xE0 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Reserved gzip flags are set",
        ));
    }

    if flags & 0x04 != 0 {
        let mut xlen = [0u8; 2];
        file.read_exact(&mut xlen)?;

        let extra_len = u16::from_le_bytes(xlen) as i64;
        file.seek(SeekFrom::Current(extra_len))?;
    }

    if flags & 0x08 != 0 {
        skip_gzip_string(&mut file)?;
    }

    if flags & 0x10 != 0 {
        skip_gzip_string(&mut file)?;
    }

    if flags & 0x02 != 0 {
        file.seek(SeekFrom::Current(2))?;
    }

    let payload_start = file.stream_position()?;

    if file_len < payload_start + 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Gzip file is too short: file_len={}, payload_start={}, remaining={}",
                file_len,
                payload_start,
                file_len.saturating_sub(payload_start)
            ),
        ));
    }

    let payload_len = file_len - payload_start - 8;

    let mut hasher = Xxh3::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    let mut remaining = payload_len;

    while remaining > 0 {
        let to_read = remaining.min(buffer.len() as u64) as usize;

        file.read_exact(&mut buffer[..to_read])?;
        hasher.update(&buffer[..to_read]);

        remaining -= to_read as u64;
    }

    Ok(hasher.digest())
}
