//! Task lifecycle: who decides what a task's state is, and when it reaches the disk.
//!
//! Before this module the answer was "whichever React handler ran last". The rules that
//! decide a task's persisted state now live here — the status machine ([`reducer`]), the
//! session owner ([`session_owner`]), the continuation strategy for a configuration
//! switch, the ordering of persist-then-spawn ([`service`]) and the idempotency ledger
//! that keeps a retried start from spawning twice ([`operations`]).
//!
//! What stays on the client, on purpose: which task is selected, toasts, terminal
//! buffers, run-count display, navigation. Those are views over the state, not the
//! state, and the acceptance test for this module is that start/resume/cancel all work
//! with no listener attached at all.
//!
//! Spawning itself is a port, not an implementation: [`executor`] declares
//! [`TaskExecutor`](executor::TaskExecutor) and Task P2-05 injects the legacy PTY/DSH/SSH/WSL
//! executor behind it, so the state machine was testable before any process could be
//! started.

pub mod cleanup;
pub mod events;
pub mod executor;
pub mod handoff;
pub mod launch_plan;
pub mod operations;
pub mod reducer;
pub mod service;
pub mod session_owner;
pub mod submissions;

/// The vocabulary the ports speak. A host implementing [`AgentFamilies`] cannot name a
/// protocol family without it, and src-tauri deliberately depends on `aeroric-core` only.
pub use aeroric_protocol::task::{PermissionMode, ProtocolFamily, TaskStatus};
pub use cleanup::{
    expired_task_ids, latest_weekly_slot, should_run_cleanup, CleanupSchedule, LocalCalendar, Mode,
    ShouldRun, DELETABLE_STATUSES,
};
pub use events::{NullSink, TaskEventSink, TaskNotification, TaskSessionFacts};
pub use executor::{
    settled_by_exit, CancelTarget, DshWindowFacts, Execution, ExecutorFailure, HostWindowFacts,
    LaunchRequest, LiveRuns, NoLiveRuns, ObservationOrigin, ResetOutcome, ResumeRequest, TaskEvent,
    TaskExecutor, TaskRuntimeObserver, WorkingDirectory,
};
pub use launch_plan::{completion_for, Completion, ContinuationMode};
pub use operations::{Claim, Claimed, LedgerState, OperationKind, OperationLedger};
pub use service::{
    Applied, Attachments, EventOutcome, NewTask, SessionSnapshot, TaskError, TaskIntent,
    TaskIntentKind, TaskService, TASK_INTENT_VOCABULARY,
};
pub use session_owner::{AgentFamilies, ConfigSwitchStrategy, SessionFields, SessionOwner};
