//! Exact legacy hex-layout local history. Native raw-layout history is distinct.
//! Shared DTOs/constants do not make the two storage/error/restore policies aliases.

use super::local_history::{
    HistoryLayout, LocalHistoryEntry, LocalHistorySnapshot, CODE_LAYOUT, MAX_SNAPSHOT_BYTES,
    SNAPSHOT_EXTENSION,
};
use super::path_policy::validate_legacy_project_root as validate_project_root;
use crate::clock::now_ms_u64;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn record_snapshot_before_write(
    project_path: &str,
    file_path: &str,
    next_content: &str,
) -> Result<Option<LocalHistoryEntry>, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    record_snapshot_for_file(CODE_LAYOUT, &root, &file, Some(next_content))
}

pub fn list_entries(project_path: &str, file_path: &str) -> Result<Vec<LocalHistoryEntry>, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    list_entries_in(CODE_LAYOUT, &root, &file)
}

pub fn list_entries_in(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
) -> Result<Vec<LocalHistoryEntry>, String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    if !history_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(history_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(created_at_ms) = entry_id_timestamp_ms(id) else {
            continue;
        };
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        entries.push(LocalHistoryEntry {
            id: id.to_string(),
            file_path: file.to_string_lossy().into_owned(),
            relative_path: relative_path.clone(),
            created_at_ms,
            size: metadata.len(),
        });
    }

    entries.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| entry_id_sequence(&b.id).cmp(&entry_id_sequence(&a.id)))
    });
    entries.truncate(layout.max_entries);
    Ok(entries)
}

pub fn read_entry(
    project_path: &str,
    file_path: &str,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    read_entry_in(CODE_LAYOUT, &root, &file, entry_id)
}

pub fn read_entry_in(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let entry = entry_for_id(layout, root, file, entry_id)?;
    let content = fs::read_to_string(entry_path(layout, root, &entry.relative_path, entry_id))
        .map_err(|e| e.to_string())?;
    Ok(LocalHistorySnapshot { entry, content })
}

pub fn restore_entry(
    project_path: &str,
    file_path: &str,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    let snapshot = read_entry(project_path, file_path, entry_id)?;
    let _ = record_snapshot_for_file(CODE_LAYOUT, &root, &file, Some(&snapshot.content))?;
    fs::write(&file, snapshot.content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(snapshot)
}

fn record_snapshot_for_file(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    skip_if_content_matches: Option<&str>,
) -> Result<Option<LocalHistoryEntry>, String> {
    if is_inside_history_dir(layout, root, file) {
        return Ok(None);
    }
    let metadata = fs::metadata(file).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {
        return Ok(None);
    }
    let current_content = match fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if skip_if_content_matches.is_some_and(|next| next == current_content) {
        return Ok(None);
    }
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    if within_min_interval(layout, &history_dir) {
        return Ok(None);
    }
    create_snapshot(layout, root, file, &current_content)
}

/// 距上一条快照还没到最小间隔。
///
/// 判据用**最新一条快照的时间戳**,不是文件 mtime:mtime 每次自动保存都会动,
/// 拿它算间隔等于不限流。目录读不动(不存在 / 权限)时返回 false —— 宁可多留
/// 一条快照,也不要因为一次读目录失败就静默丢掉历史。
fn within_min_interval(layout: HistoryLayout, history_dir: &Path) -> bool {
    if layout.min_interval_ms == 0 {
        return false;
    }
    let Some(latest) = latest_entry_timestamp_ms(history_dir) else {
        return false;
    };
    now_ms_u64().saturating_sub(latest) < layout.min_interval_ms
}

fn latest_entry_timestamp_ms(history_dir: &Path) -> Option<u64> {
    fs::read_dir(history_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {
                return None;
            }
            entry_id_timestamp_ms(path.file_stem()?.to_str()?)
        })
        .max()
}

fn create_snapshot(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    content: &str,
) -> Result<Option<LocalHistoryEntry>, String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    fs::create_dir_all(&history_dir).map_err(|e| e.to_string())?;
    let base_id = now_ms_u64().to_string();

    for suffix in 0..1000 {
        let id = if suffix == 0 {
            base_id.clone()
        } else {
            format!("{base_id}-{suffix}")
        };
        let path = entry_path(layout, root, &relative_path, &id);
        let mut file_handle = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(handle) => handle,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        };
        file_handle
            .write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
        let size = file_handle.metadata().map_err(|e| e.to_string())?.len();
        let entry = LocalHistoryEntry {
            id,
            file_path: file.to_string_lossy().into_owned(),
            relative_path,
            created_at_ms: entry_id_timestamp_ms(&base_id).unwrap_or_else(now_ms_u64),
            size,
        };
        prune_history_dir(&history_dir, layout.max_entries)?;
        return Ok(Some(entry));
    }

    Err("Could not create a unique local history snapshot".to_string())
}

fn entry_for_id(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    entry_id: &str,
) -> Result<LocalHistoryEntry, String> {
    validate_entry_id(entry_id)?;
    let relative_path = relative_file_path(root, file)?;
    let path = entry_path(layout, root, &relative_path, entry_id);
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Local history entry is not a file".to_string());
    }
    let created_at_ms = entry_id_timestamp_ms(entry_id)
        .ok_or_else(|| "Invalid local history entry id".to_string())?;
    Ok(LocalHistoryEntry {
        id: entry_id.to_string(),
        file_path: file.to_string_lossy().into_owned(),
        relative_path,
        created_at_ms,
        size: metadata.len(),
    })
}

fn validate_file_path(root: &Path, file_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(file_path);
    if !path.is_absolute() {
        return Err("File path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve file path: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("File path is outside the project".to_string());
    }
    if !canonical.is_file() {
        return Err("File path is not a file".to_string());
    }
    Ok(canonical)
}

fn validate_entry_id(entry_id: &str) -> Result<(), String> {
    if entry_id.is_empty() || !entry_id.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        return Err("Invalid local history entry id".to_string());
    }
    Ok(())
}

fn relative_file_path(root: &Path, file: &Path) -> Result<String, String> {
    let relative = file
        .strip_prefix(root)
        .map_err(|_| "File path is outside the project".to_string())?;
    let parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    Ok(parts.join("/"))
}

/// 每个文件一个子目录,名字是相对路径的 hex 编码 —— 免得目录名里出现分隔符,
/// 也免得大小写不敏感的文件系统把 `A.md` 和 `a.md` 的历史混在一起。
fn history_dir_for_relative_path(
    layout: HistoryLayout,
    root: &Path,
    relative_path: &str,
) -> PathBuf {
    root.join(layout.dir)
        .join(hex_encode(relative_path.as_bytes()))
}

fn entry_path(layout: HistoryLayout, root: &Path, relative_path: &str, entry_id: &str) -> PathBuf {
    history_dir_for_relative_path(layout, root, relative_path).join(format!("{entry_id}.txt"))
}

fn is_inside_history_dir(layout: HistoryLayout, root: &Path, file: &Path) -> bool {
    file.starts_with(root.join(layout.dir))
}

fn entry_id_timestamp_ms(entry_id: &str) -> Option<u64> {
    entry_id.split('-').next()?.parse().ok()
}

fn entry_id_sequence(entry_id: &str) -> u16 {
    entry_id
        .split_once('-')
        .and_then(|(_, suffix)| suffix.parse().ok())
        .unwrap_or(0)
}

pub fn prune_history_dir(history_dir: &Path, max_entries: usize) -> Result<(), String> {
    let mut entries = fs::read_dir(history_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let timestamp = entry_id_timestamp_ms(&id)?;
            Some((timestamp, entry_id_sequence(&id), path))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    for (_, _, path) in entries.into_iter().skip(max_entries) {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
