//! H5 (2026-11-20): the role library, projected as each harness's own
//! subagent definitions, so an agent that dispatches its native subagent by
//! a role's name gets that role. Only roles that make sense as a native
//! subagent are projected: one that must run on another harness or model,
//! in an isolated copy, or under acceptance or a budget stays `delegate`'s.
//!
//! A permission is expressed only where the harness can hold it: a
//! read-only role gets a read-only tool list (Claude, OMP). Codex's custom
//! agents cannot narrow the parent's sandbox, so nothing is projected there:
//! a read-only role would run writable.

use std::collections::BTreeSet;
use std::path::Path;

use aeroric_protocol::delegation::{AgentChoice, Role, Workspace};
use aeroric_protocol::tools::{ToolScope, AERORIC_TOOLS};

/// Marks a definition file as Aeroric's; only marked files are replaced or
/// removed, anything else in the folder is left alone.
pub const MARKER: &str = "<!-- aeroric-role -->";

/// The ask_expert role exists to reach a stronger model, which a native
/// subagent of the same session would not.
const EXPERT: &str = "expert";

/// Why `role` is not projected, or `None` when it is.
pub fn not_projected(role: &Role) -> Option<&'static str> {
    if role.id == EXPERT {
        return Some("专家角色要用另一个更强的模型（ask_expert）");
    }
    if role.agent != AgentChoice::Same {
        return Some("要换一种 Harness 或模型，只能经 delegate");
    }
    if role.workspace == Workspace::Copy {
        return Some("要一份隔离的可写副本，只能经 delegate");
    }
    if !role.acceptance.is_empty() || role.budget != Default::default() {
        return Some("带验收或预算，只能经 delegate");
    }
    None
}

/// Where the definitions go and how they are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Claude's session plugin (`<plugin>/agents/`): the model sees
    /// `aeroric:<id>`.
    ClaudePlugin,
    /// A Claude configuration's managed home (`<home>/agents/`).
    ClaudeHome,
    /// OMP's managed home (`<home>/agents/`), which also has bundled agents
    /// (`reviewer`…) a bare id would collide with.
    OmpHome,
}

fn file_id(role: &Role) -> String {
    let id: String = role
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    id.trim_matches('-').to_string()
}

/// The name the harness shows the definition under.
pub fn name(target: Target, role: &Role) -> String {
    match target {
        Target::ClaudePlugin => file_id(role),
        Target::ClaudeHome | Target::OmpHome => format!("aeroric-{}", file_id(role)),
    }
}

fn yaml_quoted(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("\"{escaped}\"")
}

/// The name the model dispatches it by: Claude prefixes a plugin's agents
/// with the plugin's name.
pub fn shown_name(target: Target, role: &Role) -> String {
    match target {
        Target::ClaudePlugin => format!("aeroric:{}", name(target, role)),
        Target::ClaudeHome | Target::OmpHome => name(target, role),
    }
}

/// Aeroric's tool a read-only role may still write with: the shared board
/// is Aeroric's own record, where a delegated read-only child reports too.
const BOARD_WRITE: &str = "board_write";

/// The read-only tools a `plan` role gets; `None` for a writable role,
/// which keeps the parent's (a native subagent never has more than it).
fn read_only_tools(target: Target) -> Vec<String> {
    match target {
        Target::ClaudePlugin | Target::ClaudeHome => {
            let mut tools: Vec<String> = ["Read", "Grep", "Glob", "WebFetch", "WebSearch"]
                .iter()
                .map(|t| t.to_string())
                .collect();
            // Claude lists an MCP tool as `mcp__<server>__<tool>`; only
            // Aeroric's read-only ones, and the board to report on.
            tools.extend(
                AERORIC_TOOLS
                    .iter()
                    .filter(|(name, _, scope)| {
                        *scope == ToolScope::ReadOnly || *name == BOARD_WRITE
                    })
                    .map(|(name, _, _)| {
                        format!(
                            "mcp__{}__{name}",
                            crate::sessions::live::context_server::SERVER_NAME
                        )
                    }),
            );
            tools
        }
        // OMP's built-in read tools; MCP tools are added on top by OMP.
        Target::OmpHome => ["read", "grep", "glob", "ast_grep", "think"]
            .iter()
            .map(|t| t.to_string())
            .collect(),
    }
}

/// One role's definition file.
pub fn definition(target: Target, role: &Role) -> String {
    let mut out = format!(
        "---\nname: {}\ndescription: {}\n",
        name(target, role),
        yaml_quoted(&format!(
            "{}（Aeroric 角色库）：{}",
            role.name, role.description
        ))
    );
    if role.permission == "plan" {
        out.push_str(&format!("tools: {}\n", read_only_tools(target).join(", ")));
    }
    out.push_str("---\n");
    out.push_str(MARKER);
    out.push('\n');
    out.push_str(role.instruction.trim());
    out.push_str(&format!(
        "\n\n写共享黑板（board_write）时，subagent 填“{}”，黑板上会标明是你写的。\n",
        shown_name(target, role)
    ));
    out
}

/// Keep `dir` holding exactly the projectable `roles` as Aeroric's marked
/// files; returns the names written. Unmarked files are never touched.
pub fn write(dir: &Path, target: Target, roles: &[Role]) -> Result<Vec<String>, String> {
    let wanted: Vec<(String, String)> = roles
        .iter()
        .filter(|role| not_projected(role).is_none() && !file_id(role).is_empty())
        .map(|role| (name(target, role), definition(target, role)))
        .collect();
    if wanted.is_empty() && !dir.exists() {
        return Ok(Vec::new());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let keep: BTreeSet<String> = wanted.iter().map(|(n, _)| format!("{n}.md")).collect();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        let ours = std::fs::read_to_string(entry.path()).is_ok_and(|t| t.contains(MARKER));
        if ours && !keep.contains(&file) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    for (n, body) in &wanted {
        let path = dir.join(format!("{n}.md"));
        let current = std::fs::read_to_string(&path).ok();
        // Someone else's file of the same name stays.
        if current.as_deref().is_some_and(|t| !t.contains(MARKER)) {
            continue;
        }
        if current.as_deref() != Some(body.as_str()) {
            std::fs::write(&path, body).map_err(|e| e.to_string())?;
        }
    }
    Ok(wanted.into_iter().map(|(n, _)| n).collect())
}

/// The role library a data folder holds (built-ins and the user's own),
/// with H6's capability packs' roles whose id it does not have yet.
pub fn library(data_dir: &Path) -> Vec<Role> {
    let mut roles =
        crate::sessions::live::delegation::roles::list(Some(&data_dir.join("delegations")));
    let (packs, _) = super::packs::load(data_dir);
    let extra: Vec<Role> = super::packs::roles(&packs, &roles)
        .into_iter()
        .filter(|(_, _, state)| state.is_ok())
        .map(|(_, role, _)| role)
        .collect();
    roles.extend(extra);
    roles
}

#[cfg(test)]
#[path = "native_roles_tests.rs"]
mod tests;
