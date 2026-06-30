use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const HISTORY_DIR: &str = ".aeroric/local-history";
const SNAPSHOT_EXTENSION: &str = "txt";
const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalHistoryEntry {
    pub id: String,
    pub file_path: String,
    pub relative_path: String,
    pub created_at_ms: u64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalHistorySnapshot {
    pub entry: LocalHistoryEntry,
    pub content: String,
}

pub(crate) fn record_snapshot_before_write(
    project_path: &str,
    file_path: &str,
    next_content: &str,
) -> Result<Option<LocalHistoryEntry>, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    record_snapshot_for_file(&root, &file, Some(next_content))
}

pub(crate) fn list_entries(
    project_path: &str,
    file_path: &str,
) -> Result<Vec<LocalHistoryEntry>, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    let relative_path = relative_file_path(&root, &file)?;
    let history_dir = history_dir_for_relative_path(&root, &relative_path);
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
            .then_with(|| b.id.cmp(&a.id))
    });
    entries.truncate(MAX_LIST_ENTRIES);
    Ok(entries)
}

pub(crate) fn read_entry(
    project_path: &str,
    file_path: &str,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    let entry = entry_for_id(&root, &file, entry_id)?;
    let content = fs::read_to_string(entry_path(&root, &entry.relative_path, entry_id))
        .map_err(|e| e.to_string())?;
    Ok(LocalHistorySnapshot { entry, content })
}

pub(crate) fn restore_entry(
    project_path: &str,
    file_path: &str,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let root = validate_project_root(project_path)?;
    let file = validate_file_path(&root, file_path)?;
    let snapshot = read_entry(project_path, file_path, entry_id)?;
    let _ = record_snapshot_for_file(&root, &file, Some(&snapshot.content))?;
    fs::write(&file, snapshot.content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn list_local_history(
    project_path: String,
    file_path: String,
) -> Result<Vec<LocalHistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || list_entries(&project_path, &file_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_local_history_entry(
    project_path: String,
    file_path: String,
    entry_id: String,
) -> Result<LocalHistorySnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || read_entry(&project_path, &file_path, &entry_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_local_history_entry(
    project_path: String,
    file_path: String,
    entry_id: String,
) -> Result<LocalHistorySnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        restore_entry(&project_path, &file_path, &entry_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn record_snapshot_for_file(
    root: &Path,
    file: &Path,
    skip_if_content_matches: Option<&str>,
) -> Result<Option<LocalHistoryEntry>, String> {
    if is_inside_history_dir(root, file) {
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
    create_snapshot(root, file, &current_content)
}

fn create_snapshot(
    root: &Path,
    file: &Path,
    content: &str,
) -> Result<Option<LocalHistoryEntry>, String> {
    let relative_path = relative_file_path(root, file)?;
    let history_dir = history_dir_for_relative_path(root, &relative_path);
    fs::create_dir_all(&history_dir).map_err(|e| e.to_string())?;
    let base_id = now_ms().to_string();

    for suffix in 0..1000 {
        let id = if suffix == 0 {
            base_id.clone()
        } else {
            format!("{base_id}-{suffix}")
        };
        let path = entry_path(root, &relative_path, &id);
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
        return Ok(Some(LocalHistoryEntry {
            id,
            file_path: file.to_string_lossy().into_owned(),
            relative_path,
            created_at_ms: entry_id_timestamp_ms(&base_id).unwrap_or_else(now_ms),
            size,
        }));
    }

    Err("Could not create a unique local history snapshot".to_string())
}

fn entry_for_id(root: &Path, file: &Path, entry_id: &str) -> Result<LocalHistoryEntry, String> {
    validate_entry_id(entry_id)?;
    let relative_path = relative_file_path(root, file)?;
    let path = entry_path(root, &relative_path, entry_id);
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

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
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

fn history_dir_for_relative_path(root: &Path, relative_path: &str) -> PathBuf {
    root.join(HISTORY_DIR)
        .join(hex_encode(relative_path.as_bytes()))
}

fn entry_path(root: &Path, relative_path: &str, entry_id: &str) -> PathBuf {
    history_dir_for_relative_path(root, relative_path).join(format!("{entry_id}.txt"))
}

fn is_inside_history_dir(root: &Path, file: &Path) -> bool {
    file.starts_with(root.join(HISTORY_DIR))
}

fn entry_id_timestamp_ms(entry_id: &str) -> Option<u64> {
    entry_id.split('-').next()?.parse().ok()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aeroric-local-history-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    #[test]
    fn records_snapshots_before_text_changes() {
        let root = temp_project();
        let file = root.join("src/app.ts");
        fs::write(&file, "const value = 1;\n").unwrap();

        let snapshot =
            record_snapshot_before_write(root.to_str().unwrap(), file.to_str().unwrap(), "new")
                .unwrap()
                .unwrap();

        assert_eq!(snapshot.relative_path, "src/app.ts");
        let entries = list_entries(root.to_str().unwrap(), file.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        let read =
            read_entry(root.to_str().unwrap(), file.to_str().unwrap(), &snapshot.id).unwrap();
        assert_eq!(read.content, "const value = 1;\n");

        let unchanged = record_snapshot_before_write(
            root.to_str().unwrap(),
            file.to_str().unwrap(),
            "const value = 1;\n",
        )
        .unwrap();
        assert!(unchanged.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restores_snapshot_and_records_current_content() {
        let root = temp_project();
        let file = root.join("src/app.ts");
        fs::write(&file, "before\n").unwrap();
        let snapshot =
            record_snapshot_before_write(root.to_str().unwrap(), file.to_str().unwrap(), "after\n")
                .unwrap()
                .unwrap();
        fs::write(&file, "after\n").unwrap();

        let restored =
            restore_entry(root.to_str().unwrap(), file.to_str().unwrap(), &snapshot.id).unwrap();

        assert_eq!(restored.content, "before\n");
        assert_eq!(fs::read_to_string(&file).unwrap(), "before\n");
        let entries = list_entries(root.to_str().unwrap(), file.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        let current_snapshot = read_entry(
            root.to_str().unwrap(),
            file.to_str().unwrap(),
            &entries[0].id,
        )
        .unwrap();
        assert_eq!(current_snapshot.content, "after\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_history_entry_traversal() {
        let root = temp_project();
        let file = root.join("src/app.ts");
        fs::write(&file, "content\n").unwrap();

        let err = read_entry(root.to_str().unwrap(), file.to_str().unwrap(), "../bad").unwrap_err();

        assert!(err.contains("Invalid local history entry id"));
        let _ = fs::remove_dir_all(root);
    }
}
