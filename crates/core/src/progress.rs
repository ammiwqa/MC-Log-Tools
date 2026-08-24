use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use progress::ProgressReporter;

use crate::events::{CoreEvent, EventSender};

pub struct ChannelProgress {
    tx: EventSender,
    pos: AtomicU64,
    total: AtomicU64,
    last_emit: Mutex<Option<Instant>>,
    throttle: Duration,
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
        }
    }

    pub fn position(&self) -> u64 {
        self.pos.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
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

static NO_EVENT_SINK: OnceLock<ChannelProgress> = OnceLock::new();

pub fn null_progress() -> &'static ChannelProgress {
    NO_EVENT_SINK.get_or_init(|| {
        let (tx, _rx) = std::sync::mpsc::channel();
        ChannelProgress::new(tx)
    })
}

impl ProgressReporter for ChannelProgress {
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

impl ProgressReporter for &ChannelProgress {
    fn inc(&self, n: u64) {
        (*self).inc(n);
    }

    fn set_total(&self, n: u64) {
        (*self).set_total(n);
    }

    fn set_message(&self, msg: &str) {
        (*self).set_message(msg);
    }

    fn finish(&self) {
        (*self).finish();
    }
}
