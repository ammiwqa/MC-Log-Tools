//! Use case: create a new base from log directories.
//!
//! This module keeps the GUI independent from the command implementation:
//! the application layer owns the workflow, while this module only defines
//! the GUI-facing entry point.

use logtools_app::{CommandResult, Presenter};

/// Create a new search base from directories (and optionally a paths file).
pub struct CreateBaseUseCase;

impl CreateBaseUseCase {
    /// Execute the create-base workflow.
    pub fn execute(
        paths: Vec<String>,
        from_file: Option<String>,
        name: String,
        presenter: &mut dyn Presenter,
    ) -> CommandResult {
        logtools_app::run_create_base(paths, from_file, name, presenter)
    }
}

/// Convenience wrapper used by the GUI worker thread.
pub fn run_create_base(
    paths: Vec<String>,
    from_file: Option<String>,
    name: String,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    CreateBaseUseCase::execute(paths, from_file, name, presenter)
}
