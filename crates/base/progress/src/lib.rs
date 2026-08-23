use indicatif::ProgressBar;
use std::sync::atomic::{AtomicU64, Ordering};

pub trait ProgressReporter: Send + Sync {
    fn inc(&self, n: u64);
    fn set_total(&self, n: u64);
    fn set_message(&self, _msg: &str) {}
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
    pub fn inc(&self) {
        self.written.fetch_add(1, Ordering::Relaxed);
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

impl ProgressReporter for WriteProgress {
    #[inline]
    fn inc(&self, n: u64) {
        for _ in 0..n {
            self.written.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
    }
}
