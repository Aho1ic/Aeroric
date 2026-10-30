//! A task's checkpoint and the handoff brief built from it (B4′, 2026-10-30).
//!
//! The host derives every field from the structured session updates
//! ([`crate::session_live::SessionUpdateKind`]); the agent writes none of it.
//! Each entry carries an [`Epistemic`] status set by where it came from — a
//! user's answer or a tool's result is `verified`, the agent's own plan is
//! `inferred`, a call that never reported back is `unknown` — so a successor
//! can tell what was checked from what was only said (frontier-roadmap §3.1).

use serde::{Deserialize, Serialize};

pub const GET_METHOD: &str = "checkpoint.get";
pub const ADD_CONSTRAINT_METHOD: &str = "checkpoint.addConstraint";
pub const SET_OBLIGATION_METHOD: &str = "checkpoint.setObligation";
pub const CLARIFY_ACCEPT_METHOD: &str = "clarify.accept";
pub const BRANCH_START_METHOD: &str = "branch.start";
pub const BRANCH_RETURN_METHOD: &str = "branch.return";
/// Event topic: a task's checkpoint was rewritten (`{taskId, revision}`).
pub const CHANGED_TOPIC: &str = "checkpointChanged";

/// Lists keep this many entries; older ones fold into the `*_folded` counts.
pub const MAX_ENTRIES: usize = 50;
/// One entry's text beyond this is cut.
pub const MAX_TEXT_CHARS: usize = 400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Epistemic {
    Verified,
    #[default]
    Inferred,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceKind {
    UserMessage,
    ToolResult,
    TurnChanges,
    AutoCheck,
    Commit,
    Permission,
}

/// Where an entry's fact can be read again: a session update by `seq`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub text: String,
    pub status: Epistemic,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
}

/// A side effect the session caused: a command, a file change, a commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub summary: String,
    /// `command` | `fileChange` | `mcp` | `changes` | `commit`.
    pub kind: String,
    pub turn: u64,
    pub status: Epistemic,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntentState {
    Pending,
    Denied,
    Executed,
    OutcomeUnknown,
}

/// What the agent asked to do before an approval, recorded when it asked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Intent {
    pub request_id: String,
    pub summary: String,
    /// How to tell afterwards whether it happened.
    pub verify: String,
    pub state: IntentState,
    pub turn: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObligationState {
    Open,
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObligationSource {
    /// An acceptance criterion the user ticked on the clarification card.
    Acceptance,
    /// An entry of the agent's own plan.
    Plan,
    /// Added by hand on the progress page.
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Obligation {
    pub id: String,
    pub text: String,
    /// A command whose exit status decides it (run by the auto check).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    pub state: ObligationState,
    pub source: ObligationSource,
}

/// What a side branch brought back to its parent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchResult {
    pub task_id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    pub returned_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub task_id: String,
    pub revision: u64,
    /// Turns that have ended so far.
    pub turn: u64,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub effects_folded: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<Effect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intents: Vec<Intent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub obligations: Vec<Obligation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<BranchResult>,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl Checkpoint {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            ..Self::default()
        }
    }

    /// Nothing a successor would need: no goal, no effects, nothing open.
    pub fn is_empty(&self) -> bool {
        self.goal.is_none()
            && self.decisions.is_empty()
            && self.effects.is_empty()
            && self.unknown.is_empty()
            && self.intents.is_empty()
            && self.obligations.is_empty()
            && self.constraints.is_empty()
            && self.branches.is_empty()
    }

    /// Something a successor must check before going on.
    pub fn needs_attention(&self) -> bool {
        !self.unknown.is_empty()
            || self
                .obligations
                .iter()
                .any(|o| matches!(o.state, ObligationState::Open | ObligationState::Unknown))
            || self
                .intents
                .iter()
                .any(|i| i.state == IntentState::OutcomeUnknown)
    }
}

/// Why a brief was handed to a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HandoffReason {
    ModelSwitch,
    SourceMove,
    AfterCompact,
    Delegate,
    Resume,
}

impl HandoffReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelSwitch => "换模型",
            Self::SourceMove => "换来源",
            Self::AfterCompact => "压缩后续接",
            Self::Delegate => "委派",
            Self::Resume => "恢复会话",
        }
    }
}

/// One acceptance criterion proposed on the clarification card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Acceptance {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_empty_checkpoint_serializes_without_its_lists() {
        let cp = Checkpoint::new("t1");
        assert_eq!(
            serde_json::to_value(&cp).unwrap(),
            json!({"taskId": "t1", "revision": 0, "turn": 0, "updatedMs": 0})
        );
        assert!(cp.is_empty());
        assert!(!cp.needs_attention());
    }

    #[test]
    fn unknown_effects_and_open_obligations_need_attention() {
        let mut cp = Checkpoint::new("t1");
        cp.obligations.push(Obligation {
            id: "o1".into(),
            text: "测试通过".into(),
            check: Some("cargo test".into()),
            state: ObligationState::Passed,
            source: ObligationSource::Acceptance,
        });
        assert!(!cp.needs_attention());
        cp.obligations[0].state = ObligationState::Open;
        assert!(cp.needs_attention());
        let round: Checkpoint = serde_json::from_value(serde_json::to_value(&cp).unwrap()).unwrap();
        assert_eq!(round, cp);
    }
}
