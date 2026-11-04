/// Domain models for the core layer.
///
/// These are pure data models without dependencies on UI or filesystem.
/// They represent the fundamental entities used throughout the application.
pub mod model {
    use serde::{Deserialize, Serialize};

    /// A chunk of log lines from a base.
    ///
    /// Chunks are the fundamental unit of storage and indexing.
    /// They allow incremental updates and efficient searching.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Chunk {
        /// Unique identifier for the chunk (typically first timestamp + index)
        pub id: String,
        /// First timestamp in the chunk (unix epoch seconds)
        pub first_ts: i64,
        /// Last timestamp in the chunk (unix epoch seconds)
        pub last_ts: i64,
        /// Number of lines in the chunk
        pub lines_count: usize,
        /// Compressed data (zstd)
        pub data: Vec<u8>,
        /// Original file path from which this chunk was created
        pub source_path: Option<String>,
    }

    impl Chunk {
        /// Create a new chunk with the given parameters
        pub fn new(
            id: String,
            first_ts: i64,
            last_ts: i64,
            lines_count: usize,
            data: Vec<u8>,
        ) -> Self {
            Self {
                id,
                first_ts,
                last_ts,
                lines_count,
                data,
                source_path: None,
            }
        }
    }

    /// Represents a collection of log lines from a base.
    ///
    /// Holds metadata about the base creation time, edit time, and path information.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Base {
        /// Name of the base
        pub name: String,
        /// Total number of lines across all chunks
        pub total_lines: usize,
        /// Number of chunks in this base
        pub chunks_count: usize,
        /// Creation time in unix epoch seconds
        pub creation_time: i64,
        /// Last edit time in unix epoch seconds
        pub last_edit_time: i64,
        /// Paths to the log directories
        pub paths: Vec<String>,
        /// Chunks making up this base
        pub chunks: Vec<Chunk>,
    }

    impl Base {
        /// Create a new base with the given parameters
        pub fn new(
            name: String,
            total_lines: usize,
            chunks_count: usize,
            creation_time: i64,
            last_edit_time: i64,
            paths: Vec<String>,
        ) -> Self {
            Self {
                name,
                total_lines,
                chunks_count,
                creation_time,
                last_edit_time,
                paths,
                chunks: Vec::new(),
            }
        }

        /// Add a chunk to this base
        pub fn add_chunk(&mut self, chunk: Chunk) {
            self.chunks.push(chunk.clone());
            self.total_lines += chunk.lines_count;
            self.chunks_count += 1;
        }

        /// Get a chunk by its ID
        pub fn get_chunk(&self, id: &str) -> Option<&Chunk> {
            self.chunks.iter().find(|c| c.id == id)
        }

        /// Check if a chunk with the given ID exists
        pub fn has_chunk(&self, id: &str) -> bool {
            self.chunks.iter().any(|c| c.id == id)
        }

        /// Get the search space (union of all chunk time ranges)
        pub fn search_space(&self) -> Vec<(i64, i64)> {
            self.chunks
                .iter()
                .map(|c| (c.first_ts, c.last_ts))
                .collect()
        }
    }

    /// A search with operators (&, |, !, (), *).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Query {
        /// The raw query text
        pub text: String,
        /// Whether to use case-insensitive matching
        pub case_insensitive: bool,
        /// Whether to apply date filtering
        pub use_date_filtering: bool,
        /// Optional date from filter
        pub date_from: Option<i64>,
        /// Optional date to filter
        pub date_to: Option<i64>,
    }

    impl Query {
        /// Create a new query from text
        pub fn new(text: String) -> Self {
            Self {
                text,
                case_insensitive: true,
                use_date_filtering: false,
                date_from: None,
                date_to: None,
            }
        }

        /// Create a query with date filtering
        pub fn with_date_filtering(text: String, date_from: i64, date_to: i64) -> Self {
            Self {
                text,
                case_insensitive: true,
                use_date_filtering: true,
                date_from: Some(date_from),
                date_to: Some(date_to),
            }
        }

        /// Get the text of the query
        pub fn text(&self) -> &str {
            &self.text
        }

        /// Check if this query has date filtering enabled
        pub fn has_date_filtering(&self) -> bool {
            self.use_date_filtering
        }

        /// Get the date from bound
        pub fn date_from(&self) -> Option<i64> {
            self.date_from
        }

        /// Get the date to bound
        pub fn date_to(&self) -> Option<i64> {
            self.date_to
        }
    }

    /// A parsed log line with timestamp and content.
    ///
    /// Represents a single line from a log file with its parsed timestamp.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LogLine {
        /// Unix timestamp of the line
        pub timestamp: i64,
        /// The log message content
        pub message: String,
        /// Original raw line
        pub raw: String,
        /// Source file path
        pub source_path: Option<String>,
    }

    impl LogLine {
        /// Create a new log line
        pub fn new(timestamp: i64, message: String, raw: String) -> Self {
            Self {
                timestamp,
                message,
                raw,
                source_path: None,
            }
        }

        /// Check if the line matches a search query (case-insensitive)
        pub fn matches_query(&self, query: &str) -> bool {
            let lower_msg = self.message.to_lowercase();
            let lower_query = query.to_lowercase();
            lower_msg.contains(&lower_query)
        }

        /// Get the timestamp
        pub fn timestamp(&self) -> i64 {
            self.timestamp
        }

        /// Get the message
        pub fn message(&self) -> &str {
            &self.message
        }
    }
}

/// Statistics about search results.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchStats {
    /// Total number of lines searched
    pub total_lines: usize,
    /// Number of matching lines
    pub matches: usize,
    /// Number of file errors encountered
    pub file_errors: usize,
    /// Number of parse errors encountered
    pub parse_errors: usize,
}

impl SearchStats {
    /// Create new search stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Add file error count
    pub fn with_file_errors(mut self, count: usize) -> Self {
        self.file_errors = count;
        self
    }

    /// Add parse error count
    pub fn with_parse_errors(mut self, count: usize) -> Self {
        self.parse_errors = count;
        self
    }

    /// Check if there were any errors
    pub fn has_errors(&self) -> bool {
        self.file_errors > 0 || self.parse_errors > 0
    }

    /// Total error count
    pub fn total_errors(&self) -> usize {
        self.file_errors + self.parse_errors
    }
}
