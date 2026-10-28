//! Structured (non-PTY) agent sessions — ADR-043. A [`SessionDriver`] speaks
//! one agent's protocol and turns it into [`update::SessionUpdate`]s plus task
//! status; [`executor::StructuredExecutor`] puts drivers behind the existing
//! `TaskExecutor` port so the task service and its reducer stay unchanged.

pub mod auto_check;
pub mod claude;
pub mod codex;
pub mod executor;
pub mod omp;
pub mod remote;
pub mod turn_changes;
pub mod update;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::tasks::{ProtocolFamily, TaskStatus};
use update::{Replay, SessionUpdate, SessionUpdateKind, UpdateLog};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    AllowOnce,
    AllowForSession,
    Deny { reason: Option<String> },
}

impl Decision {
    pub fn wire(&self) -> &'static str {
        match self {
            Decision::AllowOnce => "allowOnce",
            Decision::AllowForSession => "allowForSession",
            Decision::Deny { .. } => "deny",
        }
    }
}

/// Changes take effect from the next turn (ADR-043 decision 4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigChange {
    pub model: Option<String>,
    pub permission: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommand {
    Compact,
    Review,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DriverError {
    #[error("{0}")]
    Unsupported(String),
    #[error("no pending request {0}")]
    UnknownRequest(String),
    #[error("{0}")]
    Agent(String),
    #[error("the agent process is not available: {0}")]
    Transport(String),
}

pub use aeroric_protocol::session_live::{PendingPermission, SessionState};

pub trait SessionDriver: Send + Sync {
    fn family(&self) -> ProtocolFamily;
    /// Open the session without a turn (a resume, or a launch with no prompt);
    /// an idle session settles `done`.
    fn attach(&self) -> Result<(), DriverError>;
    fn prompt(&self, text: String) -> Result<(), DriverError>;
    fn interrupt(&self) -> Result<(), DriverError>;
    fn respond(&self, request_id: &str, decision: Decision) -> Result<(), DriverError>;
    fn configure(&self, change: ConfigChange) -> Result<(), DriverError>;
    fn command(&self, command: SlashCommand) -> Result<(), DriverError>;
    fn close(&self) -> Result<(), DriverError>;
    fn snapshot(&self) -> SessionState;
    /// True when idle long enough to release its process (no turn, no pending approval).
    fn idle_since_ms(&self) -> Option<u64>;
    /// Release the process; the next `prompt` resumes the same session transparently.
    fn suspend(&self);
    /// A resume gave the task a new generation; later statuses carry it.
    fn rebind(&self, generation: u64);
    /// Answer a question card (only agents that ask questions support it).
    fn answer(&self, question_id: &str, _answer: Answer) -> Result<(), DriverError> {
        Err(DriverError::UnknownRequest(question_id.to_string()))
    }
    /// Forget a remembered "always allow in this session" grant.
    fn revoke_grant(&self, _subject: &str) -> Result<(), DriverError> {
        Err(DriverError::Unsupported(
            "这个 Agent 没有可撤销的本会话授权".into(),
        ))
    }
}

/// The user's answer to a question card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Value(String),
    Confirmed(bool),
    Cancelled,
}

/// Where drivers report task status and session facts. The host implements it
/// with `TaskService::observe` / `attach_session`; tests capture the calls.
pub trait LiveStatusSink: Send + Sync {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    );
    #[allow(clippy::too_many_arguments)]
    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    );
    /// Every update as it is appended (the host publishes them; N3).
    fn update(&self, _update: &SessionUpdate) {}
}

/// Shared per-session update log; the driver appends, the host replays.
#[derive(Clone)]
pub struct SharedLog(Arc<Mutex<UpdateLog>>);

impl SharedLog {
    pub fn new(task_id: &str) -> Self {
        Self(Arc::new(Mutex::new(UpdateLog::new(task_id))))
    }
    pub fn push(&self, kind: SessionUpdateKind, at_ms: u64) -> SessionUpdate {
        self.0.lock().push(kind, at_ms)
    }
    pub fn since(&self, after: u64) -> Replay {
        self.0.lock().since(after)
    }
    pub fn page(&self, after: u64, limit: usize) -> Replay {
        self.0.lock().page(after, limit)
    }
    pub fn latest_seq(&self) -> u64 {
        self.0.lock().latest_seq()
    }
}

/// A closed session's log stays this long for clients catching up (N3 D5).
pub const CLOSED_LOG_RETENTION_MS: u64 = 10 * 60_000;

/// A forked session (S6, 2026-10-28) starts with `fork:<source session id>`
/// as its session: its first launch resumes the source as a new conversation
/// (Claude `--fork-session`, Codex `thread/fork`), and the agent's own new id
/// replaces it once the session attaches.
pub const FORK_PREFIX: &str = "fork:";

/// The source session of a fork placeholder.
pub fn fork_source(session: &str) -> Option<&str> {
    session.strip_prefix(FORK_PREFIX).filter(|s| !s.is_empty())
}

/// Live drivers by task id, plus their logs (kept a while after close).
#[derive(Default)]
pub struct DriverRegistry {
    drivers: Mutex<HashMap<String, Arc<dyn SessionDriver>>>,
    logs: Mutex<HashMap<String, SharedLog>>,
    closed_at: Mutex<HashMap<String, u64>>,
}

impl DriverRegistry {
    pub fn insert(&self, task_id: &str, driver: Arc<dyn SessionDriver>, log: SharedLog) {
        self.drivers.lock().insert(task_id.to_string(), driver);
        self.logs.lock().insert(task_id.to_string(), log);
        self.closed_at.lock().remove(task_id);
    }
    pub fn get(&self, task_id: &str) -> Option<Arc<dyn SessionDriver>> {
        self.drivers.lock().get(task_id).cloned()
    }
    /// Take the driver out; its log stays for [`CLOSED_LOG_RETENTION_MS`].
    pub fn remove(&self, task_id: &str) -> Option<Arc<dyn SessionDriver>> {
        let removed = self.drivers.lock().remove(task_id);
        if removed.is_some() {
            self.closed_at.lock().insert(task_id.to_string(), now_ms());
        }
        removed
    }
    pub fn log(&self, task_id: &str) -> Option<SharedLog> {
        self.logs.lock().get(task_id).cloned()
    }
    pub fn all(&self) -> Vec<(String, Arc<dyn SessionDriver>)> {
        self.drivers
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    }
    /// Drop the logs of sessions closed longer than the retention.
    pub fn prune_logs(&self, now: u64) {
        let expired: Vec<String> = self
            .closed_at
            .lock()
            .iter()
            .filter(|(_, at)| now.saturating_sub(**at) >= CLOSED_LOG_RETENTION_MS)
            .map(|(task, _)| task.clone())
            .collect();
        for task in expired {
            self.closed_at.lock().remove(&task);
            if !self.drivers.lock().contains_key(&task) {
                self.logs.lock().remove(&task);
            }
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
