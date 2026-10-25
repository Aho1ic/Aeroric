//! Structured (non-PTY) agent sessions — ADR-043. A [`SessionDriver`] speaks
//! one agent's protocol and turns it into [`update::SessionUpdate`]s plus task
//! status; [`executor::StructuredExecutor`] puts drivers behind the existing
//! `TaskExecutor` port so the task service and its reducer stay unchanged.

pub mod codex;
pub mod update;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::tasks::{ProtocolFamily, TaskStatus};
use update::{PlanEntry, Replay, SessionUpdate, SessionUpdateKind, UpdateLog};

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

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub tool_call_id: Option<String>,
}

/// What a client that just (re)connected needs besides replayed updates.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub task_id: String,
    pub session_id: Option<String>,
    pub turn_active: bool,
    pub pending: Vec<PendingPermission>,
    pub plan: Vec<PlanEntry>,
    pub model: Option<String>,
    pub permission: Option<String>,
    pub effort: Option<String>,
    pub context_used: Option<u64>,
    pub context_limit: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub latest_seq: u64,
}

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
    fn snapshot(&self) -> SessionSnapshot;
    /// True when idle long enough to release its process (no turn, no pending approval).
    fn idle_since_ms(&self) -> Option<u64>;
    /// Release the process; the next `prompt` resumes the same session transparently.
    fn suspend(&self);
    /// A resume gave the task a new generation; later statuses carry it.
    fn rebind(&self, generation: u64);
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
    pub fn latest_seq(&self) -> u64 {
        self.0.lock().latest_seq()
    }
}

/// Live drivers by task id, plus their logs (kept after close for replay).
#[derive(Default)]
pub struct DriverRegistry {
    drivers: Mutex<HashMap<String, Arc<dyn SessionDriver>>>,
    logs: Mutex<HashMap<String, SharedLog>>,
}

impl DriverRegistry {
    pub fn insert(&self, task_id: &str, driver: Arc<dyn SessionDriver>, log: SharedLog) {
        self.drivers.lock().insert(task_id.to_string(), driver);
        self.logs.lock().insert(task_id.to_string(), log);
    }
    pub fn get(&self, task_id: &str) -> Option<Arc<dyn SessionDriver>> {
        self.drivers.lock().get(task_id).cloned()
    }
    pub fn remove(&self, task_id: &str) -> Option<Arc<dyn SessionDriver>> {
        self.drivers.lock().remove(task_id)
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
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
