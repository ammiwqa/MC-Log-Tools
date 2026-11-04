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

pub fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    encode(hasher.finalize())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_gzip(path: &Path, payload: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap();
    }

    #[test]
    fn sha256_of_empty_and_fixed_input() {
        let empty = sha256_of(&[]);
        assert_eq!(empty.len(), 64);
        assert_ne!(empty, sha256_of(b"hello"));

        let a = sha256_of(b"hello");
        assert_eq!(a, sha256_of(b"hello")); // deterministic
        assert_ne!(a, sha256_of(b"Hello")); // case-sensitive
    }

    #[test]
    fn compute_file_hash_matches_sha256_of() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.txt");
        std::fs::write(&path, "hello world").unwrap();

        let from_file = compute_file_hash(&path).unwrap();
        let from_bytes = sha256_of(b"hello world");
        assert_eq!(from_file, from_bytes);
    }

    #[test]
    fn compute_file_hash_nonexistent_is_error() {
        let err = compute_file_hash(Path::new("/nonexistent/file.bin")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn compute_gzip_payload_hash_valid_gzip() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-gz-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.log.gz");
        let payload = b"line one\nline two\nline three\n";
        make_gzip(&path, payload);

        let h1 = compute_gzip_payload_hash(&path).unwrap();
        let h2 = compute_gzip_payload_hash(&path).unwrap();
        assert_eq!(h1, h2); // deterministic
        assert_ne!(h1, 0);
    }

    #[test]
    fn compute_gzip_payload_hash_different_payloads_differ() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-gzdiff-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path_a = dir.join("a.log.gz");
        let path_b = dir.join("b.log.gz");
        make_gzip(&path_a, b"payload A\n");
        make_gzip(&path_b, b"payload B\n");

        let h_a = compute_gzip_payload_hash(&path_a).unwrap();
        let h_b = compute_gzip_payload_hash(&path_b).unwrap();
        assert_ne!(h_a, h_b);
    }

    #[test]
    fn compute_gzip_payload_hash_not_a_gzip_is_error() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-notgz-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notgz.log.gz");
        std::fs::write(&path, "this is not gzip data at all").unwrap();

        let err = compute_gzip_payload_hash(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn compute_gzip_payload_hash_empty_gzip() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-emptygz-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.log.gz");
        make_gzip(&path, b"");

        let h = compute_gzip_payload_hash(&path).unwrap();
        assert_ne!(h, 0);
    }

    #[test]
    fn compute_gzip_payload_hash_truncated_gzip_is_error() {
        let dir = std::env::temp_dir().join(format!(
            "lt-hash-trunc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trunc.log.gz");
        // valid gzip header but no payload / too short
        std::fs::write(&path, &[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0]).unwrap();

        let err = compute_gzip_payload_hash(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
