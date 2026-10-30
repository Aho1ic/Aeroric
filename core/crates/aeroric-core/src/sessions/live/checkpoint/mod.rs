//! B4′ (2026-10-30): each task's checkpoint, derived from its structured
//! session updates and kept on disk so a successor — another model, another
//! harness, the same session tomorrow — starts from what happened rather than
//! from a summary (frontier-roadmap §3.1–3.2).
//!
//! [`CheckpointSink`] wraps the host's sink beneath the turn-change tracker,
//! so it also sees the `turnChanges` / `autoCheck` updates that tracker
//! appends after a turn. It never calls a model; [`derive`] is pure.

pub mod brief;
pub mod clarify;
pub mod derive;
pub mod store;

pub use aeroric_protocol::checkpoint::HandoffReason;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use aeroric_protocol::checkpoint::{Checkpoint, Entry, Epistemic, Obligation, ObligationState};

use super::{LiveStatusSink, SessionUpdate};
use crate::tasks::{ProtocolFamily, TaskStatus};
use derive::{Building, Change};

/// Told after a checkpoint is saved: `(task_id, revision)`.
pub type ChangeListener = Arc<dyn Fn(&str, u64) + Send + Sync>;

pub struct CheckpointSink {
    inner: Arc<dyn LiveStatusSink>,
    dir: Mutex<Option<PathBuf>>,
    building: Mutex<HashMap<String, Building>>,
    listener: Mutex<Option<ChangeListener>>,
    /// Briefs waiting for the task's next session start.
    armed: Mutex<HashMap<String, aeroric_protocol::checkpoint::HandoffReason>>,
    /// Where a ready clarification card is appended.
    registry: Mutex<Option<Arc<super::DriverRegistry>>>,
    /// Tasks whose next message gets a status reminder (same session, new model or compacted).
    reminders: Mutex<HashMap<String, aeroric_protocol::checkpoint::HandoffReason>>,
}

impl CheckpointSink {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            dir: Mutex::new(None),
            building: Mutex::default(),
            listener: Mutex::new(None),
            armed: Mutex::default(),
            registry: Mutex::new(None),
            reminders: Mutex::default(),
        })
    }

    /// Where checkpoints are kept (`<data>/checkpoints`); unset keeps them in
    /// memory only (tests, and hosts that opt out).
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    pub fn set_registry(&self, registry: Arc<super::DriverRegistry>) {
        *self.registry.lock() = Some(registry);
    }

    /// The accepted criteria that carry a command and have not passed yet,
    /// as checks the auto checker runs after each turn.
    pub fn acceptance_checks(&self, task_id: &str) -> Vec<super::auto_check::CheckSpec> {
        let Some(cp) = self.get(task_id) else {
            return Vec::new();
        };
        cp.obligations
            .iter()
            .filter(|o| {
                o.source == aeroric_protocol::checkpoint::ObligationSource::Acceptance
                    && o.state != ObligationState::Passed
            })
            .filter_map(|o| {
                Some(super::auto_check::CheckSpec {
                    kind: "tests",
                    profile: format!("{}{}", derive::ACCEPTANCE_PROFILE, o.id),
                    label: o.text.clone(),
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), o.check.clone()?],
                })
            })
            .collect()
    }

    pub fn set_listener(&self, listener: ChangeListener) {
        *self.listener.lock() = Some(listener);
    }

    /// The task's checkpoint as it stands (in flight, else from disk).
    pub fn get(&self, task_id: &str) -> Option<Checkpoint> {
        if let Some(b) = self.building.lock().get(task_id) {
            return Some(b.checkpoint.clone());
        }
        let dir = self.dir.lock().clone()?;
        store::load(&dir, task_id).ok().flatten()
    }

    /// The task's next session start gets a brief (a source move, a delegation).
    pub fn arm(&self, task_id: &str, reason: aeroric_protocol::checkpoint::HandoffReason) {
        self.armed.lock().insert(task_id.to_string(), reason);
    }

    pub fn take_armed(&self, task_id: &str) -> Option<aeroric_protocol::checkpoint::HandoffReason> {
        self.armed.lock().remove(task_id)
    }

    /// The task's next message gets a status reminder (model switched in place, `/compact`).
    pub fn arm_reminder(&self, task_id: &str, reason: aeroric_protocol::checkpoint::HandoffReason) {
        self.reminders.lock().insert(task_id.to_string(), reason);
    }

    /// The reminder to put in front of the message, once; `None` when none
    /// was armed or nothing is open (design §2.2).
    pub fn take_reminder(&self, task_id: &str) -> Option<String> {
        self.reminders.lock().remove(task_id)?;
        self.get(task_id).and_then(|cp| brief::status_reminder(&cp))
    }

    /// Something is still open and no turn has ended for `quiet_ms`.
    pub fn stale_with_open_work(&self, task_id: &str, quiet_ms: u64) -> bool {
        self.get(task_id).is_some_and(|cp| {
            cp.needs_attention()
                && cp.updated_ms > 0
                && now_ms().saturating_sub(cp.updated_ms) >= quiet_ms
        })
    }

    /// A message the user sent: the first one is the task's goal.
    pub fn note_prompt(&self, task_id: &str, text: &str) {
        self.edit(task_id, |b| b.note_prompt(text, None));
    }

    /// Add a constraint the user stated (clarification card, progress page).
    pub fn add_constraint(&self, task_id: &str, text: &str) {
        let text = derive::cut(text);
        if text.is_empty() {
            return;
        }
        self.edit(task_id, |b| {
            if b.checkpoint.constraints.iter().any(|c| c.text == text) {
                return Change::None;
            }
            b.checkpoint.constraints.push(Entry {
                text,
                status: Epistemic::Verified,
                evidence: vec![],
            });
            Change::Save
        });
    }

    /// Add obligations (accepted criteria); ids that exist are replaced.
    pub fn add_obligations(&self, task_id: &str, obligations: Vec<Obligation>) {
        self.edit(task_id, |b| {
            let list = &mut b.checkpoint.obligations;
            for o in obligations {
                list.retain(|x| x.id != o.id);
                list.push(o);
            }
            Change::Save
        });
    }

    /// Settle an obligation by hand; false when the task has no such one.
    pub fn set_obligation(&self, task_id: &str, id: &str, state: ObligationState) -> bool {
        let mut found = false;
        self.edit(task_id, |b| {
            match b.checkpoint.obligations.iter_mut().find(|o| o.id == id) {
                Some(o) => {
                    found = true;
                    o.state = state;
                    Change::Save
                }
                None => Change::None,
            }
        });
        found
    }

    /// Record a side branch's result on its parent.
    pub fn add_branch(&self, task_id: &str, result: aeroric_protocol::checkpoint::BranchResult) {
        self.edit(task_id, |b| {
            b.checkpoint
                .branches
                .retain(|r| r.task_id != result.task_id);
            b.checkpoint.branches.push(result);
            Change::Save
        });
    }

    /// Forget a task's checkpoint (the task was deleted).
    pub fn remove(&self, task_id: &str) {
        self.building.lock().remove(task_id);
        if let Some(dir) = self.dir.lock().clone() {
            let _ = store::remove(&dir, task_id);
        }
    }

    fn edit(&self, task_id: &str, f: impl FnOnce(&mut Building) -> Change) {
        let saved = {
            let mut map = self.building.lock();
            let building = map
                .entry(task_id.to_string())
                .or_insert_with(|| Building::new(self.load_or_new(task_id)));
            match f(building) {
                Change::Save => self.save(&mut building.checkpoint),
                _ => None,
            }
        };
        if let Some(revision) = saved {
            self.notify(task_id, revision);
        }
    }

    fn load_or_new(&self, task_id: &str) -> Checkpoint {
        self.dir
            .lock()
            .clone()
            .and_then(|dir| store::load(&dir, task_id).ok().flatten())
            .unwrap_or_else(|| Checkpoint::new(task_id))
    }

    /// Bump the revision and write; `None` when there is nowhere to write.
    fn save(&self, checkpoint: &mut Checkpoint) -> Option<u64> {
        checkpoint.revision += 1;
        checkpoint.updated_ms = now_ms();
        let dir = self.dir.lock().clone()?;
        match store::save(&dir, checkpoint) {
            Ok(()) => Some(checkpoint.revision),
            Err(error) => {
                eprintln!("checkpoint {} not saved: {error}", checkpoint.task_id);
                None
            }
        }
    }

    fn notify(&self, task_id: &str, revision: u64) {
        let listener = self.listener.lock().clone();
        if let Some(listener) = listener {
            listener(task_id, revision);
        }
    }
}

impl LiveStatusSink for CheckpointSink {
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
        self.edit(&update.task_id, |b| b.apply(update));
        self.inner.update(update);
        let card = self
            .building
            .lock()
            .get_mut(&update.task_id)
            .and_then(|b| b.clarification.take());
        let registry = self.registry.lock().clone();
        if let (Some(card), Some(registry)) = (card, registry) {
            if let Some(log) = registry.log(&update.task_id) {
                let appended = log.push(card, now_ms());
                self.inner.update(&appended);
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::session_live::{SessionUpdateKind, TurnEndReason};

    struct Quiet;
    impl LiveStatusSink for Quiet {
        fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
        fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
    }

    fn end(seq: u64) -> SessionUpdate {
        SessionUpdate {
            task_id: "t1".into(),
            seq,
            at_ms: 0,
            kind: SessionUpdateKind::TurnEnded {
                turn_id: "u".into(),
                reason: TurnEndReason::Completed,
                error: None,
            },
        }
    }

    #[test]
    fn a_saved_checkpoint_survives_a_new_sink() {
        let dir = store::tests::Scratch::new();
        let sink = CheckpointSink::new(Arc::new(Quiet));
        sink.set_dir(dir.path().to_path_buf());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        sink.set_listener(Arc::new(move |task: &str, rev| {
            seen2.lock().push((task.to_string(), rev))
        }));
        sink.note_prompt("t1", "修复超时");
        sink.update(&end(1));
        sink.update(&end(2));
        sink.add_constraint("t1", "不要推送");
        assert_eq!(seen.lock().last().cloned(), Some(("t1".to_string(), 3)));

        let again = CheckpointSink::new(Arc::new(Quiet));
        again.set_dir(dir.path().to_path_buf());
        let cp = again.get("t1").unwrap();
        assert_eq!(cp.turn, 2);
        assert_eq!(cp.goal.unwrap().text, "修复超时");
        assert_eq!(cp.constraints[0].text, "不要推送");
        assert_eq!(cp.revision, 3);

        again.remove("t1");
        assert!(again.get("t1").is_none());
    }

    #[test]
    fn without_a_dir_nothing_is_written_but_the_checkpoint_is_kept() {
        let sink = CheckpointSink::new(Arc::new(Quiet));
        sink.update(&end(1));
        assert_eq!(sink.get("t1").unwrap().turn, 1);
    }
}
