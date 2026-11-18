//! B1-1 (2026-11-18): a read-only session asks nobody for a write.
//!
//! Discussion members, code reviewers and read-only delegates run with the
//! permission mode `plan` — the only sessions that do: a person picks `ask`,
//! `auto_edit` or `full_access`. Any permission request such a session makes
//! (leaving plan mode, writing, editing, running a command, an outside tool)
//! asks to go beyond reading, so the host denies it at once instead of
//! putting a card in front of the person that one click would approve.

use std::sync::Arc;

use super::update::{SessionUpdate, SessionUpdateKind};
use super::{Decision, DriverRegistry, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// The permission mode of the read-only roles.
pub const READ_ONLY_MODE: &str = "plan";

/// What the agent is told when its request is denied.
pub const DENIED: &str = "这是只读会话：不能改文件、执行命令或交计划。请直接用文字回答。";

/// Sits on top of the sink chain and denies what a read-only session asks.
pub struct ReadOnlyGuard {
    inner: Arc<dyn LiveStatusSink>,
    registry: Arc<DriverRegistry>,
}

impl ReadOnlyGuard {
    pub fn new(inner: Arc<dyn LiveStatusSink>, registry: Arc<DriverRegistry>) -> Self {
        Self { inner, registry }
    }
}

impl LiveStatusSink for ReadOnlyGuard {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.inner
            .status(project_id, task_id, generation, status, reason);
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        self.inner
            .session(project_id, task_id, family, agent, session_id, session_path);
    }

    fn update(&self, update: &SessionUpdate) {
        self.inner.update(update);
        let SessionUpdateKind::PermissionRequest { id, .. } = &update.kind else {
            return;
        };
        let Some(driver) = self.registry.get(&update.task_id) else {
            return;
        };
        if driver.snapshot().permission.as_deref() != Some(READ_ONLY_MODE) {
            return;
        }
        // Off the driver's own thread: the answer goes back through it.
        let (task_id, id) = (update.task_id.clone(), id.clone());
        std::thread::spawn(move || {
            let decision = Decision::Deny {
                reason: Some(DENIED.into()),
            };
            if let Err(error) = driver.respond(&id, decision) {
                eprintln!("[read-only] could not deny {id} of {task_id}: {error:?}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::live::{
        ConfigChange, DriverError, SessionDriver, SessionState, SharedLog, SlashCommand,
    };
    use parking_lot::Mutex;

    struct Fake {
        permission: &'static str,
        answered: Mutex<Vec<(String, Decision)>>,
    }

    impl SessionDriver for Fake {
        fn family(&self) -> ProtocolFamily {
            ProtocolFamily::Claude
        }
        fn attach(&self) -> Result<(), DriverError> {
            Ok(())
        }
        fn prompt(&self, _text: String) -> Result<(), DriverError> {
            Ok(())
        }
        fn interrupt(&self) -> Result<(), DriverError> {
            Ok(())
        }
        fn respond(&self, request_id: &str, decision: Decision) -> Result<(), DriverError> {
            self.answered
                .lock()
                .push((request_id.to_string(), decision));
            Ok(())
        }
        fn configure(&self, _change: ConfigChange) -> Result<(), DriverError> {
            Ok(())
        }
        fn command(&self, _command: SlashCommand) -> Result<(), DriverError> {
            Ok(())
        }
        fn close(&self) -> Result<(), DriverError> {
            Ok(())
        }
        fn snapshot(&self) -> SessionState {
            SessionState {
                permission: Some(self.permission.to_string()),
                ..SessionState::default()
            }
        }
        fn idle_since_ms(&self) -> Option<u64> {
            None
        }
        fn suspend(&self) {}
        fn rebind(&self, _generation: u64) {}
    }

    struct Quiet;
    impl LiveStatusSink for Quiet {
        fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
        fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
    }

    fn request(task: &str, kind: &str) -> SessionUpdate {
        SessionUpdate {
            task_id: task.into(),
            seq: 1,
            at_ms: 1,
            kind: SessionUpdateKind::PermissionRequest {
                id: format!("{task}-r"),
                tool_call_id: None,
                kind: kind.into(),
                summary: "write a file".into(),
                options: vec!["allowOnce".into(), "deny".into()],
                subagent: None,
            },
        }
    }

    /// B1-1: whatever a read-only session asks — a write, a command,
    /// leaving plan mode — is denied by the host; a session the person runs
    /// in another mode still gets its card answered by the person.
    #[test]
    fn a_read_only_sessions_requests_are_denied_and_others_are_left_alone() {
        let registry: Arc<DriverRegistry> = Arc::default();
        let reader = Arc::new(Fake {
            permission: READ_ONLY_MODE,
            answered: Mutex::default(),
        });
        let writer = Arc::new(Fake {
            permission: "ask",
            answered: Mutex::default(),
        });
        registry.insert("reader", reader.clone(), SharedLog::new("reader"));
        registry.insert("writer", writer.clone(), SharedLog::new("writer"));
        let guard = ReadOnlyGuard::new(Arc::new(Quiet), Arc::clone(&registry));
        for kind in ["Write", "Bash", "ExitPlanMode", "mcp__fs__write_file"] {
            guard.update(&request("reader", kind));
        }
        guard.update(&request("writer", "Write"));
        for _ in 0..200 {
            if reader.answered.lock().len() == 4 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let answered = reader.answered.lock();
        assert_eq!(answered.len(), 4);
        assert!(answered.iter().all(|(_, d)| *d
            == Decision::Deny {
                reason: Some(DENIED.into())
            }));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(writer.answered.lock().is_empty());
    }
}
