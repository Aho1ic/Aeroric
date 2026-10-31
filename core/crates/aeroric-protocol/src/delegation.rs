//! B4-2 delegation (2026-10-31): the session the user talks to — or the user —
//! hands a piece of work to another native session under a contract, and gets
//! back a result card (frontier-roadmap §5.2 form C, §5.3, Q-B8–Q-B16).
//!
//! One delegation is a [`DelegationCard`]; every step of it appends a
//! `delegation` session update to the parent's log with the same
//! `delegationId`, and a client shows only the latest.

use serde::{Deserialize, Serialize};

use crate::checkpoint::{Acceptance, ObligationState};

pub const START_METHOD: &str = "delegation.start";
pub const LIST_METHOD: &str = "delegation.list";
pub const STOP_METHOD: &str = "delegation.stop";
pub const SEND_METHOD: &str = "delegation.send";
pub const REDO_METHOD: &str = "delegation.redo";
pub const ROLES_METHOD: &str = "delegation.roles";
pub const SAVE_ROLE_METHOD: &str = "delegation.saveRole";
pub const DELETE_ROLE_METHOD: &str = "delegation.deleteRole";
pub const AUDIT_METHOD: &str = "delegation.audit";
pub const STATS_METHOD: &str = "delegation.stats";
pub const ATTENTION_METHOD: &str = "delegation.attention";
pub const ACK_PROBES_METHOD: &str = "delegation.ackProbes";

/// A child's result beyond this is cut.
pub const MAX_RESULT_CHARS: usize = 2000;
/// The minutes a delegation may run when its contract names none.
pub const DEFAULT_MINUTES: u32 = 30;

/// Which session a child's context comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextMode {
    /// A fork of the parent's conversation (same family only).
    Fork,
    /// The parent's handoff brief, then the instruction.
    #[default]
    Brief,
    /// The instruction and the referenced files only.
    Isolated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Workspace {
    /// The parent's directory, read-only (permission `plan`).
    #[default]
    ReadOnly,
    /// The parent's directory, writable; one such child at a time.
    Shared,
}

/// Permission modes from least to most (permission decay compares these).
pub const PERMISSIONS: [&str; 4] = ["plan", "ask", "auto_edit", "full_access"];

/// `None` for a mode outside [`PERMISSIONS`].
pub fn permission_rank(mode: &str) -> Option<usize> {
    PERMISSIONS.iter().position(|p| *p == mode)
}

/// Which harness runs the child: `"same"` as the parent (default), `"other"`
/// family when there is one, or a named agent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AgentChoice {
    #[default]
    Same,
    Other,
    Named {
        agent: String,
        agent_config_id: Option<String>,
        model: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NamedAgent {
    agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

impl Serialize for AgentChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Same => s.serialize_str("same"),
            Self::Other => s.serialize_str("other"),
            Self::Named {
                agent,
                agent_config_id,
                model,
            } => NamedAgent {
                agent: agent.clone(),
                agent_config_id: agent_config_id.clone(),
                model: model.clone(),
            }
            .serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AgentChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(tag) if tag == "same" => Ok(Self::Same),
            serde_json::Value::String(tag) if tag == "other" => Ok(Self::Other),
            value @ serde_json::Value::Object(_) => {
                let named: NamedAgent = serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(Self::Named {
                    agent: named.agent,
                    agent_config_id: named.agent_config_id,
                    model: named.model,
                })
            }
            _ => Err(D::Error::custom(
                "agent is \"same\", \"other\" or {agent, agentConfigId?, model?}",
            )),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    /// Wall-clock minutes; `None` is [`DEFAULT_MINUTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
    /// Input plus output tokens; `None` is no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
}

/// What a delegation asks for. Fields left out take the role's defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    pub instruction: String,
    /// A role id from the library; its defaults fill what is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_mode: Option<ContextMode>,
    /// Files (paths relative to the working directory) the child should read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Workspace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<Acceptance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
}

/// A contract with every default applied: what the child really runs under.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub agent: AgentChoice,
    pub context_mode: ContextMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_refs: Vec<String>,
    pub permission: String,
    pub workspace: Workspace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<Acceptance>,
    #[serde(default)]
    pub budget: Budget,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// From the app (the `#` prefix or the progress page).
    #[default]
    User,
    /// The parent's own agent, through the context server.
    Session,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DelegationState {
    /// The child session is being created.
    #[default]
    Starting,
    Running,
    /// The child's turn ended; the project's checks are running on it.
    Checking,
    /// The child answered; the result is on the card.
    Reported,
    /// The child could not start, or its turn failed.
    Failed,
    /// The user stopped it.
    Stopped,
    /// Past its time or token budget; the child was interrupted.
    OverBudget,
    /// The host restarted while it ran.
    Interrupted,
}

impl DelegationState {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Checking)
    }

    /// Needs the user ("需要我处理").
    pub fn needs_user(self) -> bool {
        matches!(self, Self::Failed | Self::OverBudget | Self::Interrupted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceResult {
    pub id: String,
    pub text: String,
    pub state: ObligationState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationCard {
    pub delegation_id: String,
    pub parent_task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_task_id: Option<String>,
    pub origin: Origin,
    /// The role's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    /// "Codex · gpt-5"; empty until the child is started.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub harness_label: String,
    /// The agent id of the child ("claude", "codex").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub contract: Resolved,
    pub state: DelegationState,
    pub created_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    /// The child's last answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<AcceptanceResult>,
    /// The result rests on something checkable: a checked criterion or a diff.
    #[serde(default)]
    pub evidence: bool,
    /// The result went to the parent session.
    #[serde(default)]
    pub sent: bool,
    #[serde(default)]
    pub reworks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// B4-3: failure probes that fired on this delegation (never stop it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<Probe>,
}

/// B4-3 (§5.5): one failure probe hit. Kinds: `repeat` (the same tool call
/// three times), `oscillation` (A B A B), `nearBudget` (80% used, nothing
/// accepted), `unverifiedDone` (reported with criteria not all passed),
/// `noEvidence` (reported with nothing checkable).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub kind: String,
    pub detail: String,
    pub at_ms: u64,
    #[serde(default)]
    pub acknowledged: bool,
}

impl DelegationCard {
    /// Needs the user: it stopped badly, or a probe fired that nobody has seen.
    pub fn needs_user(&self) -> bool {
        self.state.needs_user() || self.probes.iter().any(|p| !p.acknowledged)
    }

    /// Every criterion checked and none failed (vacuously false with none).
    pub fn acceptance_passed(&self) -> bool {
        !self.acceptance.is_empty()
            && self
                .acceptance
                .iter()
                .all(|a| a.state == ObligationState::Passed)
    }
}

/// One entry of the role library.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Put before the delegation's own instruction.
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub agent: AgentChoice,
    #[serde(default)]
    pub context_mode: ContextMode,
    pub permission: String,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<Acceptance>,
    #[serde(default)]
    pub budget: Budget,
    /// Shipped with Aeroric; a saved role of the same id overrides it.
    #[serde(default)]
    pub builtin: bool,
    /// A built-in role the user changed (deleting it restores the default).
    #[serde(default)]
    pub customized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditAction {
    Start,
    Refuse,
    Report,
    Fail,
    Stop,
    Redo,
    Send,
    OverBudget,
}

/// One line of the append-only audit log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub at_ms: u64,
    /// `"user"`, or the task id of the session that acted.
    pub actor: String,
    pub action: AuditAction,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delegation_id: String,
    pub parent_task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// Track record of one harness × model × role (Q-B16: shown, never used to pick).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationRow {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub total: u32,
    pub reported: u32,
    pub failed: u32,
    /// Delegations whose criteria were all checked and passed.
    pub accepted: u32,
    /// Delegations that had criteria at all.
    pub with_acceptance: u32,
    pub reworks: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_choice_is_a_word_or_a_named_agent() {
        for (choice, wire) in [
            (AgentChoice::Same, serde_json::json!("same")),
            (AgentChoice::Other, serde_json::json!("other")),
            (
                AgentChoice::Named {
                    agent: "codex".into(),
                    agent_config_id: None,
                    model: Some("gpt-5".into()),
                },
                serde_json::json!({"agent": "codex", "model": "gpt-5"}),
            ),
        ] {
            assert_eq!(serde_json::to_value(&choice).unwrap(), wire);
            assert_eq!(serde_json::from_value::<AgentChoice>(wire).unwrap(), choice);
        }
        assert!(serde_json::from_value::<AgentChoice>(serde_json::json!("auto")).is_err());
    }

    #[test]
    fn permissions_rank_least_to_most() {
        assert!(permission_rank("plan") < permission_rank("ask"));
        assert!(permission_rank("auto_edit") < permission_rank("full_access"));
        assert_eq!(permission_rank("bogus"), None);
        assert!(DelegationState::Checking.is_open());
        assert!(!DelegationState::Reported.is_open());
        assert!(DelegationState::OverBudget.needs_user());
    }
}
