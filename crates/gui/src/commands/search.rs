//! Use case: search for text in a base.

use logtools_app::{CommandResult, Presenter};

/// Run a search query against a base.
pub struct SearchUseCase;

impl SearchUseCase {
    /// Execute the search workflow.
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        base_name: String,
        text: String,
        date_from: Option<i64>,
        date_to: Option<i64>,
        use_date_filtering: bool,
        presenter: &mut dyn Presenter,
    ) -> CommandResult {
        logtools_app::run_search_date_range(
            base_name,
            text,
            date_from,
            date_to,
            use_date_filtering,
            presenter,
        )
    }
}

/// Convenience wrapper used by the GUI worker thread.
#[allow(clippy::too_many_arguments)]
pub fn run_search(
    base_name: String,
    text: String,
    date_from: Option<i64>,
    date_to: Option<i64>,
    use_date_filtering: bool,
    presenter: &mut dyn Presenter,
) -> CommandResult {
    SearchUseCase::execute(
        base_name,
        text,
        date_from,
        date_to,
        use_date_filtering,
        presenter,
    )
}
