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

use aeroric_protocol::task::{ProtocolFamily, TaskStatus};

use crate::storage::compat::Record;
use crate::tasks::session_owner;

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
    "resumeFamily",
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

/// ADR-037 决策 4（Q71）：带前置的改名要比哪四样。
///
/// **不是只比 name。** 今天 `generateTaskName` 的守卫看的是 name / prompt / status / 归属族的
/// 会话路径四项（`src/state/app/taskNaming.ts:52-62`）。只比 name 会悄悄放宽一条现存的守卫：
/// 用户在 LLM 生成期间改了 prompt 或切了配置，过期的名字今天会被丢掉，改完就会被写回去。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameExpect {
    /// 旧客户端用 `task.name ?? ""` 比对，所以"没有 `name` 键"与"`name` 是空串"是同一种前置。
    pub name: String,
    pub prompt: String,
    pub status: TaskStatus,
    /// 归属族解析出来的那条会话路径；`None` = 当时就没有。这一样要 service 用**权威快照里的
    /// 这一行**算出来（owner 解析要家族的表），所以它是入参而不是 reducer 自己算。
    pub session_path: Option<String>,
}

/// 前置成立吗。成立 ⇒ 调用方才允许走 [`rename`]；不成立 ⇒ `changed: false`，且不报错
/// （与今天"静默丢弃过期结果"一字不变）。
pub fn rename_precondition_holds(
    record: &Record,
    expect: &RenameExpect,
    current_session_path: Option<&str>,
) -> bool {
    expect.name == record.str_field("name").unwrap_or("")
        && expect.prompt == record.str_field("prompt").unwrap_or("")
        && status_of(record) == Some(expect.status)
        && expect.session_path.as_deref() == current_session_path
}

/// `applyResolvedTaskSession`（`src/state/app/taskLifecycle.ts:33-81`）搬进 core（ADR-037 决策 1）。
///
/// 分工要说清："清四族、只设一族"的机制早就在
/// [`session_owner::apply_resolved_session`]，这里补的是旧客户端另外两条判据——**两个值都空
/// 就整条不动**（用的是 falsy，空串算空），以及缺值时回落到**清场之前**这一行自己的值。
/// 把这两条留在宿主，就等于允许"某个客户端忘了清场"这种状态存在。
pub fn attach_session(
    records: &[Record],
    task_id: &str,
    family: ProtocolFamily,
    session_agent: &str,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> (Vec<Record>, bool) {
    let blank = |value: Option<&str>| value.is_none_or(str::is_empty);
    if blank(session_id) && blank(session_path) {
        return (records.to_vec(), false);
    }
    let prefix = session_owner::family_prefix(family);
    let id_key = format!("{prefix}SessionId");
    let path_key = format!("{prefix}SessionPath");
    let owner = session_owner::SessionOwner {
        agent: session_agent.to_string(),
        family,
        codex_like: family == ProtocolFamily::Codex,
    };
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            let applied = session_owner::apply_resolved_session(
                record,
                &owner,
                // `?? task.<同族原值>`：回落读的是清场前的那一行。
                session_id.or_else(|| record.str_field(&id_key)),
                session_path.or_else(|| record.str_field(&path_key)),
            );
            if &applied == record {
                return record.clone();
            }
            changed = true;
            applied
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #5（`taskLifecycle.ts:425`）：worktree 三个字段一次挂上。值来自宿主的
/// `create_task_worktree` 命令，宿主只是搬运，规则是"要么三个一起有，要么不动"。
pub fn set_worktree(
    records: &[Record],
    task_id: &str,
    path: &str,
    branch: &str,
    base_branch: &str,
) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            let mut updated = record.clone();
            updated
                .0
                .insert("worktreePath".to_string(), Value::String(path.to_string()));
            updated.0.insert(
                "worktreeBranch".to_string(),
                Value::String(branch.to_string()),
            );
            updated.0.insert(
                "baseBranch".to_string(),
                Value::String(base_branch.to_string()),
            );
            // 挂上新的就摘掉"已丢弃"这个标记，与旧客户端 spread 之后的结果一致：一个被丢弃
            // 过的 worktree 重新挂上之后，UI 不该再显示它被丢弃。
            updated.0.remove("worktreeDiscarded");
            if &updated == record {
                return record.clone();
            }
            changed = true;
            updated
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #14（`worktreeOps.ts:20`）：丢弃**只打标记**，path/branch 故意留着——UI 还要显示这个任务
/// 曾经挂过哪儿。重复丢弃不再写一次（`changed: false`），否则每次点击都白烧一格 revision。
pub fn discard_worktree(records: &[Record], task_id: &str) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id)
                || record.get("worktreeDiscarded") == Some(&Value::Bool(true))
            {
                return record.clone();
            }
            let mut updated = record.clone();
            updated
                .0
                .insert("worktreeDiscarded".to_string(), Value::Bool(true));
            changed = true;
            updated
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #16（`useWorktreeDiffStats.ts:55`）：完成之后异步算出来的两个数字。它们**不是**用户意图，
/// 载荷里只有事实，去重靠调用方的 operation key（ADR-017）——这条意图自己不做幂等判断，
/// 因为两个数字算出同样的值与"这一对数字已经写过"是两回事。
pub fn record_diff_stats(
    records: &[Record],
    task_id: &str,
    additions: u64,
    deletions: u64,
) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) {
                return record.clone();
            }
            let mut updated = record.clone();
            updated
                .0
                .insert("additions".to_string(), Value::Number(additions.into()));
            updated
                .0
                .insert("deletions".to_string(), Value::Number(deletions.into()));
            if &updated == record {
                return record.clone();
            }
            changed = true;
            updated
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #11（`useAppStartup.ts:67` + `appProjectState.ts:171-210`）：启动修复，规则全部在这里，
/// 宿主交出来的只有事实（`live_task_ids`＝还挂着活子进程的 id、`is_dsh`＝宿主家族表对
/// `agent` 的回答）。四条判据与旧客户端一字不差：活孩子还在 ⇒ 非 `detached` 的行改
/// `detached`；孩子没了 ⇒ 非 `interrupted` 的行改 `interrupted`；两者都补
/// `attentionRequestedAt = 已有值 ?? now`；DSH 的 `fast` 清成 `standard`。已经是目标状态的行
/// **不改**——重复启动因此不烧 revision。
pub fn normalize_after_boot(
    records: &[Record],
    live_task_ids: &[String],
    now_ms: u64,
    is_dsh: &dyn Fn(&str) -> bool,
) -> (Vec<Record>, bool) {
    let live = |record: &Record| {
        record
            .id()
            .is_some_and(|id| live_task_ids.iter().any(|l| l == id))
    };
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            let mut updated = record.clone();
            let mut touched = false;
            let status = status_of(record);
            let active = status.is_some_and(|value| {
                matches!(
                    value,
                    TaskStatus::Pending | TaskStatus::Running | TaskStatus::InputRequired
                )
            });
            let interrupted_with_child = status == Some(TaskStatus::Interrupted) && live(record);
            if active || interrupted_with_child {
                let target = if live(record) {
                    TaskStatus::Detached
                } else {
                    TaskStatus::Interrupted
                };
                if status != Some(target) {
                    updated.0.insert(
                        "status".to_string(),
                        Value::String(
                            serde_json::to_value(target)
                                .expect("a status is a string")
                                .as_str()
                                .expect("a status is a string")
                                .to_string(),
                        ),
                    );
                    touched = true;
                }
                if !updated.contains("attentionRequestedAt") {
                    updated.0.insert(
                        "attentionRequestedAt".to_string(),
                        Value::Number(now_ms.into()),
                    );
                    touched = true;
                }
            }
            if record
                .get("speed")
                .and_then(Value::as_str)
                .is_some_and(|speed| speed == "fast")
                && record
                    .get("agent")
                    .and_then(Value::as_str)
                    .is_some_and(is_dsh)
            {
                updated
                    .0
                    .insert("speed".to_string(), Value::String("standard".to_string()));
                touched = true;
            }
            if touched {
                changed = true;
                updated
            } else {
                record.clone()
            }
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #20（`App.tsx:783` + `taskNaming.ts:7-15`）：todo 编辑三键。前置 `status === "todo"` 与
/// 静默不改都照旧客户端一字不差——守卫在 core（Q72 拍板：第 14 条具名意图，不借
/// 通用 patch），因为权威状态在 core，守卫留在宿主就是把竞态交给 core 看不见的那一侧。
pub fn edit_todo(
    records: &[Record],
    task_id: &str,
    prompt: &str,
    agent: &str,
    permission_mode: &str,
) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) || status_of(record) != Some(TaskStatus::Todo) {
                return record.clone();
            }
            let mut updated = record.clone();
            updated
                .0
                .insert("prompt".to_string(), Value::String(prompt.to_string()));
            updated
                .0
                .insert("agent".to_string(), Value::String(agent.to_string()));
            updated.0.insert(
                "permissionMode".to_string(),
                Value::String(permission_mode.to_string()),
            );
            if &updated == record {
                return record.clone();
            }
            changed = true;
            updated
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
}

/// #12 三路合并的落盘半边：宿主交得出 `previous → resolved` 的**变化**（它读到的旧快照和
/// 它解析出的新事实），交不出"当前行"——那份只在 core 的权威快照里。这里只应用 delta 里
/// 点名的键（`None` 值＝删键，与 JS 的 `undefined` 序列化后掉键一致），未被点名的键一个不碰
/// （ADR-037 后果：意图不重写未点名的键）。
pub fn apply_field_delta(
    records: &[Record],
    task_id: &str,
    delta: &[(&str, Option<Value>)],
) -> (Vec<Record>, bool) {
    let mut changed = false;
    let next: Vec<Record> = records
        .iter()
        .map(|record| {
            if record.id() != Some(task_id) || delta.is_empty() {
                return record.clone();
            }
            let mut updated = record.clone();
            for (key, value) in delta {
                match value {
                    Some(v) => {
                        if updated.get(key) != Some(v) {
                            updated.0.insert((*key).to_string(), v.clone());
                        }
                    }
                    None => {
                        updated.0.remove(*key);
                    }
                }
            }
            if &updated != record {
                changed = true;
                updated
            } else {
                record.clone()
            }
        })
        .collect();
    (if changed { next } else { records.to_vec() }, changed)
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

    /// ADR-037 决策 1：四族互斥是 core 的规则，不是宿主记着的清单。
    #[test]
    fn attaching_a_session_clears_the_other_three_families() {
        let records = vec![task(json!({
            "claudeSessionId": "old-claude", "claudeSessionPath": "/p/old.jsonl",
            "dshSessionId": "old-dsh", "dshSessionPath": "/p/old-dsh.json",
            "ompSessionPath": "/p/omp.jsonl",
        }))];
        let (next, changed) = attach_session(
            &records,
            "t1",
            ProtocolFamily::Codex,
            "codex",
            Some("new-codex"),
            None,
        );
        assert!(changed);
        let row = &next[0].0;
        assert_eq!(row["codexSessionId"], json!("new-codex"));
        // 事件只带来 id、没带路径 ⇒ 回落读的是这一行**自己那一族**的原值，这里没有，所以不出现。
        assert!(!row.contains_key("codexSessionPath"));
        for key in [
            "claudeSessionId",
            "claudeSessionPath",
            "dshSessionId",
            "dshSessionPath",
            "ompSessionPath",
        ] {
            assert!(!row.contains_key(key), "{key} 必须被清掉：一 task 一族");
        }
        assert_eq!(row["sessionFamily"], json!("codex"));
        assert_eq!(row["sessionAgent"], json!("codex"));
        assert_eq!(row["sessionCodexLike"], json!(true));
    }

    #[test]
    fn an_empty_session_changes_nothing() {
        let records = vec![task(json!({"claudeSessionPath": "/p/a.jsonl"}))];
        // 旧客户端的守卫是 falsy：空串与缺席同样算"没有会话"。
        for pair in [
            (None, None),
            (Some(""), None),
            (None, Some("")),
            (Some(""), Some("")),
        ] {
            let (next, changed) = attach_session(
                &records,
                "t1",
                ProtocolFamily::Claude,
                "claude",
                pair.0,
                pair.1,
            );
            assert!(!changed, "{pair:?} 不该动盘");
            assert_eq!(next, records, "{pair:?} 连记录都不该重建");
        }
    }

    #[test]
    fn a_missing_session_value_keeps_what_the_row_already_had() {
        let records = vec![task(json!({"codexSessionPath": "/p/keep.jsonl"}))];
        let (next, changed) = attach_session(
            &records,
            "t1",
            ProtocolFamily::Codex,
            "codex",
            Some("id-only"),
            None,
        );
        assert!(changed);
        assert_eq!(
            next[0].get("codexSessionPath"),
            Some(&json!("/p/keep.jsonl")),
            "`?? task.<同族原值>`：宿主只带回 id 时，路径不能被清成没有"
        );
    }

    #[test]
    fn attaching_the_same_session_twice_writes_once() {
        let records = vec![task(json!({}))];
        let (first, changed_first) = attach_session(
            &records,
            "t1",
            ProtocolFamily::Dsh,
            "dsh",
            None,
            Some("/p/d.json"),
        );
        assert!(changed_first);
        let (second, changed_again) = attach_session(
            &first,
            "t1",
            ProtocolFamily::Dsh,
            "dsh",
            None,
            Some("/p/d.json"),
        );
        assert!(
            !changed_again,
            "重复挂同一条会话不再烧一格 revision，与归档那条同一判据"
        );
        assert_eq!(second, first);
    }

    #[test]
    fn mounting_a_worktree_sets_three_fields_and_lifts_the_discarded_mark() {
        let records = vec![task(
            json!({"worktreeDiscarded": true, "worktreeBranch": "stale"}),
        )];
        let (next, changed) = set_worktree(&records, "t1", "/wt/x", "feature/a", "main");
        assert!(changed);
        let row = &next[0].0;
        assert_eq!(row["worktreePath"], json!("/wt/x"));
        assert_eq!(row["worktreeBranch"], json!("feature/a"));
        assert_eq!(row["baseBranch"], json!("main"));
        assert!(
            !row.contains_key("worktreeDiscarded"),
            "重新挂上之后不该还显示『已丢弃』——今天宿主 spread 的结果就是这样"
        );
        let (again, second) = set_worktree(&next, "t1", "/wt/x", "feature/a", "main");
        assert!(!second, "同样的三个值不再写一次");
        assert_eq!(again, next);
    }

    #[test]
    fn discarding_a_worktree_keeps_where_it_was() {
        let records = vec![task(
            json!({"worktreePath": "/wt/x", "worktreeBranch": "feature/a"}),
        )];
        let (next, changed) = discard_worktree(&records, "t1");
        assert!(changed);
        assert_eq!(next[0].get("worktreeDiscarded"), Some(&json!(true)));
        assert_eq!(next[0].get("worktreePath"), Some(&json!("/wt/x")));
        assert_eq!(next[0].get("worktreeBranch"), Some(&json!("feature/a")));
        let (again, second) = discard_worktree(&next, "t1");
        assert!(!second, "重复丢弃是幂等的");
        assert_eq!(again, next);
    }

    #[test]
    fn diff_stats_land_as_numbers_and_a_repeat_is_not_a_change() {
        let records = vec![task(json!({}))];
        let (next, changed) = record_diff_stats(&records, "t1", 12, 3);
        assert!(changed);
        assert_eq!(next[0].get("additions"), Some(&json!(12)));
        assert_eq!(next[0].get("deletions"), Some(&json!(3)));
        let (_, repeat) = record_diff_stats(&next, "t1", 12, 3);
        assert!(!repeat);
    }

    /// Q71（ADR-037 决策 4）：前置比的是**四样**。每一样单独判一次，防止"只比 name"这种
    /// 悄悄放宽——那条守卫今天真的存在（`taskNaming.ts:52-62`）。
    #[test]
    fn the_rename_precondition_checks_all_four_samples() {
        let records = [task(json!({
            "status": "running", "prompt": "hi",
            "claudeSessionPath": "/p/a.jsonl", "sessionFamily": "claude", "sessionAgent": "claude",
        }))];
        let row = &records[0];
        let want = RenameExpect {
            name: String::new(),
            prompt: "hi".to_string(),
            status: TaskStatus::Running,
            session_path: Some("/p/a.jsonl".to_string()),
        };
        assert!(
            rename_precondition_holds(row, &want, Some("/p/a.jsonl")),
            "四样全对才成立"
        );
        let renamed = rename(std::slice::from_ref(row), "t1", "有人先改了名");
        assert!(
            !rename_precondition_holds(&renamed[0], &want, Some("/p/a.jsonl")),
            "name 变了 ⇒ 不成立"
        );
        for (key, value) in [
            ("prompt", json!("changed prompt")),
            ("status", json!("done")),
            ("claudeSessionPath", json!("/p/b.jsonl")),
        ] {
            let mut moved = row.0.clone();
            moved.insert(key.to_string(), value);
            let moved = Record(moved);
            assert!(
                !rename_precondition_holds(
                    &moved,
                    &want,
                    if key == "claudeSessionPath" {
                        Some("/p/b.jsonl")
                    } else {
                        Some("/p/a.jsonl")
                    }
                ),
                "{key} 变了也必须不成立：只比 name 是放宽"
            );
        }
    }

    /// 宿主以为"没有 name"，行上也确实没有 ⇒ 前置成立。空串与缺席是同一种前置（JS 用
    /// `task.name ?? ""`），把这条钉住，免得有人拿 `Option` 的有无去做判等。
    #[test]
    fn an_absent_name_and_an_empty_name_are_the_same_precondition() {
        let records = [task(json!({
            "status": "running", "prompt": "hi", "name": "",
        }))];
        let want = RenameExpect {
            name: String::new(),
            prompt: "hi".to_string(),
            status: TaskStatus::Running,
            session_path: None,
        };
        assert!(rename_precondition_holds(
            &records[0],
            &want,
            records[0].get("claudeSessionPath").and_then(Value::as_str)
        ));
    }

    #[test]
    fn editing_a_todo_writes_three_keys_and_a_non_todo_row_is_untouched() {
        let records = vec![task(json!({"status": "todo", "prompt": "旧"}))];
        let (next, changed) = edit_todo(&records, "t1", "新 prompt", "codex", "full_access");
        assert!(changed);
        let row = &next[0].0;
        assert_eq!(row["prompt"], json!("新 prompt"));
        assert_eq!(row["agent"], json!("codex"));
        assert_eq!(row["permissionMode"], json!("full_access"));
        assert_eq!(row["status"], json!("todo"), "前置成立也不改状态");

        let started = vec![task(json!({"status": "running"}))];
        let (same, changed) = edit_todo(&started, "t1", "p", "a", "ask");
        assert!(
            !changed,
            "status!==\"todo\" ⇒ 静默不改，与 updateTodoTaskInList 一致"
        );
        assert_eq!(same, started);

        let (again, second) = edit_todo(&next, "t1", "新 prompt", "codex", "full_access");
        assert!(!second, "同一份值改第二遍不烧 revision");
        assert_eq!(again, next);
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
