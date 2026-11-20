//! C7/C8 (2026-11-20): what each launch of a session had — its skills, its
//! tools — kept per task under `<data>/<folder>/<task>.json`, newest last.
//! A write that fails never stops a launch; callers only log it.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Launches kept per task.
pub const KEPT: usize = 20;

/// The task's file; an id that is not a plain file name is kept under its
/// hash, so no id can point outside the folder.
pub fn file(data_dir: &Path, folder: &str, task_id: &str) -> PathBuf {
    let safe = !task_id.is_empty()
        && !task_id.starts_with('.')
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let name = if safe {
        task_id.to_string()
    } else {
        Sha256::digest(task_id.as_bytes())[..6]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    data_dir.join(folder).join(format!("{name}.json"))
}

/// Every launch kept, oldest first; none when the file is missing or
/// unreadable.
pub fn read<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// One more launch; only the last [`KEPT`] stay.
pub fn push<T: Serialize + DeserializeOwned>(path: &Path, launch: T) -> Result<(), String> {
    let mut launches: Vec<T> = read(path);
    launches.push(launch);
    let skip = launches.len().saturating_sub(KEPT);
    let launches: Vec<T> = launches.into_iter().skip(skip).collect();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_vec(&launches).map_err(|e| e.to_string())?;
    super::atomic::write_atomic(path, &raw, super::atomic::FileMode::Private)
        .map_err(|e| e.to_string())
}
