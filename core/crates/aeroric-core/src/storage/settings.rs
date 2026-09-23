//! `settings.json` — one object, kept whole.
//!
//! The shipping client reads a corrupt settings file as "no settings", then persists the
//! defaults over it on the next save (`src-tauri/src/app_settings/load_save.rs:442-458`).
//! Here absence and unreadable are different results, and only absence may be written
//! over.

use serde_json::{Map, Value};

use crate::context::CoreContext;
use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};
use crate::storage::compat;

pub fn load_settings(
    context: &CoreContext,
) -> Result<ReadOutcome<Map<String, Value>>, StorageError> {
    let path = context.paths.settings_file();
    Ok(match atomic::read_json(&path)? {
        ReadOutcome::Absent => ReadOutcome::Absent,
        ReadOutcome::Found(Value::Object(fields)) => ReadOutcome::Found(fields),
        ReadOutcome::Found(other) => return Err(compat::shape_error(&path, &other, "an object")),
    })
}

pub fn save_settings(
    context: &CoreContext,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    compat::require_writer(context)?;
    context
        .paths
        .ensure_private()
        .map_err(|e| StorageError::WriteFailed {
            path: context.paths.settings_file(),
            reason: e.to_string(),
        })?;
    atomic::replace_with_backup(
        &context.paths.settings_file(),
        &compat::pretty(&Value::Object(settings.clone())),
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
}
