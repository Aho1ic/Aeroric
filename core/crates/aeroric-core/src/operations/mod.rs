//! Agent 安装/升级操作的**后端持有状态**（P8-02 自宿主 `agent_ops.rs` 纯状态半边上收）。
//!
//! 之前忙碌态只活在 `AgentUpdatesPanel` 的组件 state 里:退出设置页即清空,而
//! Rust 侧任务还在跑;重新进入时按钮恢复成"一键升级",再点一次就真的起了第二
//! 次升级(install 有按 agent 的 `OperationGuard` 挡,但 upgrade 只有一把全局
//! 串行锁,第二次不会被拒,只会排队后重复执行)。
//!
//! 这里把"谁在跑、跑到哪一步、上次结果是什么"收敛到后端唯一一份注册表:
//!
//! - `begin` **幂等**:同一 agent 已在跑就返回现有快照,不起第二个任务;
//! - 每次状态变化先写注册表再广播,所以晚挂载的前端可以先查询对账;
//! - 终态快照保留到该 agent 下次操作开始,重进设置页仍能看到上次结果。
//!
//! 分界：本模块只收**不依赖 Tauri/AppHandle** 的纯状态半边（类型、注册表、
//! `begin`/`apply_finish`/快照查询与取消标记）。emit 包装（`update`/`finish`/
//! `report_progress` 的 `app.emit` 面）、`#[tauri::command]` 与
//! `start_agent_operation` 的任务分派留宿主 `agent_ops.rs`（刀五接线后转薄壳）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::clock::now_ms_u64;
use crate::operations::types::{AgentInstallErrorCode, AgentInstallResult, AgentInstallStage};
use crate::settings::schema::AgentUpgradeResult;

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

mod types;

struct Registry {
    /// 按二进制 agent 键存当前或最近一次操作。
    operations: HashMap<String, AgentOperationSnapshot>,
    cancellations: HashMap<String, Arc<AtomicBool>>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        operations: HashMap::new(),
        cancellations: HashMap::new(),
    })
});

fn registry() -> &'static Mutex<Registry> {
    &REGISTRY
}

/// 已登记的全部快照(当前 + 最近一次)。前端挂载时用它对账。
pub fn snapshots() -> Vec<AgentOperationSnapshot> {
    let mut rows: Vec<AgentOperationSnapshot> =
        registry().lock().operations.values().cloned().collect();
    rows.sort_by(|left, right| left.agent.cmp(&right.agent));
    rows
}

pub fn snapshot_for(agent: &str) -> Option<AgentOperationSnapshot> {
    registry().lock().operations.get(agent).cloned()
}

/// 这个 agent id(自定义 Agent 也行)对应的二进制上有没有安装/升级在跑。
///
/// DSH Web 用它区分「lifecycle 锁被升级攥着」和「另一次 start 正在启动」:
/// 前者要几分钟,得直接告诉用户;后者是毫秒到几十秒,等着就好。
pub fn binary_operation_is_running(
    agent: &str,
    binary_agent_for: impl Fn(&str) -> Option<&'static str>,
) -> bool {
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
pub enum BeginOutcome {
    Started(AgentOperationSnapshot, Arc<AtomicBool>),
    AlreadyRunning(AgentOperationSnapshot),
}

/// 登记一次新操作。同一 agent 已有 running 快照时不起第二次。
pub fn begin(agent: &str, requested_agent: &str, kind: AgentOperationKind) -> BeginOutcome {
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
        started_at_ms: now_ms_u64(),
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

/// 把终态写进快照。不带任何通道依赖,emit 由宿主包装完成。
pub fn apply_finish(
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
    snapshot.finished_at_ms = Some(now_ms_u64());
    snapshot.install_result = install_result;
    snapshot.upgrade_result = upgrade_result;
}

/// 供安装/升级实现回报阶段进度(纯注册表半边:先写后广播,广播由宿主做)。
/// 返回是否写入了快照(宿主据此决定要不要 emit)。
pub fn report_progress_snapshot(
    agent: &str,
    operation_id: &str,
    stage: AgentInstallStage,
    progress: u8,
    message: &str,
) -> Option<AgentOperationSnapshot> {
    let mut guard = registry().lock();
    let snapshot = guard.operations.get_mut(agent)?;
    // 迟到的旧操作事件不能覆盖新操作的状态。
    if snapshot.operation_id != operation_id {
        return None;
    }
    snapshot.stage = stage;
    snapshot.progress = progress;
    if !message.is_empty() {
        snapshot.message = message.to_string();
    }
    Some(snapshot.clone())
}

/// 终态写进注册表并清掉取消标记。返回是否写入了快照(宿主据此 emit)。
pub fn finish_snapshot(
    agent: &str,
    operation_id: &str,
    state: AgentOperationState,
    stage: AgentInstallStage,
    message: String,
    error_code: Option<AgentInstallErrorCode>,
    install_result: Option<AgentInstallResult>,
    upgrade_result: Option<AgentUpgradeResult>,
) -> Option<AgentOperationSnapshot> {
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
    let mut guard = registry().lock();
    let snapshot = guard.operations.get_mut(agent)?;
    if snapshot.operation_id != operation_id {
        return None;
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
    Some(snapshot.clone())
}

pub fn cancellation_flag(agent: &str) -> Option<Arc<AtomicBool>> {
    registry().lock().cancellations.get(agent).cloned()
}

/// 该 agent 是否被请求取消。安装/升级实现在长任务里轮询它。
pub fn is_cancelled(agent: &str) -> bool {
    cancellation_flag(agent).is_some_and(|flag| flag.load(Ordering::Relaxed))
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
        let BeginOutcome::Started(first, _flag) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
        let BeginOutcome::AlreadyRunning(existing) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected the running operation")
        };
        assert_eq!(existing.operation_id, first.operation_id);
        assert_eq!(existing.state, AgentOperationState::Running);
        forget(&agent);
    }

    /// 终态快照保留到下次操作开始,所以重进设置页还能看到上次结果。
    #[test]
    fn a_finished_snapshot_survives_until_the_next_operation_begins() {
        let agent = unique_agent("retained");
        let BeginOutcome::Started(snapshot, _flag) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
        {
            let mut guard = registry().lock();
            let stored = guard.operations.get_mut(&agent).expect("snapshot exists");
            stored.state = AgentOperationState::Succeeded;
            stored.finished_at_ms = Some(now_ms_u64());
        }
        let stored = snapshot_for(&agent).expect("the finished snapshot is retained");
        assert_eq!(stored.operation_id, snapshot.operation_id);
        assert_eq!(stored.state, AgentOperationState::Succeeded);
        // 终态不再阻塞下一次操作。
        let BeginOutcome::Started(next, _flag) = begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
        assert_ne!(next.operation_id, snapshot.operation_id);
        forget(&agent);
    }

    #[test]
    fn running_state_is_tracked_per_agent() {
        let agent = unique_agent("running");
        let other = unique_agent("idle");
        let BeginOutcome::Started(_snapshot, _flag) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
        assert!(is_running(&agent));
        assert!(!is_running(&other));
        forget(&agent);
    }

    #[test]
    fn cancelling_sets_the_flag_the_worker_polls() {
        let agent = unique_agent("cancel");
        let BeginOutcome::Started(_snapshot, flag) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
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
        let BeginOutcome::Started(snapshot, _flag) =
            begin(&agent, &agent, AgentOperationKind::Upgrade)
        else {
            panic!("expected a new operation")
        };
        let _ = finish_snapshot(
            &agent,
            &snapshot.operation_id,
            AgentOperationState::Succeeded,
            AgentInstallStage::Completed,
            String::new(),
            None,
            None,
            None,
        );
        assert!(cancellation_flag(&agent).is_none());
        assert!(!is_cancelled(&agent));
        forget(&agent);
    }

    fn snapshot_at(progress: u8, message: &str) -> AgentOperationSnapshot {
        let agent = unique_agent("apply");
        let BeginOutcome::Started(snapshot, _flag) =
            begin(&agent, &agent, AgentOperationKind::Install)
        else {
            panic!("expected a new operation")
        };
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

    /// 迟到的旧操作事件不能覆盖新操作的状态:report_progress 按 operation_id 过滤。
    #[test]
    fn a_stale_progress_event_does_not_overwrite_a_new_operation() {
        let agent = unique_agent("stale");
        let BeginOutcome::Started(old, _flag) = begin(&agent, &agent, AgentOperationKind::Install)
        else {
            panic!("expected a new operation")
        };
        let _ = finish_snapshot(
            &agent,
            &old.operation_id,
            AgentOperationState::Failed,
            AgentInstallStage::Failed,
            "failed".to_string(),
            Some(AgentInstallErrorCode::Internal),
            None,
            None,
        );
        let BeginOutcome::Started(current, _flag) =
            begin(&agent, &agent, AgentOperationKind::Install)
        else {
            panic!("expected a new operation")
        };
        // 旧 operation_id 的进度事件:注册表不动。
        let written = report_progress_snapshot(
            &agent,
            &old.operation_id,
            AgentInstallStage::Downloading,
            42,
            "stale",
        );
        assert!(written.is_none());
        let stored = snapshot_for(&agent).expect("snapshot exists");
        assert_eq!(stored.operation_id, current.operation_id);
        assert_eq!(stored.progress, 0);
        // 当前 operation_id 的进度事件:写入并返回快照。
        let written = report_progress_snapshot(
            &agent,
            &current.operation_id,
            AgentInstallStage::Downloading,
            42,
            "fresh",
        );
        assert_eq!(written.expect("current operation writes").progress, 42);
        forget(&agent);
    }
}
