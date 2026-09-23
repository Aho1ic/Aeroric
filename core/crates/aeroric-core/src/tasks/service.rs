//! `TaskService` — the one place a task's state and a process's life are decided together.
//!
//! Three ordering rules are the whole reason this is a service rather than a set of
//! helpers, and all three were enforced by whichever handler happened to run last:
//!
//! 1. **Persist, then spawn.** A task that is only in memory cannot be resumed after a
//!    crash, and a spawn that precedes its save can outlive the record that names it. A
//!    failed save therefore returns without ever reaching the executor.
//! 2. **Claim, then persist, then spawn.** [`OperationLedger`](super::operations::OperationLedger)
//!    keys on `(task, kind, generation)`, so a retry of the same attempt is refused before
//!    the disk is touched and one attempt spawns one process — across retries, windows and
//!    processes that share this directory.
//! 3. **Settle a claim only once the outcome is known.** A claim left open across a crash
//!    is what [`TaskService::reconcile`] turns into `interrupted`, which is the difference
//!    between "this task was cut off" and "let's start a second one".
//!
//! Notifications, which task is selected, terminal buffers and toasts are *not* here: the
//! acceptance test for this module is that start, resume and cancel all work with no
//! listener attached.

use std::sync::{Arc, Mutex};

use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use serde_json::Value;

use crate::clock::Clock;
use crate::context::CoreContext;
use crate::projects::mutations::is_addressable;
use crate::storage::atomic::StorageError;
use crate::storage::compat::Record;
use crate::storage::fingerprint::Cover;
use crate::storage::runtime;
use crate::storage::tasks as task_storage;
use crate::tasks::executor::{ExecutorFailure, TaskEvent, TaskExecutor};
use crate::tasks::launch_plan::{self, Completion};
use crate::tasks::operations::{Claim, Claimed, OperationKind, OperationLedger};
use crate::tasks::reducer;
use crate::tasks::session_owner::{self, AgentFamilies, ConfigSwitchStrategy, SessionOwner};

/// Files carried into a run. A remote project cannot take them: the client's submit path
/// refuses with `remoteProjectNoAttachments` (`src/state/app/taskLifecycle.ts:292-295`),
/// and the reason is a channel limit rather than a preference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attachments {
    pub images: Vec<String>,
    pub texts: Vec<String>,
}

/// What a caller asked to start. `immediate` is the client's "run now" switch: without it
/// the task is a `todo`, which is a plan rather than a request for a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTask {
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub agent: String,
    pub permission_mode: String,
    pub immediate: bool,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
    pub dsh_agent_preset: Option<String>,
    pub images: Vec<String>,
    pub texts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub records: Vec<Record>,
    /// The revision this change bought. `None` when nothing was written, which is also the
    /// answer to "did the revision advance?".
    pub revision: Option<String>,
    pub changed: bool,
    /// The cover the caller's *next* write in this session must present.
    pub fingerprint: Option<Value>,
    /// Which run number the host must launch, when the change was a switch or start. The
    /// spawn itself stays with the caller: the first-input gate means the terminal says when.
    pub generation: Option<u64>,
}

/// What the user asked a task to become.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSwitch {
    pub agent: String,
    pub permission_mode: String,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
}

/// What only the host can know before a switch is decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SwitchEvidence {
    /// `session_supports_native_resume` on the transcript.
    pub native_resume_supported: bool,
    pub session_id: Option<String>,
    /// The session id and path after `adopt_session_for_agent` moved the transcript.
    pub adopted_session_id: Option<String>,
    pub adopted_session_path: Option<String>,
    /// `formatSessionHandoff`, when the continuation is a handoff rather than a resume.
    pub handoff_prompt: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum TaskError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("task {task_id} is not in project {project_id}")]
    NotFound { project_id: String, task_id: String },
    #[error("project {0} has no readable location, so nothing can be run inside it")]
    Unaddressable(String),
    #[error("the task could not be saved, so no process was started: {0}")]
    PersistFailed(String),
    #[error("task id {0} is already in this project; two rows cannot share one id")]
    DuplicateId(String),
    #[error(
        "this attempt to {kind} task {task_id} is already in flight (claimed at {claimed_at_ms}ms)"
    )]
    AlreadyClaimed {
        task_id: String,
        kind: &'static str,
        claimed_at_ms: u64,
    },
    #[error("task {0} has no session to continue from; refusing to start a blank one")]
    NoSession(String),
    #[error("the process could not be started ({0}); the task is marked failed")]
    Launch(String),
    #[error("task {task_id} was cut off by the previous session and has not been acknowledged; resume is held until it is")]
    AwaitingAcknowledgement { task_id: String },
    #[error("task {0} has no worktree to resume into any more; continuing would replay a transcript whose tree is gone")]
    WorktreeDiscarded(String),
    #[error("task {0} has neither a transcript, terminal history, nor a prompt to continue from; switching would open an amnesiac session")]
    NoContinuationContext(String),
    #[error("a {kind} project cannot carry {what}; the run was not started")]
    UnsupportedOnRemote {
        kind: &'static str,
        what: &'static str,
    },
    #[error("an event for run {seen} of task {task_id} arrived before this ledger had reached that run (at {current})")]
    FutureGeneration {
        task_id: String,
        seen: u64,
        current: u64,
    },
}

/// One event accepted or refused, so a host can log *why* a status did not move without
/// guessing from the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventOutcome {
    Applied {
        status: TaskStatus,
        changed: bool,
    },
    /// Dropped: an older generation, or a transition the machine ignores.
    Dropped(&'static str),
}

pub struct TaskService {
    context: CoreContext,
    families: Arc<dyn AgentFamilies>,
    executor: Arc<dyn TaskExecutor>,
    ledger: Mutex<OperationLedger>,
}

impl TaskService {
    pub fn new(
        context: CoreContext,
        families: Arc<dyn AgentFamilies>,
        executor: Arc<dyn TaskExecutor>,
    ) -> Result<Self, StorageError> {
        let ledger = match OperationLedger::load_state(&context)? {
            Some(state) => OperationLedger::new(state),
            None => OperationLedger::empty(),
        };
        // The host's runtime observer is the *caller* of `observe`: an exit, a hook or a
        // terminal-ready edge arrives through it and lands here, so exactly one place
        // writes a task's status and it is never reached from two directions.
        Ok(TaskService {
            context,
            families,
            executor,
            ledger: Mutex::new(ledger),
        })
    }

    pub fn context(&self) -> &CoreContext {
        &self.context
    }

    pub fn clock(&self) -> &dyn Clock {
        self.context.clock.as_ref()
    }

    fn ledger<T>(&self, mutate: impl FnOnce(&mut OperationLedger) -> T) -> T {
        let mut guard = self
            .ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mutate(&mut guard)
    }

    pub fn generation(&self, task_id: &str) -> u64 {
        self.ledger(|ledger| ledger.peek_generation(task_id))
    }

    pub fn open_claims(&self, task_id: &str) -> Vec<Claim> {
        self.ledger(|ledger| ledger.open_for(task_id).into_iter().cloned().collect())
    }

    /// A snapshot plus the bytes it came from. Every mutation in this service starts here,
    /// because a write needs the cover the read established.
    pub fn load(&self, project_id: &str) -> Result<task_storage::StampedTasks, TaskError> {
        Ok(task_storage::load_stamped(&self.context, project_id)?)
    }

    /// The only write path. Persist the new list, publish its cover with the revision bump,
    /// then mirror the ledger — in that order, so a crash between the data and the metadata
    /// leaves the ledger claiming less than the disk holds, never more.
    ///
    /// The cover is always [`Cover::LastCoreWrite`], and that choice is the point: a write
    /// may only replace the bytes this core last published. Re-baselining on a fresh read
    /// would make the check unsatisfiable — every mutation loads first, so "the last thing
    /// I saw" is by construction what is on disk — and the guard would exist only in name.
    /// With the durable table as the baseline, a save the shipping client made in between
    /// is a refusal instead of an overwrite, and the caller reloads.
    fn commit(&self, project_id: &str, records: &[Record]) -> Result<Applied, TaskError> {
        let records = self.stamp_generation_copies(records);
        let (fingerprint, key) =
            task_storage::save_covering(&self.context, project_id, &records, Cover::LastCoreWrite)?;
        let bumped =
            runtime::bump_covering(&self.context, &[(key.clone(), Some(fingerprint.clone()))])?;
        self.ledger(|ledger| ledger.persist(&self.context))?;
        Ok(Applied {
            records,
            revision: Some(bumped.metadata.revision),
            changed: true,
            generation: None,
            fingerprint: Some(
                serde_json::to_value(&fingerprint).expect("a fingerprint is a value"),
            ),
        })
    }

    fn unchanged(
        &self,
        records: Vec<Record>,
        fingerprint: Option<crate::storage::fingerprint::Fingerprint>,
    ) -> Applied {
        Applied {
            records,
            revision: None,
            changed: false,
            generation: None,
            fingerprint: fingerprint
                .map(|value| serde_json::to_value(value).expect("a fingerprint is a value")),
        }
    }

    /// Mirror each row's generation counter into the record itself (ADR-016).
    ///
    /// The ledger stays the only authority; this copy exists so a project that is exported,
    /// synced or opened by a core that has never seen it still shows which run it was on.
    /// The copy is rewritten to the ledger's number on every write, including down to zero:
    /// a value that survives an import while the ledger did not would look authoritative and
    /// not be, which is worse than the history it preserves.
    fn stamp_generation_copies(&self, records: &[Record]) -> Vec<Record> {
        let mut next = records.to_vec();
        for row in &mut next {
            let Some(id) = row.id() else { continue };
            let known = self.generation(id);
            if row.get("generation") != Some(&Value::String(known.to_string())) {
                row.0
                    .insert("generation".to_string(), Value::String(known.to_string()));
            }
        }
        next
    }

    fn find<'a>(records: &'a [Record], task_id: &str) -> Option<&'a Record> {
        records.iter().find(|record| record.id() == Some(task_id))
    }

    /// The `todo`/`pending` choice plus the record shape `handleSubmitTask` built.
    pub fn create(&self, project: &Record, spec: NewTask) -> Result<(Record, Applied), TaskError> {
        let project_id = project
            .id()
            .ok_or(StorageError::InvalidId(crate::paths::PathError::EmptyId {
                label: "project",
            }))?
            .to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        let snapshot = self.load(&project_id)?;
        if Self::find(&snapshot.records, &spec.id).is_some() {
            return Err(TaskError::DuplicateId(spec.id));
        }
        let now = self.clock().now_ms();
        let family = self.families.family(&spec.agent);
        let mut record = Record(Default::default());
        record.0.insert("id".into(), Value::String(spec.id.clone()));
        record
            .0
            .insert("projectId".into(), Value::String(project_id.clone()));
        record
            .0
            .insert("prompt".into(), Value::String(spec.prompt.clone()));
        // An empty prompt gets the agent's own label as its name; a blank name is not a
        // name and would leave a row nothing can clear.
        if !spec.prompt.trim().is_empty() {
            record.0.remove("name");
        } else {
            record
                .0
                .insert("name".into(), Value::String(spec.agent.clone()));
        }
        record
            .0
            .insert("agent".into(), Value::String(spec.agent.clone()));
        if let Some(model) = spec.selected_model {
            record
                .0
                .insert("selectedModel".into(), Value::String(model));
        }
        if let Some(effort) = spec.reasoning_effort {
            record
                .0
                .insert("reasoningEffort".into(), Value::String(effort));
        }
        if let Some(speed) = spec.speed {
            record.0.insert("speed".into(), Value::String(speed));
        }
        if family == ProtocolFamily::Dsh {
            record.0.insert(
                "dshAgentPreset".into(),
                Value::String(spec.dsh_agent_preset.unwrap_or_else(|| "standard".into())),
            );
        }
        record.0.insert(
            "permissionMode".into(),
            Value::String(spec.permission_mode.clone()),
        );
        record.0.insert(
            "status".into(),
            Value::String(
                serde_json::to_value(if spec.immediate {
                    TaskStatus::Pending
                } else {
                    TaskStatus::Todo
                })
                .expect("a status is a string")
                .as_str()
                .expect("a status is a string")
                .to_string(),
            ),
        );
        record
            .0
            .insert("createdAt".into(), Value::Number(now.into()));

        let mut records = snapshot.records.clone();
        records.insert(0, record.clone());
        let applied = self.commit(&project_id, &records)?;
        Ok((record, applied))
    }

    /// Start a `todo`, or a freshly created immediate task. `persist_before_launch` is the
    /// remote-launch contract: the save must be durable *and* still be the newest word
    /// before a process exists.
    pub fn start(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        prompt_override: Option<String>,
        inject_prompt_into_terminal: bool,
        attachments: Attachments,
    ) -> Result<Applied, TaskError> {
        self.begin(
            project,
            task_id,
            operation_key,
            OperationKind::Start,
            persist_before_launch,
            &attachments,
            |service, record, project, generation| {
                let request = launch_plan::launch_request(
                    record,
                    project,
                    service.families.as_ref(),
                    generation,
                    attachments.images.clone(),
                    attachments.texts.clone(),
                    inject_prompt_into_terminal,
                    prompt_override,
                )
                .ok_or_else(|| {
                    TaskError::Unaddressable(project.id().unwrap_or_default().to_string())
                })?;
                service.executor.launch(&request).map_err(|failure| {
                    service.abandon_launch(record, failure, OperationKind::Start, operation_key)
                })
            },
        )
    }

    /// Continue a settled task. Resolving *which* session to continue is a host
    /// responsibility (it reads the filesystem and the agent homes); deciding that an
    /// unresolvable session must not silently become a new conversation is this service's.
    pub fn resume(
        &self,
        project: &Record,
        task_id: &str,
        session_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
    ) -> Result<Applied, TaskError> {
        let blocked = self
            .load(project.id().unwrap_or_default())?
            .records
            .iter()
            .find(|row| row.id() == Some(task_id))
            .and_then(|row| row.get("worktreeDiscarded").and_then(Value::as_bool));
        if blocked == Some(true) {
            // The rule is `RunningView.tsx:724,741` today: the button simply is not shown
            // once the worktree has been merged or discarded, because the transcript it
            // would resume into belongs to a tree that no longer exists. A hidden button is
            // not a rule — any other caller could still ask — so the core refuses it too.
            return Err(TaskError::WorktreeDiscarded(task_id.to_string()));
        }
        if let Some(pending) = self.awaiting_acknowledgement(task_id) {
            // ADR-018: a run the last process cut off is not continuing material until
            // somebody has seen it. `start` clears this on purpose — beginning a new run is
            // itself an acknowledgement of the last one.
            let _ = pending;
            return Err(TaskError::AwaitingAcknowledgement {
                task_id: task_id.to_string(),
            });
        }
        if session_id.trim().is_empty() {
            // `handleResumeTask` toasts `running.resumeUnavailable` and returns without
            // touching anything. A blank id is not "resume with no session" — the CLI would
            // open a fresh conversation with the user's old prompt in it, and the user would
            // read the result as their history coming back.
            return Err(TaskError::NoSession(task_id.to_string()));
        }
        self.begin(
            project,
            task_id,
            operation_key,
            OperationKind::Resume,
            persist_before_launch,
            &Attachments::default(),
            |service, record, project, generation| {
                let owner = session_owner::resolve_session_owner(record, service.families.as_ref());
                let with_session =
                    session_owner::apply_resolved_session(record, &owner, Some(session_id), None);
                // The record in the list has to be the one carrying the session, so the write
                // and the request agree about which transcript is being continued.
                let request = launch_plan::resume_request(
                    &with_session,
                    project,
                    service.families.as_ref(),
                    generation,
                    session_id,
                )
                .ok_or_else(|| {
                    TaskError::Unaddressable(project.id().unwrap_or_default().to_string())
                })?;
                service.executor.resume(&request).map_err(|failure| {
                    service.abandon_launch(
                        &with_session,
                        failure,
                        OperationKind::Resume,
                        operation_key,
                    )
                })
            },
        )
    }

    /// The remote channel limits, enforced rather than hidden behind a disabled button: a
    /// worktree is a local git object and attachments are typed into the local terminal
    /// (`src/state/app/taskLifecycle.ts:283-296`).
    fn check_target_shape(
        &self,
        project: &Record,
        task_id: &str,
        attachments: &Attachments,
    ) -> Result<(), TaskError> {
        let location = crate::projects::mutations::resolve_location(project);
        if location.is_local() {
            return Ok(());
        }
        let kind = if matches!(location, aeroric_protocol::ProjectTarget::Ssh { .. }) {
            "ssh"
        } else {
            "wsl"
        };
        if !attachments.images.is_empty() || !attachments.texts.is_empty() {
            return Err(TaskError::UnsupportedOnRemote {
                kind,
                what: "attachments",
            });
        }
        if let Some(row) = Self::find(
            &self.load(project.id().unwrap_or_default())?.records,
            task_id,
        ) {
            if row
                .get("worktreePath")
                .and_then(Value::as_str)
                .is_some_and(|path| !path.is_empty())
            {
                return Err(TaskError::UnsupportedOnRemote {
                    kind,
                    what: "a worktree",
                });
            }
        }
        Ok(())
    }

    /// The shared shape of start and resume: claim → persist → spawn, with the claim
    /// released on every path that never reached a process.
    fn begin(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        kind: OperationKind,
        persist_before_launch: bool,
        attachments: &Attachments,
        spawn: impl FnOnce(&Self, &Record, &Record, u64) -> Result<(), TaskError>,
    ) -> Result<Applied, TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id)
            .ok_or_else(|| TaskError::NotFound {
                project_id: project_id.clone(),
                task_id: task_id.to_string(),
            })?
            .clone();
        self.check_target_shape(project, task_id, attachments)?;

        // One in-flight attempt per task and kind: the check comes first so a retry neither
        // reserves a generation nor reaches the executor. Advancing the counter before
        // asking would hand every retry a brand-new key, and "spawn once" would be a rule
        // with no mechanism behind it.
        let (generation, claim) = self.ledger(|ledger| {
            match ledger.open_claim(task_id, kind, operation_key) {
                // The same operation key arriving twice is one attempt, whoever sent it. A
                // different key on the same task is a different attempt and is allowed
                // through — that is what makes two windows able to run one task (ADR-017).
                Some(existing) => (
                    ledger.peek_generation(task_id),
                    Claimed::Duplicate(existing.clone()),
                ),
                None => {
                    let generation = ledger.next_generation(task_id);
                    let claim = ledger.claim(
                        task_id,
                        kind,
                        operation_key,
                        generation,
                        self.clock().now_ms(),
                    );
                    (generation, claim)
                }
            }
        });
        match claim {
            Claimed::New(claim) => claim,
            Claimed::Duplicate(claim) => {
                return Err(TaskError::AlreadyClaimed {
                    task_id: task_id.to_string(),
                    kind: kind.as_str(),
                    claimed_at_ms: claim.claimed_at_ms,
                });
            }
        };

        let prepared = prepare_for_launch(&record, kind);
        let mut records = snapshot.records.clone();
        let position = records
            .iter()
            .position(|row| row.id() == Some(task_id))
            .expect("found above");
        records[position] = prepared.clone();

        let applied = match self.commit(&project_id, &records) {
            Ok(applied) => applied,
            Err(error) => {
                // Nothing reached the disk, so the claim describes an attempt that will
                // never be made.
                self.ledger(|ledger| {
                    ledger.settle(task_id, kind, operation_key);
                    ledger.persist(&self.context)
                })
                .ok();
                return Err(match error {
                    TaskError::Storage(inner) if inner.is_stale_write() => {
                        TaskError::PersistFailed(inner.to_string())
                    }
                    other => other,
                });
            }
        };

        if kind == OperationKind::Start && self.acknowledge(task_id).unwrap_or(false) {
            // Beginning a fresh run is itself the acknowledgement of the cut-off one: the
            // user chose a new start over "continue where it stopped", so holding the flag
            // any longer would only block the next resume for no information gained.
        }
        if let Err(error) = spawn(self, &prepared, project, generation) {
            // The record is already saved; the process never came up. The claim must be
            // released either way, or a genuine later attempt is locked out forever.
            self.settle(task_id, kind, operation_key);
            return Err(error);
        }
        if !persist_before_launch {
            // The host still has to tell us the terminal is up before the first input goes
            // in; until then the claim stays open, which is what makes a retried request a
            // duplicate rather than a second process.
            self.settle(task_id, kind, operation_key);
        }
        Ok(applied)
    }

    /// Release whatever attempt of this task reached the given run number. An event knows
    /// the generation, not the caller's idempotency key.
    fn settle_by_generation(&self, task_id: &str, generation: u64) {
        self.ledger(|ledger| {
            for kind in [OperationKind::Start, OperationKind::Resume] {
                ledger.settle_by_generation(task_id, kind, generation);
            }
            let _ = ledger.persist(&self.context);
        });
    }

    fn settle(&self, task_id: &str, kind: OperationKind, operation_key: &str) {
        self.ledger(|ledger| {
            ledger.settle(task_id, kind, operation_key);
            let _ = ledger.persist(&self.context);
        });
    }

    /// A spawn that failed before any process existed: undo the status this attempt wrote,
    /// and report the reason. `abandon_launch` mirrors `reportLaunchFailure` →
    /// `onFailed` → `updateTaskStatus(failed, reason)`, which is what the client does.
    fn abandon_launch(
        &self,
        record: &Record,
        failure: ExecutorFailure,
        kind: OperationKind,
        operation_key: &str,
    ) -> TaskError {
        let project_id = record
            .str_field("projectId")
            .unwrap_or_default()
            .to_string();
        let task_id = record.id().unwrap_or_default().to_string();
        let reason = failure.to_string();
        if let Ok(snapshot) = self.load(&project_id) {
            if let Some(current) = Self::find(&snapshot.records, &task_id) {
                let rolled_back =
                    reducer::rollback_mutation(current, record, &prepare_for_launch(record, kind));
                let transition = reducer::apply_status_transition(
                    &[rolled_back],
                    &task_id,
                    TaskStatus::Failed,
                    None,
                    Some(&reason),
                    self.clock().now_ms(),
                );
                // Best effort, and deliberately not allowed to mask the launch error: the
                // caller still has to be told the process did not come up.
                let _ = self.commit(&project_id, &transition.records);
            }
        }
        self.settle(&task_id, kind, operation_key);
        TaskError::Launch(reason)
    }

    /// `handleSwitchTaskConfig` in `src/state/app/taskSwitch.ts` — a manual configuration
    /// change, as the ordering rule it is.
    ///
    /// What the host brings is what only the host can know: the reset snapshot the running
    /// process reported, whether the transcript supports a native resume, and the handoff
    /// text it assembled. What this decides is the order and the failure path:
    /// reset → attribute the session → pick the continuation mechanism → **refuse if there is
    /// nothing to continue from** → persist → only then let the run be started. A persist that
    /// fails rolls back exactly the fields this switch moved, keeping whatever a late hook
    /// wrote in the meantime, and never reaches a spawn.
    pub fn switch_config(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        values: &ConfigSwitch,
        observed: &SwitchEvidence,
    ) -> Result<(ConfigSwitchStrategy, Applied), TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        let snapshot = self.load(&project_id)?;
        let previous = Self::find(&snapshot.records, task_id)
            .ok_or_else(|| TaskError::NotFound {
                project_id: project_id.clone(),
                task_id: task_id.to_string(),
            })?
            .clone();

        // A switch ends in a process, so it takes an attempt out before it touches anything:
        // retried with the same key it is one attempt, and the second caller learns that
        // before a reset kills the run the first one is waiting on.
        let generation = match self.ledger(|ledger| {
            match ledger.open_claim(task_id, OperationKind::Start, operation_key) {
                Some(existing) => Claimed::Duplicate(existing.clone()),
                None => {
                    let generation = ledger.next_generation(task_id);
                    ledger.claim(
                        task_id,
                        OperationKind::Start,
                        operation_key,
                        generation,
                        self.clock().now_ms(),
                    )
                }
            }
        }) {
            Claimed::New(claim) => claim.generation,
            Claimed::Duplicate(existing) => {
                return Err(TaskError::AlreadyClaimed {
                    task_id: task_id.to_string(),
                    kind: existing.kind.as_str(),
                    claimed_at_ms: existing.claimed_at_ms,
                });
            }
        };
        // Every path below that never reaches a process hands the run number back: a skipped
        // number would make an event for the *next* run look like one for a run that already
        // happened somewhere.
        let release = |service: &Self, _reason: &str| {
            service.ledger(|ledger| {
                ledger.settle(task_id, OperationKind::Start, operation_key);
                ledger.release_generation(task_id, generation);
                let _ = ledger.persist(&service.context);
            });
        };

        // The reset is the first side effect, and it happens *before* any write: a switch
        // that cannot stop the current process changes nothing, exactly as the client's
        // early `return false` does.
        let reset = self.executor.reset(task_id).map_err(|failure| {
            release(self, "reset failed");
            TaskError::Launch(failure.to_string())
        })?;
        let attributed = launch_plan::merge_reset_session(&previous, &reset);
        let owner = session_owner::resolve_session_owner(&attributed, self.families.as_ref());

        let strategy = session_owner::resolve_config_switch_strategy(
            &attributed,
            &values.agent,
            observed.native_resume_supported,
            self.families.as_ref(),
        );
        let resuming = strategy == ConfigSwitchStrategy::Resume
            || (strategy == ConfigSwitchStrategy::Adopt && observed.adopted_session_id.is_some());
        let session_id = match strategy {
            ConfigSwitchStrategy::Resume => observed.session_id.as_deref(),
            ConfigSwitchStrategy::Adopt => observed.adopted_session_id.as_deref(),
            ConfigSwitchStrategy::Handoff => None,
        };
        if !resuming
            && observed
                .handoff_prompt
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            let has_prompt = attributed
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty();
            if has_prompt {
                release(self, "nothing to continue from");
                // `switchConfigNoContext`: continuing without a transcript, terminal history
                // or a prompt would start an amnesiac session and present it as the same task.
                return Err(TaskError::NoContinuationContext(task_id.to_string()));
            }
        }

        let applied_record = {
            let mut committed = session_owner::apply_resolved_session(
                &attributed,
                &owner,
                session_id,
                observed.adopted_session_path.as_deref(),
            );
            committed
                .0
                .insert("agent".to_string(), Value::String(values.agent.clone()));
            for (key, value) in [
                ("selectedModel", &values.selected_model),
                ("reasoningEffort", &values.reasoning_effort),
                ("speed", &values.speed),
                ("permissionMode", &Some(values.permission_mode.clone())),
            ] {
                match value {
                    Some(text) => {
                        committed
                            .0
                            .insert(key.to_string(), Value::String(text.clone()));
                    }
                    None => {
                        committed.0.remove(key);
                    }
                }
            }
            committed.0.insert(
                "status".to_string(),
                Value::String(
                    serde_json::to_value(TaskStatus::Pending)
                        .expect("a status is a string")
                        .as_str()
                        .expect("a status is a string")
                        .to_string(),
                ),
            );
            committed.0.remove("attentionRequestedAt");
            committed.0.remove("failureReason");
            committed
        };

        let mut records = snapshot.records.clone();
        let position = records
            .iter()
            .position(|row| row.id() == Some(task_id))
            .expect("found above");
        records[position] = applied_record.clone();

        // Core never writes optimistically, so a failed persist has nothing on disk to
        // undo: the record set was never replaced, and the bytes that beat us to it stay
        // exactly as they are. The field-level rollback that the client needs
        // (`reducer::rollback_mutation`) is its own optimistic list, and the parity table
        // proves that rule ported; here the correct action is to hand the refusal back.
        let committed = match self.commit(&project_id, &records) {
            Ok(committed) => committed,
            Err(error) => {
                release(self, "the switch could not be saved");
                return Err(match error {
                    TaskError::Storage(ref inner) if inner.is_stale_write() => {
                        TaskError::PersistFailed(inner.to_string())
                    }
                    other => other,
                });
            }
        };
        if strategy == ConfigSwitchStrategy::Handoff {
            // A handoff starts a *new* run carrying the previous conversation as text, so it
            // acknowledges any cut-off run the same way `start` does.
            let _ = self.acknowledge(task_id);
        }
        let _ = self.ledger(|ledger| ledger.persist(&self.context));
        Ok((
            strategy,
            Applied {
                generation: Some(generation),
                ..committed
            },
        ))
    }

    /// Where a hook/exit event lands. One reducer, one owner: this is the only way an
    /// outside observation moves a task, and the generation guard is what keeps a late exit
    /// from settling a task that has already been resumed.
    pub fn observe(
        &self,
        project_id: &str,
        event: TaskEvent,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        let snapshot = self.load(project_id)?;
        let current = self.generation(event.task_id());
        let seen = event.generation();
        if seen < current && !self.is_run_live(event.task_id(), seen) {
            // "Older than the newest number" is not enough to call an event stale, because
            // two runs of one task may be live at once (ADR-017): run 1's exit is real even
            // after run 2 started. What *is* stale is a run that already reported — no claim
            // in flight — or one that never had a process behind it. Applying such an event
            // would knock a live task off `running` on the strength of a world that already
            // ended.
            return Ok((
                EventOutcome::Dropped("stale generation"),
                self.unchanged(snapshot.records.clone(), snapshot.fingerprint.clone()),
            ));
        }
        if seen > current {
            // An event for a run this process has never claimed means the host is ahead of
            // the ledger — which is a wiring bug, not something to guess past.
            return Err(TaskError::FutureGeneration {
                task_id: event.task_id().to_string(),
                seen,
                current,
            });
        }
        let (status, attention, reason) = match &event {
            TaskEvent::Status {
                status,
                attention_requested_at_ms,
                failure_reason,
                ..
            } => (*status, *attention_requested_at_ms, failure_reason.clone()),
            TaskEvent::Exit { code, .. } => {
                // A zero exit settles as done; anything else is a failure *with the reason
                // the process gave*, because a task that quietly turned itself red is
                // indistinguishable from one the user cancelled.
                let settled = if matches!(code, Some(0) | None) {
                    TaskStatus::Done
                } else {
                    TaskStatus::Failed
                };
                let reason = match code {
                    Some(code) if *code != 0 => {
                        Some(format!("the process exited with code {code}"))
                    }
                    _ => None,
                };
                (settled, None, reason)
            }
            TaskEvent::TerminalReady { .. } => {
                // First input can go in: the attempt it was waiting for is over, and from
                // here on the status is whatever the run itself reports.
                self.settle_by_generation(event.task_id(), seen);
                return Ok((
                    EventOutcome::Dropped("terminal ready moves no status"),
                    self.unchanged(snapshot.records.clone(), snapshot.fingerprint.clone()),
                ));
            }
        };
        // One task row carries one status, so with overlapping runs the honest answer to
        // "is the task finished?" is "no, not while another run is live": this run is done
        // (its claim is released below), and the task itself moves back to `running`.
        let reported_terminal = status.is_terminal();
        let overlapping = reported_terminal
            && !self
                .live_other_generations(event.task_id(), seen)
                .is_empty();
        let status = if overlapping {
            TaskStatus::Running
        } else {
            status
        };
        let transition = reducer::apply_status_transition(
            &snapshot.records,
            event.task_id(),
            status,
            attention,
            reason.as_deref(),
            self.clock().now_ms(),
        );
        if reported_terminal {
            // Released here rather than after the write: folding a terminal event into
            // `running` because another run is live usually leaves the row unchanged, and an
            // unchanged row must not keep a finished run's claim open — that would block the
            // same operation key from ever being retried.
            self.settle_by_generation(event.task_id(), seen);
        }
        if !transition.changed {
            return Ok((
                EventOutcome::Applied {
                    status,
                    changed: false,
                },
                self.unchanged(snapshot.records, snapshot.fingerprint),
            ));
        }
        let applied = self.commit(project_id, &transition.records)?;
        Ok((
            EventOutcome::Applied {
                status,
                changed: true,
            },
            applied,
        ))
    }

    /// The manual "this is finished". A live task's process is stopped and the status is
    /// left for the exit event to settle; a task with no process is moved right now.
    pub fn complete(
        &self,
        project: &Record,
        task_id: &str,
    ) -> Result<(Completion, Option<Applied>), TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.clone(),
            task_id: task_id.to_string(),
        })?;
        let kind = launch_plan::completion_for(record, self.families.as_ref());
        let working_directory = launch_plan::process_path(record, project);
        match kind {
            Completion::Dsh | Completion::StopProcess => {
                self.executor
                    .complete(task_id, &working_directory)
                    .map_err(|failure| TaskError::Launch(failure.to_string()))?;
                Ok((kind, None))
            }
            Completion::Nothing => {
                let transition = reducer::apply_status_transition(
                    &snapshot.records,
                    task_id,
                    TaskStatus::Done,
                    None,
                    None,
                    self.clock().now_ms(),
                );
                let applied = self.commit(&project_id, &transition.records)?;
                Ok((kind, Some(applied)))
            }
        }
    }

    /// Stop the process. The status is the exit event's to write, not this call's — the
    /// difference between "we asked" and "it happened".
    pub fn cancel(&self, project: &Record, task_id: &str) -> Result<CancelTargetInfo, TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.clone(),
            task_id: task_id.to_string(),
        })?;
        let target = launch_plan::cancel_target(record, project, self.families.as_ref());
        self.executor
            .cancel(task_id, &target)
            .map_err(|failure| TaskError::Launch(failure.to_string()))?;
        // Whatever was in flight on this task is now moot: a claim that outlives a cancel
        // would lock the next legitimate start out.
        self.ledger(|ledger| {
            ledger.settle_task(task_id);
            let _ = ledger.persist(&self.context);
        });
        Ok(CancelTargetInfo {
            target: format!("{target:?}"),
        })
    }

    /// `deleteTasks`. Starred rows are skipped, an unresolvable project is refused, and the
    /// transcript on disk is left alone: removing a row from the list is not permission to
    /// delete the work it describes.
    pub fn delete(&self, project_id: &str, task_ids: &[String]) -> Result<Deleted, TaskError> {
        let snapshot = self.load(project_id)?;
        let starred = |id: &str| {
            Self::find(&snapshot.records, id)
                .map(|record| {
                    record
                        .get("starred")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        };
        let requested: Vec<String> = task_ids.iter().filter(|id| !starred(id)).cloned().collect();
        // Reported even when the deletion went through: the client counts these to keep its
        // "N tasks deleted" message honest, and a silent skip reads like a failed delete.
        let skipped_starred: Vec<String> =
            task_ids.iter().filter(|id| starred(id)).cloned().collect();
        if requested.is_empty() {
            return Ok(Deleted {
                removed: Vec::new(),
                skipped_starred,
                applied: self.unchanged(snapshot.records.clone(), snapshot.fingerprint),
            });
        }
        let blocked: Vec<String> = requested
            .iter()
            .filter(|id| {
                Self::find(&snapshot.records, id)
                    .map(|record| {
                        crate::tasks::reducer::status_of(record)
                            .map(|status| status.is_live())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !blocked.is_empty() {
            return Err(TaskError::AlreadyClaimed {
                task_id: blocked.join(", "),
                kind: "running",
                claimed_at_ms: 0,
            });
        }
        let records: Vec<Record> = snapshot
            .records
            .iter()
            .filter(|record| !requested.iter().any(|id| record.id() == Some(id.as_str())))
            .cloned()
            .collect();
        for id in &requested {
            // A deleted task has nothing left to acknowledge.
            self.ledger(|ledger| {
                ledger.settle_task(id);
                ledger.acknowledge(id);
            });
        }
        let applied = self.commit(project_id, &records)?;
        Ok(Deleted {
            removed: requested,
            skipped_starred,
            applied,
        })
    }

    pub fn mutate(
        &self,
        project_id: &str,
        change: impl Fn(&[Record], u64) -> (Vec<Record>, bool),
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        let (records, changed) = change(&snapshot.records, self.clock().now_ms());
        if !changed || records == snapshot.records {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.commit(project_id, &records)
    }

    /// What a fresh process owes the user before it accepts any request: every start or
    /// resume the last process left unclaimed is now `interrupted`, and nothing is respawned
    /// on anybody's behalf.
    pub fn reconcile(&self, project_id: &str) -> Result<(Vec<String>, Option<Applied>), TaskError> {
        let snapshot = self.load(project_id)?;
        let claims = self.open_claims_all();
        let (records, cut_off) =
            crate::tasks::operations::reconcile(&claims, &snapshot.records, self.clock().now_ms());
        if cut_off.is_empty() {
            return Ok((
                Vec::new(),
                Some(self.unchanged(snapshot.records, snapshot.fingerprint)),
            ));
        }
        let applied = self.commit(project_id, &records)?;
        let reconciled: Vec<String> = cut_off.iter().map(|item| item.task_id.clone()).collect();
        self.ledger(|ledger| {
            for item in &cut_off {
                // The attempt is over; what remains is the user's turn to see it (ADR-018).
                ledger.settle_task(&item.task_id);
                ledger.await_acknowledgement(item.clone());
            }
            let _ = ledger.persist(&self.context);
        });
        Ok((reconciled, Some(applied)))
    }

    /// Which other runs of this task are still in flight. A terminal event from one of them
    /// does not make the task terminal (ADR-017).
    pub fn live_other_generations(&self, task_id: &str, except: u64) -> Vec<u64> {
        self.ledger(|ledger| ledger.live_other_than(task_id, except))
    }

    /// Does this run still have an attempt in flight?
    pub fn is_run_live(&self, task_id: &str, generation: u64) -> bool {
        self.ledger(|ledger| ledger.is_live(task_id, generation))
    }

    /// A run reconciliation cut off, waiting to be acknowledged.
    pub fn awaiting_acknowledgement(
        &self,
        task_id: &str,
    ) -> Option<crate::tasks::operations::Acknowledgement> {
        self.ledger(|ledger| ledger.awaiting_acknowledgement(task_id))
    }

    /// "The user has seen this." Returns whether anything was pending.
    pub fn acknowledge(&self, task_id: &str) -> Result<bool, StorageError> {
        let cleared = self.ledger(|ledger| {
            let cleared = ledger.acknowledge(task_id);
            if cleared {
                let _ = ledger.persist(&self.context);
            }
            cleared
        });
        Ok(cleared)
    }

    fn open_claims_all(&self) -> Vec<Claim> {
        self.ledger(|ledger| ledger.claims().to_vec())
    }

    /// The session family a host will have to run against, exposed so the remote-launch
    /// precondition ("this project is on a channel that cannot carry attachments") can be
    /// checked without duplicating the rule.
    pub fn session_owner_of(&self, record: &Record) -> SessionOwner {
        session_owner::resolve_session_owner(record, self.families.as_ref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelTargetInfo {
    pub target: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Deleted {
    pub removed: Vec<String>,
    pub skipped_starred: Vec<String>,
    pub applied: Applied,
}

/// What `start`/`resume` write before a process exists.
fn prepare_for_launch(record: &Record, kind: OperationKind) -> Record {
    let mut prepared = record.clone();
    prepared.0.insert(
        "status".into(),
        Value::String(
            serde_json::to_value(TaskStatus::Pending)
                .expect("a status is a string")
                .as_str()
                .expect("a status is a string")
                .to_string(),
        ),
    );
    prepared.0.remove("attentionRequestedAt");
    if kind == OperationKind::Resume {
        // `handleResumeTask` clears the reason a task stopped for: continuing is a claim
        // that the failure is no longer the last word about it.
        prepared.0.remove("failureReason");
    }
    prepared
}
