//! Use case: discover log directories on disk.

use logtools_app::{CommandResult, Presenter};

/// Discover directories that contain Minecraft logs.
pub struct DiscoverUseCase;

impl DiscoverUseCase {
    /// Execute the discovery workflow.
    pub fn execute(
        root: Option<String>,
        out: Option<String>,
        es: Option<String>,
        no_everything: bool,
        presenter: &mut dyn Presenter,
    ) -> CommandResult {
        logtools_app::run_discover(root, out, es, no_everything, presenter)
    }
}

/// Convenience wrapper used by the GUI worker thread.
pub fn run_discover(
    root: Option<String>,
    out: Option<String>,
    es: Option<String>,
    no_everything: bool,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    DiscoverUseCase::execute(root, out, es, no_everything, presenter)
}
