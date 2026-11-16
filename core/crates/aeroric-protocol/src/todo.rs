//! 2026-11-15 工作安排第一批: 待办. The user writes what needs doing in a
//! project and hands it to an Agent by hand (a new session). A session that
//! ends is not a todo done: the todo waits for the user to accept it, or to
//! hand it out again.
//!
//! 第二批 (2026-11-15): an Agent proposes a todo (`todo_propose`), which waits
//! for the user to take or drop it; a todo may carry a 验收检查 (a shell
//! command run after its session's turn ends, its result kept on the
//! hand-out), and a failing one may be sent back into the session for the
//! Agent to fix, at most [`FIX_ROUNDS`] times. Passing still leaves the
//! acceptance to the user.

use serde::{Deserialize, Serialize};

pub const LIST_METHOD: &str = "todo.list";
pub const SAVE_METHOD: &str = "todo.save";
pub const DELETE_METHOD: &str = "todo.delete";
pub const ASSIGN_METHOD: &str = "todo.assign";
pub const DECIDE_METHOD: &str = "todo.decide";
/// `todo.check {id}`: run the 验收检查 now (重新检查).
pub const CHECK_METHOD: &str = "todo.check";
/// The Context MCP's `todo_propose`.
pub const PROPOSE_METHOD: &str = "context.todoPropose";

/// Proposals one session may have waiting for the user.
pub const PROPOSALS_PER_SESSION: usize = 5;
/// Times a failing check is sent back into the same session.
pub const FIX_ROUNDS: u32 = 2;
/// A 验收检查 is a test suite more often than not: ten minutes.
pub const CHECK_TIMEOUT_MS: u64 = 600_000;

/// Longest title kept; the notes are free.
pub const TITLE_CHARS: usize = 200;

/// What the user said about the todo. Its sessions say the rest
/// ([`Progress`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TodoState {
    /// An Agent proposed it: the user takes it (→ `Open`) or drops it.
    Proposed,
    /// Written down, not handed out.
    #[default]
    Open,
    /// Handed to an Agent; [`Todo::progress`] says how its latest session is
    /// doing.
    Assigned,
    /// The user accepted it.
    Done,
    Cancelled,
}

/// How an assigned todo's latest session stands, read from it on every list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Progress {
    /// A turn is running.
    Running,
    /// The session waits on the user (an approval, a question).
    Waiting,
    /// The session ended its turn: the user accepts or hands it out again.
    Review,
    /// The last turn did not run (quota, login, network…).
    NotRun,
    Failed,
    /// Stopped or cut off.
    Stopped,
    /// The session is gone.
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    Running,
    Passed,
    Failed,
}

/// One run of a todo's 验收检查, for the turn it checked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRun {
    pub state: CheckState,
    /// What the command printed (the tail of it), with the exit code when it
    /// failed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
    pub at_ms: u64,
    /// When the checked turn started: a later turn is checked again.
    pub turn_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub task_id: String,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub at_ms: u64,
    /// The latest 验收检查 of this hand-out's session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckRun>,
    /// Failing checks sent back into the session so far.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fixes: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default)]
    pub state: TodoState,
    /// Every hand-out, oldest first; the last one is the one that counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<Assignment>,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub updated_ms: u64,
    /// When it was accepted or cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_ms: Option<u64>,
    /// Set by the host on reads, for an assigned todo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
    /// Set with `progress`: why it did not run or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_detail: Option<String>,
    /// The session that proposed it (`todo_propose`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_by: Option<String>,
    /// 验收检查: a shell command run in the project after the session's turn
    /// ends; exit 0 passes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// A failing check goes back into the session, at most [`FIX_ROUNDS`]
    /// times (only with a `check`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fix_on_fail: bool,
}

impl Todo {
    /// The hand-out that counts now.
    pub fn latest(&self) -> Option<&Assignment> {
        self.assignments.last()
    }
}

/// `todo.decide`: what the user says about a todo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    /// 验收通过.
    Accept,
    Cancel,
    /// Back to open (from done or cancelled).
    Reopen,
    /// 收下 an Agent's proposal: it becomes an open todo.
    Take,
}
