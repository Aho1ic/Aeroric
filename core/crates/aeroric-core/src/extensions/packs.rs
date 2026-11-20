//! H6 (2026-11-21): capability packs on disk, whether to trust them, and
//! what one launch of a harness is given from them. The pure plan here
//! decides every item; the drivers only put the given MCP servers where
//! their harness reads them.
//!
//! An untrusted pack's MCP servers and event handlers are never projected:
//! they run commands. Nothing here runs anything.

use std::path::{Path, PathBuf};

use aeroric_protocol::packs::{Pack, PackItem, PackKind, PackServer};
use aeroric_protocol::ProtocolFamily;

/// `<data>/packs/<id>/pack.json`.
pub const DIR: &str = "packs";
pub const MANIFEST: &str = "pack.json";

/// A pack's id is its folder's name: letters, digits, `-` and `_`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR)
}

/// Every pack under `<data>/packs/`, by id; a folder that does not hold a
/// readable `pack.json` of its own id is listed in the second vector with why.
pub fn load(data_dir: &Path) -> (Vec<Pack>, Vec<(String, String)>) {
    let mut packs = Vec::new();
    let mut broken = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(data_dir)) else {
        return (packs, broken);
    };
    for entry in entries.flatten() {
        let folder = entry.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') || !entry.path().is_dir() {
            continue;
        }
        let file = entry.path().join(MANIFEST);
        let read = std::fs::read_to_string(&file)
            .map_err(|e| format!("读不到 {MANIFEST}：{e}"))
            .and_then(|text| {
                serde_json::from_str::<Pack>(&text)
                    .map_err(|e| format!("{MANIFEST} 不是有效的能力包清单：{e}"))
            });
        match read {
            Ok(pack) if pack.id != folder => {
                broken.push((folder, format!("清单里的 id “{}”和文件夹名不一致", pack.id)))
            }
            Ok(pack) if !valid_id(&pack.id) => {
                broken.push((folder, "id 只能用字母、数字、- 和 _".into()))
            }
            Ok(pack) => packs.push(pack),
            Err(why) => broken.push((folder, why)),
        }
    }
    packs.sort_by(|a, b| a.id.cmp(&b.id));
    broken.sort();
    (packs, broken)
}

/// May its MCP servers and event handlers run: a built-in or official pack
/// at a known revision, or one whose source the user confirmed.
pub fn trusted(pack: &Pack) -> bool {
    pack.confirmed
        || (matches!(
            pack.source.kind.as_str(),
            "builtin" | "official-marketplace"
        ) && pack.source.revision.is_some())
}

/// A server's name as every harness can take it, and not Aeroric's own.
fn server_name_problem(name: &str) -> Option<&'static str> {
    if name == crate::sessions::live::context_server::SERVER_NAME {
        return Some("名字和 Aeroric 自己的工具服务器相同");
    }
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some("名字只能用字母、数字、- 和 _，最长 64 个字符");
    }
    None
}

/// Who is launching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub family: ProtocolFamily,
    /// The agent runs on another machine: a pack's commands are this Mac's.
    pub remote: bool,
}

impl Target {
    fn harness(self) -> &'static str {
        match self.family {
            ProtocolFamily::Claude => "Claude",
            ProtocolFamily::Codex => "Codex",
            ProtocolFamily::Omp => "OMP",
            ProtocolFamily::Dsh => "DSH",
        }
    }

    /// Where an MCP server goes for this harness.
    fn server_place(self) -> &'static str {
        match self.family {
            ProtocolFamily::Claude => "本次启动的 MCP 配置（--mcp-config）",
            ProtocolFamily::Codex => "本次会话的 config.mcp_servers",
            ProtocolFamily::Omp => "受管目录的 mcp.json",
            ProtocolFamily::Dsh => "session/new 的 mcpServers",
        }
    }
}

/// What one launch is given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub servers: Vec<PackServer>,
    pub items: Vec<PackItem>,
}

fn kind_name(kind: PackKind) -> &'static str {
    match kind {
        PackKind::Skill => "技能",
        PackKind::McpServer => "MCP 服务器",
        PackKind::Role => "角色",
        PackKind::Command => "命令",
        PackKind::Hook => "事件处理",
    }
}

/// Decide every item of every pack for `target`.
pub fn plan(packs: &[Pack], target: Target) -> Plan {
    let mut out = Plan::default();
    let mut taken: Vec<(String, String)> = Vec::new();
    for pack in packs {
        let trusted = trusted(pack);
        let item = |kind: PackKind, name: &str, ready: bool, basis: String| PackItem {
            pack: pack.id.clone(),
            kind,
            name: name.to_string(),
            ready,
            basis,
        };
        for server in &pack.mcp_servers {
            let withheld = if !pack.enabled {
                Some("能力包已停用".to_string())
            } else if !server.enabled {
                Some("这个服务器已停用".to_string())
            } else if !trusted {
                Some("能力包的来源还没有确认，会运行命令的 MCP 服务器不投影".to_string())
            } else if target.remote {
                Some("远端会话不投影这台 Mac 上的 MCP 服务器".to_string())
            } else if let Some(problem) = server_name_problem(&server.name) {
                Some(problem.to_string())
            } else if let Err(why) =
                super::mcp::validate_server(&server.name, &server.command, &server.env)
            {
                Some(why)
            } else {
                taken
                    .iter()
                    .find(|(name, _)| *name == server.name)
                    .map(|(_, by)| format!("同名服务器已由能力包“{by}”提供"))
            };
            match withheld {
                Some(why) => out
                    .items
                    .push(item(PackKind::McpServer, &server.name, false, why)),
                None => {
                    taken.push((server.name.clone(), pack.name.clone()));
                    out.servers.push(server.clone());
                    out.items.push(item(
                        PackKind::McpServer,
                        &server.name,
                        true,
                        format!("已写进{}", target.server_place()),
                    ));
                }
            }
        }
        let others = pack
            .skills
            .iter()
            .map(|s| (PackKind::Skill, s.as_str()))
            .chain(pack.roles.iter().map(|r| (PackKind::Role, r.id.as_str())))
            .chain(
                pack.commands
                    .iter()
                    .map(|c| (PackKind::Command, c.name.as_str())),
            )
            .chain(
                pack.hooks
                    .iter()
                    .map(|h| (PackKind::Hook, h.event.as_str())),
            );
        for (kind, name) in others {
            let why = if !pack.enabled {
                "能力包已停用".to_string()
            } else if kind == PackKind::Hook && !trusted {
                "能力包的来源还没有确认，会运行命令的事件处理不投影".to_string()
            } else {
                format!(
                    "Aeroric 还没有把能力包里的{}投影给 {}",
                    kind_name(kind),
                    target.harness()
                )
            };
            out.items.push(item(kind, name, false, why));
        }
    }
    out
}

/// The driver did not take the servers: each one planned is not given.
pub fn refused(items: &mut [PackItem], family: ProtocolFamily) {
    let harness = Target {
        family,
        remote: false,
    }
    .harness();
    for item in items
        .iter_mut()
        .filter(|i| i.ready && i.kind == PackKind::McpServer)
    {
        item.ready = false;
        item.basis = format!("{harness} 这次启动不接受 Aeroric 下发的 MCP 服务器");
    }
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
