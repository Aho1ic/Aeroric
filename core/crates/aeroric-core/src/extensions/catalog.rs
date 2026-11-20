//! C7 (2026-11-20): the skill catalog and the record of what each launch
//! of a session mounted.
//!
//! Four sources keep their own storage — the App's built-in bundle, learned
//! skills (B5-3b), the user's own folders, Claude Code plugins — and are
//! projected here into [`SkillDefinition`]. [`plan`] is what one launch is
//! given, by the same rules [`super::builtin_skills::mount`] follows, plus
//! what the agent reads itself; [`MountLog`] keeps it per task.
//!
//! Only reads: nothing here writes a skill or runs one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aeroric_protocol::skills::{
    MountReason, MountedSkill, SkillDefinition, SkillLaunch, SkillSource,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use super::builtin_skills::{self as builtin, LearnedIndex};
use crate::storage::launch_log;

/// The harnesses Aeroric gives skills to.
pub const HARNESSES: [&str; 4] = ["claude", "codex", "omp", "dsh"];
/// What a built-in skill's version reads as.
pub const BUILTIN_VERSION: &str = "随应用内置";
const SUMMARY_CHARS: usize = 200;

/// A SKILL.md's `description` (front matter) and a short hash of the file.
fn skill_md(dir: &Path) -> (String, String) {
    let Ok(bytes) = std::fs::read(dir.join("SKILL.md")) else {
        return (String::new(), String::new());
    };
    let text = String::from_utf8_lossy(&bytes);
    let summary = text
        .strip_prefix("---")
        .and_then(|rest| rest.split("\n---").next())
        .and_then(|front| {
            front.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("description:")
                    .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
            })
        })
        .unwrap_or_default()
        .chars()
        .take(SUMMARY_CHARS)
        .collect();
    (summary, short_hash(&bytes))
}

fn short_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn learned_index(data_dir: &Path) -> LearnedIndex {
    std::fs::read(builtin::learned_dir(data_dir).join("index.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// The user's own skills for `harness`: `(name, folder)`, first folder
/// first; Aeroric's copies and links are not theirs.
fn user_skills(harness: &str, cwd: Option<&Path>, home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for dir in builtin::user_skill_dirs(harness, cwd, home) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<(String, PathBuf)> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                !path.join(builtin::MARKER).exists()
                    && !builtin::points_into_store(path)
                    && path.join("SKILL.md").is_file()
            })
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((name, path))
            })
            .collect();
        found.sort();
        for (name, path) in found {
            if !out.iter().any(|(seen, _)| *seen == name) {
                out.push((name, path));
            }
        }
    }
    out
}

/// Skills inside the Claude Code plugins the user installed:
/// `(plugin:skill, folder, plugin version)`. Read from Claude Code's own
/// `installed_plugins.json`; a format it changes reads as none.
fn plugin_skills(home: Option<&Path>) -> Vec<(String, PathBuf, String)> {
    let Some(home) = home else { return Vec::new() };
    let Some(doc) = std::fs::read(home.join(".claude/plugins/installed_plugins.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
    else {
        return Vec::new();
    };
    let Some(plugins) = doc.get("plugins").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in plugins {
        let plugin = key.split('@').next().unwrap_or(key);
        // Version 2 lists installs; version 1 had one.
        let installs: Vec<&serde_json::Value> = match value {
            serde_json::Value::Array(list) => list.iter().collect(),
            other => vec![other],
        };
        for install in installs {
            let Some(root) = install.get("installPath").and_then(|p| p.as_str()) else {
                continue;
            };
            let version = install
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let Ok(entries) = std::fs::read_dir(Path::new(root).join("skills")) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("SKILL.md").is_file())
                .collect();
            found.sort();
            for dir in found {
                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                let id = format!("{plugin}:{name}");
                if !out
                    .iter()
                    .any(|(seen, _, _): &(String, PathBuf, String)| *seen == id)
                {
                    out.push((id, dir, version.clone()));
                }
            }
        }
    }
    out
}

/// Every skill from every source, by name; a name in more than one source
/// is listed once per source, the one a session gets `active`, the others
/// say what wins. `cwd`: the project asked about (learned skills of other
/// projects say where they apply).
pub fn catalog(data_dir: &Path, cwd: Option<&Path>, home: Option<&Path>) -> Vec<SkillDefinition> {
    let settings = builtin::load_settings(data_dir);
    let users: Vec<(&str, Vec<(String, PathBuf)>)> = HARNESSES
        .iter()
        .map(|h| (*h, user_skills(h, cwd, home)))
        .collect();
    let user_of = |name: &str| -> Vec<&str> {
        users
            .iter()
            .filter(|(_, skills)| skills.iter().any(|(n, _)| n == name))
            .map(|(h, _)| *h)
            .collect()
    };
    let mut out = Vec::new();
    let bundle = builtin::content_hash();
    let builtin_ids: BTreeSet<String> = builtin::catalog().into_iter().map(|s| s.id).collect();
    for skill in builtin::catalog() {
        let off = settings.disabled.contains(&skill.id);
        let shadowed = user_of(&skill.id);
        let note = if off {
            Some("已关闭".to_string())
        } else if !shadowed.is_empty() {
            Some(format!("在 {} 里被你自己的同名技能盖过", names(&shadowed)))
        } else {
            None
        };
        out.push(SkillDefinition {
            id: skill.id.clone(),
            name: skill.title.clone(),
            source: SkillSource::Builtin,
            version: BUILTIN_VERSION.into(),
            content_hash: bundle.clone(),
            path: None,
            embedded: true,
            summary: skill.detail.clone(),
            harnesses: HARNESSES
                .iter()
                .filter(|h| !shadowed.contains(h))
                .map(|h| h.to_string())
                .collect(),
            active: !off && shadowed.len() < HARNESSES.len(),
            note,
        });
    }
    for entry in learned_index(data_dir).skills {
        let (summary, hash) = skill_md(&entry.dir);
        let here = entry.global
            || match (&entry.project_path, cwd) {
                (Some(project), Some(cwd)) => cwd.starts_with(project),
                (_, None) => true,
                _ => false,
            };
        let shadowed = user_of(&entry.name);
        let note = if builtin_ids.contains(&entry.name) {
            Some("被同名的内置技能盖过".to_string())
        } else if !shadowed.is_empty() {
            Some(format!("在 {} 里被你自己的同名技能盖过", names(&shadowed)))
        } else if !entry.global {
            entry
                .project_path
                .as_ref()
                .map(|p| format!("只在项目 {} 里", p.display()))
        } else {
            None
        };
        out.push(SkillDefinition {
            id: entry.name.clone(),
            name: entry.name.clone(),
            source: SkillSource::Learned,
            version: format!("v{}", entry.version),
            content_hash: hash,
            path: Some(entry.dir.display().to_string()),
            embedded: false,
            summary,
            harnesses: HARNESSES.iter().map(|h| h.to_string()).collect(),
            active: here && !builtin_ids.contains(&entry.name) && shadowed.len() < HARNESSES.len(),
            note,
        });
    }
    // One entry per folder, with every harness that reads it.
    let mut folders: BTreeMap<PathBuf, (String, Vec<String>)> = BTreeMap::new();
    for (harness, skills) in &users {
        for (name, path) in skills {
            folders
                .entry(path.clone())
                .or_insert_with(|| (name.clone(), Vec::new()))
                .1
                .push(harness.to_string());
        }
    }
    for (path, (name, harnesses)) in folders {
        let (summary, hash) = skill_md(&path);
        out.push(SkillDefinition {
            id: name.clone(),
            name,
            source: SkillSource::User,
            version: String::new(),
            content_hash: hash,
            path: Some(path.display().to_string()),
            embedded: false,
            summary,
            harnesses,
            active: true,
            note: None,
        });
    }
    for (id, dir, version) in plugin_skills(home) {
        let (summary, hash) = skill_md(&dir);
        out.push(SkillDefinition {
            name: id.clone(),
            id,
            source: SkillSource::Plugin,
            version,
            content_hash: hash,
            path: Some(dir.display().to_string()),
            embedded: false,
            summary,
            harnesses: vec!["claude".into()],
            active: true,
            note: None,
        });
    }
    out.sort_by(|a, b| (&a.id, a.source).cmp(&(&b.id, b.source)));
    out
}

fn names(harnesses: &[&str]) -> String {
    harnesses
        .iter()
        .map(|h| match *h {
            "claude" => "Claude",
            "codex" => "Codex",
            "omp" => "OMP",
            "dsh" => "DSH",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("、")
}

/// What one launch of `harness` in `cwd` is given: the built-in skills
/// that are on and not shadowed, the accepted learned skills for `cwd`
/// (the user's own and built-in names win), the user's own skills, and —
/// for Claude — the plugins' skills.
pub fn plan(
    data_dir: &Path,
    harness: &str,
    cwd: Option<&Path>,
    home: Option<&Path>,
) -> Vec<MountedSkill> {
    let user = user_skills(harness, cwd, home);
    let user_names: BTreeSet<String> = user.iter().map(|(n, _)| n.clone()).collect();
    let bundle = builtin::content_hash();
    let mut out: Vec<MountedSkill> = builtin::offered(data_dir, &user_names)
        .into_iter()
        .map(|skill| MountedSkill {
            skill_id: skill.id,
            source: SkillSource::Builtin,
            version: BUILTIN_VERSION.into(),
            content_hash: bundle.clone(),
            reason: MountReason::Default,
        })
        .collect();
    let taken: BTreeSet<String> = out.iter().map(|m| m.skill_id.clone()).collect();
    for offer in builtin::learned_offers(data_dir, cwd) {
        if user_names.contains(&offer.id) || taken.contains(&offer.id) {
            continue;
        }
        out.push(MountedSkill {
            content_hash: skill_md(&offer.dir).1,
            version: offer
                .version
                .strip_prefix("learned-")
                .unwrap_or(&offer.version)
                .to_string(),
            skill_id: offer.id,
            source: SkillSource::Learned,
            reason: MountReason::Accepted,
        });
    }
    for (name, path) in &user {
        out.push(MountedSkill {
            skill_id: name.clone(),
            source: SkillSource::User,
            version: String::new(),
            content_hash: skill_md(path).1,
            reason: MountReason::UserFolder,
        });
    }
    if harness == "claude" {
        for (id, dir, version) in plugin_skills(home) {
            out.push(MountedSkill {
                skill_id: id,
                source: SkillSource::Plugin,
                version,
                content_hash: skill_md(&dir).1,
                reason: MountReason::Plugin,
            });
        }
    }
    out.sort_by(|a, b| (a.source, &a.skill_id).cmp(&(b.source, &b.skill_id)));
    out
}

/// B7: one launch's skills as a rerun's inputs compare them — how many,
/// and a fingerprint of every name and content.
pub fn fingerprint(skills: &[MountedSkill]) -> String {
    let mut lines: Vec<String> = skills
        .iter()
        .map(|s| format!("{}@{}", s.skill_id, s.content_hash))
        .collect();
    lines.sort();
    format!(
        "{} 个 · {}",
        skills.len(),
        short_hash(lines.join("\n").as_bytes())
    )
}

/// The harness a configuration's agent is.
pub fn harness_of(agent: &str) -> &str {
    HARNESSES
        .iter()
        .find(|h| agent.contains(*h))
        .copied()
        .unwrap_or(agent)
}

/// What each launch of a task mounted, newest last, under
/// `<data>/skill-mounts/<task>.json`.
#[derive(Default)]
pub struct MountLog {
    data_dir: Mutex<Option<PathBuf>>,
}

/// The folder under the data folder.
const FOLDER: &str = "skill-mounts";

impl MountLog {
    pub fn set_dir(&self, data_dir: PathBuf) {
        *self.data_dir.lock() = Some(data_dir);
    }

    fn file(&self, task_id: &str) -> Option<PathBuf> {
        let dir = self.data_dir.lock().clone()?;
        Some(launch_log::file(&dir, FOLDER, task_id))
    }

    /// A launch of `harness` in `cwd` starts: what it is given is kept.
    /// Never fails the launch: a write that fails is only logged.
    pub fn note(
        &self,
        task_id: &str,
        harness: &str,
        cwd: Option<&Path>,
        home: Option<&Path>,
        at_ms: u64,
    ) {
        let Some(data_dir) = self.data_dir.lock().clone() else {
            return;
        };
        let launch = SkillLaunch {
            at_ms,
            harness: harness.to_string(),
            skills: plan(&data_dir, harness, cwd, home),
        };
        let path = launch_log::file(&data_dir, FOLDER, task_id);
        if let Err(error) = launch_log::push(&path, launch) {
            eprintln!("[aeroric] skills mounted by {task_id} not kept: {error}");
        }
    }

    /// Every launch kept for the task, oldest first.
    pub fn launches(&self, task_id: &str) -> Vec<SkillLaunch> {
        self.file(task_id)
            .map(|path| launch_log::read(&path))
            .unwrap_or_default()
    }

    /// B7: the launch a turn that started at `at_ms` ran under — the last
    /// one before it (or, with none before, the first).
    pub fn at(&self, task_id: &str, at_ms: u64) -> Option<SkillLaunch> {
        let launches = self.launches(task_id);
        launches
            .iter()
            .rev()
            .find(|l| l.at_ms <= at_ms)
            .or_else(|| launches.first())
            .cloned()
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
