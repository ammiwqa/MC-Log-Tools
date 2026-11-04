use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::events::{CoreEvent, EventSender};

use progress::{StageKind, StageStats};


pub struct ChannelProgress {
    tx: EventSender,
    pos: AtomicU64,
    total: AtomicU64,
    last_emit: Mutex<Option<Instant>>,
    throttle: Duration,
    cancelled: Mutex<bool>,
}

impl ChannelProgress {
    pub fn new(tx: EventSender) -> Self {
        Self::with_throttle(tx, Duration::from_millis(50))
    }

    pub fn with_throttle(tx: EventSender, throttle: Duration) -> Self {
        Self {
            tx,
            pos: AtomicU64::new(0),
            total: AtomicU64::new(0),
            last_emit: Mutex::new(None),
            throttle,
            cancelled: Mutex::new(false),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        let guard = self.cancelled.lock();
        match guard {
            Ok(g) => *g,
            Err(_) => false,
        }
    }

    pub fn set_cancelled(&self) {
        *self.cancelled.lock().unwrap() = true;
    }

    pub fn reset_cancelled(&self) {
        *self.cancelled.lock().unwrap() = false;
    }

    pub fn position(&self) -> u64 {
        self.pos.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn set_progress(&self, pos: u64) {
        self.pos.store(pos, Ordering::Relaxed);
        self.try_emit_progress(false);
    }

    pub fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
        self.pos.store(0, Ordering::Relaxed);
        self.try_emit_progress(true);
    }

    pub fn add_progress(&self, n: u64) {
        self.pos.fetch_add(n, Ordering::Relaxed);
        self.try_emit_progress(false);
    }

    fn try_emit_progress(&self, force: bool) {
        let now = Instant::now();

        if !force {
            let mut last = self.last_emit.lock().unwrap();
            if let Some(t) = *last {
                if now.duration_since(t) < self.throttle {
                    return;
                }
            }
            *last = Some(now);
        }

        let ev = CoreEvent::Progress {
            pos: self.pos.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        };

        let _ = self.tx.send(ev);
    }

    fn send_message(&self, msg: &str) {
        let _ = self.tx.send(CoreEvent::Message(msg.to_string()));
    }
}

#[derive(Clone)]
pub struct ChannelProgressClone {
}

static NO_EVENT_SINK: OnceLock<ChannelProgress> = OnceLock::new();

pub fn null_progress() -> &'static ChannelProgress {
    NO_EVENT_SINK.get_or_init(|| {
        let (tx, _rx) = std::sync::mpsc::channel();
        ChannelProgress::new(tx)
    })
}

pub trait ProgressReporter: Send + Sync {
    fn stage_completed(&self, stats: StageStats);

    fn inc(&self, n: u64);

    fn set_total(&self, n: u64);

    fn set_message(&self, msg: &str);

    fn finish(&self);

    fn is_cancelled(&self) -> bool {
        false
    }

    fn set_cancelled(&self) {
        // Default no-op
    }
}

extern crate progress;
use progress::ProgressReporter as ExternalProgressReporter;

impl ProgressReporter for ChannelProgress {
    fn stage_completed(&self, stats: StageStats) {
        let event = match stats.stage {
            StageKind::Dedup => CoreEvent::DedupCompleted {
                duplicate_files: stats.duplicate_files,
            },
            StageKind::GzipParsing => CoreEvent::GzipParsingCompleted {
                files: stats.files,
                lines: stats.lines,
                file_errors: stats.file_errors,
                parse_errors: stats.parse_errors,
            },
            StageKind::LatestParsing => CoreEvent::LatestParsingCompleted {
                files: stats.files,
                lines: stats.lines,
                file_errors: stats.file_errors,
                parse_errors: stats.parse_errors,
            },
        };

        let _ = self.tx.send(event);
    }

    fn inc(&self, n: u64) {
        self.pos.fetch_add(n, Ordering::Relaxed);
        self.try_emit_progress(false);
    }

    fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
        self.pos.store(0, Ordering::Relaxed);
        self.try_emit_progress(true);
    }

    fn set_message(&self, msg: &str) {
        self.send_message(msg);
    }

    fn finish(&self) {
        self.try_emit_progress(true);
    }

    fn is_cancelled(&self) -> bool {
        ChannelProgress::is_cancelled(self)
    }

    fn set_cancelled(&self) {
        ChannelProgress::set_cancelled(self)
    }
}

impl ExternalProgressReporter for ChannelProgress {
    fn stage_completed(&self, stats: progress::StageStats) {
        let event = match stats.stage {
            progress::StageKind::Dedup => CoreEvent::DedupCompleted {
                duplicate_files: stats.duplicate_files,
            },
            progress::StageKind::GzipParsing => CoreEvent::GzipParsingCompleted {
                files: stats.files,
                lines: stats.lines,
                file_errors: stats.file_errors,
                parse_errors: stats.parse_errors,
            },
            progress::StageKind::LatestParsing => CoreEvent::LatestParsingCompleted {
                files: stats.files,
                lines: stats.lines,
                file_errors: stats.file_errors,
                parse_errors: stats.parse_errors,
            },
        };

        let _ = self.tx.send(event);
    }

    fn inc(&self, n: u64) {
        self.pos.fetch_add(n, Ordering::Relaxed);
        self.try_emit_progress(false);
    }

    fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
        self.pos.store(0, Ordering::Relaxed);
        self.try_emit_progress(true);
    }

    fn set_message(&self, msg: &str) {
        self.send_message(msg);
    }

    fn finish(&self) {
        self.try_emit_progress(true);
    }
}