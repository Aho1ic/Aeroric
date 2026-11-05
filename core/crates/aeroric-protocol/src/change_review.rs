//! R2-2 (2026-11-04): reviewing a session's changes hunk by hunk.
//!
//! Three ranges: one turn (its two snapshots), since the user last marked
//! the session seen, and the whole session (before its first turn) — the
//! last two against the working tree as it is now. A hunk can be accepted
//! (a mark only), rejected (reverse-applied to the working tree, never the
//! index), or commented; comments wait until the user sends them together.

use serde::{Deserialize, Serialize};

pub const DIFF_METHOD: &str = "changes.diff";
pub const ACCEPT_METHOD: &str = "changes.accept";
pub const REJECT_METHOD: &str = "changes.reject";
pub const COMMENT_METHOD: &str = "changes.comment";
pub const SEND_COMMENTS_METHOD: &str = "changes.sendComments";
pub const MARK_SEEN_METHOD: &str = "changes.markSeen";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeScope {
    #[default]
    Turn,
    Seen,
    Session,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HunkState {
    #[default]
    Pending,
    Accepted,
    Rejected,
    /// It no longer applies to the working tree: it cannot be rejected alone.
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// `" "`, `"+"` or `"-"`.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHunk {
    pub id: String,
    pub header: String,
    pub lines: Vec<DiffLine>,
    pub state: HunkState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub comment_sent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
    pub additions: u64,
    pub deletions: u64,
    pub hunks: Vec<ReviewHunk>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiff {
    pub scope: ChangeScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub files: Vec<ReviewFile>,
    /// When the session was last marked seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen_ms: Option<u64>,
    /// Comments written and not sent yet.
    pub unsent: u32,
    /// Why there is nothing to show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeParams {
    pub task_id: String,
    #[serde(default)]
    pub scope: Option<ChangeScope>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub hunk_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}
