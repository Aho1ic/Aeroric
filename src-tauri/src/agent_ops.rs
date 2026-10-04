//! Agent 安装/升级操作的**宿主接线壳**（P8-02 刀六）。
//!
//! 纯状态半边（类型、注册表、`begin`/`finish`/进度写入、取消标记）已上收
//! core `operations`——谁在跑、跑到哪一步、上次结果是什么只有 core 一份。
//! 这里剩三件事:
//!
//! - `#[tauri::command]` 壳（`start`/`get`/`cancel`）与任务分派;
//! - emit 包装:**双发**——旧桌面通道 `agent-operation-changed`(React 客户端
//!   监听,字面量发射:事件扫描器按 src-tauri 内的字面值找发射点)＋ v1 topic
//!   `agent.operation_changed`(常量在 core `operations`,扫描器记作 dynamic
//!   发射点——P7-06 sync daemon 先例)。payload 两侧逐字同一份快照;
//! - agent id → 二进制键归并与"该装还是该升"判定(宿主设置面私有)。
//!
//! 旧通道照发的兼容语义不变:先写注册表(core)再 emit,晚到的
//! `get_agent_operations` 不会读到比事件更旧的状态;`begin` 仍不广播,操作
//! 启动经命令返回值直达前端。

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter};

use crate::agent_tools::AgentInstallResult;
use aeroric_core::operations::types::{AgentInstallErrorCode, AgentInstallStage};
use aeroric_core::operations::{
    self, snapshot_for, snapshots, AgentOperationSnapshot, AgentOperationState, BeginOutcome,
};
use aeroric_core::settings::schema::{AgentUpgradeChannel, AgentUpgradeResult};

/// 安装/升级类型随刀六上收 core;`agent_tools` 仍经 `crate::agent_ops::` 引用。
pub use aeroric_core::operations::AgentOperationKind;

/// 供安装/升级实现回报阶段进度。
pub(crate) fn report_progress(
    app: &AppHandle,
    agent: &str,
    operation_id: &str,
    stage: AgentInstallStage,
    progress: u8,
    message: &str,
) {
    if let Some(snapshot) =
        operations::report_progress_snapshot(agent, operation_id, stage, progress, message)
    {
        emit_operation_snapshot(app, &snapshot);
    }
}

/// 把终态写进快照(core)并广播。单独拆出来是为了能不带 `AppHandle` 测——
/// 纯判定半边的测试在 core `operations::tests`,这里只留接线。
///
/// `upgrade_result` 在接线边界换型:宿主 `app_settings::schema` 与 core
/// `settings::schema` 的 `AgentUpgradeResult`/`AgentUpgradeChannel` 是刀二
/// 上收时两侧共存的同形副本(serde 形状逐字一致),core 只认自己那份;
/// `runtime_recovery` 两侧本就同源(core `dsh_host::state`,宿主 re-export)。
fn finish(
    app: &AppHandle,
    agent: &str,
    operation_id: &str,
    state: AgentOperationState,
    stage: AgentInstallStage,
    message: String,
    error_code: Option<AgentInstallErrorCode>,
    install_result: Option<AgentInstallResult>,
    upgrade_result: Option<crate::app_settings::AgentUpgradeResult>,
) {
    let upgrade_result = upgrade_result.map(|result| AgentUpgradeResult {
        agent: result.agent,
        success: result.success,
        previous_version: result.previous_version,
        current_version: result.current_version,
        message: result.message,
        channels: result
            .channels
            .into_iter()
            .map(|channel| AgentUpgradeChannel {
                channel: channel.channel,
                success: channel.success,
                message: channel.message,
            })
            .collect(),
        channel: result.channel,
        managed: result.managed,
        runtime_recovery: result.runtime_recovery,
    });
    if let Some(snapshot) = operations::finish_snapshot(
        agent,
        operation_id,
        state,
        stage,
        message,
        error_code,
        install_result,
        upgrade_result,
    ) {
        emit_operation_snapshot(app, &snapshot);
    }
}

/// 快照广播(双发)。调用方保证"先写注册表再 emit"的顺序:core 的
/// `report_progress_snapshot`/`finish_snapshot` 在返回快照前已完成写入。
fn emit_operation_snapshot(app: &AppHandle, snapshot: &AgentOperationSnapshot) {
    let _ = app.emit("agent-operation-changed", snapshot);
    let _ = app.emit(operations::AGENT_OPERATION_TOPIC, snapshot);
}

/// 把 agent id 归并到负责升级的二进制 agent 键。自定义 dsh Agent 与内置 dsh
/// 共用同一个二进制,所以也共用同一条操作状态。
fn binary_agent_for(agent: &str) -> Option<&'static str> {
    crate::app_settings::upgrade_binary_agent_for(agent)
}

/// 这个 agent id(自定义 Agent 也行)对应的二进制上有没有安装/升级在跑。
///
/// DSH Web 用它区分「lifecycle 锁被升级攥着」和「另一次 start 正在启动」:
/// 前者要几分钟,得直接告诉用户;后者是毫秒到几十秒,等着就好。
pub(crate) fn binary_operation_is_running(agent: &str) -> bool {
    operations::binary_operation_is_running(agent, binary_agent_for)
}

/// 决定这次该走安装还是升级。前端不再需要自己做这个判断。
fn kind_for(binary_agent: &str) -> AgentOperationKind {
    if crate::agent_tools::agent_is_installed(binary_agent) {
        AgentOperationKind::Upgrade
    } else {
        AgentOperationKind::Install
    }
}

/// 启动一次安装/升级。**幂等**:已在跑就返回现有快照。任务在 detached task 里
/// 执行,所以前端退出设置页、丢掉 promise 都不会中断它。
#[tauri::command]
pub async fn start_agent_operation(
    app: AppHandle,
    agent: String,
    expected_version: Option<String>,
) -> Result<AgentOperationSnapshot, String> {
    let requested = agent.trim().to_string();
    let Some(binary_agent) = binary_agent_for(&requested) else {
        return Err(format!("invalid_agent: unknown agent {requested}"));
    };
    let kind = kind_for(binary_agent);
    let (snapshot, cancelled) = match operations::begin(binary_agent, &requested, kind) {
        BeginOutcome::Started(snapshot, cancelled) => (snapshot, cancelled),
        // 已经在跑:直接回现有快照,不起第二次。
        BeginOutcome::AlreadyRunning(existing) => return Ok(existing),
    };

    let operation_id = snapshot.operation_id.clone();
    let expected_version = expected_version
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty());
    let task_app = app.clone();
    let task_agent = binary_agent.to_string();
    let task_requested = requested.clone();
    tokio::spawn(async move {
        let outcome = crate::agent_tools::run_agent_operation(
            &task_app,
            &operation_id,
            &task_agent,
            &task_requested,
            kind,
            expected_version.as_deref(),
            &cancelled,
        )
        .await;
        match outcome {
            OperationOutcome::Install(result) => {
                let success = result.success;
                let cancelled_run = result.stage == AgentInstallStage::Cancelled;
                let state = if success {
                    AgentOperationState::Succeeded
                } else if cancelled_run {
                    AgentOperationState::Cancelled
                } else {
                    AgentOperationState::Failed
                };
                let stage = result.stage.clone();
                let message = result.message.clone();
                let error_code = result.error_code.clone();
                finish(
                    &task_app,
                    &task_agent,
                    &operation_id,
                    state,
                    stage,
                    message,
                    error_code,
                    Some(result),
                    None,
                );
            }
            OperationOutcome::Upgrade(result) => {
                let state = if result.success {
                    AgentOperationState::Succeeded
                } else if operations::is_cancelled(&task_agent) {
                    AgentOperationState::Cancelled
                } else {
                    AgentOperationState::Failed
                };
                let stage = if result.success {
                    AgentInstallStage::Completed
                } else if state == AgentOperationState::Cancelled {
                    AgentInstallStage::Cancelled
                } else {
                    AgentInstallStage::Failed
                };
                let message = result.message.clone();
                finish(
                    &task_app,
                    &task_agent,
                    &operation_id,
                    state,
                    stage,
                    message,
                    None,
                    None,
                    Some(result),
                );
            }
            OperationOutcome::Error { code, message } => finish(
                &task_app,
                &task_agent,
                &operation_id,
                if code == AgentInstallErrorCode::Cancelled {
                    AgentOperationState::Cancelled
                } else {
                    AgentOperationState::Failed
                },
                if code == AgentInstallErrorCode::Cancelled {
                    AgentInstallStage::Cancelled
                } else {
                    AgentInstallStage::Failed
                },
                message,
                Some(code),
                None,
                None,
            ),
        }
    });

    Ok(snapshot)
}

/// 安装/升级实现的统一返回。`Upgrade` 携带宿主 schema 的结果类型
/// （`agent_tools` 的构造点写的是 `crate::app_settings::`），core 换型在
/// `finish` 的接线边界一次完成。
pub(crate) enum OperationOutcome {
    Install(AgentInstallResult),
    Upgrade(crate::app_settings::AgentUpgradeResult),
    Error {
        code: AgentInstallErrorCode,
        message: String,
    },
}

#[tauri::command]
pub async fn get_agent_operations() -> Result<Vec<AgentOperationSnapshot>, String> {
    Ok(snapshots())
}

#[tauri::command]
pub async fn cancel_agent_operation(agent: String) -> Result<(), String> {
    let requested = agent.trim();
    let Some(binary_agent) = binary_agent_for(requested) else {
        return Err(format!("invalid_agent: unknown agent {requested}"));
    };
    if let Some(flag) = operations::cancellation_flag(binary_agent) {
        flag.store(true, Ordering::Relaxed);
    }
    // install 管线自己还有一份按 operation_id 的取消表,一并触发。
    if let Some(operation_id) = snapshot_for(binary_agent)
        .filter(|snapshot| snapshot.state == AgentOperationState::Running)
        .map(|snapshot| snapshot.operation_id)
    {
        let _ = crate::agent_tools::cancel_agent_tool_install(operation_id);
    }
    Ok(())
}
