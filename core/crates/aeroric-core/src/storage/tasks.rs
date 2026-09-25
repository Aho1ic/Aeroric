//! `projects/<id>/tasks.json` — one array of task records per project.
//!
//! The shipping client deletes this file when the list becomes empty
//! (`src-tauri/src/storage.rs`); an ordinary save here writes an empty snapshot as `[]` and
//! keeps the previous content as `.bak`, because "the frontend briefly reported no tasks"
//! and "this project never had tasks" must not be the same event on disk. The one place
//! that DOES earn the client's delete-on-empty contract is [`remove_covering`] — reached
//! only when a delete intent empties the list (ADR-037 后果 1: project removal depends on
//! it, and the host's forwarding save (`write_task_records_raw`) mirrors the same rule).

use std::path::PathBuf;

use crate::context::CoreContext;
use crate::paths::AppPaths;
use crate::storage::atomic::StorageError;
use crate::storage::compat::{self, Record};
use crate::storage::fingerprint::{self, Cover, Fingerprint};

pub fn load_tasks(context: &CoreContext, project_id: &str) -> Result<Vec<Record>, StorageError> {
    Ok(compat::read_array(&context.paths.tasks_file(project_id)?)?
        .found()
        .unwrap_or_default())
}

pub fn save_tasks(
    context: &CoreContext,
    project_id: &str,
    records: &[Record],
) -> Result<(), StorageError> {
    compat::require_writer(context)?;
    let path = context.paths.tasks_file(project_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StorageError::WriteFailed {
            path: parent.to_path_buf(),
            reason: e.to_string(),
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    compat::write_array(&path, records)
}

/// A snapshot and the exact bytes it came from. The fingerprint is what the next write of
/// this project's list must still be holding, so a save that follows a legacy save fails
/// instead of overwriting it (ADR-012 decision 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampedTasks {
    pub records: Vec<Record>,
    pub fingerprint: Option<Fingerprint>,
    pub key: String,
}

pub fn load_stamped(context: &CoreContext, project_id: &str) -> Result<StampedTasks, StorageError> {
    let path = context.paths.tasks_file(project_id)?;
    let key = fingerprint::table_key(context, &path)?;
    let fingerprint = fingerprint::of_file(&path)?;
    let records = compat::read_array(&path)?.found().unwrap_or_default();
    Ok(StampedTasks {
        records,
        fingerprint,
        key,
    })
}

/// Replace a project's task list, but only over the bytes `cover` entitles. Returns the
/// fingerprint of what is now on disk, which is the cover the caller's *next* write needs.
pub fn save_covering(
    context: &CoreContext,
    project_id: &str,
    records: &[Record],
    cover: Cover,
) -> Result<(Fingerprint, String), StorageError> {
    compat::require_writer(context)?;
    let path = context.paths.tasks_file(project_id)?;
    let key = fingerprint::table_key(context, &path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StorageError::WriteFailed {
            path: parent.to_path_buf(),
            reason: e.to_string(),
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    fingerprint::check_cover(context, &key, &path, &cover)?;
    compat::write_array(&path, records)?;
    let written = fingerprint::of_file(&path)?
        .expect("write_array just put the file there, so it has bytes to hash");
    Ok((written, key))
}

/// 删空 ⇒ 删文件（ADR-037 后果 1）。只由"这条意图把列表删到空"的路径到达：普通保存把空列表
/// 写成 `[]`（见模块头的理由），删除则连文件一起消失——发布客户端的 `save_project_tasks`
/// 与宿主的字节转发 `write_task_records_raw` 都是这个形状，删项目依赖它。
/// 返回的指纹是 `None`：这就是"下一次写的合法覆盖面是文件不存在"。
pub fn remove_covering(
    context: &CoreContext,
    project_id: &str,
    cover: Cover,
) -> Result<(Option<Fingerprint>, String), StorageError> {
    compat::require_writer(context)?;
    let path = context.paths.tasks_file(project_id)?;
    let key = fingerprint::table_key(context, &path)?;
    fingerprint::check_cover(context, &key, &path, &cover)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| StorageError::WriteFailed {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    }
    Ok((None, key))
}

impl AppPaths {
    /// The path a record's own file would be at, for callers that report one back.
    pub fn describe_task_file(&self, project_id: &str) -> Result<PathBuf, StorageError> {
        Ok(self.tasks_file(project_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::{legacy_record, writer};
    use serde_json::Value;

    #[test]
    fn a_legacy_record_round_trips_with_every_field() {
        let (root, context) = writer("roundtrip");
        save_tasks(&context, "p1", &[legacy_record()]).expect("write");
        let back = load_tasks(&context, "p1").expect("read");
        assert_eq!(back.len(), 1);
        assert_eq!(
            back[0],
            legacy_record(),
            "no field may be dropped or reordered away"
        );
        let text = std::fs::read_to_string(context.paths.tasks_file("p1").unwrap()).unwrap();
        for key in [
            "claudeSessionId",
            "sessionCodexLike",
            "worktreePath",
            "archivedAt",
            "aFieldTheCoreHasNeverSeen",
        ] {
            assert!(
                text.contains(&format!("\"{key}\"")),
                "{key} missing from the file"
            );
        }
        // An explicit null must stay an explicit null: absent and null are different
        // states in this data model (plan §2.3).
        assert_eq!(
            back[0].get("attentionRequestedAt"),
            Some(&Value::Null),
            "absent-vs-null is the one distinction a flatten cannot blur"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_empty_snapshot_is_written_not_deleted() {
        let (root, context) = writer("empty");
        save_tasks(&context, "p1", &[legacy_record()]).unwrap();
        save_tasks(&context, "p1", &[]).unwrap();
        let path = context.paths.tasks_file("p1").unwrap();
        assert!(path.exists(), "the file stays, holding []");
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "[]");
        assert_eq!(load_tasks(&context, "p1").unwrap().len(), 0);
        // The shipping client deletes the file here; a transient empty list from the
        // frontend then erases a project's history with no trace. A backup keeps that
        // recoverable instead.
        let backup = path.with_extension("json.bak");
        assert!(
            backup.exists(),
            "the previous snapshot must remain recoverable"
        );
        assert!(std::fs::read_to_string(&backup)
            .unwrap()
            .contains("\"id\": \"t1\""));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unknown_project_id_is_refused_before_the_filesystem() {
        let (root, context) = writer("badid");
        assert!(matches!(
            load_tasks(&context, "../escape"),
            Err(StorageError::InvalidId(_))
        ));
        assert!(matches!(
            save_tasks(&context, "", &[]),
            Err(StorageError::InvalidId(_))
        ));
        std::fs::remove_dir_all(root).ok();
    }
}
