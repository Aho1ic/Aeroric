//! 远程 RPC 方法调度 —— 刻意窄面（P8-03 刀三b 自宿主上收）。
//!
//! 只暴露移动端所需的白名单方法,绝不透传桌面全部 tauri command。
//! 新增方法 = 新增一个 match 分支 + 对应实现,便于审计。
//! 宿主能力（存储/PTY/git/files/agent 配置）经 [`RemoteHostHandle`] 注入;
//! 事件补发、审批叠加、hello 身份与版本协商是 core 自有面。

use serde_json::{json, Value};

use crate::remote::event_log;
use crate::remote::identity::{host_name, lan_addresses, selected_lan_ip};
use crate::remote::services::RemoteHostHandle;
use crate::remote::state::RemoteState;

/// Capabilities are the remotely safe contract surface. Desktop-only UI
/// panels must not be inferred from this list unless they have a matching RPC.
pub const RPC_CAPABILITIES: &[&str] = &[
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

pub fn rpc_capabilities_value() -> Value {
    json!(RPC_CAPABILITIES)
}

pub fn str_param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing param: {key}"))
}

/// `hello`:除了版本信息,还回传**实时**主机身份与候选地址。
///
/// 手机端用 `hostId` 把「同一台电脑换了网段」识别成同一条已配对记录,并用
/// `lanEndpoints` 刷新其中过期的内网地址。服务未运行时端口取配置值。
pub(crate) async fn live_identity(state: &RemoteState) -> Value {
    let running_port = state.running.lock().await.as_ref().map(|h| h.port);
    let addresses = lan_addresses();
    let (port, preferred_lan_ip, relay_url, public_endpoints) = {
        let cfg = state.config.lock();
        (
            running_port.unwrap_or(cfg.port),
            cfg.preferred_lan_ip.clone(),
            cfg.relay_url
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .map(str::to_string),
            cfg.public_endpoints.clone(),
        )
    };
    let mut endpoints = Vec::new();
    if let Some(ip) = selected_lan_ip(preferred_lan_ip.as_deref(), &addresses) {
        endpoints.push(format!("ws://{ip}:{port}"));
    }
    for address in &addresses {
        let endpoint = format!("ws://{}:{port}", address.ip);
        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }
    let lan_endpoints = endpoints.clone();
    endpoints.extend(public_endpoints);
    if let Some(relay) = relay_url {
        endpoints.push(remote_protocol::client_connect_url(
            &relay,
            &state.keys.host_id(),
        ));
    }
    json!({
        "hostId": state.keys.host_id(),
        "hostName": host_name(),
        "endpoints": endpoints,
        // 仅 LAN 的子集:手机端据此只替换旧的内网地址,保留用户手填的隧道地址
        "lanEndpoints": lan_endpoints,
    })
}

/// 已认证连接的请求入口。auth 在 server 层处理,这里只见白名单业务方法。
pub async fn dispatch(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match method {
        "ping" => Ok(json!("pong")),
        "hello" => hello(state, handle).await,
        "events.since" => Ok(events_since(state, &params)),
        "agents.list" => handle.agents_list().await,
        "agents.models" => {
            let agent = str_param(&params, "agent")?;
            handle.agents_models(&agent).await
        }
        "stats.summary" => handle.stats_summary().await,
        "projects.list" => handle.load_projects().await,
        "projects.setPinned" => projects_set_pinned(state, handle, params).await,
        "tasks.list" => tasks_list(state, handle, params).await,
        "tasks.get" => tasks_get(state, handle, params).await,
        "session.messages" => handle.session_messages(params).await,
        "project.files" => handle.project_files(params).await,
        "project.readFile" => handle.project_read_file(params).await,
        "project.writeFile" => handle.project_write_file(params).await,
        "agentConfig.list" => handle.agent_config_list().await,
        "agentConfig.save" => handle.agent_config_save(params).await,
        "agentConfig.detectModels" => handle.agent_config_detect_models(params).await,
        "agentConfig.create" => handle.agent_config_create(params).await,
        "git.changes" => handle.git_changes(params).await,
        "git.diff" => handle.git_diff(params).await,
        "task.input" => {
            let task_id = str_param(&params, "taskId")?;
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim_end_matches(['\r', '\n'])
                .to_string();
            handle.task_input(&task_id, &text).await
        }
        "task.respond" => {
            let task_id = str_param(&params, "taskId")?;
            handle.task_respond(&task_id, params).await
        }
        "task.cancel" => {
            let project_id = str_param(&params, "projectId")?;
            let task_id = str_param(&params, "taskId")?;
            handle.task_cancel(&project_id, &task_id).await
        }
        "task.complete" => {
            let project_id = str_param(&params, "projectId")?;
            let task_id = str_param(&params, "taskId")?;
            handle.task_complete(&project_id, &task_id).await
        }
        "task.resume" | "task.create" => {
            // 旧 RPC 处理器在转发前打 kind 标；直连后由调度补齐（手机端
            // 兼容：params 可能不带 kind）。
            let mut payload = params;
            if payload.get("kind").is_none() {
                let kind = if method == "task.create" {
                    "create"
                } else {
                    "resume"
                };
                payload["kind"] = json!(kind);
            }
            handle.task_request(payload).await
        }
        other => Err(format!("Unknown method: {other}")),
    }
}

/// `hello`:版本信息＋实时主机身份（core 自有面）。
async fn hello(state: &RemoteState, handle: &dyn RemoteHostHandle) -> Result<Value, String> {
    let mut value = json!({
        "name": "aeroric",
        "version": handle.desktop_version(),
        "platform": std::env::consts::OS,
        "rpcVersions": [3, 2],
        "capabilities": rpc_capabilities_value(),
    });
    if let (Some(target), Some(identity)) = (
        value.as_object_mut(),
        live_identity(state).await.as_object(),
    ) {
        for (key, item) in identity {
            target.insert(key.clone(), item.clone());
        }
    }
    Ok(value)
}

/// RPC `projects.setPinned { projectId, pinned }`:跨端同步的置顶开关。
/// 只改 `pinned`,不动 `orderIndex`,避免与桌面拖拽排序语义冲突。
async fn projects_set_pinned(
    _state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    params: Value,
) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let pinned = params
        .get("pinned")
        .and_then(Value::as_bool)
        .ok_or("Missing param: pinned")?;
    handle.set_project_pinned(&project_id, pinned).await?;
    // 桌面 WebView 持有项目内存快照；字段级事件让它合并手机端写入并把
    // 当前最新快照排入持久化队列，避免下一次桌面操作把 pinned 覆盖回去。
    // 双发:legacy 通道＋v1 topic（P8-03 刀六）。
    handle.emit(
        "project-pinned-changed",
        json!({ "projectId": project_id, "pinned": pinned }),
    );
    handle.emit(
        "project.pinned_changed",
        json!({ "projectId": project_id, "pinned": pinned }),
    );
    Ok(json!({ "ok": true }))
}

/// 重连补发:返回 watermark 之后错过的推送事件(见 event_log.rs)。
fn events_since(state: &RemoteState, params: &Value) -> Value {
    let after = params.get("after").and_then(Value::as_u64).unwrap_or(0);
    let log = state.event_log.clone();
    event_log::since_response(&log, after)
}

async fn tasks_list(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    params: Value,
) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let mut value = handle.load_project_tasks(&project_id).await?;
    if let Some(items) = value.as_array_mut() {
        for item in items {
            let Some(task_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(approval) = state.approvals.get(task_id) {
                item["approval"] = serde_json::to_value(approval).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(value)
}

async fn tasks_get(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    params: Value,
) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let task_id = str_param(&params, "taskId")?;
    let value = handle.load_project_tasks(&project_id).await?;
    let task = value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|t| t.get("id").and_then(Value::as_str) == Some(task_id.as_str()))
        })
        .ok_or_else(|| format!("Task not found: {task_id}"))?
        .clone();
    let mut task = task;
    if let Some(approval) = state
        .approvals
        .get(task.get("id").and_then(Value::as_str).unwrap_or(""))
    {
        task["approval"] = serde_json::to_value(approval).map_err(|e| e.to_string())?;
    }
    Ok(task)
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
