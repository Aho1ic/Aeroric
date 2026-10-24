//! `settings.json` — one object, kept whole.
//!
//! The shipping client reads a corrupt settings file as "no settings", then persists the
//! defaults over it on the next save (`src-tauri/src/app_settings/load_save.rs:442-458`).
//! Here absence and unreadable are different results, and only absence may be written
//! over.

use serde_json::{Map, Value};
use std::sync::OnceLock;

use crate::context::CoreContext;
use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};
use crate::storage::compat;

/// The legacy settings loaders and map writers share this process-local lock.
/// A writer lease excludes other processes, not simultaneous requests here.
/// Hold it only for synchronous settings work; never across an await.
pub fn settings_lock() -> &'static parking_lot::Mutex<()> {
    static LOCK: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| parking_lot::Mutex::new(()))
}

pub fn load_settings(
    context: &CoreContext,
) -> Result<ReadOutcome<Map<String, Value>>, StorageError> {
    let path = context.paths.settings_file();
    Ok(match atomic::read_json(&path)? {
        ReadOutcome::Absent => ReadOutcome::Absent,
        ReadOutcome::Found(Value::Object(mut fields)) => {
            // Same disk boundary as `settings::load_save`: credentials come back
            // from the keychain; an unreadable entry keeps its marker.
            if let Some(store) = crate::settings::secret_fields::secret_store() {
                let _unresolved =
                    crate::settings::secret_fields::hydrate_from_store(&mut fields, store.as_ref());
            }
            ReadOutcome::Found(fields)
        }
        ReadOutcome::Found(other) => return Err(compat::shape_error(&path, &other, "an object")),
    })
}

pub fn save_settings(
    context: &CoreContext,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let _guard = settings_lock().lock();
    save_settings_unlocked(context, settings)
}

/// Update the latest raw map under one read-modify-write critical section.
/// A failed read or callback never writes a file or backup. Callbacks may inspect
/// the map but must not recursively call a settings writer or legacy loader.
pub fn update_settings(
    context: &CoreContext,
    update: impl FnOnce(&mut Map<String, Value>) -> Result<(), StorageError>,
) -> Result<Map<String, Value>, StorageError> {
    let _guard = settings_lock().lock();
    compat::require_writer(context)?;
    let mut fields = load_settings(context)?.found().unwrap_or_default();
    update(&mut fields)?;
    save_settings_unlocked(context, &fields)?;
    Ok(fields)
}

fn save_settings_unlocked(
    context: &CoreContext,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    compat::require_writer(context)?;
    let path = context.paths.settings_file();
    context
        .paths
        .ensure_private()
        .map_err(|e| StorageError::WriteFailed {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    let mut on_disk = settings.clone();
    if let Some(store) = crate::settings::secret_fields::secret_store() {
        let previous = crate::settings::secret_fields::marked_accounts_on_disk(&path);
        crate::settings::secret_fields::split_for_disk(&mut on_disk, &previous, store.as_ref())
            .map_err(|reason| StorageError::WriteFailed {
                path: path.clone(),
                reason,
            })?;
    }
    atomic::replace_with_backup(
        &path,
        &compat::pretty(&Value::Object(on_disk)),
        FileMode::Private,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use crate::storage::test_support::writer;
    use serde_json::json;

    #[test]
    fn settings_keep_keys_the_core_has_never_seen() {
        let (root, context) = writer("settings");
        let mut settings = Map::new();
        settings.insert("interactionMode".into(), json!("native"));
        settings.insert("unknownToggles".into(), json!({"future": true}));
        save_settings(&context, &settings).unwrap();
        assert_eq!(
            load_settings(&context).unwrap().found().expect("present"),
            settings
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_missing_file_is_absence_and_a_corrupt_one_is_a_refusal() {
        let (root, context) = writer("shapes");
        assert_eq!(
            load_settings(&CoreContext::read_only(AppPaths::from_user_home(
                root.join("other")
            )))
            .unwrap(),
            ReadOutcome::Absent,
            "a missing settings file is absence, not an error"
        );
        std::fs::write(context.paths.settings_file(), "").unwrap();
        let error = load_settings(&context).expect_err("an empty file is not an empty object");
        assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn settings_transaction_merges_simultaneous_updates() {
        let (root, context) = writer("settings-rmw");
        let context = std::sync::Arc::new(context);
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|worker| {
                let context = context.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    for step in 1..=20 {
                        update_settings(&context, |fields| {
                            let count = fields.get("count").and_then(Value::as_u64).unwrap_or(0);
                            std::thread::yield_now();
                            fields.insert("count".into(), json!(count + 1));
                            fields.insert(format!("worker-{worker}"), json!(step));
                            Ok(())
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let fields = load_settings(&context).unwrap().found().unwrap();
        assert_eq!(fields["count"], 160);
        for worker in 0..8 {
            assert_eq!(fields[&format!("worker-{worker}")], 20);
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn settings_transaction_refuses_read_only_before_running_the_callback() {
        let (root, context) = writer("settings-rmw-ro");
        save_settings(&context, json!({"kept": true}).as_object().unwrap()).unwrap();
        let path = context.paths.settings_file();
        let before = std::fs::read(&path).unwrap();
        let reader = CoreContext::read_only(context.paths.clone());
        let error =
            update_settings(&reader, |_| panic!("a read-only update cannot run")).unwrap_err();
        assert!(matches!(error, StorageError::WritePermission(_)));
        assert_eq!(std::fs::read(path).unwrap(), before);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn settings_transaction_does_not_commit_corrupt_data_or_failed_callbacks() {
        let (root, context) = writer("settings-rmw-abort");
        let path = context.paths.settings_file();
        std::fs::write(&path, b"{ broken").unwrap();
        assert!(matches!(
            update_settings(&context, |_| panic!(
                "corruption must fail before the update"
            )),
            Err(StorageError::Corrupt { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"{ broken");
        std::fs::write(&path, br#"{"kept":true}"#).unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = update_settings(&context, |fields| {
            fields.insert("must-not-persist".into(), json!(1));
            Err(StorageError::WriteFailed {
                path: path.clone(),
                reason: "synthetic callback refusal".into(),
            })
        });
        assert!(error.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let backup = path.with_extension("json.bak");
        assert!(!backup.exists());
        update_settings(&context, |fields| {
            fields.insert("next".into(), json!(2));
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), before);
        std::fs::remove_dir_all(root).ok();
    }
}
