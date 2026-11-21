//! D4 phase 2 (2026-11-21): what became of each answered permission
//! request, so an answer from elsewhere than the Mac's own window (the
//! phone) can be told the truth. A sink layer: it records the driver's
//! settlement — `PermissionResolved` (written to the agent, A2) or
//! `PermissionSendFailed` (`failed`: not written, still open; `ended`: gone
//! with the connection) — per task and request, and wakes whoever waits.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aeroric_protocol::session_live::{PermissionSendState, SessionUpdate, SessionUpdateKind};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use parking_lot::Mutex;
use tokio::sync::Notify;

use super::LiveStatusSink;

/// Settlements kept (the oldest go first).
const KEEP: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionOutcome {
    /// The answer reached the agent (`allowOnce`, `deny`…), or the agent
    /// settled it itself (`cancelled`, `resolved`).
    Resolved(String),
    /// Not written; the request is still open.
    Failed(String),
    /// Ended with the connection or turn, unanswered.
    Ended(String),
}

pub struct PermissionReceipts {
    inner: Arc<dyn LiveStatusSink>,
    seen: Mutex<HashMap<(String, String), (u64, PermissionOutcome)>>,
    next: Mutex<u64>,
    notify: Notify,
}

impl PermissionReceipts {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        Arc::new(PermissionReceipts {
            inner,
            seen: Mutex::default(),
            next: Mutex::new(0),
            notify: Notify::new(),
        })
    }

    /// Forget what was recorded for this request: called just before
    /// answering it, so the wait sees this answer's settlement only.
    pub fn forget(&self, task_id: &str, request_id: &str) {
        self.seen
            .lock()
            .remove(&(task_id.to_string(), request_id.to_string()));
    }

    pub fn outcome(&self, task_id: &str, request_id: &str) -> Option<PermissionOutcome> {
        self.seen
            .lock()
            .get(&(task_id.to_string(), request_id.to_string()))
            .map(|(_, o)| o.clone())
    }

    /// The settlement of a request, waiting at most `limit`; `None` when
    /// none came in time (the answer may still be on its way).
    pub async fn wait(
        &self,
        task_id: &str,
        request_id: &str,
        limit: Duration,
    ) -> Option<PermissionOutcome> {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(outcome) = self.outcome(task_id, request_id) {
                return Some(outcome);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.outcome(task_id, request_id);
            }
        }
    }

    fn record(&self, task_id: &str, request_id: &str, outcome: PermissionOutcome) {
        let order = {
            let mut next = self.next.lock();
            *next += 1;
            *next
        };
        let mut seen = self.seen.lock();
        if seen.len() >= KEEP {
            if let Some(oldest) = seen
                .iter()
                .min_by_key(|(_, (n, _))| *n)
                .map(|(k, _)| k.clone())
            {
                seen.remove(&oldest);
            }
        }
        seen.insert(
            (task_id.to_string(), request_id.to_string()),
            (order, outcome),
        );
        drop(seen);
        self.notify.notify_waiters();
    }
}

impl LiveStatusSink for PermissionReceipts {
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
        match &update.kind {
            SessionUpdateKind::PermissionResolved { id, decision } => self.record(
                &update.task_id,
                id,
                PermissionOutcome::Resolved(decision.clone()),
            ),
            SessionUpdateKind::PermissionSendFailed { id, state, reason } => {
                let outcome = match state {
                    PermissionSendState::Failed => PermissionOutcome::Failed(reason.clone()),
                    PermissionSendState::Ended => PermissionOutcome::Ended(reason.clone()),
                };
                self.record(&update.task_id, id, outcome);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing;
    impl LiveStatusSink for Nothing {
        fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
        fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
    }

    fn update(task: &str, kind: SessionUpdateKind) -> SessionUpdate {
        SessionUpdate {
            task_id: task.into(),
            seq: 1,
            at_ms: 1,
            kind,
        }
    }

    /// A waiter is woken by its request's settlement, whichever kind; others'
    /// settlements do not answer it; with none, the wait ends empty.
    #[tokio::test]
    async fn a_wait_ends_with_the_requests_settlement_or_nothing() {
        let receipts = PermissionReceipts::new(Arc::new(Nothing));
        let r = Arc::clone(&receipts);
        let waiting = tokio::spawn(async move { r.wait("t1", "p1", Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        receipts.update(&update(
            "t2",
            SessionUpdateKind::PermissionResolved {
                id: "p1".into(),
                decision: "allowOnce".into(),
            },
        ));
        receipts.update(&update(
            "t1",
            SessionUpdateKind::PermissionResolved {
                id: "p1".into(),
                decision: "deny".into(),
            },
        ));
        assert_eq!(
            waiting.await.unwrap(),
            Some(PermissionOutcome::Resolved("deny".into()))
        );

        receipts.update(&update(
            "t1",
            SessionUpdateKind::PermissionSendFailed {
                id: "p2".into(),
                state: PermissionSendState::Failed,
                reason: "写不进".into(),
            },
        ));
        assert_eq!(
            receipts.wait("t1", "p2", Duration::from_millis(10)).await,
            Some(PermissionOutcome::Failed("写不进".into()))
        );
        receipts.forget("t1", "p2");
        assert_eq!(
            receipts.wait("t1", "p2", Duration::from_millis(30)).await,
            None
        );
        receipts.update(&update(
            "t1",
            SessionUpdateKind::PermissionSendFailed {
                id: "p2".into(),
                state: PermissionSendState::Ended,
                reason: "连接结束".into(),
            },
        ));
        assert_eq!(
            receipts.outcome("t1", "p2"),
            Some(PermissionOutcome::Ended("连接结束".into()))
        );
    }
}
