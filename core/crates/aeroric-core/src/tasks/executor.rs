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

/// What to run, with the choice of channel already made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchRequest {
    pub task_id: String,
    pub project_id: String,
    /// Which run of this task this is. A result carrying an older generation than the
    /// task has now must not move its state.
    pub generation: u64,
    pub family: ProtocolFamily,
    pub agent: String,
    pub permission_mode: PermissionMode,
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
    pub working_directory: WorkingDirectory,
    pub terminal_size: Option<(u16, u16)>,
}

/// The working directory a task runs in. `Worktree` is the local-git case; a remote
/// project has no host-side path at all, which is why this is a sum and not an `Option`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkingDirectory {
    Local { path: String },
    Ssh { remote_path: String },
    Wsl { distribution: String, path: String },
}

/// Resuming is a different request, not `LaunchRequest` with a flag: it carries no prompt
/// for dsh (reconnecting must not replay the original message), and no attachments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeRequest {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub family: ProtocolFamily,
    pub agent: String,
    pub session_id: String,
    pub permission_mode: PermissionMode,
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
