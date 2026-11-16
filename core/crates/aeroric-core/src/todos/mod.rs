//! 2026-11-15 工作安排第一批: 待办 — written by the user, handed to an Agent
//! by hand, accepted by the user.
//!
//! - Todos live in one JSON file in the data folder, written whole and
//!   atomically, like the schedules.
//! - Handing a todo out opens a new session (the host's) and records an
//!   [`Assignment`]; it can be handed out again, and the latest counts.
//! - A session that ended is not a todo done: [`progress`] reads the latest
//!   session's status as `Review`, and only the user's `Accept` closes it.
//!
//! 第二批:
//! - [`Todos::propose`]: an Agent's proposal waits as `Proposed`; a session
//!   has at most `PROPOSALS_PER_SESSION` waiting, and a title already open
//!   in the project is refused.
//! - 验收检查: [`Todos::due_checks`] names the todos whose latest session
//!   ended a turn not yet checked; the host runs the command between
//!   [`Todos::begin_check`] and [`Todos::finish_check`] (one run per todo at
//!   a time). A failing check may go back into the session ([`fix_prompt`]),
//!   at most `FIX_ROUNDS` times per hand-out.

use std::path::{Path, PathBuf};

use aeroric_protocol::todo::{
    Assignment, CheckRun, CheckState, Decision, Progress, Todo, TodoState, FIX_ROUNDS,
    PROPOSALS_PER_SESSION, TITLE_CHARS,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::sessions::turn_outcome::NotRun;

/// A session as the host sees it.
pub struct SessionView {
    /// The task record's status (`running`, `done`, …).
    pub status: String,
    /// The last turn did not run.
    pub not_run: Option<NotRun>,
    /// Why the task failed, when it did.
    pub failure: Option<String>,
    /// When the last turn started (a session that is not running).
    pub last_turn_ms: Option<u64>,
}

/// A 验收检查 to run now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueCheck {
    pub todo_id: String,
    pub project_id: String,
    pub task_id: String,
    pub command: String,
    pub turn_ms: u64,
}

/// What the todo list needs from the host.
pub trait TodoEnv {
    /// `None`: the session is gone.
    fn session(&self, task_id: &str) -> Option<SessionView>;
}

#[derive(Default, Serialize, Deserialize)]
struct Document {
    #[serde(default)]
    todos: Vec<Todo>,
}

pub struct Todos {
    path: PathBuf,
    lock: Mutex<()>,
}

/// How the latest session stands, and why when it did not go well.
pub fn progress(view: Option<&SessionView>) -> (Progress, Option<String>) {
    let Some(view) = view else {
        return (Progress::Missing, Some("会话已经不在了".into()));
    };
    match view.status.as_str() {
        "pending" | "running" => (Progress::Running, None),
        "input_required" => (Progress::Waiting, None),
        "failed" => (Progress::Failed, view.failure.clone()),
        "interrupted" | "cancelled" => (Progress::Stopped, None),
        _ => match &view.not_run {
            Some(not_run) => (
                Progress::NotRun,
                Some(not_run.message.clone()).filter(|m| !m.trim().is_empty()),
            ),
            None => (Progress::Review, None),
        },
    }
}

/// What the session is told: the todo, its 验收检查, what failed last
/// time, and what is expected back.
pub fn prompt_for(todo: &Todo) -> String {
    let mut text = todo.title.trim().to_string();
    let notes = todo.notes.trim();
    if !notes.is_empty() {
        text.push_str(&format!("\n\n{notes}"));
    }
    if let Some(command) = &todo.check {
        text.push_str(&format!(
            "\n\n验收检查：`{command}`。这一轮结束后 Aeroric 会在项目目录里运行它，退出码为 0 才算通过。"
        ));
        let failed = todo
            .latest()
            .and_then(|a| a.check.as_ref())
            .filter(|c| c.state == CheckState::Failed);
        if let Some(run) = failed {
            text.push_str(&format!(
                "\n\n上一次交办后验收检查没过，输出：\n```\n{}\n```",
                run.output.trim()
            ));
        }
    }
    text.push_str("\n\n做完后用几句话说明：改了什么、怎么验证的、还有什么没做完。");
    text
}

/// What a failing check sends back into the session (`round` of
/// `FIX_ROUNDS`).
pub fn fix_prompt(command: &str, output: &str, round: u32) -> String {
    format!(
        "验收检查没过（第 {round}/{FIX_ROUNDS} 次请你接着修）。\n\n命令：`{command}`\n\n输出：\n```\n{}\n```\n\n请改到这条命令通过，做完同样说明改了什么、怎么验证的。",
        output.trim()
    )
}

/// The same words, any case and spacing at the ends.
fn same_title(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn clean_check(check: Option<String>) -> Option<String> {
    check
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

fn new_id() -> String {
    format!("todo-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

impl Todos {
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("todos.json"),
            lock: Mutex::new(()),
        }
    }

    fn load(&self) -> Document {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self, doc: &Document) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// The todos (of one project, or all), open and assigned first, newest
    /// first; an assigned one carries how its latest session stands.
    pub fn list(&self, env: &dyn TodoEnv, project_id: Option<&str>) -> Vec<Todo> {
        let _guard = self.lock.lock();
        let mut todos: Vec<Todo> = self
            .load()
            .todos
            .into_iter()
            .filter(|t| project_id.is_none_or(|p| t.project_id == p))
            .collect();
        for todo in &mut todos {
            if todo.state != TodoState::Assigned {
                continue;
            }
            let view = todo
                .assignments
                .last()
                .and_then(|a| env.session(&a.task_id));
            let (progress, detail) = progress(view.as_ref());
            todo.progress = Some(progress);
            todo.progress_detail = detail;
        }
        let rank = |t: &Todo| match t.state {
            TodoState::Proposed | TodoState::Assigned | TodoState::Open => 0,
            TodoState::Done | TodoState::Cancelled => 1,
        };
        todos.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.updated_ms.cmp(&a.updated_ms)));
        todos
    }

    /// Add a todo (no id) or change its title, notes and 验收检查.
    pub fn save_todo(&self, mut todo: Todo, now_ms: u64) -> Result<Todo, String> {
        todo.check = clean_check(todo.check);
        todo.fix_on_fail = todo.fix_on_fail && todo.check.is_some();
        todo.title = todo.title.trim().chars().take(TITLE_CHARS).collect();
        if todo.title.is_empty() {
            return Err("写上要做什么".into());
        }
        if todo.project_id.trim().is_empty() {
            return Err("选一个项目".into());
        }
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let saved = match doc
            .todos
            .iter_mut()
            .find(|t| !todo.id.is_empty() && t.id == todo.id)
        {
            Some(existing) => {
                existing.title = todo.title;
                existing.notes = todo.notes.trim().to_string();
                existing.check = todo.check;
                existing.fix_on_fail = todo.fix_on_fail;
                existing.updated_ms = now_ms;
                existing.clone()
            }
            None => {
                let fresh = Todo {
                    id: new_id(),
                    project_id: todo.project_id,
                    title: todo.title,
                    notes: todo.notes.trim().to_string(),
                    state: TodoState::Open,
                    assignments: Vec::new(),
                    created_ms: now_ms,
                    updated_ms: now_ms,
                    closed_ms: None,
                    progress: None,
                    progress_detail: None,
                    proposed_by: None,
                    check: todo.check,
                    fix_on_fail: todo.fix_on_fail,
                };
                doc.todos.push(fresh.clone());
                fresh
            }
        };
        self.save(&doc)?;
        Ok(saved)
    }

    /// An Agent's proposal from session `task_id`: it waits for the user.
    pub fn propose(
        &self,
        project_id: &str,
        title: &str,
        notes: &str,
        task_id: &str,
        now_ms: u64,
    ) -> Result<Todo, String> {
        let title: String = title.trim().chars().take(TITLE_CHARS).collect();
        if title.is_empty() {
            return Err("写上要做什么".into());
        }
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let live = |t: &&Todo| {
            matches!(
                t.state,
                TodoState::Proposed | TodoState::Open | TodoState::Assigned
            )
        };
        if let Some(twin) = doc
            .todos
            .iter()
            .filter(live)
            .find(|t| t.project_id == project_id && same_title(&t.title, &title))
        {
            return Err(format!("已经有这个待办了：[{}] {}", twin.id, twin.title));
        }
        let waiting = doc
            .todos
            .iter()
            .filter(|t| t.state == TodoState::Proposed && t.proposed_by.as_deref() == Some(task_id))
            .count();
        if waiting >= PROPOSALS_PER_SESSION {
            return Err(format!(
                "这个会话已经有 {PROPOSALS_PER_SESSION} 条提议在等用户处理，先不要再提"
            ));
        }
        let fresh = Todo {
            id: new_id(),
            project_id: project_id.to_string(),
            title,
            notes: notes.trim().to_string(),
            state: TodoState::Proposed,
            assignments: Vec::new(),
            created_ms: now_ms,
            updated_ms: now_ms,
            closed_ms: None,
            progress: None,
            progress_detail: None,
            proposed_by: Some(task_id.to_string()),
            check: None,
            fix_on_fail: false,
        };
        doc.todos.push(fresh.clone());
        self.save(&doc)?;
        Ok(fresh)
    }

    /// Some todo handed this session out.
    pub fn knows_task(&self, task_id: &str) -> bool {
        let _guard = self.lock.lock();
        self.load()
            .todos
            .iter()
            .any(|t| t.assignments.iter().any(|a| a.task_id == task_id))
    }

    pub fn get(&self, id: &str) -> Option<Todo> {
        let _guard = self.lock.lock();
        self.load().todos.into_iter().find(|t| t.id == id)
    }

    /// `false` when there was none.
    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let before = doc.todos.len();
        doc.todos.retain(|t| t.id != id);
        if doc.todos.len() == before {
            return Ok(false);
        }
        self.save(&doc)?;
        Ok(true)
    }

    /// The todo was handed out: `assignment`'s session is the one that
    /// counts now. A done or cancelled todo is handed out again too.
    pub fn assigned(&self, id: &str, assignment: Assignment) -> Result<Todo, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let todo = doc
            .todos
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("没有这个待办")?;
        if todo.state == TodoState::Proposed {
            return Err("先收下这条提议再交办".into());
        }
        todo.updated_ms = assignment.at_ms;
        todo.assignments.push(assignment);
        todo.state = TodoState::Assigned;
        todo.closed_ms = None;
        let saved = todo.clone();
        self.save(&doc)?;
        Ok(saved)
    }

    pub fn decide(&self, id: &str, decision: Decision, now_ms: u64) -> Result<Todo, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let todo = doc
            .todos
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("没有这个待办")?;
        match decision {
            Decision::Take if todo.state != TodoState::Proposed => {
                return Err("这条不是待确认的提议".into())
            }
            Decision::Take => todo.state = TodoState::Open,
            Decision::Accept if todo.state != TodoState::Assigned => {
                return Err("交办出去之后才能验收".into())
            }
            Decision::Accept => {
                todo.state = TodoState::Done;
                todo.closed_ms = Some(now_ms);
            }
            Decision::Cancel => {
                todo.state = TodoState::Cancelled;
                todo.closed_ms = Some(now_ms);
            }
            Decision::Reopen if todo.state == TodoState::Proposed => {
                return Err("这条还是提议，收下即可".into())
            }
            Decision::Reopen => {
                todo.state = if todo.assignments.is_empty() {
                    TodoState::Open
                } else {
                    TodoState::Assigned
                };
                todo.closed_ms = None;
            }
        }
        todo.updated_ms = now_ms;
        let saved = todo.clone();
        self.save(&doc)?;
        Ok(saved)
    }

    /// The 验收检查 to run: an assigned todo with a check whose latest
    /// session ended a turn (`Review`) not checked yet.
    pub fn due_checks(&self, env: &dyn TodoEnv) -> Vec<DueCheck> {
        // Sessions are read without the lock: they can be slow.
        let doc = {
            let _guard = self.lock.lock();
            self.load()
        };
        doc.todos
            .iter()
            .filter(|t| t.state == TodoState::Assigned)
            .filter_map(|t| {
                let command = t.check.clone()?;
                let latest = t.latest()?;
                let view = env.session(&latest.task_id)?;
                let turn_ms = view.last_turn_ms?;
                if progress(Some(&view)).0 != Progress::Review {
                    return None;
                }
                let checked = latest
                    .check
                    .as_ref()
                    .is_some_and(|c| c.state == CheckState::Running || c.turn_ms >= turn_ms);
                (!checked).then(|| DueCheck {
                    todo_id: t.id.clone(),
                    project_id: t.project_id.clone(),
                    task_id: latest.task_id.clone(),
                    command,
                    turn_ms,
                })
            })
            .collect()
    }

    /// 重新检查: the check of the latest session's last turn, checked or not.
    pub fn check_for(&self, id: &str, env: &dyn TodoEnv) -> Result<DueCheck, String> {
        let todo = self.get(id).ok_or("没有这个待办")?;
        let command = todo.check.clone().ok_or("这个待办没有验收检查")?;
        let latest = todo
            .latest()
            .filter(|_| todo.state == TodoState::Assigned)
            .ok_or("交办出去之后才能检查")?;
        let view = env.session(&latest.task_id);
        let turn_ms = view.as_ref().and_then(|v| v.last_turn_ms);
        match turn_ms {
            Some(turn_ms) if progress(view.as_ref()).0 == Progress::Review => Ok(DueCheck {
                todo_id: todo.id.clone(),
                project_id: todo.project_id.clone(),
                task_id: latest.task_id.clone(),
                command,
                turn_ms,
            }),
            _ => Err("会话这一轮结束后才能检查".into()),
        }
    }

    /// Mark the check running; `false` when one already runs, or the todo
    /// was handed out again meanwhile.
    pub fn begin_check(&self, due: &DueCheck, now_ms: u64) -> Result<bool, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let Some(latest) = doc
            .todos
            .iter_mut()
            .find(|t| t.id == due.todo_id)
            .and_then(|t| t.assignments.last_mut())
            .filter(|a| a.task_id == due.task_id)
        else {
            return Ok(false);
        };
        if latest
            .check
            .as_ref()
            .is_some_and(|c| c.state == CheckState::Running)
        {
            return Ok(false);
        }
        latest.check = Some(CheckRun {
            state: CheckState::Running,
            output: String::new(),
            at_ms: now_ms,
            turn_ms: due.turn_ms,
        });
        self.save(&doc)?;
        Ok(true)
    }

    /// Keep the result; `None` when the run no longer counts (handed out
    /// again, or reset by a host restart).
    pub fn finish_check(
        &self,
        due: &DueCheck,
        passed: bool,
        output: String,
        now_ms: u64,
    ) -> Result<Option<Todo>, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let Some(todo) = doc.todos.iter_mut().find(|t| t.id == due.todo_id) else {
            return Ok(None);
        };
        let Some(latest) = todo
            .assignments
            .last_mut()
            .filter(|a| a.task_id == due.task_id)
        else {
            return Ok(None);
        };
        let running = latest
            .check
            .as_ref()
            .is_some_and(|c| c.state == CheckState::Running && c.turn_ms == due.turn_ms);
        if !running {
            return Ok(None);
        }
        latest.check = Some(CheckRun {
            state: if passed {
                CheckState::Passed
            } else {
                CheckState::Failed
            },
            output,
            at_ms: now_ms,
            turn_ms: due.turn_ms,
        });
        let saved = todo.clone();
        self.save(&doc)?;
        Ok(Some(saved))
    }

    /// A failing check was sent back into session `task_id`.
    pub fn fix_sent(&self, id: &str, task_id: &str) -> Result<(), String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let latest = doc
            .todos
            .iter_mut()
            .find(|t| t.id == id)
            .and_then(|t| t.assignments.last_mut())
            .filter(|a| a.task_id == task_id)
            .ok_or("这个待办已经重新交办了")?;
        latest.fixes += 1;
        self.save(&doc)
    }

    /// After a host restart nothing runs: a check left `Running` is run again.
    pub fn reset_running(&self) -> Result<usize, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let mut reset = 0;
        for a in doc.todos.iter_mut().flat_map(|t| t.assignments.iter_mut()) {
            if a.check
                .as_ref()
                .is_some_and(|c| c.state == CheckState::Running)
            {
                a.check = None;
                reset += 1;
            }
        }
        if reset > 0 {
            self.save(&doc)?;
        }
        Ok(reset)
    }
}

/// The round to send when a failing check goes back into the session.
pub fn fix_due(todo: &Todo) -> Option<u32> {
    let latest = todo.latest()?;
    let failed = latest
        .check
        .as_ref()
        .is_some_and(|c| c.state == CheckState::Failed);
    (todo.fix_on_fail && todo.state == TodoState::Assigned && failed && latest.fixes < FIX_ROUNDS)
        .then_some(latest.fixes + 1)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::sessions::turn_outcome::NotRunReason;

    struct Fake(HashMap<String, (String, Option<NotRun>)>);

    impl TodoEnv for Fake {
        fn session(&self, task_id: &str) -> Option<SessionView> {
            self.0.get(task_id).map(|(status, not_run)| SessionView {
                status: status.clone(),
                not_run: not_run.clone(),
                failure: (status == "failed").then(|| "Agent 退出了".to_string()),
                last_turn_ms: Some(100),
            })
        }
    }

    fn store() -> Todos {
        Todos::open(&std::env::temp_dir().join(format!("aeroric-todos-{}", uuid::Uuid::new_v4())))
    }

    fn draft(title: &str) -> Todo {
        Todo {
            id: String::new(),
            project_id: "p1".into(),
            title: title.into(),
            notes: " 先看 issue #12 ".into(),
            state: TodoState::Open,
            assignments: Vec::new(),
            created_ms: 0,
            updated_ms: 0,
            closed_ms: None,
            progress: None,
            progress_detail: None,
            proposed_by: None,
            check: None,
            fix_on_fail: false,
        }
    }

    fn assignment(task: &str, at_ms: u64) -> Assignment {
        Assignment {
            task_id: task.into(),
            agent: "codex".into(),
            model: None,
            at_ms,
            check: None,
            fixes: 0,
        }
    }

    #[test]
    fn a_handed_out_todo_waits_for_review_and_only_acceptance_closes_it() {
        let s = store();
        let todo = s.save_todo(draft("  修复登录超时  "), 10).unwrap();
        assert_eq!(
            (todo.title.as_str(), todo.notes.as_str()),
            ("修复登录超时", "先看 issue #12")
        );
        assert!(todo.id.starts_with("todo-"));
        assert!(
            s.decide(&todo.id, Decision::Accept, 11).is_err(),
            "not handed out"
        );

        s.assigned(&todo.id, assignment("t1", 20)).unwrap();
        let mut sessions = HashMap::new();
        sessions.insert("t1".to_string(), ("running".to_string(), None));
        let listed = s.list(&Fake(sessions.clone()), Some("p1"));
        assert_eq!(listed[0].progress, Some(Progress::Running));

        // The turn ended: waiting for the user, not done.
        sessions.insert("t1".to_string(), ("done".to_string(), None));
        let listed = s.list(&Fake(sessions.clone()), None);
        assert_eq!(
            (listed[0].state, listed[0].progress),
            (TodoState::Assigned, Some(Progress::Review))
        );

        // Handed out again: the new session counts.
        s.assigned(&todo.id, assignment("t2", 30)).unwrap();
        sessions.insert(
            "t2".to_string(),
            (
                "done".to_string(),
                Some(NotRun {
                    reason: NotRunReason::Quota,
                    status: Some(429),
                    message: "额度用完".into(),
                }),
            ),
        );
        let listed = s.list(&Fake(sessions.clone()), None);
        assert_eq!(listed[0].assignments.len(), 2);
        assert_eq!(
            (listed[0].progress, listed[0].progress_detail.as_deref()),
            (Some(Progress::NotRun), Some("额度用完"))
        );

        let done = s.decide(&todo.id, Decision::Accept, 40).unwrap();
        assert_eq!((done.state, done.closed_ms), (TodoState::Done, Some(40)));
        assert_eq!(s.list(&Fake(sessions.clone()), None)[0].progress, None);
        let back = s.decide(&todo.id, Decision::Reopen, 50).unwrap();
        assert_eq!(back.state, TodoState::Assigned, "it was handed out");
    }

    #[test]
    fn the_list_puts_open_work_first_and_reads_each_session() {
        let s = store();
        let a = s.save_todo(draft("A"), 1).unwrap();
        let b = s.save_todo(draft("B"), 2).unwrap();
        let mut c = draft("C");
        c.project_id = "p2".into();
        s.save_todo(c, 3).unwrap();
        s.decide(&a.id, Decision::Cancel, 4).unwrap();
        s.assigned(&b.id, assignment("gone", 5)).unwrap();
        let listed = s.list(&Fake(HashMap::new()), Some("p1"));
        assert_eq!(
            listed.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            ["B", "A"]
        );
        assert_eq!(listed[0].progress, Some(Progress::Missing));
        assert_eq!(s.list(&Fake(HashMap::new()), None).len(), 3);

        // Edits keep the state; bad drafts are refused; delete.
        let mut edit = s.get(&b.id).unwrap();
        edit.title = "B2".into();
        edit.state = TodoState::Done;
        let edited = s.save_todo(edit, 6).unwrap();
        assert_eq!(
            (edited.title.as_str(), edited.state),
            ("B2", TodoState::Assigned)
        );
        assert!(s.save_todo(draft("   "), 7).is_err());
        let mut nowhere = draft("x");
        nowhere.project_id = " ".into();
        assert!(s.save_todo(nowhere, 7).is_err());
        assert!(s.delete(&a.id).unwrap());
        assert!(!s.delete(&a.id).unwrap());
    }

    #[test]
    fn progress_reads_every_status_and_the_prompt_asks_for_a_report() {
        let view = |status: &str| SessionView {
            status: status.into(),
            not_run: None,
            failure: Some("崩了".into()),
            last_turn_ms: None,
        };
        assert_eq!(progress(Some(&view("pending"))).0, Progress::Running);
        assert_eq!(progress(Some(&view("input_required"))).0, Progress::Waiting);
        assert_eq!(
            progress(Some(&view("failed"))),
            (Progress::Failed, Some("崩了".into()))
        );
        assert_eq!(progress(Some(&view("interrupted"))).0, Progress::Stopped);
        assert_eq!(progress(Some(&view("idle"))).0, Progress::Review);
        assert_eq!(progress(None).0, Progress::Missing);
        let prompt = prompt_for(&draft("修复登录超时"));
        assert!(prompt.starts_with("修复登录超时\n\n先看 issue #12\n\n做完后"));
    }
}

#[cfg(test)]
mod second_batch_tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// task id → (status, last turn start).
    struct Sessions(RefCell<HashMap<String, (String, u64)>>);

    impl Sessions {
        fn new() -> Self {
            Self(RefCell::new(HashMap::new()))
        }
        fn set(&self, task: &str, status: &str, turn_ms: u64) {
            self.0
                .borrow_mut()
                .insert(task.into(), (status.into(), turn_ms));
        }
    }

    impl TodoEnv for Sessions {
        fn session(&self, task_id: &str) -> Option<SessionView> {
            self.0
                .borrow()
                .get(task_id)
                .map(|(status, turn_ms)| SessionView {
                    status: status.clone(),
                    not_run: None,
                    failure: None,
                    last_turn_ms: Some(*turn_ms),
                })
        }
    }

    fn store() -> Todos {
        Todos::open(&std::env::temp_dir().join(format!("aeroric-todos2-{}", uuid::Uuid::new_v4())))
    }

    fn todo(title: &str, check: Option<&str>, fix_on_fail: bool) -> Todo {
        Todo {
            id: String::new(),
            project_id: "p1".into(),
            title: title.into(),
            notes: String::new(),
            state: TodoState::Open,
            assignments: Vec::new(),
            created_ms: 0,
            updated_ms: 0,
            closed_ms: None,
            progress: None,
            progress_detail: None,
            proposed_by: None,
            check: check.map(str::to_string),
            fix_on_fail,
        }
    }

    fn hand_out(s: &Todos, id: &str, task: &str) {
        s.assigned(
            id,
            Assignment {
                task_id: task.into(),
                agent: "codex".into(),
                model: None,
                at_ms: 1,
                check: None,
                fixes: 0,
            },
        )
        .unwrap();
    }

    #[test]
    fn an_agent_proposes_and_the_user_takes_or_drops_it() {
        let s = store();
        let open = s
            .save_todo(todo("补 loader 的单测", None, false), 1)
            .unwrap();
        let p = s
            .propose("p1", "  修复 NMS 阈值  ", " 见 utils.py ", "t1", 2)
            .unwrap();
        assert_eq!(
            (
                p.state,
                p.proposed_by.as_deref(),
                p.title.as_str(),
                p.notes.as_str()
            ),
            (
                TodoState::Proposed,
                Some("t1"),
                "修复 NMS 阈值",
                "见 utils.py"
            )
        );
        // The same words, open or proposed, in this project: refused, named.
        let twin = s
            .propose("p1", "补 LOADER 的单测 ", "", "t2", 3)
            .unwrap_err();
        assert!(twin.contains(&open.id), "{twin}");
        assert!(s.propose("p1", "修复 nms 阈值", "", "t2", 3).is_err());
        assert!(
            s.propose("p2", "修复 NMS 阈值", "", "t2", 3).is_ok(),
            "another project"
        );
        assert!(s.propose("p1", "  ", "", "t1", 3).is_err());
        // At most five waiting per session; another session still may.
        for i in 0..4 {
            s.propose("p1", &format!("事项 {i}"), "", "t1", 4).unwrap();
        }
        let full = s.propose("p1", "第六条", "", "t1", 5).unwrap_err();
        assert!(full.contains('5'), "{full}");
        assert!(s.propose("p1", "第六条", "", "t3", 5).is_ok());

        // A proposal is not handed out, reopened or accepted before it is taken.
        assert!(s
            .assigned(
                &p.id,
                Assignment {
                    task_id: "x".into(),
                    agent: "codex".into(),
                    model: None,
                    at_ms: 6,
                    check: None,
                    fixes: 0,
                }
            )
            .is_err());
        assert!(s.decide(&p.id, Decision::Reopen, 6).is_err());
        assert!(s.decide(&p.id, Decision::Accept, 6).is_err());
        let taken = s.decide(&p.id, Decision::Take, 7).unwrap();
        assert_eq!(taken.state, TodoState::Open);
        assert!(s.decide(&p.id, Decision::Take, 8).is_err(), "taken already");
        // Taking it freed a place for t1.
        assert!(s.propose("p1", "第六条 b", "", "t1", 9).is_ok());

        assert!(!s.knows_task("t9"));
        hand_out(&s, &p.id, "t9");
        assert!(s.knows_task("t9"));
        assert!(!s.knows_task("t1"), "proposing is not handing out");
    }

    #[test]
    fn a_check_runs_once_per_turn_and_a_failure_goes_back_at_most_twice() {
        let s = store();
        let saved = s
            .save_todo(todo("修复登录超时", Some("  cargo test -p auth "), true), 1)
            .unwrap();
        assert_eq!(saved.check.as_deref(), Some("cargo test -p auth"));
        hand_out(&s, &saved.id, "t1");
        let env = Sessions::new();
        env.set("t1", "running", 100);
        assert!(s.due_checks(&env).is_empty(), "still running");
        assert!(s.check_for(&saved.id, &env).is_err());

        env.set("t1", "idle", 100);
        let due = s.due_checks(&env);
        assert_eq!(due.len(), 1);
        assert_eq!(
            (
                due[0].task_id.as_str(),
                due[0].command.as_str(),
                due[0].turn_ms
            ),
            ("t1", "cargo test -p auth", 100)
        );
        assert!(s.begin_check(&due[0], 110).unwrap());
        assert!(!s.begin_check(&due[0], 111).unwrap(), "one run at a time");
        assert!(s.due_checks(&env).is_empty(), "running");

        let todo = s
            .finish_check(&due[0], false, "退出码 101：1 failed".into(), 120)
            .unwrap()
            .unwrap();
        let run = todo.latest().unwrap().check.clone().unwrap();
        assert_eq!((run.state, run.turn_ms), (CheckState::Failed, 100));
        assert!(s.due_checks(&env).is_empty(), "this turn is checked");
        assert_eq!(fix_due(&todo), Some(1));
        let prompt = prompt_for(&todo);
        assert!(
            prompt.contains("验收检查：`cargo test -p auth`"),
            "{prompt}"
        );
        assert!(prompt.contains("1 failed"), "{prompt}");
        assert!(fix_prompt("cargo test", "boom", 1).starts_with("验收检查没过（第 1/2 次"));

        s.fix_sent(&saved.id, "t1").unwrap();
        // The Agent's next turn ends: checked again.
        env.set("t1", "idle", 200);
        let again = s.due_checks(&env);
        assert_eq!(again[0].turn_ms, 200);
        s.begin_check(&again[0], 210).unwrap();
        let todo = s
            .finish_check(&again[0], false, "还是错".into(), 220)
            .unwrap()
            .unwrap();
        assert_eq!(fix_due(&todo), Some(2));
        s.fix_sent(&saved.id, "t1").unwrap();
        assert_eq!(fix_due(&s.get(&saved.id).unwrap()), None, "two rounds used");

        // 重新检查 the same turn; a restart in the middle runs it again.
        let manual = s.check_for(&saved.id, &env).unwrap();
        assert!(s.begin_check(&manual, 230).unwrap());
        assert_eq!(s.reset_running().unwrap(), 1);
        assert_eq!(s.due_checks(&env).len(), 1);
        assert!(
            s.finish_check(&manual, true, String::new(), 240)
                .unwrap()
                .is_none(),
            "reset"
        );

        // Handed out again: the old run no longer counts, the new one starts clean.
        s.begin_check(&manual, 250).unwrap();
        hand_out(&s, &saved.id, "t2");
        assert!(s
            .finish_check(&manual, true, String::new(), 260)
            .unwrap()
            .is_none());
        assert!(!s.begin_check(&manual, 261).unwrap());
        let todo = s.get(&saved.id).unwrap();
        assert!(todo.latest().unwrap().check.is_none());
        assert_eq!(fix_due(&todo), None);
    }

    #[test]
    fn a_save_cleans_the_check_and_only_keeps_fixing_with_one() {
        let s = store();
        let none = s.save_todo(todo("a", Some("   "), true), 1).unwrap();
        assert_eq!((none.check.as_deref(), none.fix_on_fail), (None, false));
        let mut edit = s.save_todo(todo("b", Some("make test"), false), 1).unwrap();
        edit.fix_on_fail = true;
        edit.check = Some(" make check ".into());
        let edited = s.save_todo(edit, 2).unwrap();
        assert_eq!(
            (edited.check.as_deref(), edited.fix_on_fail),
            (Some("make check"), true)
        );
        // Without a check there is nothing to run, and the prompt says nothing of it.
        assert!(!prompt_for(&none).contains("验收检查"));
        let env = Sessions::new();
        assert!(s.check_for(&none.id, &env).is_err());
    }
}
