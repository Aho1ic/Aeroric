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
