//! `projects.json` — a bare array of project records, as today.

use crate::context::CoreContext;
use crate::storage::atomic::StorageError;
use crate::storage::compat::{self, Record};

pub fn load_projects(context: &CoreContext) -> Result<Vec<Record>, StorageError> {
    Ok(compat::read_array(&context.paths.projects_file())?
        .found()
        .unwrap_or_default())
}

pub fn save_projects(context: &CoreContext, records: &[Record]) -> Result<(), StorageError> {
    compat::require_writer(context)?;
    context
        .paths
        .ensure_private()
        .map_err(|e| StorageError::WriteFailed {
            path: context.paths.projects_file(),
            reason: e.to_string(),
        })?;
    compat::write_array(&context.paths.projects_file(), records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use crate::storage::atomic::{self, FileMode, StorageError::Corrupt};
    use crate::storage::test_support::{legacy_record, writer};
    use serde_json::json;

    #[test]
    fn a_corrupt_file_is_reported_and_left_alone() {
        let (root, context) = writer("corrupt");
        let path = context.paths.projects_file();
        std::fs::write(&path, b"{\"id\": broken").unwrap();
        let error = load_projects(&context).expect_err("corrupt projects");
        assert!(matches!(error, Corrupt { .. }), "{error:?}");
        let before = std::fs::read(&path).unwrap();
        let failure = save_projects(&context, &[legacy_record()])
            .expect_err("a writer must be stopped by the corrupt file, not silently replace it");
        assert!(matches!(failure, Corrupt { .. }), "{failure:?}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "the unreadable file must be intact"
        );
        assert!(
            !path.with_extension("json.bak").exists(),
            "a refused write leaves no side effect behind, not even a backup"
        );
        // Replacing it is a human decision, so it costs an explicit write rather than
        // happening because some code path forgot to look.
        atomic::write_atomic(&path, b"[]", FileMode::Private).expect("deliberate replacement");
        assert!(load_projects(&context).unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_reader_without_a_lease_cannot_write() {
        let (root, _) = writer("nolease");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let context = CoreContext::read_only(paths);
        let error = save_projects(&context, &[legacy_record()]).expect_err("no lease, no write");
        assert!(error.to_string().contains("no write permission"), "{error}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_project_record_keeps_the_target_shape_the_ui_writes() {
        let (root, context) = writer("targetshape");
        let record = Record(
            json!({
                "id": "p1",
                "target": { "kind": "ssh", "connectionId": "c-1", "remotePath": "/srv/app" },
                "name": "远程项目",
                "pinned": true
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        save_projects(&context, std::slice::from_ref(&record)).unwrap();
        assert_eq!(load_projects(&context).unwrap(), vec![record]);
        std::fs::remove_dir_all(root).ok();
    }
}
