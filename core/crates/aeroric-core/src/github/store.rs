//! Snapshots on disk: `<data>/github/<project>/<snapshot id>.json`.

use std::path::{Path, PathBuf};

use aeroric_protocol::github::PullSnapshot;

fn safe(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn dir(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir.join("github").join(safe(project_id))
}

/// Keep `snapshot`. One already kept for the same head is returned as it
/// was (the time it was pinned stays): fetching again adds nothing.
pub fn save(data_dir: &Path, snapshot: &PullSnapshot) -> Result<PullSnapshot, String> {
    let dir = dir(data_dir, &snapshot.project_id);
    let path = dir.join(format!("{}.json", safe(&snapshot.id)));
    if let Some(kept) = read(&path) {
        return Ok(kept);
    }
    crate::settings::ensure_private_dir(&dir)?;
    let text = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&path, &text)?;
    Ok(snapshot.clone())
}

fn read(path: &Path) -> Option<PullSnapshot> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// A project's snapshots, newest first; one whose PR has a newer snapshot
/// with another head is `superseded`.
pub fn list(data_dir: &Path, project_id: &str) -> Vec<PullSnapshot> {
    let Ok(entries) = std::fs::read_dir(dir(data_dir, project_id)) else {
        return Vec::new();
    };
    let mut out: Vec<PullSnapshot> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read(&e.path()))
        .collect();
    out.sort_by(|a, b| b.fetched_ms.cmp(&a.fetched_ms).then(b.id.cmp(&a.id)));
    let mut newest: std::collections::HashMap<(u64, u64), String> = Default::default();
    for s in &out {
        newest
            .entry((s.repo.repo_id, s.number))
            .or_insert_with(|| s.head_sha.clone());
    }
    for s in &mut out {
        s.superseded = newest[&(s.repo.repo_id, s.number)] != s.head_sha;
    }
    out
}

/// One snapshot, as listed (with `superseded`).
pub fn get(data_dir: &Path, project_id: &str, id: &str) -> Option<PullSnapshot> {
    list(data_dir, project_id).into_iter().find(|s| s.id == id)
}
