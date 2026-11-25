//! 2026-11-24 设置 › 存储: how much disk Aeroric's data takes, by kind.
//! Read only: it walks folders and adds up what they hold on disk, it never
//! changes or deletes anything. A symlink counts as itself, never what it
//! points at, so a link into a project or the home folder is not added in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The kinds the page shows, in this order; anything in the data folder not
/// named below is 其他.
pub const GROUPS: &[(&str, &str)] = &[
    ("sessions", "会话与 Agent"),
    ("memory", "记忆与索引"),
    ("references", "参考代码库索引"),
    ("worktrees", "工作副本"),
    ("logs", "日志"),
    ("remotePackages", "远程主机的程序包"),
    ("other", "其他"),
];

/// Which group a top-level entry of the data folder belongs to.
pub fn group_of(name: &str) -> &'static str {
    match name {
        "agent-homes"
        | "projects"
        | "turns"
        | "checkpoints"
        | "discussions"
        | "delegations"
        | "reviews"
        | "reruns"
        | "events"
        | "terminal-history"
        | "artifacts"
        | "delivery.db"
        | "task-submissions.json"
        | "task-operations.json"
        | "context-stages"
        | "change-review"
        | "surfaces"
        | "boards"
        | "canvas"
        | "imports" => "sessions",
        "memory" | "knowledge" | "recall" => "memory",
        "references" => "references",
        // Isolated copies (R2-3b) live in the data folder; worktrees in projects.
        "copies" => "worktrees",
        "logs" => "logs",
        "remote-packages" => "remotePackages",
        _ => "other",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroup {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    /// Largest first.
    pub entries: Vec<UsageEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub data_dir: String,
    /// All groups added up.
    pub total_bytes: u64,
    /// Every group, in [`GROUPS`] order, empty ones too.
    pub groups: Vec<UsageGroup>,
    /// Files or folders that could not be read; their size is not counted.
    pub unreadable: u64,
}

/// What `data_dir` holds, by group, plus each project's
/// `.aeroric/worktrees` (counted under 工作副本).
pub fn measure(data_dir: &Path, project_roots: &[PathBuf]) -> StorageUsage {
    let mut unreadable = 0;
    let mut entries: BTreeMap<&'static str, Vec<UsageEntry>> = BTreeMap::new();
    match std::fs::read_dir(data_dir) {
        Ok(children) => {
            for child in children {
                let Ok(child) = child else {
                    unreadable += 1;
                    continue;
                };
                let name = child.file_name().to_string_lossy().into_owned();
                let path = child.path();
                let bytes = size_of(&path, &mut unreadable);
                entries
                    .entry(group_of(&name))
                    .or_default()
                    .push(UsageEntry {
                        path: path.to_string_lossy().into_owned(),
                        bytes,
                    });
            }
        }
        Err(_) => unreadable += 1,
    }
    let mut seen = std::collections::HashSet::new();
    for root in project_roots {
        let worktrees = root.join(".aeroric").join("worktrees");
        if !seen.insert(worktrees.clone()) {
            continue;
        }
        if std::fs::symlink_metadata(&worktrees).is_ok_and(|m| m.is_dir()) {
            let bytes = size_of(&worktrees, &mut unreadable);
            entries.entry("worktrees").or_default().push(UsageEntry {
                path: worktrees.to_string_lossy().into_owned(),
                bytes,
            });
        }
    }
    let groups: Vec<UsageGroup> = GROUPS
        .iter()
        .map(|(id, title)| {
            let mut list = entries.remove(id).unwrap_or_default();
            list.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
            UsageGroup {
                id: (*id).to_string(),
                title: (*title).to_string(),
                bytes: list.iter().map(|e| e.bytes).sum(),
                entries: list,
            }
        })
        .collect();
    StorageUsage {
        data_dir: data_dir.to_string_lossy().into_owned(),
        total_bytes: groups.iter().map(|g| g.bytes).sum(),
        groups,
        unreadable,
    }
}

/// What `path` takes on disk: allocated blocks where the platform says, else
/// the length. Folders are walked without following symlinks.
fn size_of(path: &Path, unreadable: &mut u64) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&next) else {
            *unreadable += 1;
            continue;
        };
        total += on_disk(&meta);
        if meta.is_dir() {
            match std::fs::read_dir(&next) {
                Ok(children) => {
                    for child in children {
                        match child {
                            Ok(child) => stack.push(child.path()),
                            Err(_) => *unreadable += 1,
                        }
                    }
                }
                Err(_) => *unreadable += 1,
            }
        }
    }
    total
}

#[cfg(unix)]
fn on_disk(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn on_disk(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aeroric-usage-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    fn group<'a>(usage: &'a StorageUsage, id: &str) -> &'a UsageGroup {
        usage.groups.iter().find(|g| g.id == id).unwrap()
    }

    #[test]
    fn the_data_folder_is_measured_by_kind() {
        let data = scratch("data");
        let project = scratch("project");
        write(&data.join("agent-homes/claude/s.jsonl"), 64 * 1024);
        write(&data.join("memory/items.json"), 8 * 1024);
        write(&data.join("logs/host.log"), 4 * 1024);
        write(&data.join("copies/t1/a.txt"), 4 * 1024);
        write(&data.join("Fonts/f.ttf"), 16 * 1024);
        write(&project.join(".aeroric/worktrees/w1/b.txt"), 32 * 1024);

        let usage = measure(&data, &[project.clone(), project.clone()]);

        assert_eq!(
            usage
                .groups
                .iter()
                .map(|g| g.id.as_str())
                .collect::<Vec<_>>(),
            GROUPS.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            "every group, in order"
        );
        assert!(group(&usage, "sessions").bytes >= 64 * 1024);
        assert!(group(&usage, "memory").bytes >= 8 * 1024);
        assert!(group(&usage, "logs").bytes >= 4 * 1024);
        assert!(group(&usage, "other").bytes >= 16 * 1024, "Fonts is 其他");
        assert_eq!(group(&usage, "references").bytes, 0);
        let worktrees = group(&usage, "worktrees");
        assert_eq!(worktrees.entries.len(), 2, "copies, and the project once");
        assert!(
            worktrees.entries[0].path.ends_with(".aeroric/worktrees"),
            "largest first"
        );
        assert_eq!(
            usage.total_bytes,
            usage.groups.iter().map(|g| g.bytes).sum::<u64>()
        );
        assert_eq!(usage.unreadable, 0);
        std::fs::remove_dir_all(&data).unwrap();
        std::fs::remove_dir_all(&project).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_counts_as_itself_not_what_it_points_at() {
        let data = scratch("links");
        let elsewhere = scratch("elsewhere");
        write(&elsewhere.join("big.bin"), 256 * 1024);
        std::os::unix::fs::symlink(&elsewhere, data.join("references")).unwrap();
        let usage = measure(&data, &[]);
        assert!(group(&usage, "references").bytes < 64 * 1024);
        std::fs::remove_dir_all(&data).unwrap();
        std::fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[test]
    fn a_missing_data_folder_says_it_could_not_read_it() {
        let usage = measure(Path::new("/nonexistent/aeroric-usage"), &[]);
        assert_eq!(usage.total_bytes, 0);
        assert_eq!(usage.unreadable, 1);
    }
}
