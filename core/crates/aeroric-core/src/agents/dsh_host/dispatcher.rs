//! host 全域 `/api/remote.mux` 下行帧 → 事件名的纯映射（P3-05 第一片）。
//!
//! 逐字迁移自宿主 `src-tauri/src/dsh_webui.rs` 的 `dispatch_host_frame` 的 10 臂
//! 表与 `src-tauri/src/event_names.rs` 的 10 个 `DSH_HOST_*` 常量；字符串本身是
//! 与前端 `DSH_HOST_*_EVENT` 对账的在网契约（`event_names.rs` 头注释原文：
//! 「两边各自持有同名常量，字符串必须逐字一致，改名时同步改两侧」）。
//! 唯一形状变化＝映射从「就地 emit」拆成纯函数：返回 `None` 表示**未知帧静默
//! 丢弃**——这是前向兼容的既有行为，逐字保留（补记一百二十定性：未知臂不报错、
//! 不计数）。`emit` 留宿主的 `dispatch_host_frame` wrapper（宿主句柄地盘）。
//!
//! 本表也是 10 条 `dsh-host-*` v1 topic 的发射定性证据面（G17/`G01`）：
//! 台账 legacy 名 ↔ 帧型 ↔ 常量三者在此一处对齐，前端只订阅其中 8 条
//! （`dsh-host-remote-event`／`dsh-host-stream-error` 为孤儿，处置见 `G01`）。

use serde_json::Value;

pub const DSH_HOST_SESSION_ADDED: &str = "dsh-host-session-added";
pub const DSH_HOST_SESSION_REMOVED: &str = "dsh-host-session-removed";
pub const DSH_HOST_SESSION_STATUS: &str = "dsh-host-session-status";
pub const DSH_HOST_AGENT_ERROR: &str = "dsh-host-agent-error";
pub const DSH_HOST_WORKSPACE_CHANGED: &str = "dsh-host-workspace-changed";
pub const DSH_HOST_WORKSPACE_REMOVED: &str = "dsh-host-workspace-removed";
pub const DSH_HOST_WORKSPACE_ORDER_CHANGED: &str = "dsh-host-workspace-order-changed";
pub const DSH_HOST_ARCHIVED_SESSIONS_CHANGED: &str = "dsh-host-archived-sessions-changed";
pub const DSH_HOST_REMOTE_EVENT: &str = "dsh-host-remote-event";
pub const DSH_HOST_STREAM_ERROR: &str = "dsh-host-stream-error";

/// 一帧下行 payload → 该转发的事件名；`None`＝未知帧型（含缺 `type`／非字符串
/// `type`），静默丢弃是既有语义。
pub fn host_frame_event_name(payload: &Value) -> Option<&'static str> {
    let frame_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match frame_type {
        "host/session-added" => Some(DSH_HOST_SESSION_ADDED),
        "host/session-removed" => Some(DSH_HOST_SESSION_REMOVED),
        "host/session-status" => Some(DSH_HOST_SESSION_STATUS),
        "host/agent-error" => Some(DSH_HOST_AGENT_ERROR),
        "host/workspace-changed" => Some(DSH_HOST_WORKSPACE_CHANGED),
        "host/workspace-removed" => Some(DSH_HOST_WORKSPACE_REMOVED),
        "host/workspace-order-changed" => Some(DSH_HOST_WORKSPACE_ORDER_CHANGED),
        "host/archived-sessions-changed" => Some(DSH_HOST_ARCHIVED_SESSIONS_CHANGED),
        "host/remote-event" => Some(DSH_HOST_REMOTE_EVENT),
        "stream/error" => Some(DSH_HOST_STREAM_ERROR),
        // Forward-compatibility: silently drop unknown frame types.
        _ => None,
    }
}
