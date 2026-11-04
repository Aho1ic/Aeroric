//! R2-1 (2026-11-04): turns that changed files, and undoing one.
//!
//! Each such turn is recorded with the two snapshot trees it was diffed
//! between and HEAD at both ends. A pending turn can be accepted or undone;
//! an undone turn's undo can be taken back. Undo only restores files whose
//! content is still what the turn left, and never touches the index or HEAD.

use serde::{Deserialize, Serialize};

use crate::session_live::TurnFileChange;

pub const LIST_METHOD: &str = "turns.list";
pub const PLAN_METHOD: &str = "turns.plan";
pub const APPLY_METHOD: &str = "turns.apply";
pub const ACCEPT_METHOD: &str = "turns.accept";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnState {
    /// Changed files; the user has not said yet.
    #[default]
    Pending,
    Accepted,
    Undone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    pub turn_id: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    /// The repository's top directory.
    pub top: String,
    /// Snapshot trees at the turn's start and end.
    pub before: String,
    pub after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_after: Option<String>,
    pub files: Vec<TurnFileChange>,
    pub additions: u64,
    pub deletions: u64,
    #[serde(default)]
    pub state: TurnState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnAction {
    /// Back to how the turn found the files.
    Undo,
    /// Take an undo back: the turn's own result again.
    Restore,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub path: String,
    /// The file is removed (it did not exist on the other side).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

/// What an undo (or its taking back) would do; `blocked` says why it cannot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPlan {
    pub files: Vec<PlannedFile>,
    pub skipped: Vec<SkippedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnParams {
    pub task_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub action: Option<TurnAction>,
}
