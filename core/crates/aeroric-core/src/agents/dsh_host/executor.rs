//! dsh 启动编排（P3-05 kernel 片，补记二百六十四 D1/D2 的执行体）。
//!
//! 自宿主 `dsh_webui.rs::run_dsh_task_core` verbatim 上收：编排顺序、错误文案、
//! 注释语义一行没改；改动的只有三类机械项——
//! 1. `AppHandle<R>`/`State<DshWebUiManager>`/`Channel<String>` 全部换成
//!    [`DshHostPorts`] 端口调用（宿主地盘清单见 ports.rs 的逐端口出处）；
//! 2. `crate::run_registry::RunKey` 拆成 `task_id + generation`（core 不持宿主
//!    RunKey）；
//! 3. `dsh_protocol::DshModelSelection` 等类型走同 crate 的 [`dto`]。
//!
//! 失败路径的 `retire_runs` + `report_status("failed", reason)` 与原文一致：
//! 状态由这条上报结算，不新增 core 侧状态机。

use serde_json::Value;

use super::api::DshApiClient;
use super::approvals::dsh_permission_preset;
use super::dto::{supported_dsh_reasoning_effort, DshModelSelection};
use super::ports::{DshHostPorts, DshSessionRegistration, DshStreamRequest, OutputSink};
use super::projection::user_prompt_echo;

/// `run_dsh_task_core` 的入参事实（宿主命令层拆 `DshWindowFacts` 后传入）。
/// 字段表与宿主原参数一一对应；`generation` 是 minted run 的代，
/// `sink` 是类型擦除后的终端输出通道。
pub struct DshLaunch {
    pub generation: u64,
    pub task_id: String,
    pub agent: String,
    pub project_path: String,
    pub prompt: String,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent_preset: Option<String>,
    pub prompt_mode: Option<String>,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub permission_mode: Option<String>,
    pub images: Option<Vec<String>>,
    pub client_time_zone: Option<String>,
}

/// 宿主 `run_dsh_task_core` 的 core 侧对应物。调用序 verbatim：
/// reasoning 门 → lifecycle 锁 → 清账 → 新会话截断 → ensure → create_session
/// → 登记三件 → {cols → stream → models → select → permission → 空 prompt
/// 早退 → pending → echo → prompt → 斜杠命令早退} → 失败结算。
pub async fn run_dsh_task(
    ports: &dyn DshHostPorts,
    launch: DshLaunch,
    sink: OutputSink,
) -> Result<(), String> {
    let DshLaunch {
        generation,
        task_id,
        agent,
        project_path,
        prompt,
        session_id,
        workspace_id,
        agent_preset,
        prompt_mode,
        selected_model,
        reasoning_effort,
        permission_mode,
        images,
        client_time_zone,
    } = launch;
    // 只有内置官方 dsh 配置带 reasoning 元数据的模型目录;提供方 / 自定义提供方
    // 档案只做模型选择,即使前端回传了强度也一律丢弃。
    let reasoning_effort = if agent == "dsh" {
        reasoning_effort
    } else {
        None
    };
    let task_lifecycle_lock = ports.lifecycle_lock(&task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    ports.clear_prerun_state(&task_id);
    // 全新会话才清空终端历史(对齐 `pty::run_task`);带 session_id 进来的是恢复,
    // 历史要留着,否则兜底终端会把之前那一段丢掉。
    if session_id.is_none() {
        ports.truncate_history_for_new_session(&task_id);
    }
    let base_url = ports.ensure_webui(&agent).await?;
    let api = DshApiClient::new(base_url.clone())?;
    let (session_id, resolved_preset) = api
        .create_session(
            &project_path,
            session_id.as_deref(),
            workspace_id.as_deref(),
            agent_preset.as_deref(),
        )
        .await?;
    // The run core minted is now addressable by the host's own registry: every status
    // report this task produces derives its generation from `runs.resource_key`, and
    // without this registration it would fall back to UNMINTED and be dropped by core's
    // staleness guard as older than the generation `start()` just minted.
    ports.register_run(&task_id, generation);
    ports.register_active_session(&task_id, &session_id, &base_url, sink.clone());
    // 任务结束后活跃会话表会清空,归属另记一份,会话详情才找得回这个实例。
    ports.remember_session_host(&session_id, &base_url);
    let session_path = ports
        .session_path(&agent, &project_path, &session_id)
        .ok_or_else(|| "Could not resolve the DSH session log path".to_string())?;
    ports.register_session(&DshSessionRegistration {
        task_id: task_id.clone(),
        generation,
        session_id: session_id.clone(),
        session_path,
        resolved_preset: resolved_preset.clone(),
    });
    // 执行器分发带来的 core 代：dsh 的运行与 pty 的运行一样按 (task, generation) 登记，
    // ADR-025 的寻址与 ADR-034 的存活判定都读同一张账。
    let result: Result<(), String> = async {
        ports.sync_terminal_cols(&task_id);
        ports
            .start_stream(DshStreamRequest {
                task_id: task_id.clone(),
                session_id: session_id.clone(),
                base_url: base_url.clone(),
                sink: sink.clone(),
            })
            .await?;
        let models = api.models(&session_id).await?;
        if selected_model.is_some() || reasoning_effort.is_some() {
            let current = &models.current;
            let model = selected_model.unwrap_or_else(|| current.model.clone());
            let provider = models
                .groups
                .iter()
                .find(|group| group.models.iter().any(|item| item.id == model))
                .map(|group| group.id.clone())
                .unwrap_or_else(|| current.provider.clone());
            let reasoning_effort =
                supported_dsh_reasoning_effort(&models, &model, reasoning_effort);
            api.select_model(
                &session_id,
                &DshModelSelection {
                    provider,
                    model,
                    reasoning_effort,
                },
            )
            .await?;
        }
        if let Some(mode) = permission_mode.as_deref() {
            let preset = dsh_permission_preset(mode)?;
            // 权限预设是参数传递,不是用户输入:先登记,让 `/permission <preset>`
            // 和它的结果行不出现在终端最前面两行。
            ports.expect_internal_command(&task_id, "permission");
            let command = api
                .execute_command(&session_id, &format!("/permission {preset}"))
                .await?;
            if command.is_null() {
                ports.clear_internal_command(&task_id);
                return Err(
                    "DSH permission command is unavailable in this agent preset".to_string()
                );
            }
        }
        if prompt.trim().is_empty() {
            // "Start terminal" with no prompt: the session is live and waiting
            // for the composer, which is an interactive state, not a finished
            // task. Completion stays an explicit user action.
            ports.report_status(&task_id, "input_required", None);
            return Ok(());
        }
        ports.report_status(&task_id, "pending", None);
        let image_count = images.as_ref().map_or(0, Vec::len);
        if let Some(echo) = user_prompt_echo(&prompt, image_count) {
            ports.send_terminal(&task_id, &sink, &echo);
        }
        let admitted = api
            .prompt(
                &session_id,
                &prompt,
                prompt_mode.as_deref().unwrap_or("queue"),
                images,
                client_time_zone.as_deref(),
            )
            .await?;
        if let Some(text) = admitted
            .get("command")
            .and_then(|command| command.get("text"))
            .and_then(Value::as_str)
        {
            ports.send_terminal(&task_id, &sink, &format!("\r\n{text}\r\n"));
            // A slash command answers inline without opening a turn, so no
            // turn/end will follow: settle the task as waiting for the user.
            ports.report_status(&task_id, "input_required", None);
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            ports.retire_runs(&task_id);
            ports.report_status(&task_id, "failed", Some(error.as_str()));
            Err(error)
        }
    }
}
