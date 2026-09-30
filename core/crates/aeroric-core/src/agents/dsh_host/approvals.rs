//! 审批/提问 waterfall 的判定与帧形状（P3-05 第四片，Action 1 的 `approvals` 文件）。
//!
//! 逐字迁自宿主 `src-tauri/src/dsh_webui.rs` 的 `PendingWaterfall`／`WaterfallKind`／
//! `WaterfallRegistry` 与 `dispatch_waterfall_request`／`dispatch_waterfall_cancel` 的
//! **判定＋帧构造两半**；`emit`（宿主句柄地盘）与 `Mutex` 包裹留在宿主。四个
//! `dsh-approval-*`／`dsh-question-*` 事件常量随判定表一起迁入并经宿主
//! `event_names.rs` re-export——字符串是前端 `DSH_*_EVENT` 逐字对账的在网契约。
//!
//! 消费语义一条不许漂（Action 3「网络重放不能再次批准」的既有形状）：
//! `remove` 是 take-and-consume，同 `eventId` 第二次拿不到条目；宿主 reply 路径
//! （`respond_dsh_remote_event`）对 unknown-but-alive generation 走
//! `all_pending().next()` 的幂等兜底——两者都在 `tests/dsh_host.rs` 钉住。
//! 更严格的「跨 task 应答拒绝」不引入：那是行为变更，属产品决策，不借迁移偷渡。

use serde_json::{json, Value};
use std::collections::HashMap;

pub const DSH_APPROVAL_REQUESTED: &str = "dsh-approval-requested";
pub const DSH_QUESTION_REQUESTED: &str = "dsh-question-requested";
pub const DSH_APPROVAL_RESOLVED: &str = "dsh-approval-resolved";
pub const DSH_QUESTION_RESOLVED: &str = "dsh-question-resolved";

/// One in-flight approval / question request, keyed by its `eventId`.
///
/// The reply must quote both the `eventId` and the `clientId` from the ready
/// frame of the generation that raised it, so the id is tracked alongside the
/// pending set rather than being re-derived at reply time.
#[derive(Debug, Clone)]
pub struct PendingWaterfall {
    pub kind: WaterfallKind,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterfallKind {
    Approval,
    Question,
}

/// The pending-waterfall map plus the current generation's `clientId`.
///
/// `clientId` lives here because it is generation-scoped exactly like the
/// pending entries: both are cleared when the carrier reconnects.
#[derive(Debug, Default)]
pub struct WaterfallRegistry {
    client_id: Option<String>,
    pending: HashMap<String, PendingWaterfall>,
}

impl WaterfallRegistry {
    pub fn set_client_id(&mut self, client_id: String) {
        self.client_id = Some(client_id);
    }

    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub fn clear(&mut self) {
        self.client_id = None;
        self.pending.clear();
    }

    pub fn insert(&mut self, event_id: String, entry: PendingWaterfall) {
        self.pending.insert(event_id, entry);
    }

    pub fn remove(&mut self, event_id: &str) -> Option<PendingWaterfall> {
        self.pending.remove(event_id)
    }

    pub fn all_pending(&self) -> Vec<&PendingWaterfall> {
        self.pending.values().collect()
    }
}

/// 权限档 → dsh preset 名的映射（原宿主 `dsh_permission_preset` 逐字）：这是
/// **权限路由的入口判定**，`grill-me [dsh]` 点名要复核的面，判定归 core、
/// 把它写进 `session.setPermissionMode` 的 RPC 留宿主。
pub fn dsh_permission_preset(mode: &str) -> Result<&'static str, String> {
    match mode {
        "ask" => Ok("read-only"),
        "auto_edit" => Ok("workspace-write"),
        "full_access" => Ok("danger-full-access"),
        other => Err(format!("Unsupported DSH permission mode {other:?}")),
    }
}
/// 下行 `permission/request`／`user-questions/request` 两臂 →（类型, 事件名）。
/// `None`＝未知 waterfall 事件：宿主**不得静默吞**（Host 会等答案等到卡死），
/// 由宿主侧发 `stream/error`——那个 emit 留宿主。
pub fn waterfall_request_kind(event: &str) -> Option<(WaterfallKind, &'static str)> {
    match event {
        "permission/request" => Some((WaterfallKind::Approval, DSH_APPROVAL_REQUESTED)),
        "user-questions/request" => Some((WaterfallKind::Question, DSH_QUESTION_REQUESTED)),
        _ => None,
    }
}

/// 撤回臂：pending 条目类型 → 该发的 `*_RESOLVED` 事件名。
pub fn waterfall_resolved_event(kind: WaterfallKind) -> &'static str {
    match kind {
        WaterfallKind::Approval => DSH_APPROVAL_RESOLVED,
        WaterfallKind::Question => DSH_QUESTION_RESOLVED,
    }
}

/// `sessionId` 先行，缺省回退 `agentId`（宿主原文顺序）。
pub fn waterfall_session_id(payload: &Value) -> Option<String> {
    payload
        .get("sessionId")
        .or_else(|| payload.get("agentId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 请求帧：注入 `eventId` 与本代 `clientId`，`sessionId` 只在缺位时补
/// （`or_insert` 语义：payload 自带的值优先）。非 object payload 原样返回。
pub fn stamp_waterfall_frame(
    payload: &Value,
    event_id: &str,
    client_id: Option<String>,
    session_id: Option<String>,
) -> Value {
    let mut frame = payload.clone();
    if let Some(object) = frame.as_object_mut() {
        object.insert("eventId".to_string(), Value::String(event_id.to_string()));
        if let Some(client_id) = client_id {
            object.insert("clientId".to_string(), Value::String(client_id));
        }
        if let Some(session_id) = session_id {
            object
                .entry("sessionId".to_string())
                .or_insert(Value::String(session_id));
        }
    }
    frame
}

/// 撤回帧（宿主 `json!` 构造逐字同形：`sessionId` 缺位发 `null`）。
pub fn waterfall_cancel_frame(event_id: &str, session_id: Option<String>) -> Value {
    json!({
        "eventId": event_id,
        "sessionId": session_id,
        "cancelled": true,
    })
}

/// 用户应答的本地判定＋构帧（P3-05 验收行 2「无 GUI 可经 typed 方法回答审批」）。
/// 自宿主 `respond_dsh_remote_event` verbatim 上收三段：kind 校验（拒绝发生在
/// consume **之前**——坏请求不得吃掉 pending）、take-and-consume（重放拿不到
/// entry，走 `all_pending().next()` 的幂等路由提示）、`$events/result` 构帧。
/// **api 路由留在宿主**（session→base_url→fallback 链读宿主活跃表）；
/// 「无回答时保持等待」由结构保证：registry 是纯数据，唯一的退场口就是这个
/// take 和宿主撤回时的 `remove`——没有 timer，也没有自动同意路径。
pub fn take_waterfall_answer(
    registry: &mut WaterfallRegistry,
    client_id: &str,
    event_id: &str,
    outcome: &Value,
) -> Result<WaterfallAnswer, String> {
    if outcome.get("kind").and_then(Value::as_str).is_none() {
        return Err("DSH event outcome must carry a kind".to_string());
    }
    let session_hint = match registry.remove(event_id) {
        Some(entry) => entry.session_id,
        // The Host accepts answers for unknown-but-alive generations
        // (idempotent replays); keep routing by an active pending entry.
        None => registry
            .all_pending()
            .into_iter()
            .next()
            .and_then(|entry| entry.session_id.clone()),
    };
    Ok(WaterfallAnswer {
        session_hint,
        args: crate::agents::dsh_host::remote_mux::remote_event_result_args(
            client_id,
            event_id,
            outcome.clone(),
        ),
    })
}

/// [`take_waterfall_answer`] 的产物：路由提示（宿主据此选 api）＋wire args。
#[derive(Debug)]
pub struct WaterfallAnswer {
    pub session_hint: Option<String>,
    pub args: Value,
}
