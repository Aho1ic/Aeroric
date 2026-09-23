//! The task state machine, with no view attached to it.
//!
//! Every rule here was written down twice before this file existed: once in
//! `src/state/app/taskStatus.ts` / `taskMutations.ts` / `taskSwitch.ts`, and once as an
//! expectation in `src/test/task-status-transitions.test.ts`. The core gets the same
//! rules, and the parity table in
//! [`aeroric-protocol`](https://github.com/Aeroric/Aeroric) replays the client's own
//! answers, so a change to either side that the other does not know about is a red gate
//! rather than a rumour.
//!
//! Records stay untyped (see [`crate::storage::compat`]). That is what lets the core move
//! a task it does not fully understand without losing a field, and it is why the helpers
//! below name the keys they touch instead of declaring a `Task` struct.

use serde_json::Value;

use aeroric_protocol::task::TaskStatus;

use crate::storage::compat::Record;

/// A field the reducer writes is either a number or *gone*. `Clear` is deliberately not
/// `None`: the client spreads `undefined` into the object, which drops the key when it
/// serialises, and "key absent" and "key holds null" are different persisted states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Set(u64),
    Clear,
}

/// What one status event produced. `changed` is the whole answer to "may we skip the
/// write?": a duplicate hook that reports the state the task is already in must not
/// reach the disk, let alone bump the revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub records: Vec<Record>,
    pub changed: bool,
    /// The task after the event, when this task set holds it at all.
    pub task: Option<Record>,
}

pub fn status_of(record: &Record) -> Option<TaskStatus> {
    record
        .get("status")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_value(Value::String(text.to_string())).ok())
}

fn write_field(record: &mut Record, key: &str, value: Field) {
    match value {
        Field::Set(number) => {
            record
                .0
                .insert(key.to_string(), Value::Number(number.into()));
        }
        Field::Clear => {
            record.0.remove(key);
        }
    }
}

/// The one migration guard the client has: once a task is `detached`, a late
/// `running`/`input_required` from a hook is dropped rather than dragging a task that is
/// no longer attached back into the live set. `interrupted` is not guarded — resuming is
/// allowed to move it, and that path is the point of the state.
pub fn should_ignore_transition(current: Option<TaskStatus>, next: TaskStatus) -> bool {
    current == Some(TaskStatus::Detached)
        && matches!(next, TaskStatus::Running | TaskStatus::InputRequired)
}

/// `applyTaskStatusTransition` in `src/state/app/taskStatus.ts`.
pub fn apply_status_transition(
    records: &[Record],
    task_id: &str,
    status: TaskStatus,
    attention_requested_at: Option<u64>,
    failure_reason: Option<&str>,
    now_ms: u64,
) -> Transition {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            if should_ignore_transition(status_of(record), status) {
                return record.clone();
            }
            let attention = if status == TaskStatus::InputRequired {
                Field::Set(attention_requested_at.unwrap_or(now_ms))
            } else {
                Field::Clear
            };
            // First value wins and never refreshes: `failed → cancelled` must not move the
            // clock on a task that had already ended. Leaving a terminal status clears it,
            // or a resumed task would be reported as having finished hours ago.
            let completed = if status.is_terminal() {
                match record.get("completedAt") {
                    Some(value @ Value::Number(_)) => Field::Set(
                        value
                            .as_u64()
                            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
                            .unwrap_or(now_ms),
                    ),
                    _ => Field::Set(now_ms),
                }
            } else {
                Field::Clear
            };

            let untouched = status_of(record) == Some(status)
                && field_matches(record, "attentionRequestedAt", &attention)
                && field_matches(record, "completedAt", &completed);
            if untouched {
                return record.clone();
            }
            changed = true;
            let mut updated = record.clone();
            updated.0.insert(
                "status".to_string(),
                Value::String(
                    serde_json::to_value(status)
                        .expect("a status is a string")
                        .as_str()
                        .expect("a status is a string")
                        .to_string(),
                ),
            );
            write_field(&mut updated, "attentionRequestedAt", attention);
            write_field(&mut updated, "completedAt", completed);
            if status == TaskStatus::Failed {
                if let Some(reason) = failure_reason {
                    updated.0.insert(
                        "failureReason".to_string(),
                        Value::String(reason.to_string()),
                    );
                }
            }
            updated
        })
        .collect();
    let task = next
        .iter()
        .find(|record| record.id() == Some(task_id))
        .cloned();
    Transition {
        records: if changed { next } else { records.to_vec() },
        changed,
        task,
    }
}

fn field_matches(record: &Record, key: &str, want: &Field) -> bool {
    match (record.get(key), want) {
        (None, Field::Clear) => true,
        (Some(Value::Number(have)), Field::Set(want)) => have.as_u64() == Some(*want),
        _ => false,
    }
}

/// `isArchivableTaskStatus`: everything except the two states that are still moving. This
/// is wider than "terminal" on purpose — `todo`, `interrupted` and `detached` are all
/// archivable, so a comparison against [`TaskStatus::is_terminal`] would be a different
/// rule, not a tidier spelling of the same one.
pub fn is_archivable(record: &Record) -> bool {
    match status_of(record) {
        Some(status) => !matches!(status, TaskStatus::Running | TaskStatus::Pending),
        // A status the core cannot read is neither of the two it would have excluded.
        None => true,
    }
}

fn has_number(record: &Record, key: &str) -> bool {
    record.get(key).is_some_and(Value::is_number)
}

/// `archiveTasksInList`. Only rows that are archivable and not already archived move, and
/// an empty change set returns the input untouched so the caller can skip the write.
pub fn archive(records: &[Record], task_ids: &[String], now_ms: u64) -> (Vec<Record>, bool) {
    let selected: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record
                .id()
                .is_some_and(|id| task_ids.iter().any(|w| w == id))
                && is_archivable(record)
                && !has_number(record, "archivedAt")
        })
        .map(|(index, _)| index)
        .collect();
    if selected.is_empty() {
        return (records.to_vec(), false);
    }
    let mut next = records.to_vec();
    for index in selected {
        next[index]
            .0
            .insert("archivedAt".to_string(), Value::Number(now_ms.into()));
    }
    (next, true)
}

/// `unarchiveTasksInList`.
pub fn unarchive(records: &[Record], task_ids: &[String]) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if !record
                .id()
                .is_some_and(|id| task_ids.iter().any(|w| w == id))
                || !has_number(record, "archivedAt")
            {
                return record.clone();
            }
            changed = true;
            let mut cleared = record.clone();
            cleared.0.remove("archivedAt");
            cleared
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// `toggleTaskStarInList`. Absent and `false` are the same state on disk; the client
/// writes a real boolean back, and so does this.
pub fn toggle_star(records: &[Record], task_id: &str) -> Vec<Record> {
    records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            let mut next = record.clone();
            let starred = record
                .get("starred")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            next.0.insert("starred".to_string(), Value::Bool(!starred));
            next
        })
        .collect()
}

/// `renameTaskInList`: an empty name is not a name, and the client drops the key rather
/// than persisting `""`, which would leave a task with a blank label that no code path can
/// clear again.
pub fn rename(records: &[Record], task_id: &str, name: &str) -> Vec<Record> {
    records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            let mut next = record.clone();
            if name.is_empty() {
                next.0.remove("name");
            } else {
                next.0
                    .insert("name".to_string(), Value::String(name.to_string()));
            }
            next
        })
        .collect()
}

/// `rollbackTaskMutation` in `src/state/app/taskLifecycle.ts` — the field-level rollback
/// that makes "persist, and undo it if the persist failed" safe under a concurrent write.
///
/// Three states per field, and only this rule is correct: undo the field when this change
/// actually moved it (`previous != applied`) *and* the value now on the record is still
/// what this change wrote (`present == applied`). A late hook that landed in between is
/// kept. Rolling the whole record back to `previous` — the obvious implementation — is the
/// bug this function exists to avoid.
pub fn rollback_mutation(current: &Record, previous: &Record, applied: &Record) -> Record {
    let mut keys: Vec<&String> = previous.0.keys().chain(applied.0.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut restored = current.clone();
    for key in keys {
        let before = previous.get(key);
        let after = applied.get(key);
        if before == after || current.get(key) != after {
            continue;
        }
        match previous.0.get(key) {
            Some(value) => {
                restored.0.insert(key.clone(), value.clone());
            }
            None => {
                restored.0.remove(key);
            }
        }
    }
    restored
}

/// The session-field subset of `mergeChangedTaskSessionFields` in
/// `src/state/app/taskSwitch.ts`: a config switch that failed must give back exactly the
/// fields it changed, so a session id the agent wrote while the switch was in flight
/// survives the rollback.
pub const SESSION_FIELDS: &[&str] = &[
    "claudeSessionId",
    "claudeSessionPath",
    "codexSessionId",
    "codexSessionPath",
    "dshSessionId",
    "dshSessionPath",
    "ompSessionId",
    "ompSessionPath",
    "sessionAgent",
    "sessionCodexLike",
    "sessionFamily",
];

/// `mergeChangedTaskSessionFields`: take `source`'s value for every session field this
/// change moved relative to `previous`, leave everything else on `current` alone.
pub fn merge_changed_session_fields(
    current: &Record,
    previous: &Record,
    source: &Record,
) -> Record {
    let mut next = current.clone();
    for key in SESSION_FIELDS {
        if source.get(key) == previous.get(key) {
            continue;
        }
        match source.0.get(*key) {
            Some(value) => {
                next.0.insert((*key).to_string(), value.clone());
            }
            None => {
                next.0.remove(*key);
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(extra: Value) -> Record {
        let mut base = json!({
            "id": "t1", "projectId": "p1", "prompt": "hi", "agent": "claude",
            "status": "running", "createdAt": 100i64
        });
        let object = base.as_object_mut().unwrap();
        if let Value::Object(fields) = extra {
            for (key, value) in fields {
                object.insert(key, value);
            }
        }
        Record(object.clone())
    }

    #[test]
    fn entering_a_terminal_status_stamps_the_end_time_once() {
        let records = vec![task(json!({}))];
        let first = apply_status_transition(&records, "t1", TaskStatus::Done, None, None, 500);
        assert!(first.changed);
        assert_eq!(
            first.task.as_ref().unwrap().get("completedAt"),
            Some(&json!(500))
        );
        // A second terminal event must not move the clock.
        let again =
            apply_status_transition(&first.records, "t1", TaskStatus::Cancelled, None, None, 900);
        assert!(again.changed, "the status itself did change");
        assert_eq!(
            again.task.as_ref().unwrap().get("completedAt"),
            Some(&json!(500)),
            "but the end time belongs to the first one to arrive"
        );
    }

    #[test]
    fn leaving_a_terminal_status_clears_the_end_time() {
        let records = vec![task(json!({"status": "failed", "completedAt": 400i64}))];
        let next = apply_status_transition(&records, "t1", TaskStatus::Pending, None, None, 700);
        assert!(next.changed);
        assert_eq!(next.task.as_ref().unwrap().get("completedAt"), None);
    }

    #[test]
    fn a_detached_task_drops_late_live_events_and_nothing_else() {
        let records = vec![task(json!({"status": "detached"}))];
        for status in [TaskStatus::Running, TaskStatus::InputRequired] {
            let outcome = apply_status_transition(&records, "t1", status, None, None, 800);
            assert!(!outcome.changed, "{status:?} must be ignored after detach");
            assert_eq!(outcome.records, records);
        }
        for status in [TaskStatus::Done, TaskStatus::Failed, TaskStatus::Cancelled] {
            assert!(
                apply_status_transition(&records, "t1", status, None, None, 800).changed,
                "{status:?} is a terminal event and detach does not protect against it"
            );
        }
    }

    #[test]
    fn a_repeated_event_that_changes_nothing_writes_nothing() {
        let records = vec![task(json!({}))];
        let outcome = apply_status_transition(&records, "t1", TaskStatus::Running, None, None, 800);
        assert!(!outcome.changed, "running → running is the same task");
        assert_eq!(outcome.records, records);
    }

    #[test]
    fn an_explicit_null_is_not_the_same_state_as_an_absent_field() {
        // The client spreads `undefined`, which drops the key; a file that still carries
        // `"attentionRequestedAt": null` is a different document and must be rewritten.
        let records = vec![task(json!({"attentionRequestedAt": null}))];
        let outcome = apply_status_transition(&records, "t1", TaskStatus::Running, None, None, 800);
        assert!(outcome.changed, "null is not absent");
        assert_eq!(
            outcome.task.as_ref().unwrap().get("attentionRequestedAt"),
            None
        );
        let settled =
            apply_status_transition(&outcome.records, "t1", TaskStatus::Running, None, None, 801);
        assert!(!settled.changed, "and once cleared it stays cleared");
    }

    #[test]
    fn only_input_required_carries_an_attention_stamp() {
        let records = vec![task(json!({}))];
        let asked =
            apply_status_transition(&records, "t1", TaskStatus::InputRequired, None, None, 640);
        assert_eq!(
            asked.task.as_ref().unwrap().get("attentionRequestedAt"),
            Some(&json!(640)),
            "with no explicit stamp the event time is the stamp"
        );
        let explicit = apply_status_transition(
            &records,
            "t1",
            TaskStatus::InputRequired,
            Some(300),
            None,
            640,
        );
        assert_eq!(
            explicit.task.as_ref().unwrap().get("attentionRequestedAt"),
            Some(&json!(300)),
            "the hook's own timestamp wins over the clock"
        );
        let moved =
            apply_status_transition(&asked.records, "t1", TaskStatus::Running, None, None, 700);
        assert_eq!(
            moved.task.as_ref().unwrap().get("attentionRequestedAt"),
            None
        );
    }

    #[test]
    fn a_failure_keeps_its_reason_and_a_later_success_lets_it_stand() {
        let records = vec![task(json!({}))];
        let failed = apply_status_transition(
            &records,
            "t1",
            TaskStatus::Failed,
            None,
            Some("spawn EACCES"),
            900,
        );
        assert_eq!(
            failed.task.as_ref().unwrap().get("failureReason"),
            Some(&json!("spawn EACCES"))
        );
        let done =
            apply_status_transition(&failed.records, "t1", TaskStatus::Done, None, None, 950);
        assert_eq!(
            done.task.as_ref().unwrap().get("failureReason"),
            Some(&json!("spawn EACCES")),
            "the client clears it on resume, not on any later transition"
        );
    }

    #[test]
    fn an_unknown_task_id_changes_nothing_but_still_reports_the_absence() {
        let records = vec![task(json!({}))];
        let outcome = apply_status_transition(&records, "ghost", TaskStatus::Done, None, None, 1);
        assert!(!outcome.changed);
        assert!(outcome.task.is_none());
    }

    #[test]
    fn archiving_skips_moving_tasks_already_archived_and_unknown_ids() {
        let running = task(json!({"id": "r", "status": "running"}));
        let todo = task(json!({"id": "d", "status": "todo"}));
        let archived = task(json!({"id": "a", "status": "done", "archivedAt": 1i64}));
        let ids: Vec<String> = ["r", "d", "a", "missing"]
            .iter()
            .map(|id| (*id).to_string())
            .collect();
        let (next, changed) = archive(&[running, todo.clone(), archived], &ids, 50);
        assert!(changed);
        assert_eq!(
            next.iter()
                .find(|r| r.id() == Some("d"))
                .unwrap()
                .get("archivedAt"),
            Some(&json!(50)),
            "todo is archivable: the rule is 'not still moving', not 'finished'"
        );
        assert_eq!(
            next.iter()
                .find(|r| r.id() == Some("r"))
                .unwrap()
                .get("archivedAt"),
            None
        );
        assert_eq!(
            next.iter()
                .find(|r| r.id() == Some("a"))
                .unwrap()
                .get("archivedAt"),
            Some(&json!(1)),
            "the first archive time survives a second call"
        );
        let (again, second) = archive(&next, &ids, 60);
        assert!(!second, "and a repeat is a no-op, not a rewrite");
        assert_eq!(again, next);
        let (back, undone) = unarchive(&next, &["d".to_string(), "nope".to_string()]);
        assert!(undone);
        assert_eq!(
            back.iter()
                .find(|r| r.id() == Some("d"))
                .unwrap()
                .get("archivedAt"),
            None
        );
        let (_, nothing) = unarchive(&back, &["nope".to_string()]);
        assert!(!nothing);
    }

    #[test]
    fn a_blank_name_is_removed_rather_than_persisted_empty() {
        let records = vec![task(json!({"name": "old"}))];
        let renamed = rename(&records, "t1", "");
        assert_eq!(renamed[0].get("name"), None);
        let renamed = rename(&renamed, "t1", "new");
        assert_eq!(renamed[0].get("name"), Some(&json!("new")));
        let starred = toggle_star(&records, "t1");
        assert_eq!(starred[0].get("starred"), Some(&json!(true)));
        let starred = toggle_star(&starred, "t1");
        assert_eq!(starred[0].get("starred"), Some(&json!(false)));
    }

    #[test]
    fn a_rollback_undoes_only_what_this_change_wrote_and_nobody_else_touched() {
        let previous = task(json!({"status": "todo", "name": "keep"}));
        let applied = {
            let mut record = previous.clone();
            record.0.insert("status".to_string(), json!("pending"));
            record.0.insert("name".to_string(), json!("renamed"));
            record
        };
        // A hook landed while the save was in flight: it moved status again and set a
        // field this change never touched.
        let current = {
            let mut record = applied.clone();
            record.0.insert("status".to_string(), json!("running"));
            record.0.insert("completedAt".to_string(), json!(77));
            record
        };
        let restored = rollback_mutation(&current, &previous, &applied);
        assert_eq!(
            restored.get("status"),
            Some(&json!("running")),
            "the late event wins — this change's value is no longer there to undo"
        );
        assert_eq!(
            restored.get("name"),
            Some(&json!("keep")),
            "the field only this change moved goes back"
        );
        assert_eq!(
            restored.get("completedAt"),
            Some(&json!(77)),
            "a field this change never touched is not theirs to remove"
        );
    }

    #[test]
    fn a_rollback_deletes_a_key_this_change_added() {
        let previous = task(json!({}));
        let mut applied = previous.clone();
        applied.0.insert("failureReason".to_string(), json!("boom"));
        let mut current = applied.clone();
        current.0.insert("extra".to_string(), json!(1));
        let restored = rollback_mutation(&current, &previous, &applied);
        assert_eq!(
            restored.get("failureReason"),
            None,
            "adding then rolling back is removal"
        );
        assert_eq!(restored.get("extra"), Some(&json!(1)));
    }

    #[test]
    fn session_merge_takes_only_the_fields_the_switch_moved() {
        let previous = task(json!({"claudeSessionId": "old", "dshSessionId": "d1"}));
        let mut source = previous.clone();
        source.0.insert("claudeSessionId".to_string(), json!("new"));
        source.0.remove("dshSessionId");
        // Meanwhile the running agent wrote its own session id.
        let mut current = previous.clone();
        current
            .0
            .insert("codexSessionId".to_string(), json!("late"));
        let merged = merge_changed_session_fields(&current, &previous, &source);
        assert_eq!(merged.get("claudeSessionId"), Some(&json!("new")));
        assert_eq!(
            merged.get("dshSessionId"),
            None,
            "a field the switch cleared stays cleared"
        );
        assert_eq!(
            merged.get("codexSessionId"),
            Some(&json!("late")),
            "and a field it never touched survives untouched"
        );
    }
}
