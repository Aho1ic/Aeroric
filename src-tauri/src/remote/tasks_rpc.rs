//! 任务操作 RPC:发 prompt、审批、取消/完成(直调 pty 内核)、
//! 新建/恢复(事件桥转桌面前端执行)。
//!
//! create/resume 为什么走事件桥而不直调 run_task/resume_task:两者都要求前端
//! 持有的输出 Channel(见 plan 风险表),桌面 App 运行是远程功能前提,因此把
//! 请求转成 `remote-task-request` 事件,由桌面前端复用现成 handleSubmitTask /
//! handleResumeTask 完整流程(worktree、附件、终端 buffer 等逻辑零重复)。

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::oneshot;

use super::agent_keymap;
use super::rpc::str_param;
use super::session_push::{is_ssh_project, load_project_task};
use super::RemoteState;
use crate::{storage, TaskManager};

const MAX_PROMPT_BYTES: usize = 100 * 1024;
const PERMISSION_MODES: &[&str] = &["ask", "auto_edit", "full_access"];
const TASK_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub(crate) struct TaskRequestBroker {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
}

impl TaskRequestBroker {
    fn register(&self) -> (String, oneshot::Receiver<Result<Value, String>>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id.clone(), tx);
        (request_id, rx)
    }

    pub(crate) fn resolve(
        &self,
        request_id: &str,
        result: Result<Value, String>,
    ) -> Result<(), String> {
        let sender = self
            .pending
            .lock()
            .remove(request_id)
            .ok_or_else(|| "Task request expired or was already completed".to_string())?;
        sender
            .send(result)
            .map_err(|_| "Task request is no longer waiting".to_string())
    }

    fn cancel(&self, request_id: &str) {
        self.pending.lock().remove(request_id);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApprovalRequest {
    pub request_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Default)]
pub(crate) struct ApprovalRegistry {
    pending: Mutex<HashMap<String, ApprovalEntry>>,
}

#[derive(Clone)]
struct ApprovalEntry {
    request: ApprovalRequest,
    agent: String,
}

impl ApprovalRegistry {
    pub(crate) fn register(
        &self,
        task_id: &str,
        agent: &str,
        kind: &str,
        tool_name: Option<&str>,
    ) -> ApprovalRequest {
        let request = ApprovalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            kind: kind.to_string(),
            tool_name: tool_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        };
        self.pending.lock().insert(
            task_id.to_string(),
            ApprovalEntry {
                request: request.clone(),
                agent: agent.to_string(),
            },
        );
        request
    }

    pub(crate) fn clear(&self, task_id: &str) {
        self.pending.lock().remove(task_id);
    }

    pub(crate) fn get(&self, task_id: &str) -> Option<ApprovalRequest> {
        self.pending
            .lock()
            .get(task_id)
            .map(|entry| entry.request.clone())
    }

    fn take_matching(&self, task_id: &str, request_id: &str) -> Result<ApprovalEntry, String> {
        let mut pending = self.pending.lock();
        let Some(entry) = pending.get(task_id) else {
            return Err("Task is no longer waiting for an approval".to_string());
        };
        if entry.request.request_id != request_id {
            return Err("Approval request is stale".to_string());
        }
        Ok(pending.remove(task_id).expect("entry checked above"))
    }

    fn restore(&self, task_id: String, entry: ApprovalEntry) {
        self.pending.lock().insert(task_id, entry);
    }
}

/// RPC `agents.list`:手机新建任务页的 agent 选项(内置 + 自定义 profile 窄面视图,
/// 不回传 base_url/api_key 等敏感字段 —— 建任务不需要它们)。
///
/// 注:凭据不是全局不可见了 —— 手机端「Agent 配置」页走独立的
/// `agentConfig.list` / `agentConfig.save`(见 `agent_config_rpc.rs`),那里按用户
/// 要求刻意回传并允许改写。本接口保持窄面,不要顺手往里加字段。
pub(crate) async fn agents_list() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let settings = crate::app_settings::load_settings_internal();
        let mut agents = vec![
            json!({ "id": "claude", "label": "Claude Code", "codexLike": false }),
            json!({ "id": "codex", "label": "Codex", "codexLike": true }),
        ];
        for profile in &settings.custom_agents {
            if profile.id.is_empty() {
                continue;
            }
            let label = if profile.label.trim().is_empty() {
                profile.id.clone()
            } else {
                profile.label.clone()
            };
            agents.push(json!({
                "id": profile.id,
                "label": label,
                "codexLike": profile.codex_like,
            }));
        }
        Ok(json!(agents))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// RPC `task.input { taskId, text }`:会话页底部输入框发 prompt。
/// bracketed paste 包裹(多行安全,与桌面终端粘贴同语义)后回车提交。
pub(crate) fn task_input<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    let task_id = str_param(params, "taskId")?;
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_end_matches(['\r', '\n']);
    if text.trim().is_empty() {
        return Err("Empty prompt".to_string());
    }
    if text.len() > MAX_PROMPT_BYTES {
        return Err("Prompt too long".to_string());
    }
    crate::pty::validate_task_id(&task_id)?;
    let tm = app.state::<TaskManager>();
    if !tm.pty_writers.lock().contains_key(&task_id) {
        return Err("Task is not running".to_string());
    }
    let data = format!("\x1b[200~{text}\x1b[201~\r");
    crate::pty::write_task_input(&tm, &task_id, &data)?;
    Ok(json!({ "sent": true }))
}

/// RPC `task.respond { taskId, requestId, action, keys? }`:审批放行/拒绝。
/// agent 始终从持久化任务读取;requestId 必须匹配当前待审批项,避免旧卡片盲注入按键。
pub(crate) async fn task_respond<R: Runtime>(
    app: &AppHandle<R>,
    params: Value,
) -> Result<Value, String> {
    let task_id = str_param(&params, "taskId")?;
    let request_id = str_param(&params, "requestId")?;
    let action = str_param(&params, "action")?;
    if !matches!(action.as_str(), "approve" | "deny") {
        return Err("Remote approval action must be approve or deny".to_string());
    }
    crate::pty::validate_task_id(&task_id)?;
    let action = agent_keymap::parse_action(&action, None)?;

    let lookup_task_id = task_id.clone();
    let task = tauri::async_runtime::spawn_blocking(move || load_task_by_id(&lookup_task_id))
        .await
        .map_err(|e| e.to_string())??;
    let agent = task.agent;
    let approval = app
        .state::<RemoteState>()
        .approvals
        .take_matching(&task_id, &request_id)?;
    if approval.agent != agent {
        app.state::<RemoteState>()
            .approvals
            .restore(task_id.clone(), approval);
        return Err("Approval agent no longer matches the task".to_string());
    }

    let is_codex = tauri::async_runtime::spawn_blocking(move || {
        crate::app_settings::is_codex_like_agent(&agent)
    })
    .await
    .map_err(|e| e.to_string())?;

    let steps = agent_keymap::keys_for(is_codex, action);
    let tm = app.state::<TaskManager>();
    if !tm.pty_writers.lock().contains_key(&task_id) {
        app.state::<RemoteState>()
            .approvals
            .restore(task_id.clone(), approval);
        return Err("Task is not running".to_string());
    }
    for step in steps {
        if step.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(step.delay_ms)).await;
        }
        if let Err(err) = crate::pty::write_task_input(&tm, &task_id, &step.data) {
            app.state::<RemoteState>()
                .approvals
                .restore(task_id.clone(), approval);
            return Err(err);
        }
    }
    Ok(json!({ "sent": true }))
}

/// RPC `task.cancel { projectId, taskId }`:直调桌面 cancel_task 同一内核。
pub(crate) async fn task_cancel<R: Runtime>(
    app: &AppHandle<R>,
    params: Value,
) -> Result<Value, String> {
    let (project, task) = load_task_for_lifecycle(&params).await?;
    let project_path = task
        .worktree_path
        .clone()
        .unwrap_or_else(|| project.path.clone());
    let tm = app.state::<TaskManager>();
    crate::pty::cancel_task_core(app, &tm, &task.id, &project_path)?;
    Ok(json!({ "ok": true }))
}

/// RPC `task.complete { projectId, taskId }`:直调桌面 complete_task 同一内核。
pub(crate) async fn task_complete<R: Runtime>(
    app: &AppHandle<R>,
    params: Value,
) -> Result<Value, String> {
    let (project, task) = load_task_for_lifecycle(&params).await?;
    let project_path = task
        .worktree_path
        .clone()
        .unwrap_or_else(|| project.path.clone());
    let tm = app.state::<TaskManager>();
    crate::pty::complete_task_core(app, &tm, &task.id, &project_path)?;
    Ok(json!({ "ok": true }))
}

async fn load_task_for_lifecycle(
    params: &Value,
) -> Result<(crate::storage::Project, crate::storage::Task), String> {
    let project_id = str_param(params, "projectId")?;
    let task_id = str_param(params, "taskId")?;
    let (project, task) = load_project_task(project_id, task_id).await?;
    if is_ssh_project(&project) {
        // SSH 任务的取消/完成涉及远端进程管理,v1 请在桌面端操作
        return Err("SSH tasks must be managed from the desktop".to_string());
    }
    Ok((project, task))
}

/// RPC `task.resume { projectId, taskId }`:转桌面前端 handleResumeTask
/// (session 定位/恢复逻辑全在前端,复用之)。
pub(crate) async fn task_resume<R: Runtime>(
    app: &AppHandle<R>,
    params: &Value,
) -> Result<Value, String> {
    let project_id = str_param(params, "projectId")?;
    let task_id = str_param(params, "taskId")?;
    crate::pty::validate_task_id(&task_id)?;
    forward_task_request(
        app,
        json!({ "kind": "resume", "projectId": project_id, "taskId": task_id }),
    )
    .await
}

/// RPC `agents.models { agent }`:某个 agent 的可选模型列表。
///
/// 安全:只回传 `models`,刻意丢弃 `AgentModels::balance`(账户余额属敏感信息)。
/// 与 `agents_list` 一样保持窄面;需要读写 base_url/api_key 的走 `agentConfig.*`。
pub(crate) async fn agents_models(params: &Value) -> Result<Value, String> {
    let agent = str_param(params, "agent")?;
    if agent.len() > 64 {
        return Err("Invalid agent".to_string());
    }
    let models = crate::app_settings::list_agent_models(agent).await?.models;
    Ok(json!({ "models": models }))
}

/// RPC `task.create { projectId, prompt, agent, permissionMode }`:参数校验后
/// 转桌面前端 handleSubmitTask。projectId 是否存在由前端判定并 toast,
/// 服务端不读存储,保持该路径零盘 IO(桌面必在线,反馈闭环在 UI)。
pub(crate) async fn task_create<R: Runtime>(
    app: &AppHandle<R>,
    params: &Value,
) -> Result<Value, String> {
    let project_id = str_param(params, "projectId")?;
    let prompt = str_param(params, "prompt")?;
    let agent = str_param(params, "agent")?;
    let permission_mode = str_param(params, "permissionMode")?;
    // 未选具体模型时省略该字段,由桌面端沿用其默认模型。
    let selected_model = params
        .get("selectedModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("Prompt too long".to_string());
    }
    if agent.len() > 64 {
        return Err("Invalid agent".to_string());
    }
    if !PERMISSION_MODES.contains(&permission_mode.as_str()) {
        return Err(format!("Invalid permissionMode: {permission_mode}"));
    }
    if selected_model.as_ref().is_some_and(|m| m.len() > 128) {
        return Err("Invalid selectedModel".to_string());
    }

    forward_task_request(
        app,
        json!({
            "kind": "create",
            "projectId": project_id,
            "prompt": prompt,
            "agent": agent,
            "permissionMode": permission_mode,
            "selectedModel": selected_model,
        }),
    )
    .await
}

async fn forward_task_request<R: Runtime>(
    app: &AppHandle<R>,
    mut payload: Value,
) -> Result<Value, String> {
    let broker = app.state::<RemoteState>().task_requests.clone();
    let (request_id, receiver) = broker.register();
    payload["requestId"] = json!(request_id);
    if let Err(err) = app.emit("remote-task-request", payload) {
        broker.cancel(&request_id);
        return Err(err.to_string());
    }
    match tokio::time::timeout(TASK_REQUEST_TIMEOUT, receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("Desktop task request was cancelled".to_string()),
        Err(_) => {
            broker.cancel(&request_id);
            Err("Desktop did not confirm the task request in time".to_string())
        }
    }
}

fn load_task_by_id(task_id: &str) -> Result<storage::Task, String> {
    let projects = storage::load_projects()?;
    let mut found = None;
    for project in projects {
        for task in storage::load_project_tasks(project.id)? {
            if task.id != task_id {
                continue;
            }
            if found.is_some() {
                return Err(format!("Task id is ambiguous: {task_id}"));
            }
            found = Some(task);
        }
    }
    found.ok_or_else(|| format!("Task not found: {task_id}"))
}

/// 首页统计卡数据。
///
/// `storage::Task` 只有 `created_at`,没有结束时间戳,因此已结束任务的耗时无法还原:
/// `agentTimeMs` 只累加当前在跑任务的 `now - created_at`(故带 approximate 标记),
/// 而 Aeroric 没有 PR 能力,`totalPRsCreated` 恒为 null 由前端渲染成占位符。
pub(crate) async fn stats_summary() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let now = chrono::Utc::now().timestamp_millis();
        let mut total_agents = 0u64;
        let mut agent_time_ms = 0i64;
        for project in storage::load_projects()? {
            for task in storage::load_project_tasks(project.id)? {
                total_agents += 1;
                if matches!(task.status.as_str(), "running" | "detached") {
                    agent_time_ms += (now - task.created_at).max(0);
                }
            }
        }
        Ok(json!({
            "totalAgentsSpawned": total_agents,
            "agentTimeMs": agent_time_ms,
            "agentTimeApproximate": true,
            "totalPRsCreated": Value::Null,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_registry_rejects_stale_and_consumed_requests() {
        let registry = ApprovalRegistry::default();
        let current = registry.register("task-1", "claude", "Notification", Some("Bash"));

        assert!(registry.take_matching("task-1", "old-request").is_err());
        let entry = registry
            .take_matching("task-1", &current.request_id)
            .expect("current request");
        assert_eq!(entry.agent, "claude");
        assert_eq!(entry.request.tool_name.as_deref(), Some("Bash"));
        assert!(registry
            .take_matching("task-1", &current.request_id)
            .is_err());
    }

    #[test]
    fn newer_approval_invalidates_previous_request_id() {
        let registry = ApprovalRegistry::default();
        let old = registry.register("task-1", "claude", "Notification", Some("Read"));
        let current = registry.register("task-1", "claude", "Notification", Some("Bash"));

        assert_ne!(old.request_id, current.request_id);
        assert!(registry.take_matching("task-1", &old.request_id).is_err());
        assert!(registry
            .take_matching("task-1", &current.request_id)
            .is_ok());
    }

    #[test]
    fn task_request_broker_resolves_once() {
        let broker = TaskRequestBroker::default();
        let (request_id, receiver) = broker.register();
        broker
            .resolve(&request_id, Ok(json!({ "accepted": true })))
            .unwrap();
        let result = tauri::async_runtime::block_on(receiver)
            .expect("sender alive")
            .expect("accepted");
        assert_eq!(result["accepted"], json!(true));
        assert!(broker
            .resolve(&request_id, Ok(json!({ "accepted": true })))
            .is_err());
    }
}
