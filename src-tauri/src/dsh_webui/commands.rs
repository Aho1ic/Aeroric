//! DSH Web 的命令壳。
//!
//! 这一批从 `dsh_webui.rs` 整块搬出来,内容一行没改。它们的共同形状是
//! 「取 HTTP client → 转发一次 → 把结果原样返回」,没有自己的状态机;
//! 真正的逻辑在 `DshApiClient` 的两个 impl 块和 `startup` / `event_stream` 里。
//!
//! 留在父模块的是**带自己逻辑的**那几个:会话流(`run_dsh_task` 等)、
//! 进程生命周期(`start/stop/get_status`)、host 事件订阅、日志导出。

use super::*;

pub(super) async fn get_dsh_api(state: &DshWebUiManager) -> Result<DshApiClient, String> {
    let web = ensure_dsh_webui("dsh", state).await?;
    DshApiClient::new(
        web.url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?,
    )
}

pub(super) async fn get_dsh_api_for_session(
    state: &DshWebUiManager,
    session_id: &str,
) -> Result<DshApiClient, String> {
    if let Some(url) = state.known_session_host(session_id) {
        return DshApiClient::new(url);
    }
    // 每个 dsh 族配置有独立的 DSH_HOME 与端口,会话只存在于持有它的那个实例里。
    // 任务结束(或应用重启)后活跃会话表已经没有这条记录,必须先按磁盘反查出
    // 归属的 agent,再确保它的实例在跑,否则会打到内置实例上拿到 not found。
    //
    // 反查要遍历每个 dsh home 下的每个 project 目录,是同步文件系统操作,
    // 交给阻塞线程池,避免占住 tokio 工作线程。归属会缓存,这条路径每个会话
    // 只走一次。
    let owner = {
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || {
            crate::session_dsh::dsh_agent_owning_session(&session_id)
        })
        .await
        .map_err(|e| format!("Could not resolve the DSH session owner: {e}"))?
    };
    if let Some(agent) = owner {
        let web = ensure_dsh_webui(&agent, state).await?;
        let url = web
            .url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?;
        state.remember_session_host(session_id, &url);
        return DshApiClient::new(url);
    }
    // 磁盘上还没有这个会话的落盘目录(例如刚创建、尚未写入),退回内置实例。
    get_dsh_api(state).await
}

#[tauri::command]
pub fn get_dsh_protocol_capabilities() -> DshProtocolCapabilities {
    DshProtocolCapabilities::snapshot()
}

/// Escape hatch for the generated Typert Remote registry.  Named wrappers are
/// used by the first-party UI, while this command keeps every Remote mounted by
/// the audited Harness Web composition reachable without duplicating its
/// generated request/response types in Rust.
#[tauri::command]
pub async fn invoke_dsh_remote(
    state: State<'_, DshWebUiManager>,
    service: String,
    method: String,
    args: Value,
    session_id: Option<String>,
) -> Result<Value, String> {
    let capability = format!("{service}.{method}");
    if !DshProtocolCapabilities::snapshot()
        .remote_methods
        .contains(&capability.as_str())
    {
        return Err(format!("Unsupported DSH Remote method: {capability}"));
    }
    let client = match session_id.as_deref() {
        Some(session_id) if !session_id.trim().is_empty() => {
            get_dsh_api_for_session(&state, session_id).await?
        }
        _ => get_dsh_api(&state).await?,
    };
    client
        .remote_call(&format!("{service}/{method}"), args)
        .await
}

#[tauri::command]
pub async fn list_dsh_commands(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .list_commands(&session_id)
        .await
}

#[tauri::command]
pub async fn execute_dsh_command(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    line: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .execute_command(&session_id, &line)
        .await
}

#[tauri::command]
pub async fn list_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .list_message_feedback(&session_id)
        .await
}

#[tauri::command]
pub async fn put_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    message_id: String,
    rating: String,
    note: Option<String>,
    if_version: Option<String>,
) -> Result<Value, String> {
    if !matches!(rating.as_str(), "positive" | "negative") {
        return Err("DSH message feedback rating must be positive or negative".to_string());
    }
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .put_message_feedback(
            &session_id,
            &message_id,
            &rating,
            note.as_deref(),
            if_version.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn delete_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    message_id: String,
    if_version: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .delete_message_feedback(&session_id, &message_id, &if_version)
        .await
}

// ── Session ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_sessions(
    state: State<'_, DshWebUiManager>,
) -> Result<Vec<DshSessionSummary>, String> {
    get_dsh_api(&state).await?.list_sessions().await
}

#[tauri::command]
pub async fn get_dsh_session_history(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    before_seq: Option<u64>,
    max_messages: Option<u32>,
) -> Result<DshSessionHistory, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .session_history(&session_id, before_seq, max_messages)
        .await
}

#[tauri::command]
pub async fn rename_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    title: String,
) -> Result<(String, u64), String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .rename_session(&session_id, &title)
        .await
}

#[tauri::command]
pub async fn fork_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    at_seq: Option<u64>,
) -> Result<String, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .fork_session(&session_id, at_seq)
        .await
}

#[tauri::command]
pub async fn search_dsh_sessions(
    state: State<'_, DshWebUiManager>,
    query: String,
) -> Result<Value, String> {
    let (items, has_more) = get_dsh_api(&state).await?.search_sessions(&query).await?;
    Ok(json!({ "items": items, "hasMore": has_more }))
}

#[tauri::command]
pub async fn update_dsh_session_queue(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    item_id: String,
    action: Value,
) -> Result<(), String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .update_session_queue(&session_id, &item_id, action)
        .await
}

// ── Workspace ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_workspaces(
    state: State<'_, DshWebUiManager>,
) -> Result<DshWorkspaceList, String> {
    get_dsh_api(&state).await?.list_workspaces().await
}

#[tauri::command]
pub async fn create_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    path: String,
) -> Result<Value, String> {
    let (workspace, created) = get_dsh_api(&state).await?.create_workspace(&path).await?;
    Ok(json!({ "workspace": workspace, "created": created }))
}

#[tauri::command]
pub async fn rename_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    title: String,
) -> Result<DshWorkspace, String> {
    get_dsh_api(&state)
        .await?
        .rename_workspace(&workspace_id, &title)
        .await
}

#[tauri::command]
pub async fn delete_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .delete_workspace(&workspace_id)
        .await
}

#[tauri::command]
pub async fn reorder_dsh_workspaces(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    before_workspace_id: Option<String>,
) -> Result<Vec<String>, String> {
    get_dsh_api(&state)
        .await?
        .workspace_insert_before(&workspace_id, before_workspace_id.as_deref())
        .await
}

#[tauri::command]
pub async fn move_dsh_session_in_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    session_id: String,
    before_session_id: Option<String>,
) -> Result<DshWorkspace, String> {
    get_dsh_api(&state)
        .await?
        .workspace_insert_session_before(&workspace_id, &session_id, before_session_id.as_deref())
        .await
}

#[tauri::command]
pub async fn archive_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<String>, String> {
    get_dsh_api(&state)
        .await?
        .workspace_archive_session(&session_id)
        .await
}

// ── Credentials ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn describe_dsh_credentials(
    state: State<'_, DshWebUiManager>,
    refs: Vec<String>,
) -> Result<HashMap<String, DshCredentialView>, String> {
    get_dsh_api(&state).await?.describe_credentials(&refs).await
}

#[tauri::command]
pub async fn set_dsh_credential(
    state: State<'_, DshWebUiManager>,
    ref_: String,
    value: String,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .set_credential(&ref_, &value)
        .await
}

#[tauri::command]
pub async fn unset_dsh_credential(
    state: State<'_, DshWebUiManager>,
    ref_: String,
) -> Result<(), String> {
    get_dsh_api(&state).await?.unset_credential(&ref_).await
}

// ── LLM ───────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_llm_providers(
    state: State<'_, DshWebUiManager>,
) -> Result<Vec<DshProviderInfo>, String> {
    get_dsh_api(&state).await?.list_llm_providers().await
}

#[tauri::command]
pub async fn list_dsh_llm_models(
    state: State<'_, DshWebUiManager>,
) -> Result<DshGlobalModels, String> {
    get_dsh_api(&state).await?.list_llm_models().await
}

#[tauri::command]
pub async fn discover_dsh_llm_models(
    state: State<'_, DshWebUiManager>,
    settings_ns: String,
    provider: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key: Option<String>,
) -> Result<Vec<DshDiscoveredModel>, String> {
    get_dsh_api(&state)
        .await?
        .discover_llm_models(
            &settings_ns,
            provider.as_deref(),
            base_url.as_deref(),
            api.as_deref(),
            api_key.as_deref(),
        )
        .await
}

// ── Subagent ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_subagents(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<DshSubagentSummary>, String> {
    get_dsh_api(&state).await?.list_subagents(&session_id).await
}

#[tauri::command]
pub async fn get_dsh_subagent_history(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
    before_seq: Option<u64>,
    max_messages: Option<u32>,
) -> Result<DshSessionHistory, String> {
    get_dsh_api(&state)
        .await?
        .subagent_history(
            &session_id,
            parent_session_id.as_deref(),
            mode.as_deref(),
            before_seq,
            max_messages,
        )
        .await
}

#[tauri::command]
pub async fn prompt_dsh_subagent(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    content: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
    client_time_zone: Option<String>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .subagent_prompt(
            &session_id,
            parent_session_id.as_deref(),
            mode.as_deref(),
            &content,
            client_time_zone.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn interrupt_dsh_subagent(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .interrupt_subagent(&session_id, parent_session_id.as_deref(), mode.as_deref())
        .await
}

// ── Goals ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    title: String,
    max_goal_rounds: Option<u32>,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .create_goal(&session_id, &title, max_goal_rounds)
        .await
}

#[tauri::command]
pub async fn edit_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
    title: Option<String>,
    max_goal_rounds: Option<u32>,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .edit_goal(
            &session_id,
            &goal_id,
            revision,
            title.as_deref(),
            max_goal_rounds,
        )
        .await
}

#[tauri::command]
pub async fn pause_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .pause_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn resume_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .resume_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn complete_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .complete_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn clear_dsh_goals(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .clear_goals(&session_id, &goal_id, revision)
        .await
}

// ── Skills ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_skills(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<DshSkillEntry>, String> {
    get_dsh_api(&state).await?.list_skills(&session_id).await
}

// ── Host ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn describe_dsh_host(state: State<'_, DshWebUiManager>) -> Result<DshHostInfo, String> {
    get_dsh_api(&state).await?.describe_host().await
}

#[tauri::command]
pub async fn list_dsh_host_directory(
    state: State<'_, DshWebUiManager>,
    path: Option<String>,
) -> Result<DshDirectoryListing, String> {
    get_dsh_api(&state)
        .await?
        .host_list_directory(path.as_deref())
        .await
}

#[tauri::command]
pub async fn create_dsh_host_directory(
    state: State<'_, DshWebUiManager>,
    path: String,
    name: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .host_create_directory(&path, &name)
        .await
}

#[tauri::command]
pub async fn open_dsh_host_path(
    state: State<'_, DshWebUiManager>,
    path: String,
) -> Result<Value, String> {
    get_dsh_api(&state).await?.host_open_path(&path).await
}

// ── AgentPreset extended ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn select_dsh_session_preset(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    agent_preset: String,
) -> Result<String, String> {
    get_dsh_api(&state)
        .await?
        .select_preset(&session_id, &agent_preset)
        .await
}

#[tauri::command]
pub async fn read_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<DshPresetReadResult, String> {
    get_dsh_api(&state).await?.read_preset(&preset).await
}

#[tauri::command]
pub async fn copy_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    from: String,
    target_preset: String,
    name: Option<String>,
) -> Result<DshPresetInfo, String> {
    get_dsh_api(&state)
        .await?
        .copy_preset(&from, &target_preset, name.as_deref())
        .await
}

#[tauri::command]
pub async fn open_dsh_agent_preset_document(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .open_preset_document(&preset)
        .await
}

#[tauri::command]
pub async fn remove_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<(), String> {
    get_dsh_api(&state).await?.remove_preset(&preset).await
}

// ── Approval / Question responses ────────────────────────────────────────────

/// Reply to a DSH `approval/requested` or `question/requested` server-request
/// using POST /api/respond (client-response, echoes the rpcId from the frame).
#[tauri::command]
pub async fn respond_dsh_server_request(
    state: State<'_, DshWebUiManager>,
    rpc_id: String,
    session_id: Option<String>,
    result: Value,
) -> Result<(), String> {
    if result.get("ok").and_then(Value::as_bool).is_none() {
        return Err("DSH response result must be a full RPC result".to_string());
    }
    let session_id = session_id.or_else(|| {
        result
            .get("value")
            .and_then(|value| value.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });

    let api = match session_id {
        Some(session_id) => get_dsh_api_for_session(&state, &session_id).await?,
        None => {
            let base_url = state
                .active_sessions
                .lock()
                .values()
                .next()
                .map(|active| active.base_url.clone());
            match base_url {
                Some(url) => DshApiClient::new(url)?,
                None => get_dsh_api(&state).await?,
            }
        }
    };
    let message = json!({
        "type": "client-response",
        "rpcId": rpc_id,
        "result": result,
    });
    let response = api
        .client
        .post(format!("{}/api/respond", api.base_url))
        .header("content-type", "application/json")
        .json(&message)
        .send()
        .await
        .map_err(|e| format!("DSH respond request failed: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "DSH respond returned HTTP {status}: {}",
            text.trim()
        ));
    }
    let receipt: Value = response
        .json()
        .await
        .map_err(|error| format!("DSH respond receipt was invalid JSON: {error}"))?;
    if receipt.get("accepted").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        let reason = receipt
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        Err(format!("DSH rejected the response: {reason}"))
    }
}

// ── Session attachment ────────────────────────────────────────────────────────

/// Read an image attachment from a DSH session (session.attachment).
/// Returns `{ mediaType: string, data: string }` (base64-encoded data).
#[tauri::command]
pub async fn get_dsh_session_attachment(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    attachment_id: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .call(
            "session.attachment",
            json!({ "sessionId": session_id, "attachmentId": attachment_id }),
        )
        .await
}

// ── Settings extended ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn open_dsh_settings_document(
    state: State<'_, DshWebUiManager>,
) -> Result<Value, String> {
    get_dsh_api(&state).await?.open_settings_document().await
}

#[tauri::command]
pub async fn replace_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    value: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .replace_settings(&ns, value, expected_revision)
        .await
}

#[tauri::command]
pub async fn mutate_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    ops: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .mutate_settings(&ns, ops, expected_revision)
        .await
}

#[tauri::command]
pub async fn update_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    patch: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .update_settings(&ns, patch, expected_revision)
        .await
}

// ── events.host downlink subscription ────────────────────────────────────────
