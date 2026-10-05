//! 任务操作 RPC:发 prompt、审批、取消/完成(直调对应 Agent 生命周期内核)、
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
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::oneshot;

use super::agent_keymap;
use super::rpc::str_param;
use std::sync::Arc;

use super::session_push::{is_ssh_project, load_project_task};
use super::RemoteState;
use crate::{storage, TaskManager};

const MAX_PROMPT_BYTES: usize = 100 * 1024;
const MAX_DSH_PRESET_LEN: usize = 64;
const PERMISSION_MODES: &[&str] = &["ask", "auto_edit", "full_access"];
const TASK_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub(crate) struct TaskRequestBroker {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
}

impl TaskRequestBroker {
    /// 刀二起 task.create/resume 原生化（execute_task_request 直连），oneshot
    /// 转发退役；register/cancel 仅存于测试（broker 语义保持可测）。
    #[cfg(test)]
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
        pending
            .remove(task_id)
            .ok_or_else(|| "Approval request disappeared before it could be consumed".to_string())
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
        /// 手机新建任务页的内置族。漏掉一族的表现不是报错,是那一列永远空着。
        const BUILTIN: &[(&str, &str, bool, &str)] = &[
            ("claude", "Claude Code", false, "claude"),
            ("codex", "Codex", true, "codex"),
            ("dsh", "DeepSeek Harness", false, "dsh"),
            ("omp", "oh-my-pi", false, "omp"),
        ];
        let mut agents: Vec<Value> = BUILTIN
            .iter()
            .map(|(id, label, codex_like, family)| {
                json!({ "id": id, "label": label, "codexLike": codex_like, "family": family })
            })
            .collect();
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
                "family": profile.agent_family().as_str(),
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
    if tm.writer_for(&task_id).is_none() {
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
    if tm.writer_for(&task_id).is_none() {
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

/// 解析任务的协议族:优先用会话族(已建会话的任务最准),否则回落到 agent 归属。
async fn lifecycle_family(
    task: &crate::storage::Task,
) -> Result<crate::app_settings::AgentFamily, String> {
    if let Some(family) = task
        .session_family
        .as_deref()
        .and_then(crate::app_settings::AgentFamily::parse)
    {
        return Ok(family);
    }
    let agent = task.agent.clone();
    tauri::async_runtime::spawn_blocking(move || crate::app_settings::agent_family(&agent))
        .await
        .map_err(|error| error.to_string())
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
    // omp 是 PTY 族,子进程就在 child_handles 里,与 claude/codex 走同一内核。
    crate::pty::cancel_task_core(app, &tm, &task.id, &project_path)?;
    Ok(json!({ "ok": true }))
}

/// RPC `task.complete { projectId, taskId }`:按会话族复用桌面完成内核。
pub(crate) async fn task_complete<R: Runtime>(
    app: &AppHandle<R>,
    params: Value,
) -> Result<Value, String> {
    let (project, task) = load_task_for_lifecycle(&params).await?;
    let project_path = task
        .worktree_path
        .clone()
        .unwrap_or_else(|| project.path.clone());
    let family = lifecycle_family(&task).await?;
    let tm = app.state::<TaskManager>();
    match family {
        crate::app_settings::AgentFamily::Dsh => {
            let dsh = app.state::<crate::dsh_webui::DshWebUiManager>();
            crate::dsh_webui::complete_dsh_task_core(app, &dsh, &tm, &task.id, &project_path)
                .await?;
        }
        _ => crate::pty::complete_task_core(app, &tm, &task.id, &project_path)?,
    }
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
    execute_task_request(
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
    require_known_agent(&agent).await?;
    let models = crate::app_settings::list_agent_models(agent).await?.models;
    Ok(json!({ "models": models }))
}

/// RPC `task.create { projectId, prompt, agent, permissionMode }`:参数校验后
/// 转桌面前端 handleSubmitTask。成功响应同时携带桌面生成的任务快照,
/// 让手机可以先就地渲染,再用 tasks.list 做一致性校验。
pub(crate) async fn task_create<R: Runtime>(
    app: &AppHandle<R>,
    params: &Value,
) -> Result<Value, String> {
    let project_id = str_param(params, "projectId")?;
    let prompt = str_param(params, "prompt")?;
    let agent = str_param(params, "agent")?;
    let permission_mode = str_param(params, "permissionMode")?;
    // 模型、推理强度和速度均由桌面端再次按 agent/model 规则校验。
    let selected_model = params
        .get("selectedModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let reasoning_effort = params
        .get("reasoningEffort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let dsh_agent_preset = params
        .get("dshAgentPreset")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut speed = params
        .get("speed")
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
    require_known_agent(&agent).await?;
    let family_agent = agent.clone();
    let family = tauri::async_runtime::spawn_blocking(move || {
        crate::app_settings::agent_family(&family_agent)
    })
    .await
    .map_err(|error| error.to_string())?;
    if !PERMISSION_MODES.contains(&permission_mode.as_str()) {
        return Err(format!("Invalid permissionMode: {permission_mode}"));
    }
    if selected_model.as_ref().is_some_and(|m| m.len() > 128) {
        return Err("Invalid selectedModel".to_string());
    }
    if reasoning_effort
        .as_ref()
        .is_some_and(|effort| effort.len() > 32)
    {
        return Err("Invalid reasoningEffort".to_string());
    }
    if speed.as_ref().is_some_and(|value| value.len() > 16) {
        return Err("Invalid speed".to_string());
    }
    if dsh_agent_preset
        .as_ref()
        .is_some_and(|value| value.len() > MAX_DSH_PRESET_LEN)
    {
        return Err("Invalid dshAgentPreset".to_string());
    }
    if family == crate::app_settings::AgentFamily::Dsh {
        if reasoning_effort.as_ref().is_some_and(|effort| {
            !matches!(effort.to_ascii_lowercase().as_str(), "off" | "high" | "max")
        }) {
            return Err("Invalid reasoningEffort for DeepSeek Harness".to_string());
        }
        // Older mobile clients always sent `standard`; DSH has no speed setting,
        // so strip either legacy value at the contract boundary.
        speed = None;
    }

    execute_task_request(
        app,
        json!({
            "kind": "create",
            "projectId": project_id,
            "prompt": prompt,
            "agent": agent,
            "permissionMode": permission_mode,
            "selectedModel": selected_model,
            "reasoningEffort": reasoning_effort,
            "speed": speed,
            "dshAgentPreset": dsh_agent_preset,
        }),
    )
    .await
}

async fn require_known_agent(agent: &str) -> Result<(), String> {
    let agent = agent.to_string();
    let known =
        tauri::async_runtime::spawn_blocking(move || crate::app_settings::is_known_agent(&agent))
            .await
            .map_err(|error| error.to_string())?;
    if known {
        Ok(())
    } else {
        Err("Unknown agent".to_string())
    }
}

async fn wait_ready(state: &RemoteState) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + TASK_REQUEST_TIMEOUT;
    while state.ready.get().is_none() {
        if tokio::time::Instant::now() >= deadline {
            return Err("Desktop initialization did not finish in time".to_string());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

async fn lock_project_queue(
    state: &RemoteState,
    project_id: &str,
) -> tokio::sync::OwnedMutexGuard<()> {
    let entry = state
        .task_queues
        .lock()
        .entry(project_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    entry.lock_owned().await
}

/// 任务是否带着可恢复会话（原 React `taskHasResumableSession` 逐字段镜像）。
fn task_has_resumable_session(task: &storage::Task) -> bool {
    task.claude_session_id.is_some()
        || task.codex_session_id.is_some()
        || task.claude_session_path.is_some()
        || task.codex_session_path.is_some()
        || task.dsh_session_id.is_some()
        || task.dsh_session_path.is_some()
        || task.omp_session_id.is_some()
        || task.omp_session_path.is_some()
}

/// P8-03 刀二：task.create / task.resume 的**宿主原生化执行器**。
///
/// 原 React 中转（`forward_task_request`）的语义在这里逐条落地：startup 等待
/// （`ready` 闸）→ 按 projectId 串行（`task_queues`，G05 的 host 侧等价判决）
/// → 校验（任务/项目归属、SSH 连接在线、SSH 可恢复会话）→ 直连 core service
/// （`TaskService.create` + `launch_via_core`/`resume_via_core`），不再 emit
/// 到 React 后等待 oneshot。应答契约逐字不变：
/// `Ok({"accepted":true,"taskId":…,"task":…})` / `Err(message)`。
async fn execute_task_request<R: Runtime>(
    app: &AppHandle<R>,
    mut payload: Value,
) -> Result<Value, String> {
    let state = app.state::<RemoteState>();
    wait_ready(&state).await?;
    let project_id = payload
        .get("projectId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let task_id = payload
        .get("taskId")
        .and_then(Value::as_str)
        .map(str::to_string);

    // 队列键：projectId ?? taskId 所属任务（原 React :116-120 语义）。
    let queue_key = match (project_id.clone(), task_id.clone()) {
        (Some(pid), _) => Some(pid),
        (None, Some(tid)) => tauri::async_runtime::spawn_blocking(move || {
            load_task_by_id(&tid).ok().map(|task| task.project_id)
        })
        .await
        .map_err(|e| e.to_string())?,
        (None, None) => None,
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    payload["requestId"] = json!(request_id);
    let result = match queue_key {
        Some(key) => {
            let _guard = lock_project_queue(&state, &key).await;
            run_task_request(app, &payload).await
        }
        None => run_task_request(app, &payload).await,
    };
    result
}

/// 校验＋启动内核（原 React runRequest 的 resume/create 两个分支）。
async fn run_task_request<R: Runtime>(
    app: &AppHandle<R>,
    payload: &Value,
) -> Result<Value, String> {
    let kind = str_param(payload, "kind")?;
    let task_id = payload
        .get("taskId")
        .and_then(Value::as_str)
        .map(str::to_string);
    if kind == "resume" {
        let Some(task_id) = task_id else {
            return Err("Resume request is missing taskId".to_string());
        };
        let lookup = task_id.clone();
        let task = tauri::async_runtime::spawn_blocking(move || load_task_by_id(&lookup))
            .await
            .map_err(|e| e.to_string())??;
        if let Some(project_id) = payload.get("projectId").and_then(Value::as_str) {
            if task.project_id != project_id {
                return Err("Task does not belong to the requested project".to_string());
            }
        }
        let projects = tauri::async_runtime::spawn_blocking(storage::load_projects)
            .await
            .map_err(|e| e.to_string())??;
        let Some(project) = projects.iter().find(|p| p.id == task.project_id) else {
            return Err("Task project is missing on the desktop".to_string());
        };
        if is_ssh_project(project) && !ssh_connection_configured(project) {
            return Err("SSH connection is not configured on the desktop".to_string());
        }
        if is_ssh_project(project) && !task_has_resumable_session(&task) {
            return Err("SSH task has no resumable session".to_string());
        }
        if task.status != "todo" {
            // 恢复：会话 id 由记录定位（原 React 的前端 session 定位逻辑，
            // 宿主版走 read_session_id 同一内核）。
            let session_id = resolve_resume_session_id(&task).await?;
            let (session_id, task_id_for_core) = (session_id, task_id.clone());
            let sink = remote_output_sink(app, &task_id_for_core);
            crate::core_adapter::resume_via_core(
                app,
                &task_id_for_core,
                &session_id,
                sink,
                true,
                None,
            )
            .map_err(|e| e.to_string())?;
            return Ok(task_accepted_payload(&task_id_for_core));
        }
        // todo 任务第一次启动 = 以记录里的 prompt 走 start。
        let sink = remote_output_sink(app, &task.id);
        crate::core_adapter::launch_via_core(
            app,
            &task.id,
            sink,
            true,
            None,
            false,
            aeroric_core::tasks::Attachments::default(),
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
        return Ok(task_accepted_payload(&task.id));
    }
    if kind != "create" {
        return Err("Invalid task creation request".to_string());
    }
    // create：记录经 core TaskService.create（唯一权威车道），启动走
    // launch_via_core 同一内核。prompt 等校验已在 task_create 完成。
    let project_id = str_param(payload, "projectId")?;
    let prompt = str_param(payload, "prompt")?;
    let agent = str_param(payload, "agent")?;
    let permission_mode = str_param(payload, "permissionMode")?;
    let projects = tauri::async_runtime::spawn_blocking(storage::load_projects)
        .await
        .map_err(|e| e.to_string())??;
    let Some(project) = projects.iter().find(|p| p.id == project_id) else {
        return Err("Project not found on the desktop".to_string());
    };
    if is_ssh_project(project) && !ssh_connection_configured(project) {
        return Err("SSH connection is not configured on the desktop".to_string());
    }
    let new_task = aeroric_core::tasks::NewTask {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.clone(),
        prompt,
        agent,
        permission_mode,
        immediate: true,
        selected_model: payload
            .get("selectedModel")
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_effort: payload
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string),
        speed: payload
            .get("speed")
            .and_then(Value::as_str)
            .map(str::to_string),
        dsh_agent_preset: payload
            .get("dshAgentPreset")
            .and_then(Value::as_str)
            .map(str::to_string),
        images: Vec::new(),
        texts: Vec::new(),
    };
    let service = app.state::<Arc<aeroric_core::tasks::TaskService>>();
    let core_project = core_project_record(&service, &project_id)?;
    let created = service
        .create(&core_project, new_task)
        .map_err(|e| e.to_string())?;
    let task_id = created.0.id().unwrap_or_default().to_string();
    let sink = remote_output_sink(app, &task_id);
    crate::core_adapter::launch_via_core(
        app,
        &task_id,
        sink,
        true,
        None,
        false,
        aeroric_core::tasks::Attachments::default(),
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(task_accepted_payload(&task_id))
}

/// SSH 项目必须在桌面配置过连接（宿主存储是权威；原 React 读 sshConnectionsRef）。
fn ssh_connection_configured(project: &storage::Project) -> bool {
    match &project.location {
        Some(storage::ProjectLocation::Ssh { connection_id, .. }) => {
            crate::ssh::ssh_connection_by_id(connection_id).is_ok()
        }
        _ => false,
    }
}

/// 项目 id → core `Record`（`project_record_for` 的按 id 版本）。
fn core_project_record(
    service: &aeroric_core::tasks::TaskService,
    project_id: &str,
) -> Result<aeroric_core::storage::Record, String> {
    aeroric_core::storage::projects::load_projects(service.context())
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|record| record.id() == Some(project_id))
        .ok_or_else(|| "Project not found on the desktop".to_string())
}

/// 原生任务输出：手机端经 terminal_hub 实时订阅，桌面端打开任务时按
/// 重连语义挂接（历史文件是权威补发源）。
fn remote_output_sink<R: Runtime>(_app: &AppHandle<R>, task_id: &str) -> crate::pty::OutputSink {
    let task_id = task_id.to_string();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let channel =
        tauri::ipc::Channel::<String>::new(move |data: tauri::ipc::InvokeResponseBody| {
            match data {
                tauri::ipc::InvokeResponseBody::Json(text) => {
                    let _ = tx.send(text);
                }
                tauri::ipc::InvokeResponseBody::Raw(bytes) => {
                    if let Ok(text) = String::from_utf8(bytes) {
                        let _ = tx.send(text);
                    }
                }
            }
            Ok(())
        });
    let hub_task_id = task_id.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            aeroric_core::remote::terminal_hub::hub().publish(&hub_task_id, &chunk);
        }
    });
    crate::pty::OutputSink::Channel(channel)
}

fn task_accepted_payload(task_id: &str) -> Value {
    json!({ "accepted": true, "taskId": task_id, "task": Value::Null })
}

/// 会话定位：优先记录里的会话 id，缺了再从会话文件头读（宿主版
/// read_session_id 同一内核）。原 React「session 定位全在前端」的宿主化。
async fn resolve_resume_session_id(task: &storage::Task) -> Result<String, String> {
    let family = lifecycle_family(task).await?;
    let (id, path) = match family {
        crate::app_settings::AgentFamily::Dsh => {
            (task.dsh_session_id.clone(), task.dsh_session_path.clone())
        }
        crate::app_settings::AgentFamily::Omp => {
            (task.omp_session_id.clone(), task.omp_session_path.clone())
        }
        _ => (
            task.claude_session_id.clone(),
            task.claude_session_path.clone(),
        ),
    };
    if let Some(id) = id {
        return Ok(id);
    }
    let Some(path) = path else {
        return Err("Task has no resumable session".to_string());
    };
    let is_codex = matches!(
        lifecycle_family(task).await?,
        crate::app_settings::AgentFamily::Codex
    );
    let session_id = crate::session::read_session_id(path, String::new(), is_codex, None).await?;
    session_id.ok_or_else(|| "Task has no resumable session".to_string())
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

    #[test]
    fn agents_list_includes_built_in_omp() {
        let listed = tauri::async_runtime::block_on(agents_list()).expect("list");
        let agents = listed.as_array().expect("array");
        let omp = agents
            .iter()
            .find(|entry| entry["id"] == "omp")
            .expect("内置 omp 必须出现在 agents.list 里");
        assert_eq!(omp["family"], "omp");
        assert_eq!(omp["label"], "oh-my-pi");
        assert_eq!(omp["codexLike"], false);
        // 旧三族不能被挤掉。
        for id in ["claude", "codex", "dsh"] {
            assert!(
                agents.iter().any(|entry| entry["id"] == id),
                "missing built-in {id}"
            );
        }
    }
}
