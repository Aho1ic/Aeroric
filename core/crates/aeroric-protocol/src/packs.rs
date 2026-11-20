//! H6 (2026-11-21): a capability pack — skills, MCP servers, roles,
//! commands and event handlers described once in Aeroric, projected into
//! each harness's own form at launch. A pack lives at
//! `<data>/packs/<id>/pack.json`, its skill folders beside it.

use serde::{Deserialize, Serialize};

use crate::delegation::Role;

/// Where a pack came from, for the trust decision.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSource {
    /// `builtin`, `official-marketplace`, `local`, `git`, `url`…
    pub kind: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// One MCP server a pack brings (a local command; stdio).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackServer {
    pub name: String,
    /// What it is for, in a sentence.
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Kept in the pack's private file; never shown back unmasked.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

/// A slash command: Claude takes it as a plugin command, the others as a skill.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCommand {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub body: String,
}

/// An event handler: a command run when the harness reaches `event`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackHook {
    /// `sessionStart`, `preToolUse`, `postToolUse`, `stop`…
    pub event: String,
    pub command: String,
}

/// `pack.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source: PackSource,
    /// The user confirmed its source: its MCP servers and event handlers
    /// may run.
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Folder names under the pack's `skills/`, each with a `SKILL.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<PackServer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<Role>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<PackCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<PackHook>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackKind {
    Skill,
    McpServer,
    Role,
    Command,
    Hook,
}

/// One item of one pack at one launch: given to the harness, or why not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackItem {
    pub pack: String,
    pub kind: PackKind,
    pub name: String,
    pub ready: bool,
    /// Where it went, or why it did not.
    pub basis: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_pack_reads_with_defaults() {
        let pack: Pack = serde_json::from_str(
            r#"{"id": "gh", "name": "GitHub", "mcpServers": [{"name": "github", "command": "gh-mcp"}]}"#,
        )
        .unwrap();
        assert!(pack.enabled && !pack.confirmed);
        assert_eq!(pack.source, PackSource::default());
        assert!(pack.mcp_servers[0].enabled);
        assert!(pack.mcp_servers[0].args.is_empty());
        let back = serde_json::to_value(&pack).unwrap();
        assert!(back.get("skills").is_none(), "{back}");
        assert_eq!(back["mcpServers"][0]["command"], "gh-mcp");
    }
}
