use std::sync::mpsc;

use progress::ProgressReporter;

use crate::error::{AppError, AppResult};
use crate::events::{CoreEvent, EventSender};
use crate::progress::ChannelProgress;
use crate::types::{BaseInfo, CreateResult, SearchHandle, SearchResult, UpdateResult};
use crate::storage::{read_base_config, write_base_config, BaseConfig};

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

pub fn list_bases() -> AppResult<Vec<BaseInfo>> {
    read_base_info::list_bases().map_err(|e| AppError::Other(e))
}

pub fn start_search(base_name: &str, query: &str) -> AppResult<SearchHandle> {
    let (progress, handle) = search::search_async(base_name, query)?;
    Ok(SearchHandle { progress, handle })
}

pub fn start_search_date_range(
    base_name: &str,
    query: &str,
    date_from: Option<i64>,
    date_to: Option<i64>,
    use_date_filtering: bool,
) -> AppResult<SearchHandle> {
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
) -> AppResult<Vec<(i64, String)>> {
    let result = search::read_context_around(name, target_ts, n_before, n_after);
    match result {
        Ok(inner) => Ok(inner),
        Err(e) => Err(AppError::Other(format!("Read context around failed: {}", e))),
    }
}

pub fn event_channel() -> (EventSender, mpsc::Receiver<CoreEvent>) {
    mpsc::channel()
}

pub fn wait_search(handle: SearchHandle) -> AppResult<SearchResult> {
    let inner = handle.handle.join()
        .map_err(|e| AppError::Other(format!("Search thread join failed: {:?}", e)))?;
    inner.map_err(|e| AppError::Other(format!("Search failed: {}", e)))
}

pub fn list_bases_facade() -> AppResult<Vec<BaseInfo>> {
    list_bases()
}

pub fn delete_base(name: &str) -> AppResult<()> {
    read_base_info::delete_base(name).map_err(|e| AppError::Other(e))?;
    Ok(())
}

pub fn load_log(name: &str) -> AppResult<read_base_info::BaseConfig> {
    let result = read_base_info::load_log(name);
    match result {
        Ok(config) => Ok(config),
        Err(e) => Err(AppError::Other(e)),
    }
}

pub fn create_base_config(name: String, paths: Vec<String>) -> BaseConfig {
    BaseConfig::new(name, paths)
}

pub fn get_base_config(name: &str) -> AppResult<BaseConfig> {
    let path = std::path::Path::new("bases").join(format!("{}.json", name));
    read_base_config(&path).map_err(|e| AppError::Other(e.to_string()))
}

pub fn save_base_config(config: &BaseConfig) -> AppResult<()> {
    let path = std::path::Path::new("bases").join(format!("{}.json", config.name));
    write_base_config(&path, config).map_err(|e| AppError::Other(e.to_string()))
}