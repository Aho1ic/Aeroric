//! The shapes files already on users' disks come in, and what survives a round trip.
//!
//! What every file shares lives here: the record, the pretty-printing that keeps a diff
//! readable for a human, and the rule that an unreadable file is never overwritten. The
//! per-file readers are [`super::projects`], [`super::tasks`] and [`super::settings`].
//!
//! Deliberately untyped. Re-declaring the shipping client's `Project`/`Task` structs in
//! the core is how fields go missing — serde drops what a struct does not name, and the
//! legacy code has already been bitten by that (the `Task` comment in
//! `src-tauri/src/storage.rs:96-99` describes fields that existed only in the frontend
//! and were silently discarded on write). A record here keeps every key it was given, so
//! the core can move a file it does not fully understand.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;

use crate::context::CoreContext;
use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};

/// One persisted object, keys and all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Record(pub Map<String, Value>);

impl Record {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    pub fn id(&self) -> Option<&str> {
        self.str_field("id")
    }

    pub fn with(mut self, key: &str, value: Value) -> Self {
        self.0.insert(key.to_string(), value);
        self
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// A record with no keys is a valid JSON object and a suspicious persisted row; the
    /// distinction matters enough to name it.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }
}

/// Pretty-printed exactly like the shipping client writes it, so a file that both
/// writers have touched keeps a readable diff for the user.
pub(crate) fn pretty(value: &Value) -> Vec<u8> {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string());
    format!("{text}\n").into_bytes()
}

pub(crate) fn read_array(path: &Path) -> Result<ReadOutcome<Vec<Record>>, StorageError> {
    match atomic::read_json(path)? {
        ReadOutcome::Absent => Ok(ReadOutcome::Absent),
        ReadOutcome::Found(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::Object(fields) => Ok(Record(fields)),
                other => Err(StorageError::Corrupt {
                    path: path.to_path_buf(),
                    reason: format!(
                        "array element is {other}, every persisted record must be an object"
                    ),
                }),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(ReadOutcome::Found),
        ReadOutcome::Found(other) => Err(shape_error(path, &other, "a top-level array")),
    }
}

pub(crate) fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "a boolean".to_string(),
        Value::Number(_) => "a number".to_string(),
        Value::String(_) => "a string".to_string(),
        Value::Array(_) => "an array".to_string(),
        Value::Object(_) => "an object".to_string(),
    }
}

/// The refusal every shape check gives when the document is a different JSON type.
pub(crate) fn shape_error(path: &Path, found: &Value, expected: &str) -> StorageError {
    StorageError::Corrupt {
        path: path.to_path_buf(),
        reason: format!("expected {expected}, found {}", describe(found)),
    }
}

pub(crate) fn write_array(path: &Path, records: &[Record]) -> Result<(), StorageError> {
    // Read first: a file we cannot parse is not ours to overwrite, however valid the
    // incoming snapshot looks.
    atomic::ensure_replaceable(path)?;
    let items: Vec<Value> = records
        .iter()
        .map(|record| Value::Object(record.0.clone()))
        .collect();
    atomic::replace_with_backup(path, &pretty(&Value::Array(items)), FileMode::Private)?;
    Ok(())
}

pub(crate) fn require_writer(context: &CoreContext) -> Result<(), StorageError> {
    context.require_write()?;
    Ok(())
}

/// Apply one change to a record set without the caller re-typing any field: unknown
/// keys survive because only the matched record is rebuilt from its own map.
pub fn update_record(records: &mut [Record], id: &str, change: impl FnOnce(&mut Record)) -> bool {
    match records.iter_mut().find(|record| record.id() == Some(id)) {
        Some(record) => {
            change(record);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::{legacy_record, writer};
    use crate::storage::{load_projects, load_tasks};
    use serde_json::json;

    #[test]
    fn a_non_array_document_is_refused_rather_than_read_as_empty() {
        let (root, context) = writer("notarray");
        context.paths.ensure_private().unwrap();
        std::fs::write(context.paths.projects_file(), b"{\"id\":\"p\"}").unwrap();
        let error = load_projects(&context).expect_err("object where array is expected");
        assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
        assert!(error.to_string().contains("top-level array"), "{error}");
        std::fs::create_dir_all(context.paths.project_dir("p2").unwrap()).unwrap();
        std::fs::write(
            context.paths.tasks_file("p2").unwrap(),
            br#"["a bare string, not a record"]"#,
        )
        .unwrap();

        let error = load_tasks(&context, "p2").expect_err("string element");
        assert!(error.to_string().contains("must be an object"), "{error}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn updating_a_record_touches_only_that_record() {
        let mut records = vec![
            legacy_record(),
            Record(
                json!({"id": "t2", "status": "todo"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ];
        let changed = update_record(&mut records, "t1", |record| {
            let mut next = record.0.clone();
            next.insert("status".into(), json!("done"));
            *record = Record(next);
        });
        assert!(changed);
        assert!(!update_record(&mut records, "nope", |_| {}));
        assert_eq!(records[0].get("status"), Some(&json!("done")));
        assert_eq!(
            records[0].len(),
            legacy_record().len(),
            "the field count must not shrink"
        );
        assert_eq!(records[1].get("status"), Some(&json!("todo")));
    }
}
