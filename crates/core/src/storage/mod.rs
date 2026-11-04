/// Storage module for reading/writing zstd-chunks, JSON-config, and chunk index.
///
/// This module handles the persistent storage of search bases, including:
/// - Reading and writing compressed chunk files
/// - Managing the chunk index for fast lookup
/// - JSON configuration management
use std::path::Path;
use std::fs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::model::{Base, Chunk};

/// Error type for storage operations
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Chunk not found: {0}")]
    ChunkNotFound(String),

    #[error("Base not found: {0}")]
    BaseNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Chunk write error: {0}")]
    ChunkWrite(String),

    #[error("Chunk read error: {0}")]
    ChunkRead(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// Configuration for a base storage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaseConfig {
    /// Base name
    pub name: String,
    /// Paths to log directories
    pub paths: Vec<String>,
    /// Creation timestamp
    pub creation_time: i64,
    /// Last edit timestamp
    pub last_edit_time: i64,
    /// Whether the base is currently being written to
    pub is_writing: bool,
}

impl BaseConfig {
    /// Create a new base config
    pub fn new(name: String, paths: Vec<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Self {
            name,
            paths,
            creation_time: now,
            last_edit_time: now,
            is_writing: false,
        }
    }
}

/// Read a base config from a JSON file
pub fn read_base_config(path: &Path) -> StorageResult<BaseConfig> {
    let content = fs::read_to_string(path).map_err(StorageError::Io)?;
    let config: BaseConfig = serde_json::from_str(&content).map_err(StorageError::Serialization)?;
    Ok(config)
}

/// Write a base config to a JSON file
pub fn write_base_config(path: &Path, config: &BaseConfig) -> StorageResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(StorageError::Io)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content).map_err(StorageError::Io)?;
    Ok(())
}

/// Chunk index entry for fast lookup without decompressing entire base
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkIndex {
    /// Chunk ID
    pub id: String,
    /// First timestamp in chunk
    pub first_ts: i64,
    /// Last timestamp in chunk
    pub last_ts: i64,
    /// File path where chunk is stored
    pub file_path: String,
    /// Number of lines in chunk
    pub lines_count: usize,
}

impl ChunkIndex {
    /// Create a new chunk index entry
    pub fn new(id: String, first_ts: i64, last_ts: i64, file_path: String, lines_count: usize) -> Self {
        Self {
            id,
            first_ts,
            last_ts,
            file_path,
            lines_count,
        }
    }
}

/// Index of all chunks in a base for fast search skipping
#[derive(Debug, Default)]
pub struct ChunkIndexMap {
    /// Mapping of chunk ID to index entry
    entries: std::collections::HashMap<String, ChunkIndex>,
    /// Mapping of timestamp range to chunk IDs for pruning
    time_index: std::collections::HashMap<i64, Vec<String>>,
}

impl ChunkIndexMap {
    /// Add a chunk to the index
    pub fn add(&mut self, index: ChunkIndex) {
        self.entries.insert(index.id.clone(), index.clone());

        // Index by first timestamp
        self.time_index
            .entry(index.first_ts)
            .or_default()
            .push(index.id.clone());
    }

    /// Get chunk index by ID
    pub fn get(&self, id: &str) -> Option<&ChunkIndex> {
        self.entries.get(id)
    }

    /// Check if a chunk exists
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Get all chunk IDs that might contain a given timestamp
    pub fn find_chunks_for_timestamp(&self, ts: i64) -> Vec<String> {
        self.time_index.get(&ts).cloned().unwrap_or_default()
    }

    /// Get total number of chunks indexed
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if index is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Read all chunks from a base directory
pub fn read_all_chunks(base_dir: &Path) -> StorageResult<Vec<Chunk>> {
    let mut chunks = Vec::new();

    if !base_dir.exists() {
        return Ok(chunks);
    }

    for entry in fs::read_dir(base_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => return Err(StorageError::Io(e)),
        };
        let path = entry.path();

        if path.extension().map_or(false, |e| e == "zst") || path.extension().map_or(false, |e| e == "zstd") {
            // Read compressed chunk data from file
            let compressed_data = match fs::read(&path) {
                Ok(d) => d,
                Err(e) => return Err(StorageError::Io(e)),
            };

            // Decompress zstd
            let decompressed = match zstd::decode_all(&compressed_data[..]) {
                Ok(d) => d,
                Err(e) => return Err(StorageError::ChunkRead(format!("zstd decode error: {}", e))),
            };
            let chunk: Chunk = match serde_json::from_slice(&decompressed) {
                Ok(c) => c,
                Err(e) => return Err(StorageError::ChunkRead(format!("json deserialize error: {}", e))),
            };
            chunks.push(chunk);
        }
    }

    Ok(chunks)
}

/// Write a chunk to a base directory as zstd-compressed JSON
pub fn write_chunk(base_dir: &Path, chunk: &Chunk) -> StorageResult<()> {
    if !base_dir.exists() {
        fs::create_dir_all(base_dir).map_err(StorageError::Io)?;
    }

    let json = serde_json::to_vec(chunk).map_err(StorageError::Serialization)?;
    let compressed = zstd::encode_all(&json[..], 0)
        .map_err(|e| StorageError::ChunkWrite(format!("zstd encode error: {}", e)))?;

    let output_path = base_dir.join(format!("{}.zst", chunk.id));
    fs::write(&output_path, compressed).map_err(StorageError::Io)?;

    Ok(())
}

/// Merge a new chunk into an existing base, adding only new lines
pub fn merge_chunk_into_base(base: &mut Base, chunk: Chunk) -> StorageResult<()> {
    // Check if chunk already exists
    if base.has_chunk(&chunk.id) {
        return Ok(());
    }

    base.add_chunk(chunk);
    Ok(())
}

/// Generate a chunk ID from its time range and index
pub fn generate_chunk_id(first_ts: i64, last_ts: i64, index: usize) -> String {
    use std::fmt::Write;
    let mut id = String::new();
    write!(id, "chunk_{}_{}_{}", first_ts, last_ts, index).unwrap();
    id
}