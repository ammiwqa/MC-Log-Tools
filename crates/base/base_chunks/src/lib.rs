//! Chunked storage for bases.
//!
//! A base is a sequence of zstd chunks ordered by timestamp. Chunks never
//! overlap: `last_ts(chunk N) <= first_ts(chunk N+1)`. Each chunk holds up to

pub mod external_sort;

use std::fs;
use std::io;
use std::path::Path;

use progress::ProgressReporter;
use read_base_info::{ChunkInfo, DEFAULT_CHUNK_LINES};

pub fn write_chunks<P: ProgressReporter + ?Sized>(
    name: &str,
    logs: &[(i64, String)],
    progress: &P,
) -> Result<Vec<ChunkInfo>, String> {
    write_chunks_with_size(name, logs, DEFAULT_CHUNK_LINES as usize, progress)
}

pub fn write_chunks_with_size<P: ProgressReporter + ?Sized>(
    name: &str,
    logs: &[(i64, String)],
    chunk_lines: usize,
    progress: &P,
) -> Result<Vec<ChunkInfo>, String> {
    if chunk_lines == 0 {
        return Err("Chunk size must be greater than zero".to_string());
    }

    let dir = read_base_info::base_dir(name)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create base dir: {}", e))?;

    let mut tmp_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut chunks: Vec<ChunkInfo> = Vec::new();

    let mut result = Ok(());

    for (index, slice) in logs.chunks(chunk_lines).enumerate() {
        let file = read_base_info::chunk_file_name(name, index);
        let tmp_path = dir.join(format!("{}.tmp", file));

        if let Err(err) = zip_writer::write_logs_to_zstd(slice, &tmp_path.to_string_lossy(), progress)
        {
            result = Err(format!("Cannot write chunk '{}': {}", file, err));
            break;
        }

        tmp_paths.push(tmp_path);

        let first_ts = slice.first().map(|(ts, _)| *ts).unwrap_or(0);
        let last_ts = slice.last().map(|(ts, _)| *ts).unwrap_or(0);

        chunks.push(ChunkInfo {
            file,
            lines: slice.len() as u64,
            first_ts,
            last_ts,
        });
    }

    if let Err(err) = result {
        for tmp in &tmp_paths {
            let _ = fs::remove_file(tmp);
        }
        return Err(err);
    }

    rename_chunks_into_place(&dir, &tmp_paths, &chunks)?;

    Ok(chunks)
}

pub fn write_chunks_streaming<I, P>(
    name: &str,
    lines: I,
    chunk_lines: usize,
    progress: &P,
) -> Result<Vec<ChunkInfo>, String>
where
    I: IntoIterator<Item = io::Result<(i64, String)>>,
    P: ProgressReporter + ?Sized,
{
    if chunk_lines == 0 {
        return Err("Chunk size must be greater than zero".to_string());
    }

    let dir = read_base_info::base_dir(name)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create base dir: {}", e))?;

    let mut tmp_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut chunks: Vec<ChunkInfo> = Vec::new();
    let mut current: Option<CurrentChunk> = None;

    let result = (|| -> Result<(), String> {
        for line in lines {
            let (ts, msg) = line.map_err(|e| format!("Cannot read sorted log line: {}", e))?;

            if current.is_none() {
                current = Some(CurrentChunk::create(name, chunks.len(), &dir, &mut tmp_paths)?);
            }

            let chunk = current.as_mut().unwrap();
            chunk
                .push(ts, &msg, progress)
                .map_err(|e| format!("Cannot write chunk '{}': {}", chunk.file, e))?;

            if chunk.lines as usize >= chunk_lines {
                chunks.push(current.take().unwrap().finish()?);
            }
        }

        if let Some(chunk) = current.take() {
            chunks.push(chunk.finish()?);
        }

        Ok(())
    })();

    if let Err(err) = result {
        for tmp in &tmp_paths {
            let _ = fs::remove_file(tmp);
        }
        return Err(err);
    }

    rename_chunks_into_place(&dir, &tmp_paths, &chunks)?;

    Ok(chunks)
}

fn rename_chunks_into_place(
    dir: &Path,
    tmp_paths: &[std::path::PathBuf],
    chunks: &[ChunkInfo],
) -> Result<(), String> {
    for (tmp, chunk) in tmp_paths.iter().zip(chunks.iter()) {
        let final_path = dir.join(&chunk.file);

        fs::rename(tmp, &final_path).map_err(|e| {
            format!(
                "Cannot move chunk '{}' into place: {}",
                final_path.display(),
                e
            )
        })?;
    }

    Ok(())
}

struct CurrentChunk {
    file: String,
    writer: zip_writer::ZstdLogWriter,
    lines: u64,
    first_ts: i64,
    last_ts: i64,
}

impl CurrentChunk {
    fn create(
        name: &str,
        index: usize,
        dir: &Path,
        tmp_paths: &mut Vec<std::path::PathBuf>,
    ) -> Result<Self, String> {
        let file = read_base_info::chunk_file_name(name, index);
        let tmp_path = dir.join(format!("{}.tmp", file));

        let writer = zip_writer::ZstdLogWriter::create(&tmp_path.to_string_lossy())
            .map_err(|e| format!("Cannot create chunk '{}': {}", file, e))?;

        tmp_paths.push(tmp_path);

        Ok(Self {
            file,
            writer,
            lines: 0,
            first_ts: 0,
            last_ts: 0,
        })
    }

    fn push<P: ProgressReporter + ?Sized>(
        &mut self,
        ts: i64,
        msg: &str,
        progress: &P,
    ) -> io::Result<()> {
        self.writer.write_line(ts, msg)?;

        if self.lines == 0 {
            self.first_ts = ts;
        }

        self.lines += 1;
        self.last_ts = ts;
        progress.inc(1);

        Ok(())
    }

    fn finish(self) -> Result<ChunkInfo, String> {
        self.writer
            .finish()
            .map_err(|e| format!("Cannot finish chunk '{}': {}", self.file, e))?;

        Ok(ChunkInfo {
            file: self.file,
            lines: self.lines,
            first_ts: self.first_ts,
            last_ts: self.last_ts,
        })
    }
}

pub fn read_chunk(path: &str) -> std::io::Result<Vec<(i64, String)>> {
    zip_writer::read_logs_from_zstd(path)
}

pub fn base_hash(chunks: &[ChunkInfo]) -> String {
    hash_utils::sha256_of(&chunk_metadata_bytes(chunks))
}

fn chunk_metadata_bytes(chunks: &[ChunkInfo]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(chunks.len() * 48);

    for chunk in chunks {
        buf.extend_from_slice(chunk.file.as_bytes());
        buf.push(b'|');
        buf.extend_from_slice(chunk.lines.to_string().as_bytes());
        buf.push(b'|');
        buf.extend_from_slice(chunk.first_ts.to_string().as_bytes());
        buf.push(b'|');
        buf.extend_from_slice(chunk.last_ts.to_string().as_bytes());
        buf.push(b'\n');
    }

    buf
}

/// Index of the chunk that should receive `ts`.
///
/// Boundaries go to the earlier chunk (`ts <= last_ts`), and timestamps older
/// than the first chunk also go to chunk 0.
fn find_chunk(chunks: &[ChunkInfo], ts: i64) -> usize {
    if chunks.is_empty() || ts <= chunks[0].first_ts {
        return 0;
    }

    let mut lo = 0usize;
    let mut hi = chunks.len();

    while lo < hi {
        let mid = (lo + hi) / 2;

        if chunks[mid].last_ts >= ts {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    if lo >= chunks.len() {
        chunks.len() - 1
    } else {
        lo
    }
}

/// Merges `new_logs` into the existing chunks of a base.
///
/// Only chunks that receive new lines are read and rewritten. Untouched chunks
/// are left on disk as-is.
pub fn merge_into_chunks<P: ProgressReporter + ?Sized>(
    name: &str,
    chunks: &[ChunkInfo],
    new_logs: &[(i64, String)],
    progress: &P,
) -> Result<Vec<ChunkInfo>, String> {
    if new_logs.is_empty() {
        return Ok(chunks.to_vec());
    }

    if chunks.is_empty() {
        return write_chunks_with_size(
            name,
            new_logs,
            DEFAULT_CHUNK_LINES as usize,
            progress,
        );
    }

    let mut sorted_new: Vec<(i64, String)> = new_logs.to_vec();
    sorted_new.sort_unstable_by_key(|x| x.0);

    let mut per_chunk: Vec<Vec<(i64, String)>> = vec![Vec::new(); chunks.len()];

    for log in sorted_new {
        let index = find_chunk(chunks, log.0);
        per_chunk[index].push(log);
    }

    let dir = read_base_info::base_dir(name)?;

    let mut updated: Vec<ChunkInfo> = chunks.to_vec();
    let mut staged: Vec<(usize, std::path::PathBuf)> = Vec::new();

    let mut result = Ok(());

    for (index, extra) in per_chunk.iter().enumerate() {
        if extra.is_empty() {
            continue;
        }

        let old_path = dir.join(&chunks[index].file);

        let mut merged = match read_chunk(&old_path.to_string_lossy()) {
            Ok(lines) => lines,
            Err(err) => {
                result = Err(format!("Cannot read chunk '{}': {}", chunks[index].file, err));
                break;
            }
        };

        merged.extend(extra.iter().cloned());
        merged.sort_unstable_by_key(|x| x.0);

        let file = read_base_info::chunk_file_name(name, index);
        let tmp_path = dir.join(format!("{}.tmp", file));

        if let Err(err) = zip_writer::write_logs_to_zstd(&merged, &tmp_path.to_string_lossy(), progress)
        {
            result = Err(format!("Cannot write chunk '{}': {}", file, err));
            break;
        }

        staged.push((index, tmp_path));

        updated[index] = ChunkInfo {
            file,
            lines: merged.len() as u64,
            first_ts: merged.first().map(|(ts, _)| *ts).unwrap_or(0),
            last_ts: merged.last().map(|(ts, _)| *ts).unwrap_or(0),
        };
    }

    if let Err(err) = result {
        for (_, tmp) in &staged {
            let _ = fs::remove_file(tmp);
        }
        return Err(err);
    }

    for (index, tmp) in &staged {
        let final_path = dir.join(&updated[*index].file);
        fs::rename(tmp, &final_path).map_err(|e| {
            format!(
                "Cannot move chunk '{}' into place: {}",
                final_path.display(),
                e
            )
        })?;
    }

    Ok(prune_empty_tail(name, updated, dir.as_path())?)
}

fn prune_empty_tail(
    name: &str,
    mut chunks: Vec<ChunkInfo>,
    dir: &Path,
) -> Result<Vec<ChunkInfo>, String> {
    while chunks.last().map(|c| c.lines == 0).unwrap_or(false) {
        let last = chunks.pop().unwrap();
        let _ = fs::remove_file(dir.join(&last.file));
    }

    for (index, chunk) in chunks.iter_mut().enumerate() {
        let expected = read_base_info::chunk_file_name(name, index);

        if chunk.file != expected {
            let old_path = dir.join(&chunk.file);
            let new_path = dir.join(&expected);

            fs::rename(&old_path, &new_path).map_err(|e| {
                format!(
                    "Cannot rename chunk '{}' to '{}': {}",
                    old_path.display(),
                    new_path.display(),
                    e
                )
            })?;

            chunk.file = expected;
        }
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(file: &str, first_ts: i64, last_ts: i64) -> ChunkInfo {
        ChunkInfo { file: file.to_string(), lines: 1, first_ts, last_ts }
    }

    #[test]
    fn find_chunk_empty_returns_zero() {
        let chunks: Vec<ChunkInfo> = Vec::new();
        assert_eq!(find_chunk(&chunks, 100), 0);
    }

    #[test]
    fn find_chunk_before_first_returns_zero() {
        let chunks = vec![chunk("a.0.zst", 100, 200), chunk("a.1.zst", 300, 400)];
        assert_eq!(find_chunk(&chunks, 50), 0);
        assert_eq!(find_chunk(&chunks, 100), 0); // boundary goes to earlier chunk
    }

    #[test]
    fn find_chunk_within_range() {
        let chunks = vec![
            chunk("a.0.zst", 0, 100),
            chunk("a.1.zst", 101, 200),
            chunk("a.2.zst", 201, 300),
        ];
        assert_eq!(find_chunk(&chunks, 150), 1);
        assert_eq!(find_chunk(&chunks, 250), 2);
    }

    #[test]
    fn find_chunk_after_last_returns_last_index() {
        let chunks = vec![chunk("a.0.zst", 0, 100)];
        assert_eq!(find_chunk(&chunks, 999), 0);
    }

    #[test]
    fn find_chunk_boundary_goes_to_earlier_chunk() {
        let chunks = vec![
            chunk("a.0.zst", 0, 100),
            chunk("a.1.zst", 101, 200),
        ];
        // ts == last_ts of chunk 0 and first_ts - 1 of chunk 1 -> chunk 0
        assert_eq!(find_chunk(&chunks, 100), 0);
    }
}