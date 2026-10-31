//! B4-3 shared board (2026-10-31): one structured board per manager session,
//! shared with the children it delegated to — goal, hypotheses, claims,
//! decisions, evidence and open questions (frontier-roadmap §5.4, Q-B17–Q-B20).
//! A member's suggestion is not a team decision: an agent's decision is only
//! ever proposed; the user or the manager accepts it.

use serde::{Deserialize, Serialize};

pub const GET_METHOD: &str = "board.get";
pub const WRITE_METHOD: &str = "board.write";
pub const DECIDE_METHOD: &str = "board.decide";
pub const ATTENTION_METHOD: &str = "board.attention";

/// An entry's text beyond this is cut.
pub const MAX_TEXT_CHARS: usize = 1000;
/// Changes kept per board (oldest dropped first).
pub const MAX_CHANGES: usize = 500;
/// A claim not touched for this long is stale (Q-B21 ⑤).
pub const STALE_CLAIM_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoardKind {
    Goal,
    Hypothesis,
    Claim,
    Decision,
    Evidence,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoardState {
    Open,
    Confirmed,
    Refuted,
    Proposed,
    Accepted,
    Rejected,
    Answered,
    Done,
}

impl BoardKind {
    /// The states an entry of this kind may be in; the first is its default.
    pub fn states(self) -> &'static [BoardState] {
        use BoardState::*;
        match self {
            Self::Goal | Self::Evidence => &[Open],
            Self::Hypothesis => &[Open, Confirmed, Refuted],
            Self::Claim => &[Open, Done],
            Self::Decision => &[Proposed, Accepted, Rejected],
            Self::Question => &[Open, Answered],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardEntry {
    pub id: String,
    pub kind: BoardKind,
    pub text: String,
    pub state: BoardState,
    /// `"user"`, or the task id of the session that wrote it.
    pub author: String,
    /// A claim's member (a task id, or a name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// The board revision of its last change.
    pub revision: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardChange {
    pub at_ms: u64,
    pub actor: String,
    pub entry_id: String,
    /// `add` `edit` `accept` `reject`.
    pub action: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub owner_task_id: String,
    pub revision: u64,
    #[serde(default)]
    pub entries: Vec<BoardEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<BoardChange>,
}

/// One write: a new entry (no `id`) or a change to one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardWrite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BoardKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<BoardState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
}

/// Something on a board that needs the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardAttention {
    pub owner_task_id: String,
    pub entry: BoardEntry,
    /// `decision` (waits to be accepted) or `staleClaim`.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_names_its_states() {
        assert_eq!(BoardKind::Decision.states()[0], BoardState::Proposed);
        assert!(!BoardKind::Goal.states().contains(&BoardState::Accepted));
        assert_eq!(
            serde_json::to_value(BoardKind::Hypothesis).unwrap(),
            "hypothesis"
        );
    }
}
