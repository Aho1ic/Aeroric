#[cfg(test)]
use aeroric_core::files::legacy_history::prune_history_dir;
pub(crate) use aeroric_core::files::legacy_history::record_snapshot_before_write;
use aeroric_core::files::legacy_history::{list_entries, read_entry, restore_entry};
#[cfg(test)]
use aeroric_core::files::local_history::HISTORY_DIR;
pub use aeroric_core::files::local_history::{LocalHistoryEntry, LocalHistorySnapshot};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
#[cfg(unix)]
mod legacy_history_tests;

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

    #[test]
    fn prunes_snapshot_files_beyond_retention_limit() {
        let root = temp_project();
        let history_dir = root.join(HISTORY_DIR).join("fixture");
        fs::create_dir_all(&history_dir).unwrap();
        for id in ["1000", "1001", "1002-1"] {
            fs::write(history_dir.join(format!("{id}.txt")), id).unwrap();
        }

        prune_history_dir(&history_dir, 2).unwrap();

        assert!(!history_dir.join("1000.txt").exists());
        assert!(history_dir.join("1001.txt").exists());
        assert!(history_dir.join("1002-1.txt").exists());
        let _ = fs::remove_dir_all(root);
    }
}
