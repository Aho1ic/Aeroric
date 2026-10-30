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
    std::fs::create_dir_all(skills_dir)
        .map_err(|e| format!("cannot create {}: {e}", skills_dir.display()))?;
    let wanted: Vec<&SkillInfo> = skills.iter().filter(|s| !skip.contains(&s.id)).collect();
    for skill in &wanted {
        let link = skills_dir.join(&skill.id);
        let target = skill_dir(root, skill);
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
    let mut hasher = Sha256::new();
    hasher.update(content_hash());
    for skill in skills {
        hasher.update(skill.id.as_bytes());
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
    link_into(&dir.join("skills"), root, skills, &BTreeSet::new())?;
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
    let dir = cwd.join(".agents/skills");
    let existing = project_copies(cwd);
    if skills.is_empty() && existing.is_empty() {
        return Ok(());
    }
    let hash = content_hash();
    for skill in skills {
        let target = dir.join(&skill.id);
        let marker = target.join(MARKER);
        if target.exists() {
            match std::fs::read_to_string(&marker) {
                Ok(version) if version.trim() == hash => continue,
                Ok(_) => std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?,
                Err(_) => continue, // the user's own folder
            }
        }
        copy_tree(&skill_dir(root, skill), &target)?;
        std::fs::write(&marker, &hash).map_err(|e| e.to_string())?;
    }
    let wanted: BTreeSet<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    for id in &existing {
        if !wanted.contains(id.as_str()) {
            remove(&dir.join(id))?;
        }
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
    let offered = offered(data_dir, &user);
    match mount {
        Mount::Home(dir) => {
            // Q17: OMP / DSH also read the project's copies.
            let skip = match (harness, cwd) {
                ("omp" | "dsh", Some(cwd)) => project_copies(cwd),
                _ => BTreeSet::new(),
            };
            link_into(&dir.join("skills"), &root, &offered, &skip)?;
            Ok(None)
        }
        Mount::ClaudePlugin => claude_plugin(data_dir, &root, &offered).map(Some),
        Mount::CodexProject => {
            let Some(cwd) = cwd else { return Ok(None) };
            sync_project(cwd, &root, &offered, &move_to_trash)?;
            Ok(None)
        }
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
