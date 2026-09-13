//! Rust 侧 `app.emit` 用到的事件名常量。
//!
//! 与前端的对账基准是 `src/tauriEvents.ts`(前端自己那份常量清单):两边各自
//! 持有同名常量,字符串必须逐字一致,改名时同步改两侧。这里只收录 Rust 侧
//! 实际 emit 用的名字。

/// 任务生命周期状态(前端 `TASK_STATUS_EVENT`)。
pub(crate) const TASK_STATUS: &str = "task-status";

// ── dsh webui 会话通道 ─────────────────────────────────────────────────────────
pub(crate) const DSH_SESSION_EVENT: &str = "dsh-session-event";
pub(crate) const DSH_SESSION_SUBSCRIBED: &str = "dsh-session-subscribed";
pub(crate) const DSH_SESSION_QUEUE: &str = "dsh-session-queue";
pub(crate) const DSH_SESSION_JOBS: &str = "dsh-session-jobs";
pub(crate) const DSH_SESSION_PROJECTION: &str = "dsh-session-projection";

// ── dsh 审批/提问(前端 `DSH_*_EVENT`)──────────────────────────────────────────
pub(crate) const DSH_APPROVAL_REQUESTED: &str = "dsh-approval-requested";
pub(crate) const DSH_QUESTION_REQUESTED: &str = "dsh-question-requested";
pub(crate) const DSH_APPROVAL_RESOLVED: &str = "dsh-approval-resolved";
pub(crate) const DSH_QUESTION_RESOLVED: &str = "dsh-question-resolved";

// ── dsh host 下行帧 → 前端事件(前端 `DSH_HOST_*_EVENT`)───────────────────────
pub(crate) const DSH_HOST_SESSION_ADDED: &str = "dsh-host-session-added";
pub(crate) const DSH_HOST_SESSION_REMOVED: &str = "dsh-host-session-removed";
pub(crate) const DSH_HOST_SESSION_STATUS: &str = "dsh-host-session-status";
pub(crate) const DSH_HOST_AGENT_ERROR: &str = "dsh-host-agent-error";
pub(crate) const DSH_HOST_WORKSPACE_CHANGED: &str = "dsh-host-workspace-changed";
pub(crate) const DSH_HOST_WORKSPACE_REMOVED: &str = "dsh-host-workspace-removed";
pub(crate) const DSH_HOST_WORKSPACE_ORDER_CHANGED: &str = "dsh-host-workspace-order-changed";
pub(crate) const DSH_HOST_ARCHIVED_SESSIONS_CHANGED: &str = "dsh-host-archived-sessions-changed";
pub(crate) const DSH_HOST_REMOTE_EVENT: &str = "dsh-host-remote-event";
pub(crate) const DSH_HOST_STREAM_ERROR: &str = "dsh-host-stream-error";
