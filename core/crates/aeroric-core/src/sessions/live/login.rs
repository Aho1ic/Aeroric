//! R1-3a (2026-11-03): logins the agent refused (Q-S11). A turn or a start
//! that failed for want of authentication marks its login — family, place
//! and source — expired; a turn under it that completes clears the mark and
//! gives what waited on it one more try. Replayed history never touches it:
//! only live updates pass through here.
//!
//! It sits in the sink chain right below the delivery queue, so it also sees
//! the queue's failed deliveries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use aeroric_protocol::delivery::DeliveryState;
use aeroric_protocol::live_capability::ExpiredLogin;
use aeroric_protocol::session_live::{SessionUpdate, SessionUpdateKind, TurnEndReason};
use parking_lot::Mutex;

use super::delivery::state::auth_failure;
use super::delivery::Deliveries;
use super::{now_ms, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

const FILE: &str = "login-state.json";

/// A task's login, as the host works it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginKey {
    pub id: String,
    pub family: ProtocolFamily,
    pub label: String,
    pub command: Option<String>,
    pub config_id: Option<String>,
    pub connection_id: Option<String>,
}

type Resolver = Arc<dyn Fn(&str) -> Option<LoginKey> + Send + Sync>;

pub struct Logins {
    inner: Arc<dyn LiveStatusSink>,
    resolve: Mutex<Option<Resolver>>,
    path: Mutex<Option<PathBuf>>,
    expired: Mutex<BTreeMap<String, ExpiredLogin>>,
    deliveries: Mutex<Weak<Deliveries>>,
}

impl Logins {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            resolve: Mutex::new(None),
            path: Mutex::new(None),
            expired: Mutex::default(),
            deliveries: Mutex::new(Weak::new()),
        })
    }

    pub fn set_resolver(&self, resolve: impl Fn(&str) -> Option<LoginKey> + Send + Sync + 'static) {
        *self.resolve.lock() = Some(Arc::new(resolve));
    }

    pub(super) fn set_deliveries(&self, deliveries: Weak<Deliveries>) {
        *self.deliveries.lock() = deliveries;
    }

    /// Keep the marks in `dir` (the data directory), across restarts.
    pub fn set_dir(&self, dir: &Path) {
        let path = dir.join(FILE);
        let saved: BTreeMap<String, ExpiredLogin> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        *self.expired.lock() = saved;
        *self.path.lock() = Some(path);
    }

    /// Every expired login, newest first.
    pub fn list(&self) -> Vec<ExpiredLogin> {
        let mut list: Vec<_> = self.expired.lock().values().cloned().collect();
        list.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
        list
    }

    /// This task's login, when it is expired.
    pub fn of_task(&self, task_id: &str) -> Option<ExpiredLogin> {
        let key = self.key(task_id)?;
        self.expired.lock().get(&key.id).cloned()
    }

    fn key(&self, task_id: &str) -> Option<LoginKey> {
        let resolve = self.resolve.lock().clone()?;
        resolve(task_id)
    }

    fn mark(&self, task_id: &str, reason: &str) {
        if !auth_failure(reason) {
            return;
        }
        let Some(key) = self.key(task_id) else {
            return;
        };
        let entry = ExpiredLogin {
            id: key.id.clone(),
            family: key.family,
            label: key.label,
            reason: reason.to_string(),
            at_ms: now_ms(),
            command: key.command,
            config_id: key.config_id,
            connection_id: key.connection_id,
        };
        self.expired.lock().insert(key.id, entry);
        self.save();
    }

    fn clear(&self, task_id: &str) {
        let Some(key) = self.key(task_id) else {
            return;
        };
        if self.expired.lock().remove(&key.id).is_none() {
            return;
        }
        self.save();
        // Q7: what waited on this login gets one more try.
        let Some(deliveries) = self.deliveries.lock().upgrade() else {
            return;
        };
        let resolve = self.resolve.lock().clone();
        deliveries.retry_blocked(|task| {
            resolve
                .as_ref()
                .and_then(|r| r(task))
                .is_some_and(|k| k.id == key.id)
        });
    }

    fn save(&self) {
        let Some(path) = self.path.lock().clone() else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec_pretty(&*self.expired.lock()) else {
            return;
        };
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, bytes).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

impl LiveStatusSink for Logins {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        if status == TaskStatus::Failed {
            if let Some(reason) = &reason {
                self.mark(task_id, reason);
            }
        }
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
        match &update.kind {
            SessionUpdateKind::TurnEnded {
                reason: TurnEndReason::Completed,
                ..
            } => self.clear(&update.task_id),
            SessionUpdateKind::TurnEnded {
                reason: TurnEndReason::Failed,
                error: Some(error),
                ..
            } => self.mark(&update.task_id, error),
            SessionUpdateKind::Delivery { delivery } if delivery.state == DeliveryState::Failed => {
                if let Some(reason) = &delivery.reason {
                    self.mark(&update.task_id, reason);
                }
            }
            _ => {}
        }
        self.inner.update(update);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Quiet;
    impl LiveStatusSink for Quiet {
        fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
        fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
    }

    fn logins() -> (Arc<Logins>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aeroric-login-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let logins = Logins::new(Arc::new(Quiet));
        logins.set_dir(&dir);
        logins.set_resolver(|task| {
            Some(LoginKey {
                id: format!("claude|local|{task}"),
                family: ProtocolFamily::Claude,
                label: "Claude（本机）".into(),
                command: Some("claude".into()),
                config_id: None,
                connection_id: None,
            })
        });
        (logins, dir)
    }

    fn ended(task: &str, reason: TurnEndReason, error: Option<&str>) -> SessionUpdate {
        SessionUpdate {
            task_id: task.into(),
            seq: 1,
            at_ms: 1,
            kind: SessionUpdateKind::TurnEnded {
                turn_id: "u".into(),
                reason,
                error: error.map(str::to_string),
            },
        }
    }

    #[test]
    fn a_refused_login_is_marked_kept_and_cleared_by_a_completed_turn() {
        let (logins, dir) = logins();
        logins.update(&ended(
            "t",
            TurnEndReason::Failed,
            Some("Error: 401 Unauthorized"),
        ));
        assert_eq!(logins.of_task("t").unwrap().label, "Claude（本机）");
        logins.update(&ended("u", TurnEndReason::Failed, Some("模型返回了空回复")));
        assert!(logins.of_task("u").is_none(), "not an auth failure");

        let again = Logins::new(Arc::new(Quiet));
        again.set_dir(&dir);
        assert_eq!(again.list().len(), 1, "kept across a restart");

        logins.update(&ended("t", TurnEndReason::Completed, None));
        assert!(logins.of_task("t").is_none());
        let after = Logins::new(Arc::new(Quiet));
        after.set_dir(&dir);
        assert!(after.list().is_empty());
    }

    #[test]
    fn a_failed_start_marks_it_too() {
        let (logins, _) = logins();
        logins.status(
            "p",
            "t",
            1,
            TaskStatus::Failed,
            Some("Claude 未登录".into()),
        );
        assert!(logins.of_task("t").is_some());
    }
}
