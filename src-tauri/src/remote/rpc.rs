//! 远程 RPC 方法调度 —— 刻意窄面。
//!
//! 只暴露移动端所需的白名单方法,绝不透传桌面全部 tauri command。
//! 新增方法 = 新增一个 match 分支 + 对应实现,便于审计。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use super::{event_log, files_rpc, session_push, tasks_rpc};
use crate::storage;

pub(crate) fn str_param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing param: {key}"))
}

/// 已认证连接的请求入口。auth 在 server 层处理,这里只见白名单业务方法。
pub async fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match method {
        "ping" => Ok(json!("pong")),
        "hello" => hello(app),
        "events.since" => events_since(app, &params),
        "agents.list" => tasks_rpc::agents_list().await,
        "projects.list" => projects_list().await,
        "tasks.list" => tasks_list(app, params).await,
        "tasks.get" => tasks_get(app, params).await,
        "session.messages" => session_push::session_messages(params).await,
        "project.files" => files_rpc::project_files(params).await,
        "project.readFile" => files_rpc::project_read_file(params).await,
        "git.changes" => files_rpc::git_changes(params).await,
        "git.diff" => files_rpc::git_diff(params).await,
        "task.input" => tasks_rpc::task_input(app, &params),
        "task.respond" => tasks_rpc::task_respond(app, params).await,
        "task.cancel" => tasks_rpc::task_cancel(app, params).await,
        "task.complete" => tasks_rpc::task_complete(app, params).await,
        "task.resume" => tasks_rpc::task_resume(app, &params).await,
        "task.create" => tasks_rpc::task_create(app, &params).await,
        other => Err(format!("Unknown method: {other}")),
    }
}

fn hello<R: Runtime>(app: &AppHandle<R>) -> Result<Value, String> {
    let info = app.package_info();
    Ok(json!({
        "name": "aeroric",
        "version": info.version.to_string(),
        "platform": std::env::consts::OS,
    }))
}

/// 重连补发:返回 watermark 之后错过的推送事件(见 event_log.rs)。
fn events_since<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    let after = params.get("after").and_then(Value::as_u64).unwrap_or(0);
    let log = app.state::<super::RemoteState>().event_log.clone();
    Ok(event_log::since_response(&log, after))
}

async fn projects_list() -> Result<Value, String> {
    let projects = tauri::async_runtime::spawn_blocking(storage::load_projects)
        .await
        .map_err(|e| e.to_string())??;
    serde_json::to_value(projects).map_err(|e| e.to_string())
}

async fn tasks_list<R: Runtime>(app: &AppHandle<R>, params: Value) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let tasks =
        tauri::async_runtime::spawn_blocking(move || storage::load_project_tasks(project_id))
            .await
            .map_err(|e| e.to_string())??;
    let mut value = serde_json::to_value(tasks).map_err(|e| e.to_string())?;
    if let Some(items) = value.as_array_mut() {
        let approvals = app.state::<super::RemoteState>().approvals.clone();
        for item in items {
            let Some(task_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(approval) = approvals.get(task_id) {
                item["approval"] = serde_json::to_value(approval).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(value)
}

async fn tasks_get<R: Runtime>(app: &AppHandle<R>, params: Value) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let task_id = str_param(&params, "taskId")?;
    let tasks =
        tauri::async_runtime::spawn_blocking(move || storage::load_project_tasks(project_id))
            .await
            .map_err(|e| e.to_string())??;
    let task = tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("Task not found: {task_id}"))?;
    let task_id = task.id.clone();
    let mut value = serde_json::to_value(task).map_err(|e| e.to_string())?;
    if let Some(approval) = app.state::<super::RemoteState>().approvals.get(&task_id) {
        value["approval"] = serde_json::to_value(approval).map_err(|e| e.to_string())?;
    }
    Ok(value)
}
