//! The port core uses to tell the world what a task did (ADR-023).
//!
//! Direction matters. [`TaskRuntimeObserver`](super::executor::TaskRuntimeObserver) is how
//! the host reports *into* the state machine; this is how the settled state gets back *out*.
//! Both exist so there is exactly one place a status is decided and exactly one place it is
//! announced — the shape that makes "double events" impossible rather than merely unlikely.
//!
//! Core defines the port and never implements it: emitting is a host capability (Tauri's
//! `emit`, a Swift `AsyncStream`, a CLI printing nothing at all). The payload fields are
//! spelled here as *facts*, not as a wire object, because the wire shape belongs to the
//! adapter that has to stay byte-compatible with a shipping client — a different constraint
//! than the one that decides what a task's state means.

use aeroric_protocol::task::{ProtocolFamily, TaskStatus};

/// A transcript location worth telling listeners about.
///
/// `family` and `agent_preset` are `Option` because the shipping events are asymmetric in
/// exactly this way: claude/codex transcripts carry neither, omp carries `family`, and dsh
/// carries `family` plus its agent preset. Modelling them as absent-or-present (rather than
/// always sending `null`) is what lets an adapter reproduce the payloads unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSessionFacts {
    pub task_id: String,
    pub session_id: String,
    pub session_path: String,
    pub codex_like: bool,
    pub family: Option<ProtocolFamily>,
    pub agent_preset: Option<String>,
}

/// One announcement about one task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskNotification {
    /// The task moved to `status`. Emitted for every settled transition the reducer allowed.
    ///
    /// `failure_reason` is `Option` for the same reason `TaskSessionFacts::family` is: the
    /// shipping client's own listener destructures `{ task_id, status, failure_reason? }`
    /// (`src/state/app/useAppTauriEvents.ts`), and today only the `failed` emits carry the key —
    /// a `running` or `done` event sends two keys and no `null`. Absent-or-present is therefore
    /// the only modelling that reproduces the wire unchanged (ADR-023). Dropping it would take
    /// away the text the UI shows for why a task failed, silently, because nothing else reads it.
    Status {
        task_id: String,
        status: TaskStatus,
        failure_reason: Option<String>,
    },
    /// Where the transcript now lives — including when a guess is replaced by a real path.
    Session(TaskSessionFacts),
}

impl TaskNotification {
    /// 切片一百零五：窗口事件的载荷构造（Tauri `task_status_payload`/
    /// `task_session_payload` verbatim 上收——宿主 EventLog 与 Embedded 窗口
    /// 必须逐字节同形，两个发射面共用这一份实现）。`status_word`/`family_word`
    /// 与 serde 序列化的等价性由本模块 pin 测试钉住。
    pub fn window_payload(&self) -> serde_json::Value {
        match self {
            TaskNotification::Status {
                task_id,
                status,
                failure_reason,
            } => {
                let mut payload = serde_json::json!({
                    "task_id": task_id,
                    "status": status_word(*status),
                });
                if let Some(reason) = failure_reason {
                    payload["failure_reason"] = serde_json::json!(reason);
                }
                payload
            }
            TaskNotification::Session(facts) => {
                let mut payload = serde_json::json!({
                    "task_id": facts.task_id,
                    "session_id": facts.session_id,
                    "session_path": facts.session_path,
                    "codex_like": facts.codex_like,
                });
                if let Some(family) = facts.family {
                    payload["family"] = serde_json::json!(family_word(family));
                }
                if let Some(preset) = &facts.agent_preset {
                    payload["agent_preset"] = serde_json::json!(preset);
                }
                payload
            }
        }
    }

    pub fn status(task_id: impl Into<String>, status: TaskStatus) -> Self {
        TaskNotification::Status {
            task_id: task_id.into(),
            status,
            failure_reason: None,
        }
    }

    /// The same announcement with the reason the runtime gave. Core keeps that text on the
    /// record as `failureReason`, so an adapter reaches it from the committed row rather than
    /// inventing one.
    pub fn status_with_reason(
        task_id: impl Into<String>,
        status: TaskStatus,
        reason: impl Into<String>,
    ) -> Self {
        TaskNotification::Status {
            task_id: task_id.into(),
            status,
            failure_reason: Some(reason.into()),
        }
    }

    /// A claude/codex transcript: no `family`, no preset.
    pub fn session(
        task_id: impl Into<String>,
        session_id: impl Into<String>,
        session_path: impl Into<String>,
        codex_like: bool,
    ) -> Self {
        TaskNotification::Session(TaskSessionFacts {
            task_id: task_id.into(),
            session_id: session_id.into(),
            session_path: session_path.into(),
            codex_like,
            family: None,
            agent_preset: None,
        })
    }

    pub fn task_id(&self) -> &str {
        match self {
            TaskNotification::Status { task_id, .. }
            | TaskNotification::Session(TaskSessionFacts { task_id, .. }) => task_id,
        }
    }
}

/// What an adapter implements. One notification in, one announcement out — no return value,
/// because nothing about a task's state may depend on whether a listener was ready.
pub trait TaskEventSink: Send + Sync {
    fn notify(&self, notification: TaskNotification);
}

/// The do-nothing sink: a headless CLI run, or a test that only cares about the records.
#[derive(Default)]
pub struct NullSink;

impl TaskEventSink for NullSink {
    fn notify(&self, _notification: TaskNotification) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder {
        seen: Arc<Mutex<Vec<TaskNotification>>>,
    }

    impl TaskEventSink for Recorder {
        fn notify(&self, notification: TaskNotification) {
            self.seen.lock().unwrap().push(notification);
        }
    }

    #[test]
    fn the_two_optional_fields_are_absent_rather_than_null_for_claude_and_codex() {
        // This is the difference between "the adapter can rebuild today's payload" and
        // "it can't": sending `"family": null` for a claude transcript is a new key on the
        // wire, and the shipping frontend reads `family` to route dsh/omp sessions.
        let note = TaskNotification::session("t1", "s1", "/p.jsonl", true);
        let facts = match &note {
            TaskNotification::Session(facts) => facts,
            other => panic!("expected a session notification, got {other:?}"),
        };
        assert_eq!(facts.family, None);
        assert_eq!(facts.agent_preset, None);
        assert!(facts.codex_like);
        assert_eq!(note.task_id(), "t1");
    }

    /// A status announcement has to be able to carry the reason, or the adapter that replaces the
    /// legacy emitters has nowhere to get it from. The frontend reads `failure_reason` as an
    /// optional key and only the failed path sends one, so `None` is a real state rather than a
    /// placeholder.
    #[test]
    fn a_failure_notice_keeps_its_reason_and_a_plain_one_does_not_invent_it() {
        let plain = TaskNotification::status("t1", TaskStatus::Running);
        let failed = TaskNotification::status_with_reason("t1", TaskStatus::Failed, "exit code 1");
        match (&plain, &failed) {
            (
                TaskNotification::Status {
                    status: running,
                    failure_reason: plain_reason,
                    ..
                },
                TaskNotification::Status {
                    status: failed_status,
                    failure_reason,
                    ..
                },
            ) => {
                assert_eq!(*running, TaskStatus::Running);
                assert_eq!(*failed_status, TaskStatus::Failed);
                assert_eq!(plain_reason.as_deref(), None);
                assert_eq!(failure_reason.as_deref(), Some("exit code 1"));
            }
            other => panic!("expected two status notifications, got {other:?}"),
        }
        assert_eq!(plain.task_id(), "t1");
        assert_eq!(failed.task_id(), "t1");
    }

    #[test]
    fn dsh_carries_both_extras_and_omp_only_the_family() {
        let omp = TaskNotification::Session(TaskSessionFacts {
            task_id: "t2".into(),
            session_id: "s2".into(),
            session_path: "/o.jsonl".into(),
            codex_like: false,
            family: Some(ProtocolFamily::Omp),
            agent_preset: None,
        });
        let dsh = TaskNotification::Session(TaskSessionFacts {
            agent_preset: Some("standard".into()),
            family: Some(ProtocolFamily::Dsh),
            ..match &omp {
                TaskNotification::Session(facts) => facts.clone(),
                _ => panic!(),
            }
        });
        match (&omp, &dsh) {
            (TaskNotification::Session(omp_facts), TaskNotification::Session(dsh_facts)) => {
                assert_eq!(omp_facts.agent_preset, None, "omp has no preset to send");
                assert_eq!(dsh_facts.agent_preset.as_deref(), Some("standard"));
                assert_eq!(dsh_facts.task_id, "t2");
            }
            other => panic!("both must be sessions: {other:?}"),
        }
    }

    #[test]
    fn a_sink_sees_notifications_in_order_and_a_null_sink_sees_nothing_at_all() {
        let recorder = Recorder::default();
        let seen = Arc::clone(&recorder.seen);
        recorder.notify(TaskNotification::status("t1", TaskStatus::Running));
        recorder.notify(TaskNotification::session("t1", "s1", "/p", false));
        let logged = seen.lock().unwrap().clone();
        assert_eq!(logged.len(), 2, "the sink is the only announcement channel");
        assert_eq!(
            logged[0],
            TaskNotification::status("t1", TaskStatus::Running)
        );
        NullSink.notify(TaskNotification::status("t1", TaskStatus::Done));
    }
}

/// 冻结的九个状态词与四个家族词（Tauri core_adapter verbatim 上收）：
/// 手写全函数而非 serde 透传，是为了让新增变体在两侧 pin 测试处当场红掉。
pub(crate) const fn status_word(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::InputRequired => "input_required",
        TaskStatus::Detached => "detached",
        TaskStatus::Interrupted => "interrupted",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

pub(crate) const fn family_word(family: ProtocolFamily) -> &'static str {
    match family {
        ProtocolFamily::Claude => "claude",
        ProtocolFamily::Codex => "codex",
        ProtocolFamily::Dsh => "dsh",
        ProtocolFamily::Omp => "omp",
    }
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    #[test]
    fn the_event_words_match_their_serialized_forms() {
        for status in TaskStatus::ALL {
            let serialized = serde_json::to_value(status).expect("a status is a string");
            assert_eq!(
                serialized.as_str(),
                Some(status_word(*status)),
                "{status:?}"
            );
        }
        for family in ProtocolFamily::ALL {
            let serialized = serde_json::to_value(family).expect("a family is a string");
            assert_eq!(
                serialized.as_str(),
                Some(family_word(*family)),
                "{family:?}"
            );
        }
    }

    #[test]
    fn the_window_payload_matches_the_shipping_wire() {
        let status = TaskNotification::status_with_reason("t1", TaskStatus::Failed, "boom");
        assert_eq!(
            status.window_payload(),
            serde_json::json!({"task_id": "t1", "status": "failed", "failure_reason": "boom"})
        );
        let plain = TaskNotification::status("t1", TaskStatus::Running);
        assert_eq!(
            plain.window_payload(),
            serde_json::json!({"task_id": "t1", "status": "running"})
        );
    }
}
