//! Which start attempts have already been claimed, so retrying one cannot spawn twice.
//!
//! The client's only defence here was object identity: compare the task it was about to
//! launch against the task still in `tasksRef`, and hand the spawn to a one-shot callback
//! that the terminal-ready event deletes. That holds inside one renderer with one clock.
//! Two windows, or a request that times out and is retried, or a process that dies between
//! "saved" and "spawned" — none of those are covered by an `!==` comparison.
//!
//! So the claim is derived, not generated: `(task id, kind, generation)` *is* the key. A
//! retry of the same attempt recomputes the same key and is told it is a duplicate, which
//! needs no clock, no uuid, and no agreement between processes beyond the record itself.
//!
//! This file is core-exclusive (`task-operations.json`), for the same reason
//! `project-groups.json` is: the legacy client would silently drop fields it does not know
//! on its next full-list save. See the `legacy-client-ignores-core-exclusive-files` gate.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use serde_json::Value;

use aeroric_protocol::task::TaskStatus;

use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};
use crate::storage::compat::{self, Record};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Start,
    Resume,
    Cancel,
    Complete,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OperationKind::Start => "start",
            OperationKind::Resume => "resume",
            OperationKind::Cancel => "cancel",
            OperationKind::Complete => "complete",
        }
    }
}

/// A claim on one attempt. `generation` is the task's run counter, so "start again after a
/// resume" is a different key and legitimately spawns again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claim {
    pub task_id: String,
    pub kind: OperationKind,
    /// The caller's own identity for this attempt (plan §2.3 `idempotencyKey`). Two
    /// submissions carrying the same key are one attempt; two carrying different keys are two
    /// attempts, even on the same task (ADR-017).
    pub operation_key: String,
    pub generation: u64,
    /// When the claim was taken. Reporting only — the key is derived, so the timestamp can
    /// never decide whether an attempt is a duplicate.
    pub claimed_at_ms: u64,
}

impl Claim {
    pub fn key(&self) -> String {
        attempt_key(&self.task_id, self.kind, &self.operation_key)
    }
}

/// What makes two submissions the same attempt.
///
/// This used to be `(task, kind, generation)` — core's own run counter — which quietly meant
/// core could only tell a retry from a new run by forbidding a second run at all. The counter
/// still exists (it is what rejects a stale exit and what the executor reports events
/// against), but it no longer doubles as the retry identity.
pub fn attempt_key(task_id: &str, kind: OperationKind, operation_key: &str) -> String {
    format!("{task_id}|{}|{operation_key}", kind.as_str())
}

/// What asking for a claim produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claimed {
    /// Take this attempt: the caller may write, then spawn.
    New(Claim),
    /// Somebody already claimed exactly this attempt. Do not spawn again.
    Duplicate(Claim),
}

/// A claim the last process left behind, and what the state may be reconciled to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orphan {
    pub claim: Claim,
    /// The task as found on disk. `None` when it has since been deleted, which settles the
    /// claim with no state to reconcile.
    pub task: Option<Record>,
}

/// The whole file. Settled claims are pruned, but a task's generation counter is kept:
/// it is what makes "an exit for run 2 arrived after run 3 started" decidable, and an
/// append-forever claim log is not a price worth paying for that.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerState {
    #[serde(default)]
    pub claims: Vec<Claim>,
    /// Highest generation ever claimed per task, so the next one is `+1` and an old
    /// event can be recognised as old.
    #[serde(default)]
    pub generations: BTreeMap<String, u64>,
    /// Runs reconciliation cut off without restarting, which nobody has acknowledged yet
    /// (ADR-018). Until it is, the task may not be *resumed*: continuing "from where it
    /// stopped" is a claim about a point the user never agreed was lost.
    #[serde(default)]
    pub awaiting_acknowledgement: Vec<Acknowledgement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Acknowledgement {
    pub task_id: String,
    /// Which run was cut off, so a confirmation can name it.
    pub generation: u64,
    pub since_ms: u64,
}

/// The ledger, held in memory and mirrored to disk by [`store`].
#[derive(Clone, Debug, Default)]
pub struct OperationLedger {
    claims: Vec<Claim>,
    generations: BTreeMap<String, u64>,
    awaiting_acknowledgement: Vec<Acknowledgement>,
}

impl OperationLedger {
    /// What the last process left on disk, if anything.
    pub fn load_state(
        context: &crate::context::CoreContext,
    ) -> Result<Option<LedgerState>, StorageError> {
        Ok(load(context)?.found())
    }

    pub fn persist(&self, context: &crate::context::CoreContext) -> Result<(), StorageError> {
        store(context, &self.state())
    }

    pub fn new(state: LedgerState) -> Self {
        let mut ledger = OperationLedger {
            claims: state.claims,
            generations: state.generations,
            awaiting_acknowledgement: state.awaiting_acknowledgement,
        };
        ledger.dedupe();
        ledger
    }

    pub fn empty() -> Self {
        OperationLedger::default()
    }

    /// The generation the next attempt on this task gets. Reading it does not consume it.
    pub fn peek_generation(&self, task_id: &str) -> u64 {
        self.generations.get(task_id).copied().unwrap_or(0)
    }

    pub fn next_generation(&mut self, task_id: &str) -> u64 {
        let next = self.peek_generation(task_id) + 1;
        self.generations.insert(task_id.to_string(), next);
        next
    }

    /// Two entries for one key would make "is this a duplicate?" depend on which one a
    /// scan happens to hit first.
    fn dedupe(&mut self) {
        let mut seen = Vec::<String>::new();
        self.claims.retain(|claim| {
            let key = claim.key();
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
            true
        });
    }

    pub fn claims(&self) -> &[Claim] {
        &self.claims
    }

    /// Give an unused generation back. A claim refused before it could spawn must not leave
    /// a hole in the counter, or an event from the future would look current.
    pub fn release_generation(&mut self, task_id: &str, generation: u64) {
        let still_open = self
            .claims
            .iter()
            .any(|claim| claim.task_id == task_id && claim.generation == generation);
        if still_open || self.peek_generation(task_id) != generation {
            return;
        }
        let previous = generation.saturating_sub(1);
        if previous == 0 {
            self.generations.remove(task_id);
        } else {
            self.generations.insert(task_id.to_string(), previous);
        }
    }

    pub fn state(&self) -> LedgerState {
        LedgerState {
            claims: self.claims.clone(),
            generations: self.generations.clone(),
            awaiting_acknowledgement: self.awaiting_acknowledgement.clone(),
        }
    }

    pub fn awaiting_acknowledgement(&self, task_id: &str) -> Option<Acknowledgement> {
        self.awaiting_acknowledgement
            .iter()
            .find(|item| item.task_id == task_id)
            .cloned()
    }

    pub fn acknowledge(&mut self, task_id: &str) -> bool {
        let before = self.awaiting_acknowledgement.len();
        self.awaiting_acknowledgement
            .retain(|item| item.task_id != task_id);
        self.awaiting_acknowledgement.len() != before
    }

    pub fn await_acknowledgement(&mut self, item: Acknowledgement) {
        self.awaiting_acknowledgement
            .retain(|existing| existing.task_id != item.task_id);
        self.awaiting_acknowledgement.push(item);
    }

    /// The attempt of this kind that is still in flight, if any. Checked *before* a
    /// generation is reserved: a retry must not cost a number, or every retry would arrive
    /// with a fresh key and the dedupe could never fire.
    pub fn open_claim(
        &self,
        task_id: &str,
        kind: OperationKind,
        operation_key: &str,
    ) -> Option<&Claim> {
        let key = attempt_key(task_id, kind, operation_key);
        self.claims.iter().find(|claim| claim.key() == key)
    }

    /// Every attempt of this task still in flight. With concurrent runs allowed this is what
    /// "the task is busy" means (ADR-017 leaves the status consequence to P2-05).
    /// The runs of this task still in flight, ignoring one generation (the one that just
    /// reported). This is what "the task is not finished yet" means once runs can overlap.
    pub fn live_other_than(&self, task_id: &str, except: u64) -> Vec<u64> {
        self.claims
            .iter()
            .filter(|claim| {
                claim.task_id == task_id
                    && claim.generation != except
                    && matches!(claim.kind, OperationKind::Start | OperationKind::Resume)
            })
            .map(|claim| claim.generation)
            .collect()
    }

    /// Is this run of this task still in flight? A generation that has no open claim has
    /// already reported its outcome once, so an event arriving for it afterwards is stale —
    /// which is a different question from "is it the newest number".
    pub fn is_live(&self, task_id: &str, generation: u64) -> bool {
        self.claims
            .iter()
            .any(|claim| claim.task_id == task_id && claim.generation == generation)
    }

    pub fn live_for(&self, task_id: &str) -> Vec<&Claim> {
        self.claims
            .iter()
            .filter(|claim| claim.task_id == task_id)
            .collect()
    }

    /// Take (or refuse) an attempt.
    pub fn claim(
        &mut self,
        task_id: &str,
        kind: OperationKind,
        operation_key: &str,
        generation: u64,
        now_ms: u64,
    ) -> Claimed {
        let key = attempt_key(task_id, kind, operation_key);
        if let Some(existing) = self.claims.iter().find(|claim| claim.key() == key) {
            return Claimed::Duplicate(existing.clone());
        }
        let claim = Claim {
            task_id: task_id.to_string(),
            kind,
            operation_key: operation_key.to_string(),
            generation,
            claimed_at_ms: now_ms,
        };
        self.claims.push(claim.clone());
        Claimed::New(claim)
    }

    /// The attempt reached a settled outcome (or was refused before spawning), so a later
    /// identical request is a genuine new intent rather than a retry.
    /// Release an attempt the caller names by its own key.
    pub fn settle(&mut self, task_id: &str, kind: OperationKind, operation_key: &str) -> bool {
        let key = attempt_key(task_id, kind, operation_key);
        let before = self.claims.len();
        self.claims.retain(|claim| claim.key() != key);
        self.claims.len() != before
    }

    /// Release an attempt an *event* named by generation. A hook or an exit does not know
    /// the caller's idempotency key, but the run number it carries is enough to find the
    /// claim that was taken for it.
    pub fn settle_by_generation(
        &mut self,
        task_id: &str,
        kind: OperationKind,
        generation: u64,
    ) -> bool {
        let before = self.claims.len();
        self.claims.retain(|claim| {
            !(claim.task_id == task_id && claim.kind == kind && claim.generation == generation)
        });
        self.claims.len() != before
    }

    pub fn settle_task(&mut self, task_id: &str) -> usize {
        let before = self.claims.len();
        self.claims.retain(|claim| claim.task_id != task_id);
        before - self.claims.len()
    }

    /// Every claim still open on a task: a start that has not settled must not be followed
    /// by a second spawn, whatever the status field says.
    pub fn open_for(&self, task_id: &str) -> Vec<&Claim> {
        self.claims
            .iter()
            .filter(|claim| claim.task_id == task_id)
            .collect()
    }

    /// What the last process left half-done. Only start/resume claims can mean a process
    /// was spawned; a dangling cancel or complete is bookkeeping, not a running task.
    pub fn orphans(&self, records: &[Record]) -> Vec<Orphan> {
        self.claims
            .iter()
            .filter(|claim| matches!(claim.kind, OperationKind::Start | OperationKind::Resume))
            .map(|claim| Orphan {
                claim: claim.clone(),
                task: records
                    .iter()
                    .find(|record| record.id() == Some(claim.task_id.as_str()))
                    .cloned(),
            })
            .collect()
    }
}

/// Reconciliation after a crash: an unresolved start or resume becomes `interrupted`,
/// which is resumable by the user but is *not* restarted by us. Re-spawning a task whose
/// process may still be running would give one task two agents writing one tree.
///
/// A task already `detached` keeps that status: detaching is the state a process the user
/// left running produces, and rewriting it to `interrupted` would claim the process was
/// lost when it was not.
///
/// No `completedAt` is stamped: `interrupted` is not a terminal status, and P2-03's rule is
/// that a non-terminal row carries no end time. An earlier draft stamped one, which put an
/// end date on a task that had not ended.
///
/// The cut-off runs come back as [`Acknowledgement`]s, which is what blocks a resume until
/// someone has seen them (ADR-018).
pub fn reconcile(
    claims: &[Claim],
    records: &[Record],
    now_ms: u64,
) -> (Vec<Record>, Vec<Acknowledgement>) {
    let mut next = records.to_vec();
    let mut reconciled = Vec::new();
    for claim in claims {
        if !matches!(claim.kind, OperationKind::Start | OperationKind::Resume) {
            continue;
        }
        let Some(record) = next
            .iter_mut()
            .find(|record| record.id() == Some(claim.task_id.as_str()))
        else {
            continue;
        };
        let status = crate::tasks::reducer::status_of(record);
        if matches!(status, Some(TaskStatus::Detached)) {
            continue;
        }
        let needs_mark = !matches!(
            status,
            Some(TaskStatus::Interrupted) | Some(TaskStatus::Done) | Some(TaskStatus::Failed)
        );
        if !needs_mark {
            continue;
        }
        record.0.insert(
            "status".to_string(),
            Value::String(
                serde_json::to_value(TaskStatus::Interrupted)
                    .expect("a status is a string")
                    .as_str()
                    .expect("a status is a string")
                    .to_string(),
            ),
        );
        record.0.remove("attentionRequestedAt");
        reconciled.push(Acknowledgement {
            task_id: claim.task_id.clone(),
            generation: claim.generation,
            since_ms: now_ms,
        });
    }
    (next, reconciled)
}

pub fn load(
    context: &crate::context::CoreContext,
) -> Result<ReadOutcome<LedgerState>, StorageError> {
    let path = context.paths.task_operations_file();
    match atomic::read_json(&path)? {
        ReadOutcome::Absent => Ok(ReadOutcome::Absent),
        ReadOutcome::Found(value) => serde_json::from_value::<LedgerState>(value)
            .map(ReadOutcome::Found)
            .map_err(|e| StorageError::Corrupt {
                path,
                reason: e.to_string(),
            }),
    }
}

pub fn store(
    context: &crate::context::CoreContext,
    state: &LedgerState,
) -> Result<(), StorageError> {
    context.require_write()?;
    let path = context.paths.task_operations_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StorageError::WriteFailed {
            path: parent.to_path_buf(),
            reason: e.to_string(),
        })?;
    }
    let value = serde_json::to_value(state).expect("a ledger is serializable");
    let bytes = compat::pretty(&value);
    atomic::write_atomic(&path, &bytes, FileMode::Private)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::writer;
    use serde_json::json;

    fn record(value: Value) -> Record {
        Record(value.as_object().unwrap().clone())
    }

    fn running() -> Record {
        record(json!({"id": "t1", "status": "pending", "createdAt": 1i64}))
    }

    #[test]
    fn a_retry_of_the_same_attempt_is_the_same_key() {
        let mut ledger = OperationLedger::empty();
        let first = ledger.claim("t1", OperationKind::Start, "k-0", 0, 100);
        assert!(matches!(first, Claimed::New(_)));
        // A different caller, a different clock, the same attempt.
        let second = ledger.claim("t1", OperationKind::Start, "k-0", 0, 9_999);
        assert_eq!(
            second,
            Claimed::Duplicate(Claim {
                task_id: "t1".into(),
                kind: OperationKind::Start,
                operation_key: "k-0".to_string(),
                generation: 0,
                claimed_at_ms: 100,
            }),
            "the duplicate reports the original claim, not a fresh timestamp"
        );
        assert_eq!(ledger.claims().len(), 1, "and nothing was appended");
        // The key is the identity now, so a *different* generation under the same key is
        // still the same attempt: core's counter cannot reopen it.
        assert!(matches!(
            ledger.claim("t1", OperationKind::Start, "k-0", 7, 101),
            Claimed::Duplicate(_)
        ));
        // A different operation key on the same task is a different attempt (ADR-017), which
        // is what lets two windows run one task.
        assert!(matches!(
            ledger.claim("t1", OperationKind::Start, "k-1", 2, 101),
            Claimed::New(_)
        ));
        assert!(matches!(
            ledger.claim("t1", OperationKind::Resume, "k-1", 0, 101),
            Claimed::New(_)
        ));
        assert!(matches!(
            ledger.claim("t2", OperationKind::Start, "k-2", 0, 101),
            Claimed::New(_)
        ));
    }

    #[test]
    fn settling_opens_the_attempt_up_again_and_only_for_itself() {
        let mut ledger = OperationLedger::empty();
        ledger.claim("t1", OperationKind::Start, "k-0", 0, 1);
        ledger.claim("t1", OperationKind::Resume, "k-r", 1, 2);
        assert!(ledger.settle("t1", OperationKind::Start, "k-0"));
        assert!(
            !ledger.settle("t1", OperationKind::Start, "k-0"),
            "already gone"
        );
        assert!(matches!(
            ledger.claim("t1", OperationKind::Start, "k-0", 1, 101),
            Claimed::New(_)
        ));
        assert_eq!(
            ledger
                .claims()
                .iter()
                .filter(|claim| claim.kind == OperationKind::Resume)
                .count(),
            1,
            "the resume claim was untouched by settling the start"
        );
        assert_eq!(ledger.settle_task("t1"), 2);
        assert!(ledger.claims().is_empty());
    }

    #[test]
    fn a_reloaded_ledger_collapses_duplicate_rows_instead_of_choosing_one() {
        let duplicated = LedgerState {
            awaiting_acknowledgement: Vec::new(),
            claims: vec![
                Claim {
                    task_id: "t1".into(),
                    kind: OperationKind::Start,
                    operation_key: "k-0".to_string(),
                    generation: 0,
                    claimed_at_ms: 1,
                },
                Claim {
                    task_id: "t1".into(),
                    kind: OperationKind::Start,
                    operation_key: "k-0".to_string(),
                    generation: 0,
                    claimed_at_ms: 2,
                },
            ],
            generations: Default::default(),
        };
        let ledger = OperationLedger::new(duplicated);
        assert_eq!(ledger.claims().len(), 1);
        assert_eq!(ledger.claims()[0].claimed_at_ms, 1, "the first wins");
    }

    #[test]
    fn a_crash_between_saving_and_spawning_reconciles_to_interrupted_not_to_a_new_process() {
        let mut ledger = OperationLedger::empty();
        ledger.claim("t1", OperationKind::Start, "k-0", 0, 1);
        let records = vec![running(), record(json!({"id": "t2", "status": "detached"}))];
        let orphans = ledger.orphans(&records);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].claim.task_id, "t1");
        let (next, reconciled) = reconcile(ledger.claims(), &records, 500);
        assert_eq!(
            reconciled
                .iter()
                .map(|item| item.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1"],
            "the cut-off run is reported as an acknowledgement, not just a name"
        );
        assert_eq!(reconciled[0].generation, 0);
        assert_eq!(reconciled[0].since_ms, 500);
        assert_eq!(next[0].get("status"), Some(&json!("interrupted")));
        assert_eq!(
            next[0].get("completedAt"),
            None,
            "interrupted is not terminal, so it carries no end time (ADR-018)"
        );
        assert_eq!(
            next[1].get("status"),
            Some(&json!("detached")),
            "a detached task's process is still running; marking it lost would be a lie"
        );
        // Reconciliation never clears a claim, so a second pass cannot quietly re-run it.
        let (again, second_pass) = reconcile(ledger.claims(), &next, 900);
        assert!(second_pass.is_empty(), "interrupted is already settled");
        assert_eq!(again, next);
    }

    #[test]
    fn an_orphan_on_a_task_that_has_since_finished_or_vanished_is_left_alone() {
        let mut ledger = OperationLedger::empty();
        ledger.claim("gone", OperationKind::Start, "k-gone", 0, 1);
        ledger.claim("done", OperationKind::Start, "k-done", 0, 1);
        ledger.claim("running", OperationKind::Cancel, "k-run", 0, 1);
        let records = vec![record(
            json!({"id": "done", "status": "done", "completedAt": 5i64}),
        )];
        let (next, reconciled) = reconcile(ledger.claims(), &records, 80);
        assert!(reconciled.is_empty(), "nothing to mark");
        assert_eq!(next, records);
    }

    #[test]
    fn the_ledger_round_trips_and_a_corrupt_one_refuses_rather_than_resetting() {
        let (root, context) = writer("ledger");
        assert_eq!(load(&context).unwrap(), ReadOutcome::Absent);
        let mut ledger = OperationLedger::empty();
        assert_eq!(
            ledger.next_generation("t1"),
            1,
            "the first run is generation 1"
        );
        ledger.claim("t1", OperationKind::Start, "k-1", 1, 41);
        assert_eq!(
            ledger.peek_generation("t1"),
            1,
            "claiming did not advance it"
        );
        let state = ledger.state();
        store(&context, &state).unwrap();
        assert_eq!(load(&context).unwrap().found(), Some(state));
        let mut reloaded = OperationLedger::new(load(&context).unwrap().found().unwrap());
        assert_eq!(
            reloaded.peek_generation("t1"),
            1,
            "the counter survives the process"
        );
        assert_eq!(
            reloaded.claim("t1", OperationKind::Start, "k-1", 1, 42),
            Claimed::Duplicate(reloaded.claims()[0].clone()),
            "and a claim reloads as still open"
        );
        let path = context.paths.task_operations_file();
        std::fs::write(&path, b"{\"not\":\"an object\"}").unwrap();
        let error = load(&context).expect_err("a wrong shape is not an empty ledger");
        assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_reader_without_the_lease_cannot_write_the_ledger() {
        let (root, _) = writer("ledger-ro");
        let context = crate::context::CoreContext::read_only(
            crate::paths::AppPaths::with_data_dir(&root, Some(&root.join("managed"))),
        );
        assert!(store(&context, &LedgerState::default()).is_err());
        std::fs::remove_dir_all(root).ok();
    }
}
