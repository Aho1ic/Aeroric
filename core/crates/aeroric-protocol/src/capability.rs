//! Capability model. `v == 1` never implies a capability: the plan requires the
//! handshake to negotiate the version *intersection* plus an explicit feature list,
//! because `UNSUPPORTED_CAPABILITY` must be a server-side refusal, not a client
//! guess about what the host probably has.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One named capability the host advertises. `since` is the protocol version that
/// introduced it; `note` exists so a false value can carry a reason for the UI
/// ("needs a Windows host") instead of being presented as a missing feature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Per-agent structured abilities, kept apart on purpose: a family that can answer
/// a permission request is not thereby able to fork a session, and a PTY-only agent
/// must not be offered `permissionRespond` because a terminal exists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub native_prompt: bool,
    #[serde(default)]
    pub terminal_input: bool,
    #[serde(default)]
    pub permission_respond: bool,
    #[serde(default)]
    pub questions_respond: bool,
    #[serde(default)]
    pub plan_mode: bool,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub fork: bool,
    #[serde(default)]
    pub revert: bool,
    #[serde(default)]
    pub share: bool,
    #[serde(default)]
    pub usage: bool,
    #[serde(default)]
    pub attachments: bool,
    /// Values the host will actually accept, e.g. thinking levels. Absent means
    /// "unknown", which is not the same as "empty".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_levels: Option<Vec<String>>,
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    pub protocol_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_format: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// Keyed by agent id (built-in name or custom profile id).
    #[serde(default)]
    pub agents: BTreeMap<String, AgentCapabilities>,
}

impl HostCapabilities {
    pub fn enabled(&self, name: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.name == name && c.enabled)
    }

    pub fn reason_for(&self, name: &str) -> Option<&str> {
        self.capabilities
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.note.as_deref())
    }

    /// Whether a method gated on `requires_capability` may be called at all.
    pub fn permits(&self, requires: Option<&str>) -> bool {
        match requires {
            None => true,
            Some(name) => self.enabled(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostCapabilities {
        serde_json::from_str(
            r#"{"protocolVersion":1,"capabilities":[
                  {"name":"wsl","enabled":false,"note":"needs a Windows host"},
                  {"name":"remote","enabled":true,"since":1}],
                "agents":{"omp":{"terminalInput":true,"permissionRespond":false}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_false_capability_still_explains_itself() {
        let value = host();
        assert!(!value.enabled("wsl"));
        assert_eq!(value.reason_for("wsl"), Some("needs a Windows host"));
    }

    #[test]
    fn gating_blocks_only_what_is_declared_missing() {
        let value = host();
        assert!(value.permits(None));
        assert!(value.permits(Some("remote")));
        assert!(!value.permits(Some("wsl")));
        assert!(!value.permits(Some("never_advertised")));
    }

    #[test]
    fn terminal_input_does_not_imply_permission_ability() {
        let value = host();
        let omp = value.agents.get("omp").expect("omp present");
        assert!(omp.terminal_input);
        assert!(!omp.permission_respond);
        assert_eq!(omp.thinking_levels, None);
    }
}
