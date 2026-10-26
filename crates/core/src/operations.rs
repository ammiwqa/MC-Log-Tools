use std::sync::mpsc;

use progress::ProgressReporter;

use crate::error::{AppError, AppResult};
use crate::events::{CoreEvent, EventSender};
use crate::progress::ChannelProgress;
use crate::types::{
    BaseInfo, CreateResult, SearchHandle, SearchResult, UpdateResult,
};

pub fn create_base(
    paths: Vec<String>,
    name: String,
    tx: EventSender,
) -> AppResult<CreateResult> {
    let progress = ChannelProgress::new(tx);
    run_create(paths, name, &progress)
}

pub fn run_create<P: ProgressReporter + ?Sized>(
    paths: Vec<String>,
    name: String,
    progress: &P,
) -> AppResult<CreateResult> {
    create_base::create_base(paths, name, progress).map_err(AppError::from)
}

pub fn update_base(name: &str, tx: EventSender) -> AppResult<UpdateResult> {
    let progress = ChannelProgress::new(tx);
    run_update(name, &progress)
}

pub fn run_update<P: ProgressReporter + ?Sized>(
    name: &str,
    progress: &P,
) -> AppResult<UpdateResult> {
    update_base::update_base(name, progress).map_err(AppError::from)
}

pub fn list_bases() -> Result<Vec<BaseInfo>, String> {
    read_base_info::list_bases()
}

pub fn start_search(base_name: &str, query: &str) -> Result<SearchHandle, String> {
    let (progress, handle) = search::search_async(base_name, query)?;
    Ok(SearchHandle { progress, handle })
}

pub fn start_search_date_range(
    base_name: &str,
    query: &str,
    date_from: Option<i64>,
    date_to: Option<i64>,
    use_date_filtering: bool,
) -> Result<SearchHandle, String> {
    let (progress, handle) = search::search_async_date_range(
        base_name,
        query,
        date_from,
        date_to,
        use_date_filtering,
    )?;
    Ok(SearchHandle { progress, handle })
}

pub fn read_context_around(
    name: &str,
    target_ts: i64,
    n_before: usize,
    n_after: usize,
) -> Result<Vec<(i64, String)>, String> {
    search::read_context_around(name, target_ts, n_before, n_after)
}

pub fn event_channel() -> (EventSender, mpsc::Receiver<CoreEvent>) {
    mpsc::channel()
}

pub fn wait_search(handle: SearchHandle) -> Result<SearchResult, String> {
    handle.handle.join().unwrap_or_else(|_| {
        Err("Search thread panicked".to_string())
    })
}
