//! 结构化会话 → 手机端:全量拉取 RPC 与 watcher 增量推送。
//!
//! - `session.messages`:按 projectId/taskId 定位会话文件,复用
//!   `session::read_session_messages` 全量解析(桌面 SessionView 同源)。
//! - `session.appended`:session.rs 的 JSONL watcher 每读到一批新行,经
//!   `publish_session_appended` 解析成 SessionMessage 推给全部在线手机。
//!   手机端刚打开详情页的瞬间可能出现「全量已含、增量又推」的少量重叠,
//!   由手机侧「加载中标记 dirty、完成后重拉」策略消化(见 mobile SessionPane)。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use super::rpc::str_param;
use super::RemoteState;
use crate::storage::{self, Project, ProjectLocation, Task};

/// 读取项目与任务(阻塞 IO 走 spawn_blocking)。tasks_rpc 复用。
pub(crate) async fn load_project_task(
    project_id: String,
    task_id: String,
) -> Result<(Project, Task), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let projects = storage::load_projects()?;
        let project = projects
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        let tasks = storage::load_project_tasks(project_id)?;
        let task = tasks
            .into_iter()
            .find(|t| t.id == task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;
        Ok((project, task))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn is_ssh_project(project: &Project) -> bool {
    matches!(project.location, Some(ProjectLocation::Ssh { .. }))
}

/// RPC `session.messages { projectId, taskId }`。
/// 会话尚未建立(任务没跑过 / SSH 远端)不是错误:返回 `available:false` + 原因,
/// 手机端据此引导用户切终端 tab。
pub(crate) async fn session_messages(params: Value) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let task_id = str_param(&params, "taskId")?;
    let (project, task) = load_project_task(project_id, task_id).await?;

    if is_ssh_project(&project) {
        // SSH 任务的会话 JSONL 在远端主机,v1 手机端只提供终端流
        return Ok(json!({ "available": false, "reason": "ssh", "messages": [] }));
    }

    let agent = task.agent.clone();
    let family =
        tauri::async_runtime::spawn_blocking(move || crate::app_settings::agent_family(&agent))
            .await
            .map_err(|e| e.to_string())?;
    // 实际会话族以持久化的 sessionFamily 为准(配置切换后 task.agent 可能误导)。
    let family = task
        .session_family
        .as_deref()
        .and_then(crate::app_settings::AgentFamily::parse)
        .unwrap_or(family);
    let is_codex = family.is_codex_like();

    let session_path = match family {
        crate::app_settings::AgentFamily::Codex => task.codex_session_path.clone(),
        crate::app_settings::AgentFamily::Claude => task.claude_session_path.clone(),
        crate::app_settings::AgentFamily::Dsh => task.dsh_session_path.clone(),
    };
    let Some(session_path) = session_path else {
        return Ok(json!({ "available": false, "reason": "no_session", "messages": [] }));
    };

    let project_path = task
        .worktree_path
        .clone()
        .unwrap_or_else(|| project.path.clone());
    let messages = crate::session::read_session_messages(
        session_path,
        project_path,
        is_codex,
        Some(family.as_str().to_string()),
    )
    .await?;
    Ok(json!({ "available": true, "isCodex": is_codex, "messages": messages }))
}

/// session.rs watcher 钩子:新到的一批 JSONL 行 → SessionMessage → 推送。
/// 无在线手机时零成本返回(不解析)。批内保留既有合并语义(codex 相邻
/// assistant 合并),批与批之间不合并,由手机端渲染层做相邻合并。
pub(crate) fn publish_session_appended<R: Runtime>(
    app: &AppHandle<R>,
    task_id: &str,
    lines: &[String],
    is_codex: bool,
) {
    if lines.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<RemoteState>() else {
        return;
    };
    if state.clients.online_count() == 0 {
        return;
    }
    let mut messages = Vec::new();
    crate::session::parse_session_lines(lines, is_codex, &mut messages);
    if messages.is_empty() {
        return;
    }
    let data = json!({ "task_id": task_id, "messages": messages });
    state
        .clients
        .broadcast_push("session.appended", None, &data);
}
