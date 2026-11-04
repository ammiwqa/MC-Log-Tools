pub mod discovery;
pub mod error;
pub mod events;
pub mod operations;
pub mod progress;
pub mod storage;
pub mod types;
pub mod domain;

pub use error::AppError;
pub use events::CoreEvent;
pub use progress::ChannelProgress;
pub use types::{CreateResult, ErrorStats, SearchHandle, SearchResult, UpdateResult};
pub use domain::model::{self, Chunk, Base, Query, LogLine};
pub use domain::SearchStats;
