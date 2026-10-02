//! P6-02 Action 1：本地历史（core 独占；快照记录/列出/读取/恢复——与
//! `src-tauri/src/local_history.rs` 同源语义 verbatim 上收；restore 有预览/
//! 确认且可撤回：恢复前先兜底快照）。
use std::path::{Path, PathBuf};

pub const HISTORY_DIR: &str = ".aeroric/local-history";
pub const SNAPSHOT_EXTENSION: &str = "txt";
pub const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_LIST_ENTRIES: usize = 100;

/// 快照仓库的位置与保留策略（与 Tauri `HistoryLayout` 同源）。
#[derive(Debug, Clone, Copy)]
pub struct HistoryLayout {
    pub dir: &'static str,
    pub max_entries: usize,
    /// 两次快照之间的最小间隔（0＝每次写入都留一条）。
    pub min_interval_ms: u64,
}

/// 代码文件的布局：显式保存触发，不限流。
pub const CODE_LAYOUT: HistoryLayout = HistoryLayout {
    dir: HISTORY_DIR,
    max_entries: MAX_LIST_ENTRIES,
    min_interval_ms: 0,
};

/// 历史条目（与 Tauri `LocalHistoryEntry` camelCase 字段对齐）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHistoryEntry {
    pub id: String,
    pub file_path: String,
    pub relative_path: String,
    pub created_at_ms: u64,
    pub size: u64,
}

/// 快照内容（与 Tauri `LocalHistorySnapshot` 对齐）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHistorySnapshot {
    pub entry: LocalHistoryEntry,
    pub content: String,
}

pub fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 相对路径（root 内）。
fn relative_file_path(root: &Path, file: &Path) -> Result<String, String> {
    file.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| "file is outside the project root".to_string())
}

pub fn history_dir_for_relative_path(
    layout: HistoryLayout,
    root: &Path,
    relative: &str,
) -> PathBuf {
    root.join(layout.dir).join(relative)
}

pub fn is_inside_history_dir(layout: HistoryLayout, root: &Path, file: &Path) -> bool {
    file.starts_with(root.join(layout.dir))
}

/// 记一条快照（写前快照：内容与当前一致时跳过——autosave 噪音压除）。
pub fn record_snapshot(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    skip_if_content_matches: Option<&str>,
    now_ms: u64,
) -> Result<Option<LocalHistoryEntry>, String> {
    if is_inside_history_dir(layout, root, file) {
        return Ok(None);
    }
    let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {
        return Ok(None);
    }
    let current_content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if skip_if_content_matches.is_some_and(|next| next == current_content) {
        return Ok(None);
    }
    create_snapshot(layout, root, file, &current_content, now_ms)
}

/// 无视 min_interval 强制留一条（回滚前的兜底快照——回滚是用户主动的破坏性
/// 操作，恰好落在限流窗口里就丢掉兜底，等于让回滚不可撤销）。
pub fn force_snapshot(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    now_ms: u64,
) -> Result<Option<LocalHistoryEntry>, String> {
    let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {
        return Ok(None);
    }
    let current = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    create_snapshot(layout, root, file, &current, now_ms)
}

fn create_snapshot(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    content: &str,
    now_ms: u64,
) -> Result<Option<LocalHistoryEntry>, String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    std::fs::create_dir_all(&history_dir).map_err(|e| e.to_string())?;
    let id = format!("{now_ms}");
    let snapshot_path = history_dir.join(format!("{id}.{SNAPSHOT_EXTENSION}"));
    std::fs::write(&snapshot_path, content.as_bytes()).map_err(|e| e.to_string())?;
    // 超出上限从最旧开始删。
    prune_snapshots(&history_dir, layout.max_entries)?;
    Ok(Some(LocalHistoryEntry {
        id,
        file_path: file.to_string_lossy().into_owned(),
        relative_path,
        created_at_ms: now_ms,
        size: content.len() as u64,
    }))
}

fn prune_snapshots(history_dir: &Path, max_entries: usize) -> Result<(), String> {
    let mut snapshots: Vec<PathBuf> = std::fs::read_dir(history_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(SNAPSHOT_EXTENSION))
        .collect();
    if snapshots.len() <= max_entries {
        return Ok(());
    }
    snapshots.sort();
    let excess = snapshots.len() - max_entries;
    for path in snapshots.into_iter().take(excess) {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 列出某文件的历史（新→旧）。
pub fn list_entries(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
) -> Result<Vec<LocalHistoryEntry>, String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    // 目录不存在（无历史）＝空列表，不是错误。
    let Ok(dir) = std::fs::read_dir(&history_dir) else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<LocalHistoryEntry> = dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(SNAPSHOT_EXTENSION) {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            let created_at_ms = stem.parse::<u64>().ok()?;
            let content = std::fs::read(&path).ok()?;
            Some(LocalHistoryEntry {
                id: stem,
                file_path: file.to_string_lossy().into_owned(),
                relative_path: relative_path.clone(),
                created_at_ms,
                size: content.len() as u64,
            })
        })
        .collect();
    entries.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    Ok(entries)
}

/// 读一条快照内容。
pub fn read_entry(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let relative_path = relative_file_path(root, file)?;
    let snapshot_path = history_dir_for_relative_path(layout, root, &relative_path)
        .join(format!("{entry_id}.{SNAPSHOT_EXTENSION}"));
    if !snapshot_path.exists() {
        return Err("snapshot not found".to_string());
    }
    let content = std::fs::read_to_string(&snapshot_path).map_err(|e| e.to_string())?;
    let size = content.len() as u64;
    let created_at_ms = entry_id.parse().unwrap_or(0);
    Ok(LocalHistorySnapshot {
        entry: LocalHistoryEntry {
            id: entry_id.to_string(),
            file_path: file.to_string_lossy().into_owned(),
            relative_path,
            created_at_ms,
            size,
        },
        content,
    })
}

/// 恢复（restore 有预览/确认且可撤回：恢复前先兜底快照——回滚不可撤销的
/// 兜底；UI 层的预览/确认在 Editor 视图）。
pub fn restore_entry(
    layout: HistoryLayout,
    root: &Path,
    file: &Path,
    entry_id: &str,
    now_ms: u64,
) -> Result<LocalHistorySnapshot, String> {
    let snapshot = read_entry(layout, root, file, entry_id)?;
    let _ = force_snapshot(layout, root, file, now_ms)?;
    std::fs::write(file, snapshot.content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(snapshot)
}

/// 丢弃某文件的全部历史（文件删除时的清理）。
pub fn discard_history(layout: HistoryLayout, root: &Path, file: &Path) -> Result<(), String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    if history_dir.exists() {
        std::fs::remove_dir_all(&history_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}
