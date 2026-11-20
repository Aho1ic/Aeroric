//! H6 step 3 (2026-11-21): managing capability packs — what the 能力包 page
//! shows and changes. Writes go to `<data>/packs/<id>/pack.json` (0600: a
//! server's environment can hold keys); nothing here runs a pack.

use std::path::Path;

use aeroric_protocol::packs::{Pack, PackItem, PackServer, PackSource};
use aeroric_protocol::ProtocolFamily;
use serde::Serialize;

use super::packs::{self, Target};

/// What a masked environment value reads as; saving it back keeps the old one.
pub const MASK: &str = "••••••••";

/// Where a pack the user wrote in Aeroric comes from.
pub const OWN_KIND: &str = "local";
pub const OWN_ORIGIN: &str = "aeroric";

/// The six places a launch can be, as the page names them.
pub fn targets() -> Vec<(&'static str, Target)> {
    let at = |family, managed| Target {
        family,
        remote: false,
        managed,
    };
    vec![
        ("Claude", at(ProtocolFamily::Claude, false)),
        ("Claude（Agent 预设）", at(ProtocolFamily::Claude, true)),
        ("Codex", at(ProtocolFamily::Codex, false)),
        ("Codex（Agent 预设）", at(ProtocolFamily::Codex, true)),
        ("OMP", at(ProtocolFamily::Omp, true)),
        ("DSH", at(ProtocolFamily::Dsh, true)),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceItems {
    pub place: String,
    pub items: Vec<PackItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    /// The manifest, environment values masked.
    pub pack: Pack,
    pub trusted: bool,
    /// Every item at each of the six places.
    pub places: Vec<PlaceItems>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Broken {
    pub folder: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub packs: Vec<PackView>,
    pub broken: Vec<Broken>,
}

fn masked(mut pack: Pack) -> Pack {
    for server in &mut pack.mcp_servers {
        server.env = super::mcp::mask_env(&server.env);
    }
    pack
}

/// Every pack with what each place would be given, as the plan decides it.
pub fn overview(data_dir: &Path) -> Overview {
    let (found, broken) = packs::load(data_dir);
    let plans: Vec<(&str, packs::Plan)> = targets()
        .into_iter()
        .map(|(place, target)| (place, packs::plan(data_dir, &found, target)))
        .collect();
    let views = found
        .iter()
        .map(|pack| PackView {
            pack: masked(pack.clone()),
            trusted: packs::trusted(pack),
            places: plans
                .iter()
                .map(|(place, plan)| PlaceItems {
                    place: place.to_string(),
                    items: plan
                        .items
                        .iter()
                        .filter(|i| i.pack == pack.id)
                        .cloned()
                        .collect(),
                })
                .collect(),
        })
        .collect();
    Overview {
        packs: views,
        broken: broken
            .into_iter()
            .map(|(folder, reason)| Broken { folder, reason })
            .collect(),
    }
}

fn pack_dir(data_dir: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    if !packs::valid_id(id) {
        return Err("id 只能用字母、数字、- 和 _".into());
    }
    Ok(packs::dir(data_dir).join(id))
}

fn read(data_dir: &Path, id: &str) -> Result<Pack, String> {
    let file = pack_dir(data_dir, id)?.join(packs::MANIFEST);
    let text = std::fs::read_to_string(&file).map_err(|_| format!("没有能力包 {id}"))?;
    serde_json::from_str(&text).map_err(|e| format!("能力包 {id} 的清单读不了：{e}"))
}

fn write(data_dir: &Path, pack: &Pack) -> Result<(), String> {
    let file = pack_dir(data_dir, &pack.id)?.join(packs::MANIFEST);
    let text = serde_json::to_string_pretty(pack).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&file, &format!("{text}\n"))
}

/// An MCP server the user adds in Aeroric: a pack of its own, named after
/// it, trusted because the user wrote it. Saving one that exists replaces
/// it; an environment value left masked keeps what it was.
pub fn save_server(data_dir: &Path, server: PackServer) -> Result<Pack, String> {
    let name = server.name.trim().to_string();
    let dir = pack_dir(data_dir, &name)?;
    let existing = read(data_dir, &name).ok();
    if let Some(pack) = &existing {
        if pack.source.kind != OWN_KIND || pack.source.origin != OWN_ORIGIN {
            return Err(format!(
                "已有一个叫 {name} 的能力包，不是在这里添加的服务器"
            ));
        }
    } else if dir.exists() {
        return Err(format!("{} 已存在", dir.display()));
    }
    let old_env = existing
        .as_ref()
        .and_then(|p| p.mcp_servers.first())
        .map(|s| s.env.clone())
        .unwrap_or_default();
    let mut server = PackServer {
        name: name.clone(),
        command: server.command.trim().to_string(),
        ..server
    };
    for (key, value) in server.env.iter_mut() {
        if value == MASK {
            *value = old_env.get(key).cloned().unwrap_or_default();
        }
    }
    super::mcp::validate_server(&server.name, &server.command, &server.env)?;
    let pack = Pack {
        id: name.clone(),
        name: name.clone(),
        description: server.description.clone(),
        source: PackSource {
            kind: OWN_KIND.into(),
            origin: OWN_ORIGIN.into(),
            revision: None,
        },
        confirmed: true,
        enabled: existing.as_ref().is_none_or(|p| p.enabled),
        mcp_servers: vec![server],
        ..Pack::default()
    };
    write(data_dir, &pack)?;
    Ok(masked(pack))
}

/// Copy a pack folder (holding `pack.json`) in. Its source becomes the
/// folder, whatever the manifest claims, and it starts unconfirmed: its
/// servers and handlers run only after the user confirms it.
pub fn import(data_dir: &Path, folder: &Path) -> Result<Pack, String> {
    let text = std::fs::read_to_string(folder.join(packs::MANIFEST))
        .map_err(|_| format!("{} 里没有 {}", folder.display(), packs::MANIFEST))?;
    let mut pack: Pack = serde_json::from_str(&text)
        .map_err(|e| format!("{} 不是有效的能力包清单：{e}", packs::MANIFEST))?;
    let dir = pack_dir(data_dir, &pack.id)?;
    if dir.exists() {
        return Err(format!("已有一个 id 为 {} 的能力包", pack.id));
    }
    pack.source = PackSource {
        kind: OWN_KIND.into(),
        origin: folder.to_string_lossy().into_owned(),
        revision: pack.source.revision.take(),
    };
    pack.confirmed = false;
    let staging =
        packs::dir(data_dir).join(format!(".importing-{}", uuid::Uuid::new_v4().simple()));
    let copied = super::builtin_skills::copy_tree(folder, &staging)
        .and_then(|()| write_at(&staging, &pack))
        .and_then(|()| std::fs::rename(&staging, &dir).map_err(|e| e.to_string()));
    if let Err(why) = copied {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(why);
    }
    Ok(masked(pack))
}

fn write_at(dir: &Path, pack: &Pack) -> Result<(), String> {
    let text = serde_json::to_string_pretty(pack).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&dir.join(packs::MANIFEST), &format!("{text}\n"))
}

pub fn set_enabled(data_dir: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut pack = read(data_dir, id)?;
    pack.enabled = enabled;
    write(data_dir, &pack)
}

/// The user confirms (or withdraws) a pack's source.
pub fn set_confirmed(data_dir: &Path, id: &str, confirmed: bool) -> Result<(), String> {
    let mut pack = read(data_dir, id)?;
    pack.confirmed = confirmed;
    write(data_dir, &pack)
}

/// Remove a pack's folder through `discard` (the host moves it to the Trash).
pub fn remove(
    data_dir: &Path,
    id: &str,
    discard: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let dir = pack_dir(data_dir, id)?;
    if !dir.is_dir() {
        return Err(format!("没有能力包 {id}"));
    }
    discard(&dir)
}

#[cfg(test)]
#[path = "pack_store_tests.rs"]
mod tests;
