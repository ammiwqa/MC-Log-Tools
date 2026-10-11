#[derive(Debug, Clone)]
pub enum CoreEvent {
    Progress {
        pos: u64,
        total: u64,
    },
    Message(String),

    DedupCompleted {
        duplicate_files: usize,
    },

    GzipParsingCompleted {
        files: usize,
        lines: usize,
        file_errors: usize,
        parse_errors: usize,
    },

    LatestParsingCompleted {
        files: usize,
        lines: usize,
        file_errors: usize,
        parse_errors: usize,
    },

    WritingCompleted {
        lines: usize,
    },
}

pub type EventSender = std::sync::mpsc::Sender<CoreEvent>;
