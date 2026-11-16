//! One row per ACP agent: what the protocol leaves open.
//!
//! The rule every row keeps: Aeroric's permission tier is pinned at launch,
//! never inherited from the user's own agent config (a `yolo` set elsewhere
//! must not reach an Aeroric session). A tier the agent cannot be held to is
//! refused before the process starts — never quietly loosened.

use crate::sessions::live::update::ToolKind;
use crate::tasks::ProtocolFamily;

/// How one permission tier is held for one launch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pin {
    /// Set last, over anything inherited.
    pub env: Vec<(String, String)>,
    /// A `session/set_mode` id the agent must offer; the session is refused
    /// when it does not.
    pub mode: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Dialect {
    pub family: ProtocolFamily,
    /// The agent's name in messages.
    pub label: &'static str,
    /// Arguments after the program.
    pub args: &'static [&'static str],
    pin: fn(&str) -> Option<Pin>,
    /// Card kind from the tool's name and ACP's own `kind`.
    pub tool_kind: fn(&str, Option<&str>) -> ToolKind,
}

impl Dialect {
    /// The pin for Aeroric's `permission`, or why the session is refused.
    pub fn pin(&self, permission: &str) -> Result<Pin, String> {
        (self.pin)(permission).ok_or_else(|| {
            format!(
                "{} 没有和“{permission}”对应、能被强制的权限档，已拒绝连接",
                self.label
            )
        })
    }
}

/// The dialect for a family that runs over ACP.
pub fn for_family(family: ProtocolFamily) -> Option<&'static Dialect> {
    match family {
        ProtocolFamily::Dsh => Some(&DSH),
        _ => None,
    }
}

/// DSH (`dsh acp`): no ACP modes. Its own sandbox holds the tier
/// (`DSH_PERMISSION_MODE`), and a write outside it comes back as a
/// `session/request_permission` escalation (recorded 2026-11-13). Every tool
/// says `kind: other`; the name is the title.
pub static DSH: Dialect = Dialect {
    family: ProtocolFamily::Dsh,
    label: "DSH",
    args: &["acp"],
    pin: dsh_pin,
    tool_kind: dsh_tool_kind,
};

fn dsh_pin(permission: &str) -> Option<Pin> {
    // `plan` (discussion members, read-only reviewers) reads only.
    let mode = match permission {
        "ask" | "plan" => "read-only",
        _ => crate::agents::launch::dsh_permission_mode(permission)?,
    };
    Some(Pin {
        env: vec![("DSH_PERMISSION_MODE".into(), mode.into())],
        mode: None,
    })
}

fn dsh_tool_kind(name: &str, kind: Option<&str>) -> ToolKind {
    match name {
        "bash" | "shell" | "exec" | "exec_command" => ToolKind::Command,
        "write" | "edit" | "multi_edit" | "multiedit" | "apply_patch" | "patch" => {
            ToolKind::FileChange
        }
        "task" | "agent" | "subagent" => ToolKind::Subagent,
        "web_search" | "websearch" | "web_fetch" | "webfetch" | "fetch" => ToolKind::WebSearch,
        name if name.starts_with("mcp__") || name.starts_with("mcp_") => ToolKind::Mcp,
        _ => acp_kind(kind),
    }
}

/// ACP's own tool kinds, for agents that fill them in.
pub fn acp_kind(kind: Option<&str>) -> ToolKind {
    match kind {
        Some("edit" | "delete" | "move") => ToolKind::FileChange,
        Some("execute") => ToolKind::Command,
        Some("fetch") => ToolKind::WebSearch,
        _ => ToolKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_pins_every_tier_and_refuses_the_unknown() {
        let env = |p: &str| DSH.pin(p).unwrap().env;
        assert_eq!(
            env("ask"),
            vec![("DSH_PERMISSION_MODE".into(), "read-only".into())]
        );
        assert_eq!(env("plan")[0].1, "read-only");
        assert_eq!(env("auto_edit")[0].1, "workspace-write");
        assert_eq!(env("full_access")[0].1, "danger-full-access");
        let refused = DSH.pin("yolo").unwrap_err();
        assert!(
            refused.contains("DSH") && refused.contains("拒绝连接"),
            "{refused}"
        );
        assert!(DSH.pin("").is_err());
    }

    #[test]
    fn dsh_tools_are_told_apart_by_name() {
        let kind = DSH.tool_kind;
        assert_eq!(kind("write", Some("other")), ToolKind::FileChange);
        assert_eq!(kind("edit", Some("other")), ToolKind::FileChange);
        assert_eq!(kind("bash", Some("other")), ToolKind::Command);
        assert_eq!(kind("mcp__aeroric__history", None), ToolKind::Mcp);
        assert_eq!(kind("read", Some("other")), ToolKind::Other);
        // A kind the agent did fill in still counts.
        assert_eq!(kind("rm", Some("delete")), ToolKind::FileChange);
    }

    #[test]
    fn only_dsh_runs_over_acp_so_far() {
        assert!(for_family(ProtocolFamily::Dsh).is_some());
        assert!(for_family(ProtocolFamily::Codex).is_none());
        assert!(for_family(ProtocolFamily::Omp).is_none());
    }
}
