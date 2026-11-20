//! C8 (2026-11-20): the tools one launch of a session really has — the ones
//! Aeroric injects (the context server), the harness's own known ones —
//! and, for those it does not have, why.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolOwner {
    /// Served by Aeroric's context server (`aeroric-cli mcp-context`).
    Aeroric,
    /// Built into the harness; Aeroric does not control it.
    Harness,
}

/// Which of the context server's families a tool is in; each family is
/// offered on its own condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolGroup {
    Base,
    Memory,
    Wiki,
    References,
    Expert,
    Harness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolScope {
    ReadOnly,
    /// Changes files, or Aeroric's own records (board, memory, todos).
    Writes,
    /// Sends content off this Mac (another model, the web).
    SendsOut,
}

/// Every tool the context server can offer: `(name, group, scope)`. The
/// server's `tools/list` is checked against it by test.
pub const AERORIC_TOOLS: &[(&str, ToolGroup, ToolScope)] = &[
    ("search_sessions", ToolGroup::Base, ToolScope::ReadOnly),
    ("read_session", ToolGroup::Base, ToolScope::ReadOnly),
    ("read_handoff", ToolGroup::Base, ToolScope::ReadOnly),
    ("delegate", ToolGroup::Base, ToolScope::Writes),
    ("delegations", ToolGroup::Base, ToolScope::ReadOnly),
    ("list_roles", ToolGroup::Base, ToolScope::ReadOnly),
    ("board_read", ToolGroup::Base, ToolScope::ReadOnly),
    ("board_write", ToolGroup::Base, ToolScope::Writes),
    ("board_decide", ToolGroup::Base, ToolScope::Writes),
    ("todo_propose", ToolGroup::Base, ToolScope::Writes),
    ("artifact_list", ToolGroup::Base, ToolScope::ReadOnly),
    ("artifact_read", ToolGroup::Base, ToolScope::ReadOnly),
    ("memory_recall", ToolGroup::Memory, ToolScope::ReadOnly),
    ("memory_propose", ToolGroup::Memory, ToolScope::Writes),
    ("memory_feedback", ToolGroup::Memory, ToolScope::Writes),
    ("read_wiki", ToolGroup::Wiki, ToolScope::ReadOnly),
    ("search_wiki", ToolGroup::Wiki, ToolScope::ReadOnly),
    ("flag_wiki_issue", ToolGroup::Wiki, ToolScope::Writes),
    (
        "search_references",
        ToolGroup::References,
        ToolScope::ReadOnly,
    ),
    ("read_reference", ToolGroup::References, ToolScope::ReadOnly),
    ("ask_expert", ToolGroup::Expert, ToolScope::SendsOut),
];

/// One tool of one launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveTool {
    pub name: String,
    pub owner: ToolOwner,
    pub group: ToolGroup,
    pub scope: ToolScope,
    /// Offered to the agent this launch.
    pub ready: bool,
    /// Why it is offered, or why not.
    pub basis: String,
}

/// One launch (start or resume) of a session and its tools.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLaunch {
    pub at_ms: u64,
    pub harness: String,
    /// Whether the context server was given to this launch.
    pub injected: bool,
    /// Why not, when it was not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub tools: Vec<EffectiveTool>,
}
