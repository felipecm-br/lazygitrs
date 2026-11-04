use std::sync::{Arc, Mutex, mpsc};

use crate::acp::{AcpEvent, SessionId, SseSender};
use crate::config::user_config::EditorLaunch;

/// Filters applied to the commits list view.
#[derive(Debug, Default, Clone)]
pub struct CommitFilterState {
    pub branch: Vec<String>,
    pub path: Option<String>,
    pub author: Vec<String>,
}

impl CommitFilterState {
    pub fn is_active(&self) -> bool {
        !self.branch.is_empty() || self.path.is_some() || !self.author.is_empty()
    }

    pub fn clear(&mut self) {
        self.branch.clear();
        self.path = None;
        self.author.clear();
    }
}

/// Agent-Client Protocol (ACP) review notes integration state.
pub struct AcpState {
    pub rx: mpsc::Receiver<AcpEvent>,
    pub session_id: SessionId,
    pub sse_tx: SseSender,
    pub server_url: Arc<Mutex<Option<String>>>,
    pub notify_command: Arc<Mutex<Option<String>>>,
}

/// Pending external editor launch handoff after TUI suspension.
#[derive(Debug, Default)]
pub struct EditorHandoff {
    pub pending_launch: Option<EditorLaunch>,
}
