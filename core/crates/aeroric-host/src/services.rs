//! 只读与幂等登记服务（P3-07 ③a：project.list/open、task.list/get）。
//!
//! 执行类服务（create/start/cancel/resume）在 ③b——那需要 core 侧 PTY executor，
//! 本文件不预建空壳。`project.open` 虽是写路径，但它的幂等由 `newId` 承担
//! （同 id 重发＝reused），与 Action 6 的契约同形，随读侧一起立。
//!
//! handler 的失败一律回 protocol `Failure`（NotFound/InvalidArgument），
//! **不 panic、不断连**——CLI 侧按码映射退出码是 client.rs 的事。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, ProjectService};
use aeroric_core::storage::tasks::load_stamped;
use aeroric_protocol::envelope::{Frame, Request, Success};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};

pub fn register_read_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    {
        let context = Arc::clone(&context);
        set.register(
            "project.list",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_list(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.open",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_open(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "task.list",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { task_list(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "task.get",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { task_get(request, context) })
            }),
        );
    }
}

fn ok(request: &Request, result: Value) -> Frame {
    Frame::Success(Success {
        v: aeroric_protocol::PROTOCOL_VERSION,
        id: request.id.clone(),
        ok: true,
        result,
        extra: Default::default(),
    })
}

fn project_list(request: Request, context: Arc<CoreContext>) -> Frame {
    match ProjectService::new(context).list() {
        Ok(projects) => ok(&request, json!({ "projects": projects })),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot read the project list: {e}"),
        ),
    }
}

fn project_open(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(path) = request
        .params
        .get("path")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.open requires params.path (non-empty)",
        );
    };
    let Some(new_id) = request
        .params
        .get("newId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.open requires params.newId (the caller mints it; ADR-013)",
        );
    };
    let now_ms_inner = context.clock.now_ms() as i64;
    match ProjectService::new(context).open_local(path, new_id, now_ms_inner) {
        Ok((upserted, applied)) => ok(
            &request,
            json!({
                "project": upserted.project,
                "reused": upserted.reused,
                "revision": applied.revision,
            }),
        ),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot open the project: {e}"),
        ),
    }
}

fn task_list(request: Request, context: Arc<CoreContext>) -> Frame {
    let projects = match ProjectService::new(Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::IoError,
                &format!("cannot read the project list: {e}"),
            )
        }
    };
    let wanted_path = request
        .params
        .get("projectPath")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty());
    let mut tasks = Vec::new();
    for project in &projects {
        let id = match mutations::id_of(project) {
            Some(id) => id.to_string(),
            None => continue,
        };
        if let Some(path) = wanted_path {
            if mutations::text(project, "path") != Some(path) {
                continue;
            }
        }
        match load_stamped(&context, &id) {
            Ok(stamped) => {
                for mut record in stamped.records {
                    record.0.insert("projectId".to_string(), json!(id.clone()));
                    tasks.push(record);
                }
            }
            Err(e) => {
                return failure(
                    request.id,
                    ErrorCode::IoError,
                    &format!("cannot read tasks of project {id}: {e}"),
                )
            }
        }
    }
    ok(&request, json!({ "tasks": tasks }))
}

fn task_get(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "task.get requires params.taskId",
        );
    };
    let projects = match ProjectService::new(Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::IoError,
                &format!("cannot read the project list: {e}"),
            )
        }
    };
    for project in &projects {
        let Some(id) = mutations::id_of(project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(&context, id) else {
            continue;
        };
        if let Some(found) = stamped
            .records
            .iter()
            .find(|record| mutations::text(record, "id") == Some(task_id))
        {
            return ok(&request, json!({"task": found, "projectId": id}));
        }
    }
    failure(
        request.id,
        ErrorCode::NotFound,
        "no task with that id in any known project",
    )
}

// ── 执行服务（b1.2：task.run / task.cancel / task.complete）──────────────

use aeroric_core::tasks::Attachments;
use aeroric_core::tasks::{executor::HostWindowFacts, NewTask, TaskService};

/// 找到任务所在的 project record（与 `task.get` 同一条扫描链——**归属判定
/// 只此一份逻辑**，两处扫描将来要合并成一个 helper）。
fn find_project_of_task(
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
) -> Option<aeroric_core::storage::Record> {
    let projects = ProjectService::new(std::sync::Arc::clone(context))
        .list()
        .ok()?;
    for project in projects {
        let Some(id) = mutations::id_of(&project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(context, id) else {
            continue;
        };
        if stamped
            .records
            .iter()
            .any(|record| mutations::text(record, "id") == Some(task_id))
        {
            return Some(project);
        }
    }
    None
}

pub fn register_task_services(
    set: &mut ServiceSet,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    executor: std::sync::Arc<crate::executor::CliPtyExecutor>,
    terminal: std::sync::Arc<aeroric_core::terminal::pty::TerminalService>,
    registry: std::sync::Arc<aeroric_core::runtime::registry::TaskRegistry>,
) {
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.run",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_run(request, service, context) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.cancel",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_settle_verdict(request, service, context, false) })
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        let registry = std::sync::Arc::clone(&registry);
        set.register(
            "task.output",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                let registry = std::sync::Arc::clone(&registry);
                Box::pin(async move { task_output(request, &terminal, &registry) })
            }),
        );
    }
    {
        let executor = std::sync::Arc::clone(&executor);
        set.register(
            "task.resize",
            std::sync::Arc::new(move |request, _call| {
                let executor = std::sync::Arc::clone(&executor);
                Box::pin(async move { task_resize(request, &executor) })
            }),
        );
    }
    {
        let executor = std::sync::Arc::clone(&executor);
        set.register(
            "task.input",
            std::sync::Arc::new(move |request, _call| {
                let executor = std::sync::Arc::clone(&executor);
                Box::pin(async move { task_input(request, &executor) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.complete",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_settle_verdict(request, service, context, true) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.resume",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_resume(request, service, context) })
            }),
        );
    }
}

fn task_run(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(path) = request
        .params
        .get("projectPath")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.projectPath",
        );
    };
    let Some(agent) = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.agent",
        );
    };
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.taskId (the caller mints it; ADR-013)",
        );
    };
    let prompt = request
        .params
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let new_project_id = request
        .params
        .get("newProjectId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("project");
    let projects = ProjectService::new(std::sync::Arc::clone(&context));
    let (upserted, _applied) =
        match projects.open_local(path, new_project_id, context.clock.now_ms() as i64) {
            Ok(opened) => opened,
            Err(e) => {
                return failure(
                    id,
                    ErrorCode::IoError,
                    &format!("cannot open the project: {e}"),
                )
            }
        };
    let project = upserted.project;
    let project_id = mutations::id_of(&project).unwrap_or_default().to_string();
    let permission_mode = request
        .params
        .get("permissionMode")
        .and_then(Value::as_str)
        .unwrap_or("ask")
        .to_string();
    let new_task = NewTask {
        id: task_id.to_string(),
        project_id: project_id.clone(),
        prompt,
        agent: agent.to_string(),
        permission_mode,
        immediate: true,
        selected_model: request
            .params
            .get("selectedModel")
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_effort: request
            .params
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string),
        speed: None,
        dsh_agent_preset: None,
        images: Vec::new(),
        texts: Vec::new(),
    };
    if let Err(e) = service.create(&project, new_task) {
        return failure(
            id,
            ErrorCode::IoError,
            &format!("cannot create the task: {e}"),
        );
    }
    let terminal_size = request
        .params
        .get("terminalSize")
        .and_then(Value::as_array)
        .and_then(|pair| {
            Some((
                pair.first()?.as_u64()? as u16,
                pair.get(1)?.as_u64()? as u16,
            ))
        });
    let operation_key = format!("cli-run-{task_id}");
    match service.start(
        &project,
        task_id,
        &operation_key,
        true,
        None,
        false,
        Attachments {
            images: Vec::new(),
            texts: Vec::new(),
        },
        HostWindowFacts {
            terminal_size,
            dsh: None,
        },
    ) {
        Ok(applied) => {
            let status = applied
                .records
                .iter()
                .find(|record| mutations::text(record, "id") == Some(task_id))
                .and_then(|record| mutations::text(record, "status"))
                .unwrap_or("unknown")
                .to_string();
            ok(
                &request,
                json!({ "taskId": task_id, "projectId": project_id, "status": status }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("cannot start the task: {e}"),
        ),
    }
}

/// `task.input {taskId, text}`：PTY 输入经控制租约（executor 的短顶替写）——
/// **结构化 Agent 的声明能力路径（dsh prompt 注入等）不在此方法**：dsh 任务
/// 的后续输入走宿主 `prompt_dsh_task` 的专用口，而 CLI host 不接 dsh 启动
/// （⑦ 收尾定性，见 executor 模块头）；对不认识的协议猜按键是 P3-07 验收行
/// 点名禁止的。
fn task_input(request: Request, executor: &crate::executor::CliPtyExecutor) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.input requires params.taskId",
        );
    };
    // `bytes`（base64）优先于 `text`：attach 的任意按键序列（Alt、NUL、Latin-1）
    // 经 base64 无损；`text` 留给脚本化调用的可读性。
    use base64::Engine as _;
    let payload: Vec<u8> = if let Some(b64) = request.params.get("bytes").and_then(Value::as_str) {
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(bytes) => bytes,
            Err(e) => return failure(id, ErrorCode::InvalidArgument, &format!("bad base64: {e}")),
        }
    } else if let Some(text) = request.params.get("text").and_then(Value::as_str) {
        text.as_bytes().to_vec()
    } else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.input requires params.bytes (base64) or params.text",
        );
    };
    let written = payload.len();
    match executor.input(task_id, &payload) {
        Ok(()) => ok(&request, json!({ "written": written })),
        Err(e) => failure(id, ErrorCode::NotFound, &e),
    }
}

/// `task.output {taskId, offset}`：字节环的 RPC 读面（attach 的转发源）。
/// `gap:true` = 流已退场（进程结束后环不在），客户端改走 `session.history`
/// 拉全量——**环是直播面，不是历史面**（P3-03 的分界在 wire 上的投影）。
/// `OutOfRange` 是「追平」（返回空 chunk + 当前末端），不是错误。
fn task_output(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
    registry: &aeroric_core::runtime::registry::TaskRegistry,
) -> Frame {
    use aeroric_core::terminal::stream::{StreamError, TerminalIdentity};
    use base64::Engine as _;
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.output requires params.taskId",
        );
    };
    let offset = request
        .params
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let Some(run) = registry.resource_run(task_id) else {
        return ok(
            &request,
            json!({ "gap": true, "reason": "no live run on this host; use session.history" }),
        );
    };
    let identity = TerminalIdentity::new(task_id, run.generation());
    match terminal.read_from(&identity, offset, 64 * 1024) {
        Ok(chunk) => ok(
            &request,
            json!({
                "bytes": base64::engine::general_purpose::STANDARD.encode(&chunk.bytes),
                "nextOffset": offset + chunk.bytes.len() as u64,
                "gap": false,
            }),
        ),
        Err(StreamError::OutOfRange { next_offset }) => ok(
            &request,
            json!({ "bytes": "", "nextOffset": next_offset.min(offset), "gap": false }),
        ),
        Err(StreamError::UnknownStream) => ok(
            &request,
            json!({ "gap": true, "reason": "stream retired; use session.history" }),
        ),
    }
}

fn task_resize(request: Request, executor: &crate::executor::CliPtyExecutor) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resize requires params.taskId",
        );
    };
    let (Some(cols), Some(rows)) = (
        request.params.get("cols").and_then(Value::as_u64),
        request.params.get("rows").and_then(Value::as_u64),
    ) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resize requires params.cols and params.rows (numbers)",
        );
    };
    match executor.resize(task_id, cols as u16, rows as u16) {
        Ok(()) => ok(&request, json!({ "cols": cols, "rows": rows })),
        Err(e) => failure(id, ErrorCode::NotFound, &e),
    }
}

fn task_settle_verdict(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    complete: bool,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "this method requires params.taskId",
        );
    };
    let Some(project) = find_project_of_task(&context, task_id) else {
        return failure(
            id,
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let outcome = if complete {
        service
            .complete(&project, task_id)
            .map(|(completion, _)| json!({ "completion": format!("{completion:?}") }))
    } else {
        service
            .cancel(&project, task_id)
            .map(|target| json!({ "target": target.target }))
    };
    match outcome {
        Ok(result) => ok(&request, result),
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("the settle verdict failed: {e}"),
        ),
    }
}

fn task_resume(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resume requires params.taskId",
        );
    };
    let Some(project) = find_project_of_task(&context, task_id) else {
        return failure(
            id,
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let terminal_size = request
        .params
        .get("terminalSize")
        .and_then(Value::as_array)
        .and_then(|pair| {
            Some((
                pair.first()?.as_u64()? as u16,
                pair.get(1)?.as_u64()? as u16,
            ))
        });
    let session_id = request
        .params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let tasks = service.load(project.id().unwrap_or_default()).ok()?.records;
            let record = tasks
                .iter()
                .find(|r| mutations::text(r, "id") == Some(task_id))?;
            mutations::text(record, "sessionId")
                .or_else(|| mutations::text(record, "sessionPath"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default-session".to_string());
    let operation_key = format!("cli-resume-{task_id}");
    match service.resume(
        &project,
        task_id,
        &session_id,
        &operation_key,
        true,
        terminal_size,
    ) {
        Ok(applied) => {
            let status = applied
                .records
                .iter()
                .find(|record| mutations::text(record, "id") == Some(task_id))
                .and_then(|record| mutations::text(record, "status"))
                .unwrap_or("running")
                .to_string();
            ok(
                &request,
                json!({
                    "taskId": task_id,
                    "projectId": project.id(),
                    "sessionId": session_id,
                    "status": status,
                }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("cannot resume the task: {e}"),
        ),
    }
}

use aeroric_core::agents::dsh_host::approvals::{
    take_waterfall_answer, PendingWaterfall, WaterfallKind, WaterfallRegistry,
};

#[derive(Debug, Default)]
pub struct HostApprovals {
    pub registry: WaterfallRegistry,
    pub resolved_events: std::collections::HashSet<String>,
}

pub fn register_approval_services(
    set: &mut ServiceSet,
    approvals: std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) {
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.list",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_list(request, &approvals) })
            }),
        );
    }
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.respond",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_respond(request, &approvals) })
            }),
        );
    }
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.test_request",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_test_request(request, &approvals) })
            }),
        );
    }
}

fn approval_list(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let guard = approvals.lock().expect("lock approvals");
    let items: Vec<Value> = guard
        .registry
        .entries()
        .into_iter()
        .map(|(event_id, entry)| {
            json!({
                "eventId": event_id,
                "kind": match entry.kind {
                    WaterfallKind::Approval => "approval",
                    WaterfallKind::Question => "question",
                },
                "sessionId": entry.session_id,
            })
        })
        .collect();
    ok(&request, json!({ "approvals": items }))
}

fn approval_respond(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let id = request.id.clone();
    let Some(event_id) = request
        .params
        .get("eventId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "approval.respond requires params.eventId",
        );
    };
    let Some(outcome) = request.params.get("outcome").and_then(Value::as_object) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "approval.respond requires params.outcome (JSON object)",
        );
    };
    let outcome_val = Value::Object(outcome.clone());
    let mut guard = approvals.lock().expect("lock approvals");
    if guard.resolved_events.contains(event_id) {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("approval request '{event_id}' has already been resolved or expired"),
        );
    }
    if !guard.registry.entries().iter().any(|(k, _)| k == event_id) {
        return failure(
            id,
            ErrorCode::NotFound,
            &format!("approval request '{event_id}' not found"),
        );
    }
    let client_id = guard
        .registry
        .client_id()
        .unwrap_or("cli-default")
        .to_string();
    match take_waterfall_answer(&mut guard.registry, &client_id, event_id, &outcome_val) {
        Ok(answer) => {
            guard.resolved_events.insert(event_id.to_string());
            ok(
                &request,
                json!({
                    "accepted": true,
                    "eventId": event_id,
                    "sessionHint": answer.session_hint,
                    "args": answer.args,
                }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("cannot answer approval: {e}"),
        ),
    }
}

fn approval_test_request(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let id = request.id.clone();
    let Some(event_id) = request.params.get("eventId").and_then(Value::as_str) else {
        return failure(id, ErrorCode::InvalidArgument, "eventId required");
    };
    let kind = match request.params.get("kind").and_then(Value::as_str) {
        Some("question") => WaterfallKind::Question,
        _ => WaterfallKind::Approval,
    };
    let session_id = request
        .params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut guard = approvals.lock().expect("lock approvals");
    guard
        .registry
        .insert(event_id.to_string(), PendingWaterfall { kind, session_id });
    ok(&request, json!({ "inserted": event_id }))
}

// ── ④：session 三方法＋task.input（lease 控制的写入口） ──────────────────

use aeroric_core::sessions::{
    dsh::parse_dsh_session_lines,
    export::{export_session_markdown, ExportSource, ExportTaskMeta},
    omp::parse_omp_session_lines,
    parser::parse_session_lines,
};

/// 会话消息的家族分派——与宿主 `session.rs` 同一条四族链（P3-04 的 core 读侧）。
fn parse_messages(lines: &[String], family: ProtocolFamily) -> Vec<Value> {
    match family {
        ProtocolFamily::Dsh => parse_dsh_session_lines(lines)
            .into_iter()
            .map(serde_json::to_value)
            .map(Result::unwrap_or_default)
            .collect(),
        ProtocolFamily::Omp => parse_omp_session_lines(lines)
            .into_iter()
            .map(serde_json::to_value)
            .map(Result::unwrap_or_default)
            .collect(),
        other => {
            let mut messages = Vec::new();
            parse_session_lines(lines, other == ProtocolFamily::Codex, &mut messages);
            messages
                .into_iter()
                .map(serde_json::to_value)
                .map(Result::unwrap_or_default)
                .collect()
        }
    }
}

use aeroric_core::tasks::ProtocolFamily;

#[allow(clippy::type_complexity)]
/// 终端重放面与租约面（P5-01，ADR-038）：`terminal.plan_replay` 承接回放与
/// 活流（同一字节环，轮询节奏客户端自持），`terminal.acquire_control`/
/// `terminal.release_control` 把租约语义上协议。`UnknownStream` 在宿主分类成
/// `notStarted`（在册未产字节）/`evicted`（流已退场）/`unknownStream`（任务
/// 不可知）——判定材料（任务注册表、任务记录）在宿主手里，core 不改。
pub fn register_terminal_replay_services(
    set: &mut ServiceSet,
    terminal: std::sync::Arc<aeroric_core::terminal::pty::TerminalService>,
    registry: std::sync::Arc<aeroric_core::runtime::registry::TaskRegistry>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) {
    {
        let terminal = std::sync::Arc::clone(&terminal);
        let registry = std::sync::Arc::clone(&registry);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "terminal.plan_replay",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                let registry = std::sync::Arc::clone(&registry);
                let context = std::sync::Arc::clone(&context);
                Box::pin(
                    async move { terminal_plan_replay(request, &terminal, &registry, &context) },
                )
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        set.register(
            "terminal.acquire_control",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                Box::pin(async move { terminal_acquire_control(request, &terminal) })
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        set.register(
            "terminal.release_control",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                Box::pin(async move { terminal_release_control(request, &terminal) })
            }),
        );
    }
}

fn terminal_identity_of(request: &Request) -> Option<(String, u64)> {
    let task_id = request.params.get("taskId")?.as_str()?.trim().to_string();
    if task_id.is_empty() {
        return None;
    }
    let generation = request.params.get("generation").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.parse::<u64>().ok())
    })?;
    Some((task_id, generation))
}

/// 在册且未退场的运行 ⇒ 流还没产出第一字节（notStarted）；任务记录还在但
/// 流不在册 ⇒ 已被退场（evicted）；两处都查无 ⇒ 任务本身不可知（unknownStream）。
fn classify_unknown_stream(
    registry: &aeroric_core::runtime::registry::TaskRegistry,
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
    generation: u64,
) -> &'static str {
    if registry
        .resource_run(task_id)
        .is_some_and(|run| run.id() == task_id && run.generation() == generation)
    {
        return "notStarted";
    }
    if find_project_of_task(context, task_id).is_some() {
        return "evicted";
    }
    "unknownStream"
}

fn terminal_plan_replay(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
    registry: &aeroric_core::runtime::registry::TaskRegistry,
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    use aeroric_core::terminal::replay::{plan_replay, ReplayPlan, ResetReason, ResumePoint};
    use aeroric_core::terminal::stream::TerminalIdentity;
    use base64::Engine as _;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.plan_replay requires params.taskId and params.generation",
        );
    };
    let cursor = request.params.get("cursor").cloned().unwrap_or(Value::Null);
    let kind = cursor
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("bytes")
        .to_string();
    let Some(value) = cursor
        .get("value")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()))
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.plan_replay requires cursor.value (a byte offset)",
        );
    };
    let max_bytes = request
        .params
        .get("maxBytes")
        .and_then(Value::as_u64)
        .unwrap_or(aeroric_protocol::terminal::MAX_TERMINAL_CHUNK_BYTES as u64)
        .min(aeroric_protocol::terminal::MAX_TERMINAL_CHUNK_BYTES as u64)
        as usize;
    let identity = TerminalIdentity::new(&task_id, generation);
    let resume = match kind.as_str() {
        "unitsFromStart" => ResumePoint::UnitsFromStart(value),
        _ => ResumePoint::Bytes(value),
    };
    let plan = plan_replay(terminal, &identity, resume, max_bytes);
    let (kind_out, from, bytes, reason) = match &plan {
        ReplayPlan::Seamless { offset, bytes } => ("seamless", *offset, bytes, Value::Null),
        ReplayPlan::Reset {
            from,
            bytes,
            reason,
        } => {
            let reason_json = match reason {
                ResetReason::Compacted { earliest } => {
                    json!({ "kind": "compacted", "earliest": earliest })
                }
                ResetReason::CursorBeyondStream { next_offset } => {
                    json!({ "kind": "cursorBeyondStream", "nextOffset": next_offset })
                }
                // UnknownStream 在宿主分类成两个具名理由（ADR-038 决策 2）：
                // 在册未产字节 ⇒ notStarted；任务还在但流已退场 ⇒ evicted。
                ResetReason::UnknownStream => {
                    let classified =
                        classify_unknown_stream(registry, context, &task_id, generation);
                    json!({ "kind": classified })
                }
                ResetReason::UnitsNotAnchorable => json!({ "kind": "unitsNotAnchorable" }),
            };
            ("reset", *from, bytes, reason_json)
        }
    };
    let ResumePoint::Bytes(resume_after) = plan.resume_after() else {
        // resume_after 的契约恒为字节游标（core 注释钉死），不可达分支。
        return failure(id, ErrorCode::IoError, "unreachable: non-byte resume point");
    };
    ok(
        &request,
        json!({
            "kind": kind_out,
            "from": from,
            "bytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            "reason": reason,
            "resumeAfter": resume_after,
        }),
    )
}

fn terminal_acquire_control(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
) -> Frame {
    use aeroric_core::terminal::stream::TerminalIdentity;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.acquire_control requires params.taskId and params.generation",
        );
    };
    let identity = TerminalIdentity::new(task_id, generation);
    let handle = terminal.acquire_control(&identity);
    ok(&request, json!({ "ticket": handle.ticket() }))
}

fn terminal_release_control(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
) -> Frame {
    use aeroric_core::terminal::lease::LeaseHandle;
    use aeroric_core::terminal::stream::TerminalIdentity;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.release_control requires params.taskId and params.generation",
        );
    };
    let Some(ticket) = request
        .params
        .get("ticket")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()))
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.release_control requires params.ticket",
        );
    };
    let identity = TerminalIdentity::new(task_id, generation);
    let released = terminal.release_control(&identity, &LeaseHandle::from_ticket(ticket));
    ok(&request, json!({ "released": released }))
}

pub fn register_session_services(
    set: &mut ServiceSet,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    families: std::sync::Arc<dyn aeroric_core::tasks::AgentFamilies>,
) {
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "session.list",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { session_list(request, service, context) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        let families = std::sync::Arc::clone(&families);
        set.register(
            "session.history",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                let families = std::sync::Arc::clone(&families);
                Box::pin(async move { session_history(request, service, context, families) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "session.export",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { session_export(request, service, context) })
            }),
        );
    }
}

/// 找到任务记录（跨项目扫描，与 `find_project_of_task` 同行——⑦ 评审记合并）。
fn find_task_record(
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
) -> Option<(aeroric_core::storage::Record, aeroric_core::storage::Record)> {
    let projects = ProjectService::new(std::sync::Arc::clone(context))
        .list()
        .ok()?;
    for project in projects {
        let Some(id) = mutations::id_of(&project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(context, id) else {
            continue;
        };
        if let Some(record) = stamped
            .records
            .iter()
            .find(|record| mutations::text(record, "id") == Some(task_id))
        {
            return Some((record.clone(), project));
        }
    }
    None
}

fn session_list(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let projects = match ProjectService::new(std::sync::Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("cannot list projects: {e}"),
            )
        }
    };
    let mut sessions = Vec::new();
    for project in &projects {
        let Some(project_id) = mutations::id_of(project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(&context, project_id) else {
            continue;
        };
        for record in &stamped.records {
            if let Some(path) = mutations::text(record, "sessionPath") {
                sessions.push(json!({
                    "taskId": mutations::text(record, "id"),
                    "projectId": project_id,
                    "sessionPath": path,
                    "sessionId": mutations::text(record, "sessionId"),
                }));
            }
        }
    }
    ok(&request, json!({ "sessions": sessions }))
}

fn session_history(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    families: std::sync::Arc<dyn aeroric_core::tasks::AgentFamilies>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.history requires params.taskId",
        );
    };
    let Some((record, _project)) = find_task_record(&context, task_id) else {
        return failure(id, ErrorCode::NotFound, "no task with that id");
    };
    let Some(path) = mutations::text(&record, "sessionPath") else {
        return failure(
            id,
            ErrorCode::NotFound,
            "this task has no session transcript on disk yet",
        );
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("cannot read transcript: {e}"),
            )
        }
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let agent = mutations::text(&record, "agent").unwrap_or_default();
    let messages = parse_messages(&lines, families.session_family(agent));
    ok(&request, json!({ "messages": messages, "path": path }))
}

fn session_export(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.export requires params.taskId",
        );
    };
    let Some(out) = request
        .params
        .get("out")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.export requires params.out",
        );
    };
    let Some((record, _project)) = find_task_record(&context, task_id) else {
        return failure(id, ErrorCode::NotFound, "no task with that id");
    };
    let Some(path) = mutations::text(&record, "sessionPath") else {
        return failure(
            id,
            ErrorCode::NotFound,
            "this task has no session transcript to export",
        );
    };
    let agent = mutations::text(&record, "agent")
        .unwrap_or_default()
        .to_string();
    let source = match agent.as_str() {
        "dsh" => ExportSource::Dsh,
        "omp" => ExportSource::Omp,
        _ => ExportSource::ClaudeOrCodex,
    };
    let meta = ExportTaskMeta {
        name: mutations::text(&record, "name").map(str::to_string),
        prompt: mutations::text(&record, "prompt")
            .unwrap_or_default()
            .to_string(),
        agent,
        created_at: mutations::text(&record, "createdAt")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        session_id: mutations::text(&record, "sessionId").map(str::to_string),
        worktree_branch: mutations::text(&record, "worktreeBranch").map(str::to_string),
        base_branch: mutations::text(&record, "baseBranch").map(str::to_string),
        additions: None,
        deletions: None,
        failure_reason: mutations::text(&record, "failureReason").map(str::to_string),
    };
    match export_session_markdown(std::path::Path::new(path), out, source, &meta) {
        Ok(()) => ok(&request, json!({ "exported": out })),
        Err(e) => failure(id, ErrorCode::IoError, &e),
    }
}

/// P6-01：文件服务（宿主 command 只做 adapter；业务主体在 core `files/`）。
/// 路径校验 server 端权威（不依赖客户端过滤 `..`）；projectId → 项目根经
/// `find_project_of_task` 同款的 record 扫描。
pub fn register_file_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_dir",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_dir(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_file(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.write_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_write_file(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.create_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_create(request, context, false) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.create_directory",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_create(request, context, true) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.delete_path",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_delete(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.rename_path",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_rename(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.replace_preview",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_replace_preview(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.apply_replacements",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_apply_replacements(request, context) })
            }),
        );
    }
}

/// projectId → 项目根（record 的 `path` 字段；与 `find_task_record` 同款扫描）。
fn find_project_root(context: &Arc<CoreContext>, project_id: &str) -> Option<String> {
    let projects = aeroric_core::storage::load_projects(context).ok()?;
    for record in projects {
        if mutations::text(&record, "id") == Some(project_id) {
            return mutations::text(&record, "path").map(str::to_string);
        }
    }
    None
}

fn file_params(request: &Request) -> Result<(String, String), Box<Frame>> {
    let id = request.id.clone();
    let Some(project_id) = request
        .params
        .get("projectId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return Err(Box::new(failure(
            id,
            ErrorCode::InvalidArgument,
            "requires params.projectId",
        )));
    };
    let Some(relative) = request.params.get("path").and_then(Value::as_str) else {
        return Err(Box::new(failure(
            id,
            ErrorCode::InvalidArgument,
            "requires params.path",
        )));
    };
    Ok((project_id.to_string(), relative.to_string()))
}

fn files_read_dir(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    match aeroric_core::files::read_write::read_dir_entries(&root, &relative) {
        Ok(entries) => ok(&request, serde_json::json!({ "entries": entries })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_read_file(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    match aeroric_core::files::read_write::read_file_content(&root, &relative) {
        Ok(content) => ok(&request, serde_json::json!({ "content": content })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_write_file(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(content) = request.params.get("content").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.content",
        );
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    match aeroric_core::files::read_write::write_file_content(&root, &relative, content) {
        Ok(()) => ok(&request, serde_json::json!({ "written": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_create(request: Request, context: Arc<CoreContext>, directory: bool) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    let result = if directory {
        aeroric_core::files::read_write::create_directory(&root, &relative)
    } else {
        aeroric_core::files::read_write::create_file(&root, &relative)
    };
    match result {
        Ok(()) => ok(&request, serde_json::json!({ "created": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_delete(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    // 删除进废纸篓（不默认永久删除）；显式永久删除走独立的授权路径。
    match aeroric_core::files::read_write::delete_path(&root, &relative) {
        Ok(()) => ok(&request, serde_json::json!({ "deleted": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_rename(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(new_name) = request.params.get("newName").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.newName",
        );
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    match aeroric_core::files::read_write::rename_path(&root, &relative, new_name) {
        Ok(()) => ok(&request, serde_json::json!({ "renamed": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_replace_preview(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(query) = request.params.get("query").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.query",
        );
    };
    let Some(replacement) = request.params.get("replacement").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.replacement",
        );
    };
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    let content = match aeroric_core::files::read_write::read_file_content(&root, &relative) {
        Ok(content) => content,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    };
    // 单文件 preview：按行匹配 query（大小写不敏感；regex 走 search 面后续接）。
    let mut intervals: Vec<(u64, u64)> = Vec::new();
    let lower = content.to_lowercase();
    let needle = query.to_lowercase();
    let mut offset = 0usize;
    while let Some(found) = lower[offset..].find(&needle) {
        let start = offset + found;
        intervals.push((start as u64, (start + needle.len()) as u64));
        offset = start + needle.len();
    }
    ok(
        &request,
        serde_json::json!({
            "query": query,
            "replacement": replacement,
            "files": [{ "path": relative, "matches": intervals }],
        }),
    )
}

fn files_apply_replacements(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(replacement) = request.params.get("replacement").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.replacement",
        );
    };
    let Some(intervals) = request.params.get("matches").and_then(Value::as_array) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.matches",
        );
    };
    let matches: Vec<(u64, u64)> = intervals
        .iter()
        .filter_map(|item| Some((item.get("start")?.as_u64()?, item.get("end")?.as_u64()?)))
        .collect();
    let Some(root) = find_project_root(&context, &project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no project with that id",
        );
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    let files = vec![aeroric_core::files::search::ReplacePreviewFile {
        path: relative,
        matches,
    }];
    match aeroric_core::files::search::apply_replacements(&root, replacement, &files) {
        Ok(summary) => ok(&request, serde_json::json!(summary)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

#[cfg(test)]
mod terminal_replay_tests {
    use super::*;
    use aeroric_core::context::CoreContext;
    use aeroric_core::runtime::registry::TaskRegistry;
    use aeroric_core::terminal::pty::TerminalService;
    use aeroric_core::terminal::stream::TerminalIdentity;
    use aeroric_protocol::envelope::Frame;
    use base64::Engine as _;

    fn request(params: Value) -> Request {
        Request {
            v: 1,
            id: "t1".into(),
            method: "terminal.plan_replay".into(),
            idempotency_key: None,
            params,
            extra: Default::default(),
        }
    }

    fn result_of(frame: Frame) -> Value {
        match frame {
            Frame::Success(success) => success.result,
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    fn temp_context(tag: &str) -> std::sync::Arc<CoreContext> {
        let dir =
            std::env::temp_dir().join(format!("aeroric-replay-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::sync::Arc::new(CoreContext::read_only(
            aeroric_core::paths::AppPaths::with_data_dir(std::path::Path::new("/"), Some(&dir)),
        ))
    }

    #[test]
    fn plan_replay_returns_seamless_bytes_and_a_byte_resume_after() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("seamless");
        let identity = TerminalIdentity::new("task-1", 1);
        let _ = &identity;
        terminal.publish(&identity, b"hello terminal");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-1", "generation": 1,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "seamless");
        assert_eq!(result["from"], 0);
        assert_eq!(result["resumeAfter"], 14);
        assert_eq!(
            result["bytes"],
            base64::engine::general_purpose::STANDARD.encode(b"hello terminal")
        );
    }

    #[test]
    fn plan_replay_reports_cursor_beyond_the_stream_end() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("beyond");
        let identity = TerminalIdentity::new("task-1", 1);
        let _ = &identity;
        terminal.publish(&identity, b"abc");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-1", "generation": 1,
                "cursor": {"kind": "bytes", "value": 99},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "reset");
        assert_eq!(result["reason"]["kind"], "cursorBeyondStream");
        assert_eq!(result["reason"]["nextOffset"], 3);
        assert_eq!(result["resumeAfter"], 3);
    }

    #[test]
    fn plan_replay_classifies_a_registered_live_run_as_not_started() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("not-started");
        registry.runs.register("task-live", 3);
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-live", "generation": 3,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "reset");
        assert_eq!(result["reason"]["kind"], "notStarted");
    }

    #[test]
    fn plan_replay_falls_back_to_unknown_when_nothing_knows_the_task() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("unknown");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "never-heard-of", "generation": 9,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["reason"]["kind"], "unknownStream");
    }

    #[test]
    fn lease_tickets_round_trip_through_the_wire_shape() {
        let terminal = TerminalService::new();
        let acquired = result_of(terminal_acquire_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1})),
            &terminal,
        ));
        let ticket = acquired["ticket"].as_u64().expect("ticket is a number");
        assert!(ticket > 0);
        let released = result_of(terminal_release_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1, "ticket": ticket})),
            &terminal,
        ));
        assert_eq!(released["released"], true);
        // 过期票据再放＝no-op 成功（P3-07 语义），不报错。
        let again = result_of(terminal_release_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1, "ticket": ticket})),
            &terminal,
        ));
        assert_eq!(again["released"], false);
    }
}
