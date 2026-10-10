#[derive(Debug, Clone)]

pub enum CoreEvent {
    Progress { pos: u64, total: u64 },
    Message(String),
}

pub type EventSender = std::sync::mpsc::Sender<CoreEvent>;
