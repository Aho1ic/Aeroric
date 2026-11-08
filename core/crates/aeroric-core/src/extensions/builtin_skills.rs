//! Aeroric's built-in skills (B2 工作配方库, 2026-10-30).
//!
//! Twenty-seven SKILL.md packages vendored byte-for-byte under
//! `core/skills/builtin` (superpowers and K-Dense, MIT; see the NOTICE files)
//! and compiled into this crate. They are written once per content version to
//! `<data-dir>/builtin-skills/<hash>/` and offered to each session without
//! writing the user's own `~/.claude` or `~/.codex`:
//! - a managed home (Aeroric configurations, OMP, DSH) gets symlinks in its
//!   `skills/`;
//! - a direct Claude Code session gets a session-only plugin (`--plugin-dir`);
//! - a direct Codex session, which has no other way, gets marked copies in the
//!   project's `.agents/skills`, kept out of git through `.git/info/exclude`
//!   (owner's Q9 choice).
//!
//! The user's own skill of the same name always wins (Q6); turned-off skills
//! (`settings.json`) are not offered (Q4).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/builtin_skills.rs"));
}

/// File inside a project copy that marks it as Aeroric's; holds the version.
pub const MARKER: &str = ".aeroric-builtin";
/// Beside the marker: the copy's files and their hashes when it was made,
/// so an update can tell whether someone changed it since.
pub const MANIFEST: &str = ".aeroric-builtin-files";
/// Where a copy is built before it is swapped in, outside `.agents/skills`
/// so a half-built one is never loaded as a skill.
const STAGING: &str = ".agents/.aeroric-staging";
const EXCLUDE_BEGIN: &str = "# >>> Aeroric built-in skills (managed) >>>";
const EXCLUDE_END: &str = "# <<< Aeroric built-in skills <<<";
const COMPLETE: &str = ".complete";

/// One entry of `catalog.json`: Aeroric's own Chinese name and line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    /// `develop` or `science`.
    pub group: String,
    pub title: String,
    pub detail: String,
    pub symbol: String,
    #[serde(default)]
    pub needs: Vec<String>,
}

#[derive(Deserialize)]
struct Catalog {
    skills: Vec<SkillInfo>,
}

pub fn catalog() -> Vec<SkillInfo> {
    let raw = embedded::FILES
        .iter()
        .find(|(path, _, _)| *path == "catalog.json")
        .map(|(_, bytes, _)| *bytes)
        .unwrap_or(b"{\"skills\":[]}");
    serde_json::from_slice::<Catalog>(raw)
        .map(|catalog| catalog.skills)
        .unwrap_or_default()
}

/// Short content hash: a new App version with changed skills gets a new folder.
pub fn content_hash() -> String {
    let mut hasher = Sha256::new();
    for (path, bytes, executable) in embedded::FILES {
        hasher.update(path.as_bytes());
        hasher.update([0, u8::from(*executable)]);
        hasher.update(bytes);
    }
    hasher.finalize()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn base_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("builtin-skills")
}

/// Where one skill's folder is inside a materialized root.
pub fn skill_dir(root: &Path, skill: &SkillInfo) -> PathBuf {
    root.join(&skill.group).join(&skill.id)
}

/// One skill a session is offered, wherever it lives: a built-in one, or
/// (B5-3b, 2026-10-31) one Aeroric learned and the user accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub dir: PathBuf,
    /// What a project copy's marker records; a change replaces the copy.
    pub version: String,
}

fn builtin_offers(root: &Path, skills: &[SkillInfo]) -> Vec<Offer> {
    let version = content_hash();
    skills
        .iter()
        .map(|skill| Offer {
            id: skill.id.clone(),
            dir: skill_dir(root, skill),
            version: version.clone(),
        })
        .collect()
}

/// B5-3b: where accepted learned skills live (inside the store, so links to
/// them count as Aeroric's), and the index the memory store keeps.
pub fn learned_dir(data_dir: &Path) -> PathBuf {
    base_dir(data_dir).join("learned")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedEntry {
    pub name: String,
    /// The active version's folder (holds SKILL.md).
    pub dir: PathBuf,
    pub version: u32,
    pub global: bool,
    /// The project it came from; offered under this folder only unless global.
    #[serde(default)]
    pub project_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnedIndex {
    #[serde(default)]
    pub skills: Vec<LearnedEntry>,
}

pub fn write_learned_index(data_dir: &Path, index: &LearnedIndex) -> Result<(), String> {
    let path = learned_dir(data_dir).join("index.json");
    std::fs::create_dir_all(learned_dir(data_dir)).map_err(|e| e.to_string())?;
    let raw = serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?;
    crate::storage::atomic::write_atomic(&path, &raw, crate::storage::atomic::FileMode::Private)
        .map_err(|e| e.to_string())
}

/// The accepted learned skills for a session in `cwd`.
pub fn learned_offers(data_dir: &Path, cwd: Option<&Path>) -> Vec<Offer> {
    let index: LearnedIndex = std::fs::read(learned_dir(data_dir).join("index.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    index
        .skills
        .into_iter()
        .filter(|entry| {
            entry.global
                || matches!((&entry.project_path, cwd), (Some(project), Some(cwd)) if cwd.starts_with(project))
        })
        .filter(|entry| entry.dir.join("SKILL.md").is_file())
        .map(|entry| Offer {
            version: format!("learned-v{}", entry.version),
            id: entry.name,
            dir: entry.dir,
        })
        .collect()
}

/// Writes the embedded skills to `<data-dir>/builtin-skills/<hash>/` unless
/// that version is already complete; returns the folder.
pub fn materialize(data_dir: &Path) -> Result<PathBuf, String> {
    let root = base_dir(data_dir).join(content_hash());
    if root.join(COMPLETE).is_file() {
        return Ok(root);
    }
    for (rel, bytes, executable) in embedded::FILES {
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if *executable { 0o755 } else { 0o644 };
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
        }
        #[cfg(not(unix))]
        let _ = executable;
    }
    std::fs::write(root.join(COMPLETE), content_hash())
        .map_err(|e| format!("cannot finish {}: {e}", root.display()))?;
    Ok(root)
}

/// Which skills the user turned off (default: none, Q4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSettings {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}

fn settings_path(data_dir: &Path) -> PathBuf {
    base_dir(data_dir).join("settings.json")
}

pub fn load_settings(data_dir: &Path) -> SkillSettings {
    std::fs::read(settings_path(data_dir))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

pub fn save_settings(data_dir: &Path, settings: &SkillSettings) -> Result<(), String> {
    let path = settings_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    crate::storage::atomic::write_atomic(&path, &raw, crate::storage::atomic::FileMode::Private)
        .map_err(|e| e.to_string())
}

/// The folders whose skills are the user's own for `harness`, read only.
pub fn user_skill_dirs(harness: &str, cwd: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push_home = |rel: &str| {
        if let Some(home) = home {
            dirs.push(home.join(rel));
        }
    };
    match harness {
        "claude" => push_home(".claude/skills"),
        "codex" => push_home(".codex/skills"),
        _ => {}
    }
    if let Some(cwd) = cwd {
        let project: &[&str] = match harness {
            "claude" => &[".claude/skills"],
            "codex" => &[".agents/skills", ".codex/skills"],
            "omp" => &[".omp/skills", ".agents/skills"],
            "dsh" => &[".dsh/skills", ".agents/skills"],
            _ => &[],
        };
        dirs.extend(project.iter().map(|rel| cwd.join(rel)));
    }
    dirs
}

/// Skill names in `dirs` that are the user's: folders with a SKILL.md that are
/// neither Aeroric's marked copies nor links into the built-in store.
pub fn user_skill_names(dirs: &[PathBuf]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join(MARKER).exists() || points_into_store(&path) {
                continue;
            }
            if path.join("SKILL.md").is_file() {
                names.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names
}

fn points_into_store(path: &Path) -> bool {
    std::fs::read_link(path)
        .map(|target| {
            target
                .components()
                .any(|c| c.as_os_str() == "builtin-skills")
        })
        .unwrap_or(false)
}

/// What a session should be offered: enabled, and not shadowed by the user's
/// own skill of the same name.
pub fn offered(data_dir: &Path, user_names: &BTreeSet<String>) -> Vec<SkillInfo> {
    let settings = load_settings(data_dir);
    catalog()
        .into_iter()
        .filter(|skill| !settings.disabled.contains(&skill.id) && !user_names.contains(&skill.id))
        .collect()
}

/// Keeps `skills_dir` holding links to exactly `skills` (minus `skip`) among
/// the entries Aeroric owns there. Anything else in the folder is left alone.
pub fn link_into(
    skills_dir: &Path,
    root: &Path,
    skills: &[SkillInfo],
    skip: &BTreeSet<String>,
) -> Result<(), String> {
    link_offers(skills_dir, &builtin_offers(root, skills), skip)
}

/// [`link_into`] for any offers.
pub fn link_offers(
    skills_dir: &Path,
    offers: &[Offer],
    skip: &BTreeSet<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(skills_dir)
        .map_err(|e| format!("cannot create {}: {e}", skills_dir.display()))?;
    let wanted: Vec<&Offer> = offers.iter().filter(|s| !skip.contains(&s.id)).collect();
    for skill in &wanted {
        let link = skills_dir.join(&skill.id);
        let target = skill.dir.clone();
        match std::fs::read_link(&link) {
            Ok(existing) if existing == target => continue,
            Ok(_) if points_into_store(&link) => {
                std::fs::remove_file(&link).map_err(|e| e.to_string())?;
            }
            Ok(_) => continue,                   // someone else's link
            Err(_) if link.exists() => continue, // a real folder: the user's
            Err(_) => {}
        }
        symlink_dir(&target, &link)?;
    }
    let keep: BTreeSet<&str> = wanted.iter().map(|s| s.id.as_str()).collect();
    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if points_into_store(&path) && !keep.contains(name.as_str()) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| format!("cannot link {}: {e}", link.display()))
}

#[cfg(not(unix))]
fn symlink_dir(_: &Path, link: &Path) -> Result<(), String> {
    Err(format!(
        "cannot link {}: unsupported platform",
        link.display()
    ))
}

/// A session-only Claude Code plugin named `aeroric` holding links to
/// `skills`; one folder per distinct set so concurrent sessions don't race.
pub fn claude_plugin(
    data_dir: &Path,
    root: &Path,
    skills: &[SkillInfo],
) -> Result<PathBuf, String> {
    claude_plugin_offers(data_dir, &builtin_offers(root, skills))
}

/// [`claude_plugin`] for any offers.
pub fn claude_plugin_offers(data_dir: &Path, offers: &[Offer]) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    hasher.update(content_hash());
    for skill in offers {
        hasher.update(skill.id.as_bytes());
        hasher.update([0]);
        hasher.update(skill.version.as_bytes());
        hasher.update([0]);
    }
    let key: String = hasher.finalize()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let dir = base_dir(data_dir).join("claude-plugins").join(key);
    let manifest = dir.join(".claude-plugin").join("plugin.json");
    if !manifest.is_file() {
        std::fs::create_dir_all(manifest.parent().unwrap()).map_err(|e| e.to_string())?;
        let body = serde_json::json!({
            "name": "aeroric",
            "description": "Aeroric 内置技能（工作配方）",
            "version": content_hash(),
        });
        std::fs::write(&manifest, serde_json::to_vec_pretty(&body).unwrap())
            .map_err(|e| e.to_string())?;
    }
    link_offers(&dir.join("skills"), offers, &BTreeSet::new())?;
    Ok(dir)
}

/// Ids of Aeroric's marked copies in `cwd/.agents/skills`.
pub fn project_copies(cwd: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(cwd.join(".agents/skills")) {
        for entry in entries.flatten() {
            if entry.path().join(MARKER).is_file() {
                ids.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    ids
}

/// Brings the project's marked copies in line with `skills` (Q14): missing
/// ones are copied, outdated ones replaced, turned-off ones handed to
/// `remove` (the host moves them to the Trash, Q16). A folder without the
/// marker is the user's and never touched. Updates `.git/info/exclude` (Q15).
pub fn sync_project(
    cwd: &Path,
    root: &Path,
    skills: &[SkillInfo],
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    sync_offers(cwd, &builtin_offers(root, skills), remove)
}

/// [`sync_project`] for any offers; each copy's marker holds its version.
pub fn sync_offers(
    cwd: &Path,
    skills: &[Offer],
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let dir = cwd.join(".agents/skills");
    let existing = project_copies(cwd);
    if skills.is_empty() && existing.is_empty() {
        return Ok(());
    }
    for skill in skills {
        place(cwd, skill, remove)?;
    }
    let wanted: BTreeSet<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    for id in &existing {
        if !wanted.contains(id.as_str()) {
            remove(&dir.join(id))?;
        }
    }
    write_exclude(cwd, &project_copies(cwd))
}

/// Learned skills only, as marked copies in `cwd/.agents/skills`: the
/// built-in copies a direct Codex session keeps there are left alone, and
/// only learned copies that are no longer wanted go.
pub fn sync_learned(
    cwd: &Path,
    learned: &[Offer],
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let dir = cwd.join(".agents/skills");
    let stale: Vec<String> = project_copies(cwd)
        .into_iter()
        .filter(|id| {
            std::fs::read_to_string(dir.join(id).join(MARKER))
                .is_ok_and(|v| v.trim().starts_with("learned-"))
                && !learned.iter().any(|o| &o.id == id)
        })
        .collect();
    if learned.is_empty() && stale.is_empty() {
        return Ok(());
    }
    for skill in learned {
        place(cwd, skill, remove)?;
    }
    for id in stale {
        remove(&dir.join(id))?;
    }
    write_exclude(cwd, &project_copies(cwd))
}

/// Removes every marked copy and the exclude block; returns how many.
pub fn remove_from_project(
    cwd: &Path,
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<usize, String> {
    let copies = project_copies(cwd);
    for id in &copies {
        remove(&cwd.join(".agents/skills").join(id))?;
    }
    write_exclude(cwd, &BTreeSet::new())?;
    Ok(copies.len())
}

/// Brings `skill`'s copy in `cwd` to its version: one at that version and a
/// folder without the marker (the user's) are left alone.
fn place(
    cwd: &Path,
    skill: &Offer,
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let target = cwd.join(".agents/skills").join(&skill.id);
    if target.exists() {
        match std::fs::read_to_string(target.join(MARKER)) {
            Ok(version) if version.trim() == skill.version => return Ok(()),
            Ok(_) => {}
            Err(_) => return Ok(()), // the user's own folder
        }
    }
    install(cwd, &target, skill, remove)
}

/// Puts `skill` at `target` whole or not at all (C 批 2026-11-08): it is
/// built in [`STAGING`], marked, then swapped in. It used to be deleted and
/// copied in place: a copy that failed halfway was left without its marker,
/// taken for the user's folder and never repaired, and edits made to the
/// copy were silently gone. Now a failed build leaves the old copy as it
/// was, and an old copy changed since it was made goes to `remove` (the
/// Trash) rather than being deleted.
fn install(
    cwd: &Path,
    target: &Path,
    skill: &Offer,
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let staging = cwd.join(STAGING);
    let work = staging.join(uuid::Uuid::new_v4().simple().to_string());
    let fresh = work.join("new").join(&skill.id);
    let built = (|| {
        copy_tree(&skill.dir, &fresh)?;
        let manifest = manifest_of(&fresh)?;
        std::fs::write(fresh.join(MANIFEST), manifest).map_err(|e| e.to_string())?;
        std::fs::write(fresh.join(MARKER), &skill.version).map_err(|e| e.to_string())?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if !target.exists() {
            return std::fs::rename(&fresh, target).map_err(|e| e.to_string());
        }
        let old = work.join("old").join(&skill.id);
        std::fs::create_dir_all(work.join("old")).map_err(|e| e.to_string())?;
        std::fs::rename(target, &old).map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(&fresh, target) {
            let _ = std::fs::rename(&old, target);
            return Err(error.to_string());
        }
        if changed_since_made(&old) {
            // An edited copy that will not go to the Trash stays where it is
            // now; clearing the staging area below would have lost it.
            if let Err(error) = remove(&old) {
                return Err(format!("{error}；改过的旧副本留在 {}", old.display()));
            }
            Ok(())
        } else {
            std::fs::remove_dir_all(&old).map_err(|e| e.to_string())
        }
    })();
    if work.join("old").join(&skill.id).exists() && built.is_err() {
        return built;
    }
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir(&staging);
    built
}

/// One `hash  path` line per file under `dir` (but the marker files),
/// sorted.
fn manifest_of(dir: &Path) -> Result<String, String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if dir == base && (name == MARKER || name == MANIFEST) {
                continue;
            }
            if path.is_dir() {
                walk(&path, base, out)?;
            } else {
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let hash: String = Sha256::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let relative = path.strip_prefix(base).unwrap_or(&path);
                out.push(format!("{hash}  {}", relative.display()));
            }
        }
        Ok(())
    }
    let mut lines = Vec::new();
    walk(dir, dir, &mut lines)?;
    lines.sort();
    Ok(lines.join("\n") + "\n")
}

/// Whether a copy's files differ from its manifest. A copy made before
/// manifests existed cannot tell, and counts as unchanged.
fn changed_since_made(copy: &Path) -> bool {
    match std::fs::read_to_string(copy.join(MANIFEST)) {
        Ok(made) => manifest_of(copy).map_or(true, |now| now != made),
        Err(_) => false,
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)
                .map_err(|e| format!("cannot copy {}: {e}", source.display()))?;
        }
    }
    Ok(())
}

/// The repository's shared `info/exclude`, or None outside git.
fn exclude_file(cwd: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let common = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let common = if common.is_absolute() {
        common
    } else {
        cwd.join(common)
    };
    Some(common.join("info").join("exclude"))
}

/// Rewrites Aeroric's block in `info/exclude` to list exactly `ids`; the rest
/// of the file is kept as it was.
fn write_exclude(cwd: &Path, ids: &BTreeSet<String>) -> Result<(), String> {
    let Some(path) = exclude_file(cwd) else {
        return Ok(());
    };
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let next = exclude_with(&current, ids, &relative_skills_dir(cwd));
    if next == current {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, next).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// `.agents/skills` relative to the repository top, as exclude patterns need.
fn relative_skills_dir(cwd: &Path) -> String {
    let top = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-prefix"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    format!("/{top}.agents/skills")
}

/// Pure: `current` with Aeroric's block replaced by one line per id.
pub fn exclude_with(current: &str, ids: &BTreeSet<String>, skills_dir: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in current.lines() {
        if line == EXCLUDE_BEGIN {
            inside = true;
        } else if line == EXCLUDE_END {
            inside = false;
        } else if !inside {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !ids.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(EXCLUDE_BEGIN);
        for id in ids {
            out.push_str(&format!("\n{skills_dir}/{id}/"));
        }
        out.push('\n');
        out.push_str(EXCLUDE_END);
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Moves Aeroric's own copy to the Trash (Q16): never a permanent delete.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("cannot move {} to the Trash: {e}", path.display()))
}

/// Where a session finds the skills.
pub enum Mount<'a> {
    /// A home Aeroric manages (configurations, OMP, DSH): links in `<home>/skills`.
    Home(&'a Path),
    /// A direct Claude Code session: a session-only plugin.
    ClaudePlugin,
    /// A direct Codex session: marked copies in the project (owner's Q9).
    CodexProject,
    /// CU 2026-10-31: a configured Codex session. Its server is shared by
    /// every project, so its home holds only the built-in skills; the
    /// project's learned skills go into the project as marked copies.
    CodexProjectLearned,
}

/// Prepares the skills for one launch of `harness` in `cwd`; returns the
/// plugin folder for [`Mount::ClaudePlugin`].
pub fn mount(
    data_dir: &Path,
    harness: &str,
    cwd: Option<&Path>,
    mount: Mount,
) -> Result<Option<PathBuf>, String> {
    let root = materialize(data_dir)?;
    let home = crate::settings::home_dir();
    let user = user_skill_names(&user_skill_dirs(harness, cwd, home.as_deref()));
    let mut offers = builtin_offers(&root, &offered(data_dir, &user));
    // B5-3b: accepted learned skills for this folder; the user's own of the
    // same name still wins, and a built-in name is never shadowed.
    let taken: BTreeSet<String> = offers.iter().map(|o| o.id.clone()).collect();
    // A home shared by every project (no cwd) never holds learned skills.
    let learned: Vec<Offer> = if matches!(mount, Mount::Home(_)) && cwd.is_none() {
        Vec::new()
    } else {
        learned_offers(data_dir, cwd)
            .into_iter()
            .filter(|o| !user.contains(&o.id) && !taken.contains(&o.id))
            .collect()
    };
    if let (Mount::CodexProjectLearned, Some(cwd)) = (&mount, cwd) {
        sync_learned(cwd, &learned, &move_to_trash)?;
        return Ok(None);
    }
    offers.extend(learned);
    match mount {
        Mount::Home(dir) => {
            // Q17: OMP / DSH also read the project's copies.
            let skip = match (harness, cwd) {
                ("omp" | "dsh", Some(cwd)) => project_copies(cwd),
                _ => BTreeSet::new(),
            };
            link_offers(&dir.join("skills"), &offers, &skip)?;
            Ok(None)
        }
        Mount::ClaudePlugin => claude_plugin_offers(data_dir, &offers).map(Some),
        Mount::CodexProject => {
            let Some(cwd) = cwd else { return Ok(None) };
            sync_offers(cwd, &offers, &move_to_trash)?;
            Ok(None)
        }
        Mount::CodexProjectLearned => Ok(None),
    }
}

/// The invocation a harness understands for a skill (Q13).
pub fn invocation(harness: &str, id: &str, direct_claude: bool) -> String {
    match harness {
        "codex" => format!("${id}"),
        "claude" if direct_claude => format!("/aeroric:{id}"),
        _ => format!("/{id}"),
    }
}

#[cfg(test)]
#[path = "builtin_skills_tests.rs"]
mod tests;
