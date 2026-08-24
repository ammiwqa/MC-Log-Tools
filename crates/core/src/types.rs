pub use create_base::CreateResult;
pub use log_parser::ErrorStats;
pub use read_base_info::BaseInfo;
pub use search::SearchResult;pub use update_base::UpdateResult;

use std::sync::Arc;
use std::thread::JoinHandle;

pub struct SearchHandle {
    pub progress: Arc<progress::ParseProgress>,
    pub handle: JoinHandle<Result<SearchResult, String>>,
}
