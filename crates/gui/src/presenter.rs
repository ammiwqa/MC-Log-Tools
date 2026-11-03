use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use logtools_app::{CommandResult, Presenter};

use crate::theme::open_in_system_app;

pub enum PresenterEvent {
    Progress {
        pos: u64,
        total: u64,
    },
    Message(String),
    Log(String),
    Error(String),
    ConfirmRequest {
        prompt: String,
        reply_tx: Sender<bool>,
    },
}

pub struct EguiPresenter {
    pub tx: Sender<PresenterEvent>,
}

impl Presenter for EguiPresenter {
    fn progress(&mut self, pos: u64, total: u64) {
        let _ = self.tx.send(PresenterEvent::Progress { pos, total });
    }

    fn message(&mut self, msg: &str) {
        let _ = self.tx.send(PresenterEvent::Message(msg.to_string()));
    }

    fn log(&mut self, line: &str) {
        let _ = self.tx.send(PresenterEvent::Log(line.to_string()));
    }

    fn error(&mut self, msg: &str) {
        let _ = self.tx.send(PresenterEvent::Error(msg.to_string()));
    }

    fn confirm(&mut self, prompt: &str) -> bool {
        let (reply_tx, reply_rx) = channel::<bool>();

        let sent = self
            .tx
            .send(PresenterEvent::ConfirmRequest {
                prompt: prompt.to_string(),
                reply_tx,
            })
            .is_ok();

        if !sent {
            return false;
        }

        reply_rx.recv().unwrap_or(false)
    }

    fn open_file(&mut self, path: &PathBuf) {
        open_in_system_app(path);
    }

    fn search_completed(&mut self, _path: &PathBuf) {
        // Do not auto-open in GUI; results are shown in a dedicated window instead.
    }
}

impl EguiPresenter {
    pub fn new() -> (Self, std::sync::mpsc::Receiver<PresenterEvent>) {
        let (tx, rx) = channel::<PresenterEvent>();
        (Self { tx }, rx)
    }
}
