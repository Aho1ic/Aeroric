//! Agent 安装/升级操作的**后端持有状态**。
//!
//! 之前忙碌态只活在 `AgentUpdatesPanel` 的组件 state 里:退出设置页即清空,而
//! Rust 侧任务还在跑;重新进入时按钮恢复成"一键升级",再点一次就真的起了第二
//! 次升级(install 有按 agent 的 `OperationGuard` 挡,但 upgrade 只有一把全局
//! 串行锁,第二次不会被拒,只会排队后重复执行)。
//!
//! 这里把"谁在跑、跑到哪一步、上次结果是什么"收敛到后端唯一一份注册表:
//!
//! - `start_agent_operation` **幂等**:同一 agent 已在跑就返回现有快照,不起第二个任务;
//! - 任务在 detached tokio task 里跑,前端丢掉 promise 也不影响;
//! - 每次状态变化先写注册表再 emit,所以晚挂载的前端可以先 `get_agent_operations` 对账;
//! - 终态快照保留到该 agent 下次操作开始,重进设置页仍能看到上次结果。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::agent_tools::{AgentInstallErrorCode, AgentInstallResult, AgentInstallStage};
use crate::app_settings::AgentUpgradeResult;

/// 带完整快照的操作变更事件。install 进度事件仍然照发,不破坏既有监听方。
pub const AGENT_OPERATION_EVENT: &str = "agent-operation-changed";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentOperationKind {
    Install,
    Upgrade,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentOperationState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentOperationSnapshot {
    pub operation_id: String,
    /// 二进制 agent 键(claude / codex / dsh)。自定义 Agent 会归并到它的二进制。
    pub agent: String,
    /// 触发这次操作的 agent id(自定义 Agent 时与 `agent` 不同)。
    pub requested_agent: String,
    pub kind: AgentOperationKind,
    pub state: AgentOperationState,
    pub stage: AgentInstallStage,
    pub progress: u8,
    pub message: String,
    pub error_code: Option<AgentInstallErrorCode>,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_result: Option<AgentInstallResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_result: Option<AgentUpgradeResult>,
}

struct Registry {
    /// 按二进制 agent 键存当前或最近一次操作。
    operations: HashMap<String, AgentOperationSnapshot>,
    cancellations: HashMap<String, Arc<AtomicBool>>,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            operations: HashMap::new(),
            cancellations: HashMap::new(),
        })
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

/// 已登记的全部快照(当前 + 最近一次)。前端挂载时用它对账。
pub(crate) fn snapshots() -> Vec<AgentOperationSnapshot> {
    let mut rows: Vec<AgentOperationSnapshot> =
        registry().lock().operations.values().cloned().collect();
    rows.sort_by(|left, right| left.agent.cmp(&right.agent));
    rows
}

pub(crate) fn snapshot_for(agent: &str) -> Option<AgentOperationSnapshot> {
    registry().lock().operations.get(agent).cloned()
}

/// 这个 agent id(自定义 Agent 也行)对应的二进制上有没有安装/升级在跑。
///
/// DSH Web 用它区分「lifecycle 锁被升级攥着」和「另一次 start 正在启动」:
/// 前者要几分钟,得直接告诉用户;后者是毫秒到几十秒,等着就好。
pub(crate) fn binary_operation_is_running(agent: &str) -> bool {
    binary_agent_for(agent).is_some_and(is_running)
}

fn is_running(agent: &str) -> bool {
    registry()
        .lock()
        .operations
        .get(agent)
        .is_some_and(|snapshot| snapshot.state == AgentOperationState::Running)
}

/// `begin` 的结果。`AlreadyRunning` 不是错误:调用方把现有快照原样回给前端,
/// 于是重复点击只会看到同一次操作 —— 这就是重复升级的根治点。
enum BeginOutcome {
    Started(AgentOperationSnapshot, Arc<AtomicBool>),
    AlreadyRunning(AgentOperationSnapshot),
}

#[cfg(test)]
impl BeginOutcome {
    fn started(self) -> (AgentOperationSnapshot, Arc<AtomicBool>) {
        match self {
            BeginOutcome::Started(snapshot, cancelled) => (snapshot, cancelled),
            BeginOutcome::AlreadyRunning(snapshot) => {
                panic!(
                    "expected a new operation, got the running {}",
                    snapshot.operation_id
                )
            }
        }
    }

    fn already_running(self) -> AgentOperationSnapshot {
        match self {
            BeginOutcome::AlreadyRunning(snapshot) => snapshot,
            BeginOutcome::Started(snapshot, _) => {
                panic!(
                    "expected the running operation, started {}",
                    snapshot.operation_id
                )
            }
        }
    }
}

/// 登记一次新操作。同一 agent 已有 running 快照时不起第二次。
fn begin(agent: &str, requested_agent: &str, kind: AgentOperationKind) -> BeginOutcome {
    let mut guard = registry().lock();
    if let Some(existing) = guard
        .operations
        .get(agent)
        .filter(|snapshot| snapshot.state == AgentOperationState::Running)
    {
        return BeginOutcome::AlreadyRunning(existing.clone());
    }
    let snapshot = AgentOperationSnapshot {
        operation_id: uuid::Uuid::new_v4().to_string(),
        agent: agent.to_string(),
        requested_agent: requested_agent.to_string(),
        kind,
        state: AgentOperationState::Running,
        stage: AgentInstallStage::Detecting,
        progress: 0,
        message: String::new(),
        error_code: None,
        started_at_ms: now_ms(),
        finished_at_ms: None,
        install_result: None,
        upgrade_result: None,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    guard
        .cancellations
        .insert(agent.to_string(), cancelled.clone());
    guard.operations.insert(agent.to_string(), snapshot.clone());
    BeginOutcome::Started(snapshot, cancelled)
}

/// 就地更新快照并广播。先写注册表再 emit,保证晚到的 `get_agent_operations`
/// 不会读到比事件更旧的状态。
fn update(app: &AppHandle, agent: &str, mutate: impl FnOnce(&mut AgentOperationSnapshot)) {
    let snapshot = {
        let mut guard = registry().lock();
        let Some(snapshot) = guard.operations.get_mut(agent) else {
            return;
        };
        mutate(snapshot);
        snapshot.clone()
    };
    let _ = app.emit(AGENT_OPERATION_EVENT, &snapshot);
}

/// 供安装/升级实现回报阶段进度。
pub(crate) fn report_progress(
    app: &AppHandle,
    agent: &str,
    operation_id: &str,
    stage: AgentInstallStage,
    progress: u8,
    message: &str,
) {
    update(app, agent, |snapshot| {
        // 迟到的旧操作事件不能覆盖新操作的状态。
        if snapshot.operation_id != operation_id {
            return;
        }
        snapshot.stage = stage;
        snapshot.progress = progress;
        if !message.is_empty() {
            snapshot.message = message.to_string();
        }
    });
}

/// 把终态写进快照。单独拆出来是为了能不带 `AppHandle` 测。
fn apply_finish(
    snapshot: &mut AgentOperationSnapshot,
    state: AgentOperationState,
    stage: AgentInstallStage,
    message: String,
    error_code: Option<AgentInstallErrorCode>,
    install_result: Option<AgentInstallResult>,
    upgrade_result: Option<AgentUpgradeResult>,
) {
    snapshot.state = state;
    snapshot.stage = stage;
    // 只有成功才补到 100%。失败/取消停在哪就是哪 —— 进度条冲到满格再标红,
    // 会让人以为「装完了但校验没过」,而实际上多半是半路断的。
    if matches!(state, AgentOperationState::Succeeded) {
        snapshot.progress = 100;
    }
    // 空消息不许覆盖:进度阶段一路写进来的最后一句往往是唯一的线索,
    // 收尾时若无话可说,保留它比清成空白有用。
    if !message.is_empty() {
        snapshot.message = message;
    }
    snapshot.error_code = error_code;
    snapshot.finished_at_ms = Some(now_ms());
    snapshot.install_result = install_result;
    snapshot.upgrade_result = upgrade_result;
}

fn finish(
    app: &AppHandle,
    agent: &str,
    operation_id: &str,
    state: AgentOperationState,
    stage: AgentInstallStage,
    message: String,
    error_code: Option<AgentInstallErrorCode>,
    install_result: Option<AgentInstallResult>,
    upgrade_result: Option<AgentUpgradeResult>,
) {
    {
        let mut guard = registry().lock();
        if guard
            .operations
            .get(agent)
            .is_some_and(|snapshot| snapshot.operation_id == operation_id)
        {
            guard.cancellations.remove(agent);
        }
    }
    update(app, agent, |snapshot| {
        if snapshot.operation_id != operation_id {
            return;
        }
        apply_finish(
            snapshot,
            state,
            stage,
            message,
            error_code,
            install_result,
            upgrade_result,
        );
    });
}

pub(crate) fn cancellation_flag(agent: &str) -> Option<Arc<AtomicBool>> {
    registry().lock().cancellations.get(agent).cloned()
}

/// 该 agent 是否被请求取消。安装/升级实现在长任务里轮询它。
pub(crate) fn is_cancelled(agent: &str) -> bool {
    cancellation_flag(agent).is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// 把 agent id 归并到负责升级的二进制 agent 键。自定义 dsh Agent 与内置 dsh
/// 共用同一个二进制,所以也共用同一条操作状态。
fn binary_agent_for(agent: &str) -> Option<&'static str> {
    crate::app_settings::upgrade_binary_agent_for(agent)
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
    let (snapshot, cancelled) = match begin(binary_agent, &requested, kind) {
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
                } else if crate::agent_ops::is_cancelled(&task_agent) {
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

/// 安装/升级实现的统一返回。
pub(crate) enum OperationOutcome {
    Install(AgentInstallResult),
    Upgrade(AgentUpgradeResult),
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
    if let Some(flag) = cancellation_flag(binary_agent) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 注册表是进程级全局的,而 cargo 默认并行跑测试。给每个用例一个独占的
    /// agent 键,用例之间就天然互不干扰,不需要串行化,也不会互相清状态。
    fn unique_agent(label: &str) -> String {
        format!("test-{label}-{}", uuid::Uuid::new_v4())
    }

    fn forget(agent: &str) {
        let mut guard = registry().lock();
        guard.operations.remove(agent);
        guard.cancellations.remove(agent);
    }

    /// 重复点击"一键升级"必须只产生一次操作。
    #[test]
    fn a_second_begin_returns_the_running_snapshot_instead_of_starting_again() {
        let agent = unique_agent("duplicate");
        let (first, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        let existing = begin(&agent, &agent, AgentOperationKind::Upgrade).already_running();
        assert_eq!(existing.operation_id, first.operation_id);
        assert_eq!(existing.state, AgentOperationState::Running);
        forget(&agent);
    }

    /// 终态快照保留到下次操作开始,所以重进设置页还能看到上次结果。
    #[test]
    fn a_finished_snapshot_survives_until_the_next_operation_begins() {
        let agent = unique_agent("retained");
        let (snapshot, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        {
            let mut guard = registry().lock();
            let stored = guard.operations.get_mut(&agent).expect("snapshot exists");
            stored.state = AgentOperationState::Succeeded;
            stored.finished_at_ms = Some(now_ms());
        }
        let stored = snapshot_for(&agent).expect("the finished snapshot is retained");
        assert_eq!(stored.operation_id, snapshot.operation_id);
        assert_eq!(stored.state, AgentOperationState::Succeeded);
        // 终态不再阻塞下一次操作。
        let (next, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        assert_ne!(next.operation_id, snapshot.operation_id);
        forget(&agent);
    }

    #[test]
    fn running_state_is_tracked_per_agent() {
        let agent = unique_agent("running");
        let other = unique_agent("idle");
        let (_snapshot, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        assert!(is_running(&agent));
        assert!(!is_running(&other));
        forget(&agent);
    }

    #[test]
    fn cancelling_sets_the_flag_the_worker_polls() {
        let agent = unique_agent("cancel");
        let (_snapshot, flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        assert!(!is_cancelled(&agent));
        cancellation_flag(&agent)
            .expect("a running operation has a cancellation flag")
            .store(true, Ordering::Relaxed);
        assert!(is_cancelled(&agent));
        assert!(flag.load(Ordering::Relaxed));
        forget(&agent);
    }

    /// 完成时要清掉取消标记,否则下一次操作会立刻被当成"已取消"。
    #[test]
    fn finishing_clears_the_cancellation_flag() {
        let agent = unique_agent("finish");
        let (snapshot, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade).started();
        {
            let mut guard = registry().lock();
            if guard
                .operations
                .get(&agent)
                .is_some_and(|stored| stored.operation_id == snapshot.operation_id)
            {
                guard.cancellations.remove(&agent);
            }
        }
        assert!(cancellation_flag(&agent).is_none());
        assert!(!is_cancelled(&agent));
        forget(&agent);
    }

    fn snapshot_at(progress: u8, message: &str) -> AgentOperationSnapshot {
        let agent = unique_agent("apply");
        let (snapshot, _flag) = begin(&agent, &agent, AgentOperationKind::Install).started();
        forget(&agent);
        AgentOperationSnapshot {
            progress,
            message: message.to_string(),
            ..snapshot
        }
    }

    /// 半路失败/取消的进度不能被补成 100% —— 满格再标红像是"装完了没过校验",
    /// 实际多半是中途断的。
    #[test]
    fn a_failed_operation_keeps_the_progress_it_reached() {
        let mut snapshot = snapshot_at(35, "downloading");
        apply_finish(
            &mut snapshot,
            AgentOperationState::Failed,
            AgentInstallStage::Failed,
            "download failed".to_string(),
            Some(AgentInstallErrorCode::DownloadFailed),
            None,
            None,
        );
        assert_eq!(snapshot.progress, 35);
        assert_eq!(snapshot.message, "download failed");
        assert_eq!(snapshot.state, AgentOperationState::Failed);

        let mut cancelled = snapshot_at(60, "installing");
        apply_finish(
            &mut cancelled,
            AgentOperationState::Cancelled,
            AgentInstallStage::Cancelled,
            "cancelled".to_string(),
            Some(AgentInstallErrorCode::Cancelled),
            None,
            None,
        );
        assert_eq!(cancelled.progress, 60);
    }

    #[test]
    fn a_successful_operation_is_topped_up_to_one_hundred() {
        let mut snapshot = snapshot_at(90, "verifying");
        apply_finish(
            &mut snapshot,
            AgentOperationState::Succeeded,
            AgentInstallStage::Completed,
            "done".to_string(),
            None,
            None,
            None,
        );
        assert_eq!(snapshot.progress, 100);
        assert!(snapshot.finished_at_ms.is_some());
    }

    /// 收尾时没话说就别把最后一条进度消息清成空白 —— 那往往是唯一的线索。
    #[test]
    fn an_empty_finish_message_keeps_the_last_progress_message() {
        let mut snapshot = snapshot_at(40, "resolving the npm registry");
        apply_finish(
            &mut snapshot,
            AgentOperationState::Failed,
            AgentInstallStage::Failed,
            String::new(),
            Some(AgentInstallErrorCode::NetworkUnavailable),
            None,
            None,
        );
        assert_eq!(snapshot.message, "resolving the npm registry");
    }
}
