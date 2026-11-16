//! 2026-11-15 工作安排第一批: 待办. The `todo.*` methods; handing a todo out
//! opens a session the way a 定时任务 does (`schedule::open_session`), named
//! after the todo.
//!
//! 第二批:
//! - `context.todoPropose {token, title, notes?}`: the Context MCP's
//!   `todo_propose`; the proposal waits on the 待办 page.
//! - 验收检查: every `TICK_MS` the host runs, one at a time, the checks of
//!   todos whose latest session ended a turn not checked yet; `todo.check
//!   {id}` (重新检查) runs one at once on its own thread. A failing check of a
//!   todo with `fixOnFail` goes back into the same session.
//! - A handed-out todo's session counts as unattended (`restart::unattended`).

use std::sync::Arc;
use std::time::Duration;

use aeroric_core::schedule::check;
use aeroric_core::sessions::turn_outcome::NotRun;
use aeroric_core::todos::{fix_due, fix_prompt, prompt_for, DueCheck, SessionView, TodoEnv, Todos};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::todo::{self as wire, Assignment, Decision, Todo, TodoState};
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn todos(live: &Live) -> Todos {
    Todos::open(live.context.paths.data_dir())
}

/// Some todo handed this session out.
pub(super) fn knows_task(live: &Live, task_id: &str) -> bool {
    todos(live).knows_task(task_id)
}

struct Env<'a> {
    live: &'a Live,
}

impl TodoEnv for Env<'_> {
    fn session(&self, task_id: &str) -> Option<SessionView> {
        let (_, record) = self.live.record(task_id)?;
        let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_string);
        let status = text("status").unwrap_or_default();
        // Only a session that is not running has a last turn worth grading.
        let last = (!matches!(status.as_str(), "pending" | "running" | "input_required"))
            .then(|| super::rerun::turns(self.live, &record).pop())
            .flatten();
        let not_run: Option<NotRun> = last.as_ref().and_then(|t| t.not_run.clone());
        Some(SessionView {
            status,
            not_run,
            failure: text("failureReason"),
            last_turn_ms: last.map(|t| t.started_at),
        })
    }
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(wire::LIST_METHOD, blocking(live, todo_list));
    set.register(wire::SAVE_METHOD, blocking(live, todo_save));
    set.register(wire::DELETE_METHOD, blocking(live, todo_delete));
    set.register(wire::ASSIGN_METHOD, blocking(live, todo_assign));
    set.register(wire::DECIDE_METHOD, blocking(live, todo_decide));
    set.register(
        wire::CHECK_METHOD,
        super::wiki::in_background(live, todo_check),
    );
    set.register(wire::PROPOSE_METHOD, blocking(live, context_propose));
    // Nothing runs yet: a check a restart cut runs again.
    let _ = todos(live).reset_running();
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    handle.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(aeroric_protocol::schedule::TICK_MS)).await;
            let Some(live) = weak.upgrade() else {
                break;
            };
            let _ = tokio::task::spawn_blocking(move || tick(&live)).await;
        }
    });
}

/// Run the due checks, one at a time.
fn tick(live: &Live) {
    let book = todos(live);
    for due in book.due_checks(&Env { live }) {
        if book.begin_check(&due, now_ms()).unwrap_or(false) {
            run_check(live, &book, &due);
        }
    }
}

/// One 验收检查 of `due` (begun), then, when it failed and the todo asks
/// for it, the output back into the session.
fn run_check(live: &Live, book: &Todos, due: &DueCheck) {
    let result = match super::schedule::project_dir(live, &due.project_id) {
        Some(dir) => check::run_tail(
            &due.command,
            &dir,
            Duration::from_millis(wire::CHECK_TIMEOUT_MS),
        ),
        None => check::CheckResult {
            passed: false,
            output: "项目不在本机或已移除，没能运行".into(),
        },
    };
    let Ok(Some(todo)) = book.finish_check(due, result.passed, result.output.clone(), now_ms())
    else {
        return;
    };
    let Some(round) = fix_due(&todo) else {
        return;
    };
    let text = fix_prompt(&due.command, &result.output, round);
    let sent = match live.driver(&due.task_id) {
        Some(driver) if driver.snapshot().turn_active => Err("会话正在跑".to_string()),
        Some(driver) => Ok(driver),
        None => live.reopen_quiet(&due.task_id),
    }
    .and_then(|driver| driver.prompt(text).map_err(|e| e.to_string()));
    if sent.is_ok() {
        let _ = book.fix_sent(&todo.id, &due.task_id);
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListParams {
    #[serde(default)]
    project_id: Option<String>,
}

/// `todo.list {projectId?}` → `{todos}`, each assigned one with how its
/// latest session stands.
fn todo_list(live: &Live, request: &Request) -> Frame {
    let p: ListParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let list = todos(live).list(&Env { live }, p.project_id.as_deref());
    ok(request, json!({ "todos": list }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveParams {
    todo: Todo,
}

/// `todo.save {todo}` → `{todo}`: a new one (empty id) or new words.
fn todo_save(live: &Live, request: &Request) -> Frame {
    let p: SaveParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if super::schedule::project(live, &p.todo.project_id).is_none() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "没有这个项目",
        );
    }
    match todos(live).save_todo(p.todo, now_ms()) {
        Ok(todo) => ok(request, json!({ "todo": todo })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdParams {
    id: String,
}

/// `todo.delete {id}` → `{deleted}`; its sessions stay.
fn todo_delete(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match todos(live).delete(&p.id) {
        Ok(true) => ok(request, json!({ "deleted": true })),
        Ok(false) => failure(request.id.clone(), ErrorCode::NotFound, "没有这个待办"),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignParams {
    id: String,
    agent: String,
    #[serde(default)]
    model: Option<String>,
}

/// `todo.assign {id, agent, model?}` → `{todo, taskId}`: a new session in
/// the todo's project with the todo as its prompt.
fn todo_assign(live: &Live, request: &Request) -> Frame {
    let p: AssignParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let book = todos(live);
    let Some(todo) = book.get(&p.id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "没有这个待办");
    };
    if todo.state == TodoState::Proposed {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "先收下这条提议再交办",
        );
    }
    if p.agent.trim().is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "选一个 Agent",
        );
    }
    let opened = super::schedule::open_session(
        live,
        &todo.project_id,
        &p.agent,
        p.model.as_deref(),
        &prompt_for(&todo),
        &format!("{}（待办）", todo.title),
        &format!("todo-{}", todo.id),
    );
    let task_id = match opened {
        Ok(task_id) => task_id,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    let assignment = Assignment {
        task_id: task_id.clone(),
        agent: p.agent,
        model: p.model,
        at_ms: now_ms(),
        check: None,
        fixes: 0,
    };
    match book.assigned(&p.id, assignment) {
        Ok(todo) => ok(request, json!({ "todo": todo, "taskId": task_id })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecideParams {
    id: String,
    decision: Decision,
}

/// `todo.decide {id, decision}` → `{todo}`: accept, cancel or reopen.
fn todo_decide(live: &Live, request: &Request) -> Frame {
    let p: DecideParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match todos(live).decide(&p.id, p.decision, now_ms()) {
        Ok(todo) => ok(request, json!({ "todo": todo })),
        Err(error) if error == "没有这个待办" => {
            failure(request.id.clone(), ErrorCode::NotFound, &error)
        }
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// `todo.check {id}` → `{todo}`: 重新检查, on its own thread; the todo shows
/// the check running.
fn todo_check(live: Arc<Live>, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let book = todos(&live);
    let due = match book.check_for(&p.id, &Env { live: &live }) {
        Ok(due) => due,
        Err(error) if error == "没有这个待办" => {
            return failure(request.id.clone(), ErrorCode::NotFound, &error)
        }
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match book.begin_check(&due, now_ms()) {
        Ok(true) => {}
        Ok(false) => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "检查已经在跑了",
            )
        }
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    }
    let running = book.get(&p.id);
    std::thread::spawn(move || run_check(&live, &todos(&live), &due));
    ok(request, json!({ "todo": running }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposeParams {
    token: String,
    title: String,
    #[serde(default)]
    notes: Option<String>,
}

/// `context.todoPropose {token, title, notes?}` → `{text, todo}`: an Agent's
/// proposal, waiting for the user on the 待办 page.
fn context_propose(live: &Live, request: &Request) -> Frame {
    let p: ProposeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((task, project)) = live.executor.context_tokens().resolve(&p.token) else {
        return failure(
            request.id.clone(),
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        );
    };
    let notes = p.notes.unwrap_or_default();
    match todos(live).propose(&project, &p.title, &notes, &task, now_ms()) {
        Ok(todo) => ok(
            request,
            json!({
                "text": format!("已提议，等用户在 Aeroric 的待办页收下：[{}] {}", todo.id, todo.title),
                "todo": todo,
            }),
        ),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}
