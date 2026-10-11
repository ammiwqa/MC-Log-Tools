use indicatif::ProgressBar;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub trait ProgressReporter: Send + Sync {
    fn inc(&self, n: u64);
    fn set_total(&self, n: u64);
    fn set_message(&self, _msg: &str) {}
    fn finish(&self) {}
    fn stage_completed(&self, _stats: StageStats) {}
}

pub struct ProgressBarWrapper {
    inner: ProgressBar,
}

impl ProgressBarWrapper {
    pub fn new(pb: ProgressBar) -> Self {
        Self { inner: pb }
    }

    pub fn finish_and_clear(&self) {
        self.inner.finish_and_clear();
    }
}

impl ProgressReporter for ProgressBarWrapper {
    #[inline]
    fn inc(&self, n: u64) {
        self.inner.inc(n);
    }

    #[inline]
    fn set_total(&self, n: u64) {
        self.inner.set_length(n);
    }

    #[inline]
    fn finish(&self) {
        self.finish_and_clear();
    }
}

impl ProgressReporter for &ProgressBarWrapper {
    #[inline]
    fn inc(&self, n: u64) {
        (*self).inc(n);
    }

    #[inline]
    fn set_total(&self, n: u64) {
        (*self).set_total(n);
    }

    #[inline]
    fn finish(&self) {
        (*self).finish();
    }
}

pub struct ParseProgress {
    pub processed: AtomicU64,
    pub max_lines: AtomicU64,
}

impl ParseProgress {
    pub fn new() -> Self {
        Self {
            processed: AtomicU64::new(0),
            max_lines: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn inc_progress(&self, n: u64) {
        self.processed.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn get_progress(&self) -> u64 {
        self.processed.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn get_max_progress(&self) -> u64 {
        self.max_lines.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_max_progress(&self, n: u64) {
        self.max_lines.store(n, Ordering::Relaxed);
    }
}

impl ProgressReporter for ParseProgress {
    #[inline]
    fn inc(&self, n: u64) {
        self.inc_progress(n);
    }

    #[inline]
    fn set_total(&self, n: u64) {
        self.set_max_progress(n);
    }
}

pub struct WriteProgress {
    pub written: AtomicU64,
    pub total: AtomicU64,
}

impl WriteProgress {
    pub fn new() -> Self {
        Self {
            written: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn inc(&self, n: u64) {
        for _ in 0..n {
            self.written.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn get_written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn get_total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
}

impl ProgressReporter for Arc<WriteProgress> {
    #[inline]
    fn inc(&self, n: u64) {
        self.as_ref().inc(n);
    }

    #[inline]
    fn set_total(&self, n: u64) {
        self.as_ref().set_total(n);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Dedup,
    GzipParsing,
    LatestParsing,
}

#[derive(Debug, Clone)]
pub struct StageStats {
    pub stage: StageKind,
    pub files: usize,
    pub lines: usize,
    pub file_errors: usize,
    pub parse_errors: usize,
    pub duplicate_files: usize,
}
