//! Use case: update an existing base if it changed.

use logtools_app::{CommandResult, Presenter};

/// Update an existing search base.
pub struct UpdateBaseUseCase;

impl UpdateBaseUseCase {
    /// Execute the update-base workflow.
    pub fn execute(name: String, presenter: &mut dyn Presenter) -> CommandResult {
        logtools_app::run_update_base(name, presenter)
    }
}

/// Convenience wrapper used by the GUI worker thread.
pub fn run_update_base(name: String, presenter: &mut dyn Presenter) -> CommandResult {
    UpdateBaseUseCase::execute(name, presenter)
}
