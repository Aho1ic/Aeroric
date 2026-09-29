//! Rust 侧 `app.emit` 用到的事件名常量。
//!
//! 与前端的对账基准是 `src/tauriEvents.ts`(前端自己那份常量清单):两边各自
//! 持有同名常量,字符串必须逐字一致,改名时同步改两侧。这里只收录 Rust 侧
//! 实际 emit 用的名字。

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
// 这 10 个常量已随 P3-05 第一片迁入 core `aeroric_core::agents::dsh_host::dispatcher`
// （补记二百五十），re-export 保持 `crate::event_names::…` 路径与原可见性不变；
// 「与前端 `DSH_HOST_*_EVENT` 逐字对账、改名两侧同动」的契约口径随字符串一起迁，
// 10 臂映射表的测试在 `core/crates/aeroric-core/tests/dsh_dispatcher.rs`。
pub(crate) use aeroric_core::agents::dsh_host::dispatcher::{
    DSH_HOST_REMOTE_EVENT, DSH_HOST_STREAM_ERROR,
};
