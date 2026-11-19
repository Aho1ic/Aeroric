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
use aeroric_protocol::submission::AgentConfiguration;
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
    // E5: fixes before this one that came to nothing stop it.
    if let Some((_, record)) = live.record(&due.task_id) {
        if super::restart::guard_before(live, &due.task_id, &record).is_err() {
            return;
        }
    }
    let text = fix_prompt(&due.command, &result.output, round);
    let sent = match live.driver(&due.task_id) {
        Some(driver) if driver.snapshot().turn_active => Err("会话正在跑".to_string()),
        Some(driver) => Ok(driver),
        None => live.reopen_quiet(&due.task_id),
    }
    .and_then(|driver| driver.prompt(text).map_err(|e| e.to_string()));
    if sent.is_ok() {
        let _ = book.fix_sent(&todo.id, &due.task_id);
        super::restart::guard_sent(
            live,
            &due.task_id,
            aeroric_core::sessions::continue_guard::SendKind::TodoFix,
        );
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
    /// Optional with a `configurationId`: the configuration names it.
    #[serde(default)]
    agent: String,
    #[serde(default)]
    model: Option<String>,
    /// 2026-11-17: a configuration of the project's session catalog.
    #[serde(default)]
    configuration_id: Option<String>,
}

/// What a hand-out runs with, once its configuration is checked.
#[derive(Debug, PartialEq, Eq)]
struct Picked {
    agent: String,
    model: Option<String>,
    configuration_id: Option<String>,
    agent_config_id: Option<String>,
    label: Option<String>,
}

/// 2026-11-17 交办选配置: check `configuration` against the project's
/// catalog — it exists, nothing blocks it, it offers `model`, and `agent`
/// (when given) is its Agent. Without a configuration, the Agent alone —
/// B1-10: still one the project's catalog offers. A configuration's
/// `blockedReasons` speak for that configuration's submission, not for
/// starting the Agent by name (`task.run`), so they do not refuse it.
fn pick_configuration(
    catalog: &[AgentConfiguration],
    agent: &str,
    model: Option<&str>,
    configuration: Option<&str>,
) -> Result<Picked, String> {
    let model = model.map(str::trim).filter(|m| !m.is_empty());
    let Some(id) = configuration.map(str::trim).filter(|c| !c.is_empty()) else {
        if agent.trim().is_empty() {
            return Err("选一个 Agent".into());
        }
        if !catalog.iter().any(|c| c.agent == agent) {
            return Err(format!("{agent}：这个项目里没有这个 Agent"));
        }
        return Ok(Picked {
            agent: agent.to_string(),
            model: model.map(str::to_string),
            configuration_id: None,
            agent_config_id: None,
            label: None,
        });
    };
    let Some(found) = catalog.iter().find(|c| c.id == id) else {
        return Err(format!("{id}：配置不存在"));
    };
    if !found.blocked_reasons.is_empty() {
        return Err(format!(
            "{}现在不能用：{}",
            found.display_name,
            found.blocked_reasons.join("；")
        ));
    }
    if !agent.trim().is_empty() && agent != found.agent {
        return Err(format!(
            "{} 属于 {}，不是 {agent}",
            found.display_name, found.agent
        ));
    }
    if let Some(model) = model {
        if !found.models.is_empty() && !found.models.iter().any(|m| m == model) {
            return Err(format!("{}：不支持模型 {model}", found.display_name));
        }
    }
    Ok(Picked {
        agent: found.agent.clone(),
        model: model.map(str::to_string),
        configuration_id: Some(found.id.clone()),
        agent_config_id: aeroric_core::agent_connections::launch::config_id_from_catalog(&found.id)
            .map(str::to_string),
        label: Some(match model {
            Some(model) => format!("{} · {model}", found.display_name),
            None => found.display_name.clone(),
        }),
    })
}

/// `todo.assign {id, agent?, model?, configurationId?}` → `{todo, taskId}`:
/// a new session in the todo's project with the todo as its prompt.
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
    // B1-10: one hand-out of a todo at a time; a second click waits for
    // nothing and opens no second session.
    let Some(_claim) = Claim::take(&p.id) else {
        return failure(request.id.clone(), ErrorCode::Busy, "这条待办正在交办");
    };
    let catalog =
        match aeroric_core::settings::session_catalog::for_project(&live.context, &todo.project_id)
        {
            Ok((_, catalog)) => catalog.configurations,
            Err(reason) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &reason),
        };
    let picked = match pick_configuration(
        &catalog,
        &p.agent,
        p.model.as_deref(),
        p.configuration_id.as_deref(),
    ) {
        Ok(picked) => picked,
        Err(reason) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &reason),
    };
    let opened = super::schedule::open_session(
        live,
        &todo.project_id,
        &super::schedule::Runner {
            agent: &picked.agent,
            model: picked.model.as_deref(),
            agent_config_id: picked.agent_config_id.as_deref(),
        },
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
        agent: picked.agent,
        model: picked.model,
        configuration_id: picked.configuration_id,
        label: picked.label,
        at_ms: now_ms(),
        check: None,
        fixes: 0,
    };
    match book.assigned(&p.id, assignment) {
        Ok(todo) => ok(request, json!({ "todo": todo, "taskId": task_id })),
        Err(error) => {
            // The session no todo will own is stopped, not left running.
            live.executor.close_session(&task_id);
            failure(
                request.id.clone(),
                ErrorCode::IoError,
                &format!("{error}；刚开的会话已停下"),
            )
        }
    }
}

/// The todos being handed out right now, across requests (each opens its
/// own `Todos`, whose lock does not reach the next).
static ASSIGNING: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

/// A todo held while it is handed out; let go when dropped.
struct Claim(String);

impl Claim {
    fn take(id: &str) -> Option<Claim> {
        ASSIGNING
            .lock()
            .insert(id.to_string())
            .then(|| Claim(id.to_string()))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        ASSIGNING.lock().remove(&self.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(id: &str, agent: &str, models: &[&str], blocked: &[&str]) -> AgentConfiguration {
        AgentConfiguration {
            id: id.into(),
            version: "v1".into(),
            display_name: format!("配置 {id}"),
            agent: agent.into(),
            harness: agent.into(),
            models: models.iter().map(|m| m.to_string()).collect(),
            efforts: Vec::new(),
            permission_modes: Vec::new(),
            blocked_reasons: blocked.iter().map(|b| b.to_string()).collect(),
            model_details: Vec::new(),
        }
    }

    fn catalog() -> Vec<AgentConfiguration> {
        vec![
            config("codex", "codex", &["gpt-6.1-sol", "gpt-6.1-mini"], &[]),
            config("agent-config:deepseek", "codex", &["deepseek-v4"], &[]),
            config("omp", "omp", &[], &["没装 omp"]),
            config("claude", "claude", &[], &[]),
        ]
    }

    #[test]
    fn a_configuration_names_the_agent_its_source_and_the_label() {
        let picked = pick_configuration(
            &catalog(),
            "",
            Some("deepseek-v4"),
            Some("agent-config:deepseek"),
        )
        .unwrap();
        assert_eq!(picked.agent, "codex");
        assert_eq!(picked.model.as_deref(), Some("deepseek-v4"));
        assert_eq!(
            picked.configuration_id.as_deref(),
            Some("agent-config:deepseek")
        );
        assert_eq!(picked.agent_config_id.as_deref(), Some("deepseek"));
        assert_eq!(
            picked.label.as_deref(),
            Some("配置 agent-config:deepseek · deepseek-v4")
        );

        // A built-in has no agentConfigId: the session keeps the Agent's own source.
        let builtin =
            pick_configuration(&catalog(), "codex", Some("gpt-6.1-mini"), Some("codex")).unwrap();
        assert_eq!(builtin.agent_config_id, None);
        assert_eq!(builtin.label.as_deref(), Some("配置 codex · gpt-6.1-mini"));

        // No model list: any model, or its default.
        let open = pick_configuration(&catalog(), "", None, Some("claude")).unwrap();
        assert_eq!((open.agent.as_str(), open.model), ("claude", None));
        assert_eq!(open.label.as_deref(), Some("配置 claude"));
    }

    #[test]
    fn what_cannot_run_is_refused_with_its_reason() {
        let missing = pick_configuration(&catalog(), "", None, Some("agent-config:gone"));
        assert_eq!(missing.unwrap_err(), "agent-config:gone：配置不存在");
        let blocked = pick_configuration(&catalog(), "", None, Some("omp")).unwrap_err();
        assert!(
            blocked.contains("现在不能用") && blocked.contains("没装 omp"),
            "{blocked}"
        );
        let model = pick_configuration(&catalog(), "", Some("gpt-4"), Some("codex")).unwrap_err();
        assert!(model.contains("不支持模型 gpt-4"), "{model}");
        let other = pick_configuration(&catalog(), "claude", None, Some("codex")).unwrap_err();
        assert!(other.contains("不是 claude"), "{other}");
    }

    #[test]
    fn a_todo_is_handed_out_once_at_a_time() {
        let first = Claim::take("todo-claim-test").unwrap();
        assert!(Claim::take("todo-claim-test").is_none());
        assert!(Claim::take("todo-claim-other").is_some());
        drop(first);
        assert!(Claim::take("todo-claim-test").is_some());
    }

    #[test]
    fn without_a_configuration_the_agent_must_be_offered() {
        let missing = pick_configuration(&catalog(), "dsh", None, None).unwrap_err();
        assert!(missing.contains("没有这个 Agent"), "{missing}");
        // A blocked configuration does not refuse starting the Agent by name.
        assert!(pick_configuration(&catalog(), "omp", None, None).is_ok());
    }

    #[test]
    fn without_a_configuration_the_agent_alone_as_before() {
        let picked = pick_configuration(&catalog(), "codex", Some(" "), None).unwrap();
        assert_eq!(
            picked,
            Picked {
                agent: "codex".into(),
                model: None,
                configuration_id: None,
                agent_config_id: None,
                label: None,
            }
        );
        assert_eq!(
            pick_configuration(&[], " ", None, None).unwrap_err(),
            "选一个 Agent"
        );
    }
}
