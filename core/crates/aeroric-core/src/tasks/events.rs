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
    Status { task_id: String, status: TaskStatus },
    /// Where the transcript now lives — including when a guess is replaced by a real path.
    Session(TaskSessionFacts),
}

impl TaskNotification {
    pub fn status(task_id: impl Into<String>, status: TaskStatus) -> Self {
        TaskNotification::Status {
            task_id: task_id.into(),
            status,
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
