//! GUI use-case layer.
//!
//! Each module exposes a single application workflow to the GUI. Keeping
//! these entry points separate from `app.rs` prevents UI code from depending
//! on storage internals and makes the workflows reusable from other frontends.

pub mod create_base;
pub mod delete;
pub mod discover;
pub mod search;
pub mod update_base;

pub use create_base::run_create_base;
pub use delete::run_delete;
pub use discover::run_discover;
pub use search::run_search;
pub use update_base::run_update_base;
