//! Use case: delete a base.

use logtools_app::{CommandResult, Presenter};

/// Delete a search base (configuration and logs).
pub struct DeleteUseCase;

impl DeleteUseCase {
    /// Execute the delete-base workflow.
    pub fn execute(name: String, force_yes: bool, presenter: &mut dyn Presenter) -> CommandResult {
        logtools_app::run_delete(name, force_yes, presenter)
    }
}

/// Convenience wrapper used by the GUI worker thread.
pub fn run_delete(name: String, force_yes: bool, presenter: &mut dyn Presenter) -> CommandResult {
    DeleteUseCase::execute(name, force_yes, presenter)
}
