//! R2-3b (2026-11-05): a session run in an isolated copy of its project's
//! directory — a detached git worktree under `<repo>/.aeroric/worktrees/` —
//! whose changes the user brings back hunk by hunk.

use serde::{Deserialize, Serialize};

pub const GET_METHOD: &str = "copies.get";
pub const REMOVE_METHOD: &str = "copies.remove";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyRecord {
    pub task_id: String,
    /// The copy's repository root.
    pub path: String,
    /// The original repository root.
    pub origin: String,
    /// The tree the copy started from (moved forward after a conflict merge).
    pub base: String,
    pub created_ms: u64,
    /// When nothing was left to bring back; cleared again on a new change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared_ms: Option<u64>,
    /// B7 (2026-11-19): the copy's own snapshot right after it was made —
    /// the tree its agent really found. Unset on copies from before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyParams {
    pub task_id: String,
}
