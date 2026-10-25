//! Chunked storage for bases.
//!
//! A base is a sequence of zstd chunks ordered by timestamp. Chunks never
//! overlap: `last_ts(chunk N) <= first_ts(chunk N+1)`. Each chunk holds up to
//! `read_base_info::CHUNK_LINES` lines, but chunks that receive an out-of-order
//! insert are allowed to grow beyond that limit instead of cascading rewrites.

use std::fs;
use std::path::Path;

use progress::ProgressReporter;
use read_base_info::{ChunkInfo, CHUNK_LINES};

/// Writes `logs` (already sorted by timestamp) split into chunks.
///
/// Chunk files are written as `<name>.<i>.zst.tmp` and renamed into place only
/// after the whole write succeeds. Returns metadata for every written chunk.
pub fn write_chunks<P: ProgressReporter + ?Sized>(
    name: &str,
    logs: &[(i64, String)],
    progress: &P,
) -> Result<Vec<ChunkInfo>, String> {
    let dir = read_base_info::base_dir(name)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create base dir: {}", e))?;

    let chunk_lines = CHUNK_LINES as usize;

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

    Ok(chunks)
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