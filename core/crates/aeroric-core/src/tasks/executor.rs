//! The port every spawn goes through, and the only way a process gets started.
//!
//! Declaring it here rather than calling into `pty.rs` is what makes the state machine
//! testable before a runtime exists: Task P2-05 injects the legacy PTY/DSH/SSH/WSL
//! executor, Task P3-01 replaces it with the extracted one, and neither changes a rule in
//! [`crate::tasks::service`]. The requests are typed and carry a `generation`, because
//! "which run of this task produced that exit event" is the question a late event can only
//! be judged against.

use aeroric_protocol::task::{PermissionMode, ProtocolFamily, TaskStatus};

/// Where a running task's process lives. Mirrors `CancelTarget` in
/// `src/state/app/taskLaunch.ts`: the four channels have different cancel commands, and
/// picking one is a business decision, not a transport detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelTarget {
    Ssh,
    Wsl,
    Dsh,
    Local { project_path: String },
}

/// The three per-window facts a DSH launch needs and no other family asks for
/// (§12 of `docs/native/reviews/tauri-adapter.md`): the webui instance to open against,
/// how the prompt should be applied, and the viewer's time zone for rendering. They are
/// `Option` on the request because non-DSH requests never carry them, and the executor's
/// DSH branch refuses by name when they are missing rather than inventing defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DshWindowFacts {
    pub workspace_id: String,
    pub prompt_mode: String,
    pub client_time_zone: Option<String>,
}

/// What to run, with the choice of channel already made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchRequest {
    pub task_id: String,
    pub project_id: String,
    /// Which call of the caller's operation key produced this spawn (ADR-017). Core needs
    /// it to make a retry idempotent; the host needs it for a second reason — the output
    /// channel is a webview object that cannot travel inside a `LaunchRequest`, so the
    /// command that started this operation parks it in a host-side mailbox under this key,
    /// and the executor takes it back out here. Without the key the executor could not
    /// tell which of two concurrent runs of one task a channel belongs to.
    pub operation_key: String,
    /// Which run of this task this is. A result carrying an older generation than the
    /// task has now must not move its state.
    pub generation: u64,
    pub family: ProtocolFamily,
    pub agent: String,
    pub permission_mode: PermissionMode,
    /// The string the record actually carries. The enum above is what core *reasons* over;
    /// this is what the CLI flags are built from, and the two are not interchangeable: an
    /// unrecognised stored value collapses to `Ask` in the enum, while the legacy arg maps
    /// emit **no flag at all** for it. Reconstructing the flag list from the enum would
    /// therefore change the command line of a real user's task — the same two-facts-one-
    /// field mistake ADR-028 was written about.
    pub permission_mode_wire: String,
    pub prompt: String,
    /// `promptOverride` in the client: a handoff prompt replaces the original message
    /// rather than being appended to it.
    pub prompt_override: Option<String>,
    pub images: Vec<String>,
    pub texts: Vec<String>,
    /// Whether the prompt is typed into the terminal instead of passed as an argument.
    pub inject_prompt_into_terminal: bool,
    pub session_id: Option<String>,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
    pub dsh_agent_preset: Option<String>,
    pub created_at_ms: u64,
    /// DSH-only window facts; `None` for every other family.
    pub dsh_window: Option<DshWindowFacts>,
    pub working_directory: WorkingDirectory,
    pub terminal_size: Option<(u16, u16)>,
}

/// The working directory a task runs in. `Worktree` is the local-git case; a remote
/// project has no host-side path at all, which is why this is a sum and not an `Option`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkingDirectory {
    Local {
        path: String,
    },
    /// A remote project is identified by its *connection*, not just its path: two saved
    /// connections can both put a project at `/srv/app`, and spawning into the wrong one is
    /// not a cosmetic mistake. The host resolves this id to the connection record (auth,
    /// port, key) at spawn time — `ProjectTarget::Ssh` already carries it, and dropping it
    /// here would leave the host unable to start the process at all.
    Ssh {
        connection_id: String,
        remote_path: String,
    },
    Wsl {
        distribution: String,
        path: String,
    },
}

/// Resuming is a different request, not `LaunchRequest` with a flag: it carries no prompt
/// for dsh (reconnecting must not replay the original message), and no attachments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeRequest {
    pub task_id: String,
    pub project_id: String,
    /// See [`LaunchRequest::operation_key`]: the continuation is its own operation, and the
    /// window that asked for it is the one whose channel the resumed process must write to.
    pub operation_key: String,
    pub generation: u64,
    pub family: ProtocolFamily,
    pub agent: String,
    pub session_id: String,
    pub permission_mode: PermissionMode,
    /// See [`LaunchRequest::permission_mode_wire`].
    pub permission_mode_wire: String,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
    pub dsh_agent_preset: Option<String>,
    /// Always false by construction for dsh; carried for the other three families, whose
    /// CLI resumes with the original prompt available.
    pub prompt: Option<String>,
    pub working_directory: WorkingDirectory,
    pub terminal_size: Option<(u16, u16)>,
}

/// What a reset reported. The client folds this into `mergeResetTaskSession`, which is a
/// session-attribution decision, so the core needs the fields rather than a void.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetOutcome {
    pub claude_session_id: Option<String>,
    pub claude_session_path: Option<String>,
    pub codex_session_id: Option<String>,
    pub codex_session_path: Option<String>,
    pub dsh_session_id: Option<String>,
    pub dsh_session_path: Option<String>,
    pub omp_session_id: Option<String>,
    pub omp_session_path: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum ExecutorFailure {
    #[error("the process could not be started ({reason})")]
    Spawn { reason: String },
    #[error("this task cannot be continued here: {reason}")]
    Unsupported { reason: String },
    #[error("the worktree could not be prepared: {reason}")]
    Worktree { reason: String },
    #[error("the running task could not be stopped: {reason}")]
    Stop { reason: String },
}

/// Who told us about a status. ADR-034 decision 2 draws the line by **source**, not by word:
/// transcript and hook observations are what the late-observation guard exists for, while
/// "the host changed this on purpose" (a launch, a cancel, a manual completion, dsh's own state
/// machine) is emitted unconditionally today and stays that way.
///
/// Without this field the rule silently grows to cover the sites ADR-034 excludes — which is
/// exactly what happened first, and what made dsh's nine reports unremovable: dsh runs never
/// appear in core's ledger, so a liveness-only guard would drop all nine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationOrigin {
    /// A transcript watcher read a line (codex / claude).
    Transcript,
    /// An agent hook wrote an event file.
    Hook,
    /// The host itself moved the task: launch, resume, cancel, manual completion, dsh's
    /// session state machine. Never suppressed by the late-observation guard.
    Runtime,
}

impl ObservationOrigin {
    /// Is this the kind of report ADR-034's guard exists to catch?
    pub fn is_guarded(self) -> bool {
        matches!(
            self,
            ObservationOrigin::Transcript | ObservationOrigin::Hook
        )
    }
}

/// The side a task can be moved to by something outside the state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskEvent {
    /// A hook or an exit reported a status. `generation` is whose run reported it.
    Status {
        task_id: String,
        generation: u64,
        status: TaskStatus,
        attention_requested_at_ms: Option<u64>,
        failure_reason: Option<String>,
        /// ADR-034 decision 2: the guard applies to transcript and hook reports only.
        origin: ObservationOrigin,
    },
    /// The process ended. Only the current generation may settle a task.
    Exit {
        task_id: String,
        generation: u64,
        code: Option<i32>,
    },
    /// The terminal is up and can take the first input. One-shot per generation.
    TerminalReady { task_id: String, generation: u64 },
}

/// What a process exit means as a status, and the reason the process gave.
///
/// Shared by [`TaskService::observe`](super::service::TaskService::observe) and
/// [`TaskEvent::notification_when_unrecorded`] so the two can never disagree about what a
/// nonzero exit is called: a task that quietly turns itself red is indistinguishable from one
/// the user cancelled, which is why the text travels with the status.
pub fn settled_by_exit(code: Option<i32>) -> (TaskStatus, Option<String>) {
    let settled = if matches!(code, Some(0) | None) {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    let reason = match code {
        Some(code) if code != 0 => Some(format!("the process exited with code {code}")),
        _ => None,
    };
    (settled, reason)
}

impl TaskEvent {
    pub fn generation(&self) -> u64 {
        match self {
            TaskEvent::Status { generation, .. }
            | TaskEvent::Exit { generation, .. }
            | TaskEvent::TerminalReady { generation, .. } => *generation,
        }
    }
    pub fn task_id(&self) -> &str {
        match self {
            TaskEvent::Status { task_id, .. }
            | TaskEvent::Exit { task_id, .. }
            | TaskEvent::TerminalReady { task_id, .. } => task_id,
        }
    }

    /// ADR-033 §4's floor under ADR-035: an observation whose task this process cannot find in
    /// any project still announces itself.
    ///
    /// Every guard in the reducer exists to protect a record — a settled row, a stale
    /// generation, an overlapping run. With no record there is nothing to protect, and silence
    /// here would be precisely the "quietly stop emitting to make a count look good" outcome
    /// ADR-033 forbids. This is the *no-row* path, not a way around the guards: it runs only
    /// when routing failed, and it states its reason in the log line the adapter writes.
    pub fn notification_when_unrecorded(&self) -> Option<crate::tasks::events::TaskNotification> {
        use crate::tasks::events::TaskNotification;
        match self {
            TaskEvent::Status {
                task_id,
                status,
                failure_reason,
                ..
            } => Some(match failure_reason {
                Some(reason) => {
                    TaskNotification::status_with_reason(task_id, *status, reason.clone())
                }
                None => TaskNotification::status(task_id, *status),
            }),
            TaskEvent::Exit { task_id, code, .. } => {
                let (status, reason) = settled_by_exit(*code);
                Some(match reason {
                    Some(reason) => TaskNotification::status_with_reason(task_id, status, reason),
                    None => TaskNotification::status(task_id, status),
                })
            }
            // A terminal-ready edge moves nothing, recorded or not — it is not a status, and
            // inventing one would show a card that never changed.
            TaskEvent::TerminalReady { .. } => None,
        }
    }
}

/// Starting, stopping and finishing processes. Everything here is fire-and-acknowledge:
/// the outcome a caller acts on arrives as a [`TaskEvent`], never as a return value, so a
/// slow exit cannot block a state transition.
pub trait TaskExecutor: Send + Sync {
    fn launch(&self, request: &LaunchRequest) -> Result<(), ExecutorFailure>;
    fn resume(&self, request: &ResumeRequest) -> Result<(), ExecutorFailure>;
    fn cancel(&self, task_id: &str, target: &CancelTarget) -> Result<(), ExecutorFailure>;
    /// "The user said this is finished." The executor stops the process; the *status* is
    /// settled by the observer, not here — see `TaskService::complete`.
    fn complete(&self, task_id: &str, working_directory: &str) -> Result<(), ExecutorFailure>;
    fn reset(&self, task_id: &str) -> Result<ResetOutcome, ExecutorFailure>;
}

/// Where the executor's world gets reported back. One reducer, one owner.
pub trait TaskRuntimeObserver: Send + Sync {
    fn observe(&self, event: TaskEvent);

    /// Where this task's transcript now lives — ADR-023's second family (`task-session`).
    ///
    /// Deliberately a separate method instead of another `TaskEvent` variant: every guard in
    /// [`TaskService::observe`](super::service::TaskService::observe) is about *which run* may
    /// move a status, and a session path is not a status. No default body either — a silent
    /// default would be the "quietly stop emitting" outcome ADR-033 §4 forbids.
    ///
    /// What is *not* done here: writing the path into the record. That is
    /// [`apply_resolved_session`](super::session_owner::apply_resolved_session), and attaching
    /// it belongs to the same store cutover that lets status observations commit (ADR-035).
    fn report_session(&self, facts: crate::tasks::events::TaskSessionFacts);
}

/// ADR-036: who is still running is a question core **asks**, not a table it keeps.
///
/// ADR-034's guard drops a non-terminal observation when the task has no live run, and on the
/// shipping client most runs are not core's — the product path still launches through the
/// legacy commands, so core's operation ledger holds no claim for them. A mirror table would
/// fail in the dangerous direction: one missed `retire` leaves a run alive forever and the
/// suppression gate silently stops suppressing. The process handles live in the host's
/// registry, so the answer lives there too.
///
/// The implementation must not call back into core (P2-05 Action 4's recursion ban), and must
/// treat an unminted run the same as a minted one — "is any run of this id live" is the
/// question ADR-025 settled, not "is the newest one".
pub trait LiveRuns: Send + Sync {
    fn has_live_run(&self, task_id: &str) -> bool;
}

/// Nothing is running: a headless CLI, a test that drives no processes. Combined with core's
/// own claims, this leaves the guard exactly where ADR-034 put it for core-launched runs.
pub struct NoLiveRuns;

impl LiveRuns for NoLiveRuns {
    fn has_live_run(&self, _task_id: &str) -> bool {
        false
    }
}

/// What an injected failure looks like to a caller.
pub type Execution<T> = Result<T, ExecutorFailure>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_carries_the_generation_that_owns_it() {
        let events = [
            TaskEvent::Status {
                task_id: "t".into(),
                generation: 3,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
            TaskEvent::Exit {
                task_id: "t".into(),
                generation: 4,
                code: Some(0),
            },
            TaskEvent::TerminalReady {
                task_id: "t".into(),
                generation: 5,
            },
        ];
        assert_eq!(
            events.iter().map(TaskEvent::generation).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert!(events.iter().all(|event| event.task_id() == "t"));
    }
}
