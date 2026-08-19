//! 远程 RPC 方法调度 —— 刻意窄面。
//!
//! 只暴露移动端所需的白名单方法,绝不透传桌面全部 tauri command。
//! 新增方法 = 新增一个 match 分支 + 对应实现,便于审计。

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::{agent_config_rpc, event_log, files_rpc, session_push, tasks_rpc};
use crate::storage;

/// Capabilities are the remotely safe contract surface. Desktop-only UI
/// panels must not be inferred from this list unless they have a matching RPC.
pub(crate) const RPC_CAPABILITIES: &[&str] = &[
    "typed-envelope",
    "structured-error",
    "events-replay",
    "projects.grouping",
    "projects.pinning",
    "tasks.lifecycle",
    "tasks.models",
    "tasks.approvals",
    "session.structured",
    "session.dsh",
    "terminal.stream",
    "files.read",
    "files.write",
    "git.read",
    "agent-config.status",
    "agent-config.write",
    "stats.summary",
];

pub(crate) fn rpc_capabilities_value() -> Value {
    json!(RPC_CAPABILITIES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_contract_is_additive_and_contains_core_mobile_flows() {
        let capabilities = rpc_capabilities_value();
        let values = capabilities.as_array().expect("capability array");
        assert!(values.iter().any(|value| value == "tasks.lifecycle"));
        assert!(values.iter().any(|value| value == "tasks.models"));
        assert!(values.iter().any(|value| value == "session.structured"));
        assert!(values.iter().any(|value| value == "files.read"));
    }
}

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
        "hello" => hello(app).await,
        "events.since" => events_since(app, &params),
        "agents.list" => tasks_rpc::agents_list().await,
        "agents.models" => tasks_rpc::agents_models(&params).await,
        "stats.summary" => tasks_rpc::stats_summary().await,
        "projects.list" => projects_list().await,
        "projects.setPinned" => projects_set_pinned(app, params).await,
        "tasks.list" => tasks_list(app, params).await,
        "tasks.get" => tasks_get(app, params).await,
        "session.messages" => session_push::session_messages(params).await,
        "project.files" => files_rpc::project_files(params).await,
        "project.readFile" => files_rpc::project_read_file(params).await,
        "project.writeFile" => files_rpc::project_write_file(params).await,
        "agentConfig.list" => agent_config_rpc::agent_config_list().await,
        "agentConfig.save" => agent_config_rpc::agent_config_save(app, params).await,
        "agentConfig.detectModels" => agent_config_rpc::agent_config_detect_models(params).await,
        "agentConfig.create" => agent_config_rpc::agent_config_create(app, params).await,
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

/// `hello`:除了版本信息,还回传**实时**主机身份与候选地址。
///
/// 手机端用 `hostId` 把「同一台电脑换了网段」识别成同一条已配对记录,并用
/// `lanEndpoints` 刷新其中过期的内网地址(见 `mod.rs::live_identity`)。
async fn hello<R: Runtime>(app: &AppHandle<R>) -> Result<Value, String> {
    let info = app.package_info();
    let mut value = json!({
        "name": "aeroric",
        "version": info.version.to_string(),
        "platform": std::env::consts::OS,
        "rpcVersions": [3, 2],
        "capabilities": rpc_capabilities_value(),
    });
    if let (Some(target), Some(identity)) = (
        value.as_object_mut(),
        super::live_identity(app).await.as_object(),
    ) {
        for (key, item) in identity {
            target.insert(key.clone(), item.clone());
        }
    }
    Ok(value)
}

/// RPC `projects.setPinned { projectId, pinned }`:跨端同步的置顶开关。
/// 只改 `pinned`,不动 `orderIndex`,避免与桌面拖拽排序语义冲突。
async fn projects_set_pinned<R: Runtime>(
    app: &AppHandle<R>,
    params: Value,
) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let pinned = params
        .get("pinned")
        .and_then(Value::as_bool)
        .ok_or("Missing param: pinned")?;
    let updated_project_id = project_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        storage::update_projects(|projects| {
            let project = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| format!("Project not found: {project_id}"))?;
            project.pinned = pinned;
            Ok(())
        })
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())??;
    // 桌面 WebView 持有项目内存快照；字段级事件让它合并手机端写入并把
    // 当前最新快照排入持久化队列，避免下一次桌面操作把 pinned 覆盖回去。
    let _ = app.emit(
        "project-pinned-changed",
        json!({ "projectId": updated_project_id, "pinned": pinned }),
    );
    Ok(json!({ "ok": true }))
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
