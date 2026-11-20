//! H6 (2026-11-21): capability packs on disk, whether to trust them, and
//! what one launch of a harness is given from them. The plan here decides
//! every item; the drivers put the given MCP servers where their harness
//! reads them, and the skill mount (`builtin_skills::mount`) places the
//! skills, commands, roles and event handlers with the same rules.
//!
//! An untrusted pack's MCP servers and event handlers are never projected:
//! they run commands. Nothing here runs anything.

use std::path::{Path, PathBuf};

use aeroric_protocol::delegation::Role;
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
    /// The session runs in a home Aeroric manages (an Agent configuration,
    /// OMP, DSH), not the user's own Claude or Codex.
    pub managed: bool,
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
            ProtocolFamily::Omp => {
                "受管目录的 mcp.json（OMP 从其他 Harness 导入的同名服务器让位，项目里 .omp/mcp.json 的同名服务器优先）"
            }
            ProtocolFamily::Dsh => "session/new 的 mcpServers",
        }
    }

    /// Where a skill (or a command, given as a skill) goes.
    fn skill_place(self, name: &str) -> String {
        match (self.family, self.managed) {
            (ProtocolFamily::Claude, false) => {
                format!("放进会话插件的 skills/（用 /aeroric:{name}）")
            }
            (ProtocolFamily::Codex, false) => "复制到项目的 .agents/skills（带标记）".into(),
            _ => "放进受管目录的 skills/".into(),
        }
    }
}

/// What one launch is given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub servers: Vec<PackServer>,
    pub items: Vec<PackItem>,
}

const OFF: &str = "能力包已停用";

fn item(pack: &Pack, kind: PackKind, name: &str, ready: bool, basis: String) -> PackItem {
    PackItem {
        pack: pack.id.clone(),
        kind,
        name: name.to_string(),
        ready,
        basis,
    }
}

/// Decide every item of every pack for `target`. `data_dir` holds the packs'
/// skill folders and the role library.
pub fn plan(data_dir: &Path, packs: &[Pack], target: Target) -> Plan {
    let mut out = Plan::default();
    plan_servers(packs, target, &mut out);
    for (pack, kind, name, skill) in skills(data_dir, packs) {
        let withheld = match skill {
            Err(why) => Some(why),
            Ok(_) if target.remote => Some("远端会话不投影这台 Mac 上的技能".into()),
            Ok(_) => None,
        };
        out.items.push(match withheld {
            Some(why) => item(pack, kind, &name, false, why),
            None => {
                let place = target.skill_place(&name);
                let given = match kind {
                    PackKind::Command => format!("作为技能{place}"),
                    _ => place,
                };
                item(
                    pack,
                    kind,
                    &name,
                    true,
                    format!("{given}；你自己有同名技能时用你的"),
                )
            }
        });
    }
    plan_roles(data_dir, packs, target, &mut out);
    plan_hooks(packs, target, &mut out);
    out
}

fn plan_servers(packs: &[Pack], target: Target, out: &mut Plan) {
    let mut taken: Vec<(String, String)> = Vec::new();
    for pack in packs {
        for server in &pack.mcp_servers {
            let withheld = if !pack.enabled {
                Some(OFF.to_string())
            } else if !server.enabled {
                Some("这个服务器已停用".to_string())
            } else if !trusted(pack) {
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
                Some(why) => {
                    out.items
                        .push(item(pack, PackKind::McpServer, &server.name, false, why))
                }
                None => {
                    taken.push((server.name.clone(), pack.name.clone()));
                    out.servers.push(server.clone());
                    out.items.push(item(
                        pack,
                        PackKind::McpServer,
                        &server.name,
                        true,
                        format!("已写进{}", target.server_place()),
                    ));
                }
            }
        }
    }
}

/// Where a skill comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillSource {
    /// The pack's `skills/<name>/` folder.
    Folder(PathBuf),
    /// A command, written as a skill's `SKILL.md`.
    Text(String),
}

/// The skills and commands of all packs, in order, each with where it comes
/// from or why it is not offered. A name is offered once: a built-in skill
/// keeps its name, an earlier pack keeps one it took.
pub fn skills<'a>(
    data_dir: &Path,
    packs: &'a [Pack],
) -> Vec<(&'a Pack, PackKind, String, Result<SkillSource, String>)> {
    let builtin: Vec<String> = super::builtin_skills::catalog()
        .into_iter()
        .map(|s| s.id)
        .collect();
    let mut taken: Vec<(String, String)> = Vec::new();
    let mut out = Vec::new();
    for pack in packs {
        let entries = pack
            .skills
            .iter()
            .map(|name| {
                let folder = dir(data_dir).join(&pack.id).join("skills").join(name);
                let source = if folder.join("SKILL.md").is_file() {
                    Ok(SkillSource::Folder(folder))
                } else {
                    Err(format!("包里没有 skills/{name}/SKILL.md"))
                };
                (PackKind::Skill, name.clone(), source)
            })
            .chain(pack.commands.iter().map(|command| {
                let source = if command.body.trim().is_empty() {
                    Err("命令正文为空".to_string())
                } else {
                    let description = if command.description.trim().is_empty() {
                        format!("能力包“{}”的命令 {}", pack.name, command.name)
                    } else {
                        command.description.clone()
                    };
                    Ok(SkillSource::Text(format!(
                        "---\nname: {}\ndescription: {}\n---\n\n{}\n",
                        command.name,
                        serde_json::Value::String(description.replace('\n', " ")),
                        command.body.trim()
                    )))
                };
                (PackKind::Command, command.name.clone(), source)
            }));
        for (kind, name, source) in entries {
            let source = if !pack.enabled {
                Err(OFF.to_string())
            } else if !valid_id(&name) {
                Err("名字只能用字母、数字、- 和 _".to_string())
            } else if builtin.contains(&name) {
                Err("和 Aeroric 的内置技能同名，用内置的".to_string())
            } else if let Some((_, by)) = taken.iter().find(|(n, _)| *n == name) {
                Err(format!("同名技能已由能力包“{by}”提供"))
            } else {
                source
            };
            if source.is_ok() {
                taken.push((name.clone(), pack.name.clone()));
            }
            out.push((pack, kind, name, source));
        }
    }
    out
}

/// Put a pack skill in Aeroric's skill store (so links to it count as
/// Aeroric's) under a folder named by its content; returns the offer.
pub fn materialize(
    data_dir: &Path,
    pack: &str,
    name: &str,
    source: &SkillSource,
) -> Result<super::builtin_skills::Offer, String> {
    use sha2::{Digest, Sha256};
    let digest = match source {
        SkillSource::Folder(folder) => {
            Sha256::digest(super::builtin_skills::manifest_of(folder)?.as_bytes())
        }
        SkillSource::Text(text) => Sha256::digest(text.as_bytes()),
    };
    let hash: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    let parent = super::builtin_skills::base_dir(data_dir)
        .join(DIR)
        .join(pack);
    let target = parent.join(format!("{name}-{hash}"));
    let done = parent.join(format!("{name}-{hash}.complete"));
    if !done.is_file() {
        let _ = std::fs::remove_dir_all(&target);
        match source {
            SkillSource::Folder(folder) => super::builtin_skills::copy_tree(folder, &target)?,
            SkillSource::Text(text) => {
                std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                std::fs::write(target.join("SKILL.md"), text).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(&done, &hash).map_err(|e| e.to_string())?;
    }
    Ok(super::builtin_skills::Offer {
        id: name.to_string(),
        dir: target,
        version: format!("pack-{hash}"),
    })
}

/// The skills a session on this Mac is offered from the packs; one that
/// cannot be placed is logged and left out.
pub fn skill_offers(data_dir: &Path) -> Vec<super::builtin_skills::Offer> {
    let (packs, _) = load(data_dir);
    skills(data_dir, &packs)
        .into_iter()
        .filter_map(|(pack, _, name, source)| {
            let source = source.ok()?;
            materialize(data_dir, &pack.id, &name, &source)
                .map_err(|why| {
                    eprintln!("[aeroric] pack skill {}/{name} not placed: {why}", pack.id)
                })
                .ok()
        })
        .collect()
}

/// The packs' roles that join the role library for the harnesses' own
/// subagents: those of enabled packs whose id the library (or an earlier
/// pack) does not have yet.
pub fn roles(packs: &[Pack], library: &[Role]) -> Vec<(String, Role, Result<(), String>)> {
    let mut taken: Vec<String> = library.iter().map(|r| r.id.clone()).collect();
    let mut out = Vec::new();
    for pack in packs {
        for role in &pack.roles {
            let state = if !pack.enabled {
                Err(OFF.to_string())
            } else if taken.contains(&role.id) {
                Err("角色库里已有同 id 的角色".to_string())
            } else {
                taken.push(role.id.clone());
                Ok(())
            };
            out.push((pack.id.clone(), role.clone(), state));
        }
    }
    out
}

fn plan_roles(data_dir: &Path, packs: &[Pack], target: Target, out: &mut Plan) {
    let library =
        crate::sessions::live::delegation::roles::list(Some(&data_dir.join("delegations")));
    for (pack_id, role, state) in roles(packs, &library) {
        let pack = packs
            .iter()
            .find(|p| p.id == pack_id)
            .expect("the role's pack");
        let target_of = match target.family {
            ProtocolFamily::Claude if target.managed => {
                Some(super::native_roles::Target::ClaudeHome)
            }
            ProtocolFamily::Claude => Some(super::native_roles::Target::ClaudePlugin),
            ProtocolFamily::Omp => Some(super::native_roles::Target::OmpHome),
            _ => None,
        };
        let withheld = match (&state, target_of) {
            (Err(why), _) => Some(why.clone()),
            _ if target.remote => Some("远端会话不投影这台 Mac 上的角色".into()),
            (_, None) if target.family == ProtocolFamily::Codex => {
                Some("Codex 的自定义代理不能收窄父会话的沙盒，不投影".into())
            }
            (_, None) => Some(format!("{} 没有原生子代理定义", target.harness())),
            (Ok(()), Some(_)) => super::native_roles::not_projected(&role).map(str::to_string),
        };
        out.items.push(match (withheld, target_of) {
            (Some(why), _) => item(pack, PackKind::Role, &role.id, false, why),
            (None, Some(native)) => item(
                pack,
                PackKind::Role,
                &role.id,
                true,
                format!(
                    "写成原生子代理 {}",
                    super::native_roles::shown_name(native, &role)
                ),
            ),
            (None, None) => unreachable!("a role with no native target is withheld"),
        });
    }
}

/// Claude's name for an event, or `None` when it has no such event.
fn claude_event(event: &str) -> Option<&'static str> {
    Some(match event {
        "sessionStart" => "SessionStart",
        "sessionEnd" => "SessionEnd",
        "userPromptSubmit" => "UserPromptSubmit",
        "preToolUse" => "PreToolUse",
        "postToolUse" => "PostToolUse",
        "stop" => "Stop",
        "subagentStop" => "SubagentStop",
        "preCompact" => "PreCompact",
        "notification" => "Notification",
        _ => return None,
    })
}

fn plan_hooks(packs: &[Pack], target: Target, out: &mut Plan) {
    for pack in packs {
        for hook in &pack.hooks {
            let withheld = if !pack.enabled {
                Some(OFF.to_string())
            } else if !trusted(pack) {
                Some("能力包的来源还没有确认，会运行命令的事件处理不投影".to_string())
            } else if target.remote {
                Some("远端会话不投影这台 Mac 上的事件处理".to_string())
            } else if hook.command.trim().is_empty() {
                Some("命令为空".to_string())
            } else {
                match (target.family, target.managed) {
                    (ProtocolFamily::Claude, false) => claude_event(&hook.event)
                        .is_none()
                        .then(|| format!("Claude 没有 {} 这个事件", hook.event)),
                    (ProtocolFamily::Claude, true) => Some(
                        "走 Agent 配置的 Claude 没有会话插件，事件处理只投给直连的 Claude"
                            .to_string(),
                    ),
                    (ProtocolFamily::Codex, _) => Some(
                        "Codex 按会话下发的事件处理还没在真实会话里验证过，先不投影".to_string(),
                    ),
                    (ProtocolFamily::Omp, _) => {
                        Some("OMP 的事件处理是 JS/TS 模块，不接受命令".to_string())
                    }
                    (ProtocolFamily::Dsh, _) => Some("DSH 没有事件处理的入口".to_string()),
                }
            };
            out.items.push(match withheld {
                Some(why) => item(pack, PackKind::Hook, &hook.event, false, why),
                None => item(
                    pack,
                    PackKind::Hook,
                    &hook.event,
                    true,
                    "写进会话插件的 hooks/hooks.json".to_string(),
                ),
            });
        }
    }
}

/// The `hooks/hooks.json` a direct Claude session's plugin holds: every
/// trusted, enabled pack's handler for an event Claude has. `None` when
/// there are none.
pub fn claude_hooks(packs: &[Pack]) -> Option<serde_json::Value> {
    let mut events = serde_json::Map::new();
    for pack in packs.iter().filter(|p| p.enabled && trusted(p)) {
        for hook in &pack.hooks {
            let Some(event) = claude_event(&hook.event) else {
                continue;
            };
            if hook.command.trim().is_empty() {
                continue;
            }
            let handler = serde_json::json!({"type": "command", "command": hook.command});
            let group = if matches!(event, "PreToolUse" | "PostToolUse") {
                serde_json::json!({"matcher": "*", "hooks": [handler]})
            } else {
                serde_json::json!({"hooks": [handler]})
            };
            events
                .entry(event)
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .expect("an array")
                .push(group);
        }
    }
    (!events.is_empty()).then(|| serde_json::json!({ "hooks": events }))
}

/// The driver did not take the servers: each one planned is not given.
pub fn refused(items: &mut [PackItem], family: ProtocolFamily) {
    let harness = Target {
        family,
        remote: false,
        managed: false,
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
