use std::sync::atomic::{AtomicU64, Ordering};

pub struct Progress {
    pub processed: AtomicU64,
    pub max_lines: AtomicU64,
}

impl Progress {
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
