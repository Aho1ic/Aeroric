//! Native raw-layout local history with force-backup restore semantics.
//! Legacy hex-layout history has distinct admission/errors/skip behavior and
//! lives in `legacy_history`; only their DTOs/layout values are shared.
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
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);
    if within_min_interval(layout, &history_dir, now_ms) {
        return Ok(None);
    }
    create_snapshot(layout, root, file, &current_content, now_ms)
}

/// 距上一条快照还没到最小间隔。
///
/// 判据用**最新一条快照的时间戳**,不是文件 mtime:mtime 每次自动保存都会动,
/// 拿它算间隔等于不限流。目录读不动(不存在 / 权限)时返回 false —— 宁可多留
/// 一条快照,也不要因为一次读目录失败就静默丢掉历史。`now_ms` 是调用方传入的
/// 当前时间,与快照 id 用同一个时间源。(`CODE_LAYOUT` 的间隔是 0,此分支对代码
/// 文件历史永远不触发;它守的是 notebook `NOTE_LAYOUT` 的三分钟限流。)
fn within_min_interval(layout: HistoryLayout, history_dir: &Path, now_ms: u64) -> bool {
    if layout.min_interval_ms == 0 {
        return false;
    }
    let Some(latest) = latest_entry_timestamp_ms(history_dir) else {
        return false;
    };
    now_ms.saturating_sub(latest) < layout.min_interval_ms
}

fn latest_entry_timestamp_ms(history_dir: &Path) -> Option<u64> {
    std::fs::read_dir(history_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(SNAPSHOT_EXTENSION) {
                return None;
            }
            path.file_stem()?.to_str()?.parse::<u64>().ok()
        })
        .max()
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
    let base_id = now_ms.to_string();

    // 同毫秒的第二条快照不覆盖第一条：`create_new` 抢名字，冲突就加 `-N` 后缀
    // 重试（与宿主 `local_history.rs` 同一算法——P6-02 上收时丢了这条，P7-05
    // 被同毫秒碰撞测试当场抓回）。
    for suffix in 0..1000u32 {
        let id = if suffix == 0 {
            base_id.clone()
        } else {
            format!("{base_id}-{suffix}")
        };
        let path = history_dir_for_relative_path(layout, root, &relative_path)
            .join(format!("{id}.{SNAPSHOT_EXTENSION}"));
        let mut handle = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(handle) => handle,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        };
        {
            use std::io::Write;
            handle
                .write_all(content.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let size = handle.metadata().map_err(|e| e.to_string())?.len();
        let entry = LocalHistoryEntry {
            id,
            file_path: file.to_string_lossy().into_owned(),
            relative_path,
            created_at_ms: entry_id_timestamp_ms(&base_id).unwrap_or(now_ms),
            size,
        };
        prune_snapshots(&history_dir, layout.max_entries)?;
        return Ok(Some(entry));
    }
    Err("Could not create a unique local history snapshot".to_string())
}

/// 快照 id（`<毫秒>` 或 `<毫秒>-<序号>`）里的毫秒部分。
fn entry_id_timestamp_ms(entry_id: &str) -> Option<u64> {
    entry_id.split('-').next()?.parse().ok()
}

/// 同毫秒内的序号（`-N` 后缀；无后缀＝0）。
fn entry_id_sequence(entry_id: &str) -> u16 {
    entry_id
        .split_once('-')
        .and_then(|(_, suffix)| suffix.parse().ok())
        .unwrap_or(0)
}

fn prune_snapshots(history_dir: &Path, max_entries: usize) -> Result<(), String> {
    let mut entries: Vec<(u64, u16, PathBuf)> = std::fs::read_dir(history_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(SNAPSHOT_EXTENSION) {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let timestamp = entry_id_timestamp_ms(&id)?;
            Some((timestamp, entry_id_sequence(&id), path))
        })
        .collect();
    // 新的在前（毫秒优先，同毫秒序号大的在前），超出上限的从最旧开始删。
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    for (_, _, path) in entries.into_iter().skip(max_entries) {
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
            let id = path.file_stem()?.to_str()?.to_string();
            // id 可能带同毫秒 `-N` 序号后缀，毫秒部分取 `-` 前的段。
            let created_at_ms = entry_id_timestamp_ms(&id)?;
            let content = std::fs::read(&path).ok()?;
            Some(LocalHistoryEntry {
                id,
                file_path: file.to_string_lossy().into_owned(),
                relative_path: relative_path.clone(),
                created_at_ms,
                size: content.len() as u64,
            })
        })
        .collect();
    // 新→旧；同毫秒时序号大的在前（与 create 的 `-N` 顺序一致）。
    entries.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| entry_id_sequence(&b.id).cmp(&entry_id_sequence(&a.id)))
    });
    entries.truncate(layout.max_entries);
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
    // id 可能带同毫秒 `-N` 序号后缀，毫秒部分取 `-` 前的段。
    let created_at_ms = entry_id_timestamp_ms(entry_id).unwrap_or(0);
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
