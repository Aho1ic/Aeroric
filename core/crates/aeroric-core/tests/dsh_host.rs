//! P3-05 第一片金样：host 下行帧 → 事件名的 10 臂映射（Action 1 验收名单
//! `tests/{dsh_host,dsh_approvals,dsh_reconnect}.rs` 的 `dsh_host` 自本片起用）。
//!
//! 判据两条来源：① 字符串值是**在网契约**——前端 `src/tauriEvents.ts` 的
//! `DSH_HOST_*_EVENT` 逐字对账（`event_names.rs` 头注释原文「改名时同步改两侧」；
//! 台账的 ts_consts/rust_consts 双列由 `inventory --check` 每天核），所以这里
//! 逐条钉**字面量**而不是钉常量名互等；② 「未知帧静默丢弃」是补记一百二十
//! 定性过的既有前向兼容行为，返回 `None` 即其纯形。

use aeroric_core::agents::dsh_host::dispatcher::host_frame_event_name;
use aeroric_core::agents::dsh_host::dispatcher::{
    DSH_HOST_AGENT_ERROR, DSH_HOST_ARCHIVED_SESSIONS_CHANGED, DSH_HOST_REMOTE_EVENT,
    DSH_HOST_SESSION_ADDED, DSH_HOST_SESSION_REMOVED, DSH_HOST_SESSION_STATUS,
    DSH_HOST_STREAM_ERROR, DSH_HOST_WORKSPACE_CHANGED, DSH_HOST_WORKSPACE_ORDER_CHANGED,
    DSH_HOST_WORKSPACE_REMOVED,
};
use serde_json::json;

#[test]
fn ten_arms_map_to_their_pinned_wire_strings() {
    let cases = [
        (
            "host/session-added",
            DSH_HOST_SESSION_ADDED,
            "dsh-host-session-added",
        ),
        (
            "host/session-removed",
            DSH_HOST_SESSION_REMOVED,
            "dsh-host-session-removed",
        ),
        (
            "host/session-status",
            DSH_HOST_SESSION_STATUS,
            "dsh-host-session-status",
        ),
        (
            "host/agent-error",
            DSH_HOST_AGENT_ERROR,
            "dsh-host-agent-error",
        ),
        (
            "host/workspace-changed",
            DSH_HOST_WORKSPACE_CHANGED,
            "dsh-host-workspace-changed",
        ),
        (
            "host/workspace-removed",
            DSH_HOST_WORKSPACE_REMOVED,
            "dsh-host-workspace-removed",
        ),
        (
            "host/workspace-order-changed",
            DSH_HOST_WORKSPACE_ORDER_CHANGED,
            "dsh-host-workspace-order-changed",
        ),
        (
            "host/archived-sessions-changed",
            DSH_HOST_ARCHIVED_SESSIONS_CHANGED,
            "dsh-host-archived-sessions-changed",
        ),
        (
            "host/remote-event",
            DSH_HOST_REMOTE_EVENT,
            "dsh-host-remote-event",
        ),
        (
            "stream/error",
            DSH_HOST_STREAM_ERROR,
            "dsh-host-stream-error",
        ),
    ];
    for (frame, constant, literal) in cases {
        assert_eq!(
            host_frame_event_name(&json!({"type": frame})),
            Some(constant)
        );
        assert_eq!(constant, literal, "在网字符串逐字钉死（前端对账基准）");
    }
}

#[test]
fn unknown_frames_drop_silently_including_near_misses() {
    // 前向兼容：未知帧型返回 None（宿主 wrapper 的早退），不报错不计数。
    assert_eq!(
        host_frame_event_name(&json!({"type": "host/session-added-v2"})),
        None
    );
    assert_eq!(
        host_frame_event_name(&json!({"type": "dsh/host/session-added"})),
        None
    );
    // 事件名当帧型喂回来不成立——映射方向是 帧型→事件名，不是恒等。
    assert_eq!(
        host_frame_event_name(&json!({"type": "dsh-host-session-added"})),
        None
    );
    // 缺 type／非字符串 type／null payload：全部 None，不许 panic。
    assert_eq!(host_frame_event_name(&json!({})), None);
    assert_eq!(host_frame_event_name(&json!({"type": 7})), None);
    assert_eq!(host_frame_event_name(&json!(null)), None);
}

#[test]
fn payload_beyond_type_does_not_change_the_mapping() {
    let bare = json!({"type": "host/workspace-changed"});
    let loaded = json!({"type": "host/workspace-changed", "id": "ws-1", "nested": {"a": [1, 2]}});
    assert_eq!(host_frame_event_name(&bare), host_frame_event_name(&loaded));
    assert_eq!(
        host_frame_event_name(&loaded),
        Some(DSH_HOST_WORKSPACE_CHANGED),
        "整帧原样转发是宿主 emit 的语义；映射只看 type"
    );
}

#[test]
fn rpc_envelope_decodes_value_and_keeps_both_error_wordings_verbatim() {
    // 与宿主 re-export 上的两条既有测试同一判据——core API 直接钉，防止
    // 未来某次「只改宿主」的漂移绕过。
    use aeroric_core::agents::dsh_host::client::decode_result_value;
    assert_eq!(
        decode_result_value(
            json!({"result":{"ok":true,"value":{"id":7}}}),
            "session.list"
        )
        .unwrap(),
        json!({"id":7})
    );
    assert_eq!(
        decode_result_value(json!({"result":{"ok":true}}), "x").unwrap(),
        json!(null),
        "缺 value 默认 Null 是既有语义"
    );
    assert_eq!(
        decode_result_value(json!({}), "session.list").unwrap_err(),
        "DSH API session.list response has no result"
    );
    assert_eq!(
        decode_result_value(
            json!({"result":{"ok":false,"error":{"message":"denied"}}}),
            "session.list"
        )
        .unwrap_err(),
        "DSH API session.list rejected the request: denied"
    );
    // error 缺 message ⇒ "unknown DSH API error" 兜底。
    assert_eq!(
        decode_result_value(json!({"result":{"ok":false,"error":{}}}), "m").unwrap_err(),
        "DSH API m rejected the request: unknown DSH API error"
    );
}

#[test]
fn pinned_inventory_keeps_generation_anchors_and_excludes_retired_names() {
    // 第三片钉面：三 pin 值＝快照对账基准（G17）；现用帧表不得重新收进 DSH-14 退役名；
    // `REMOTE_EVENTS.zip(REMOTE_EVENT_MODES)` 是宿主 `snapshot()` 的硬前提——长度必须成对。
    use aeroric_core::agents::dsh_host::client::{
        PACKAGE_VERSION, PROTOCOL_VERSION, REMOTE_EVENTS, REMOTE_EVENT_MODES, RPC_METHODS,
        SOURCE_COMMIT, STREAM_CONTROL_FRAMES, STREAM_DOWNLINK_FRAMES, STREAM_FOLLOW_FRAMES,
    };
    assert_eq!(SOURCE_COMMIT, "c291e7961a515f6d7af9304e7fd1d257929aef26");
    assert_eq!(PACKAGE_VERSION, "0.1.5-rc.2");
    assert_eq!(PROTOCOL_VERSION, 2);
    assert!(RPC_METHODS.contains(&"session.list") && RPC_METHODS.len() >= 60);
    assert_eq!(
        REMOTE_EVENTS.len(),
        REMOTE_EVENT_MODES.len(),
        "snapshot() 按 zip 配对，长度错位就静默吞尾"
    );
    for retired in [
        "session/event",
        "session/subscribed",
        "approval/requested",
        "stream/error",
    ] {
        assert!(
            !STREAM_FOLLOW_FRAMES.contains(&retired)
                && !STREAM_CONTROL_FRAMES.contains(&retired)
                && !STREAM_DOWNLINK_FRAMES.contains(&retired),
            "退役 firehose 名 {retired} 不得回流现用帧表"
        );
    }
}

#[test]
fn waterfall_verdicts_map_arms_and_pin_event_strings() {
    use aeroric_core::agents::dsh_host::approvals::{
        waterfall_request_kind, waterfall_resolved_event, WaterfallKind,
    };
    let (kind, name) = waterfall_request_kind("permission/request").unwrap();
    assert!(matches!(kind, WaterfallKind::Approval));
    assert_eq!(name, "dsh-approval-requested");
    let (kind, name) = waterfall_request_kind("user-questions/request").unwrap();
    assert!(matches!(kind, WaterfallKind::Question));
    assert_eq!(name, "dsh-question-requested");
    // 未知臂＝None，由宿主发 stream/error（不得静默吞的形状在宿主侧，判定面在此）。
    assert!(waterfall_request_kind("permission/answered").is_none());
    assert_eq!(
        waterfall_resolved_event(WaterfallKind::Approval),
        "dsh-approval-resolved"
    );
    assert_eq!(
        waterfall_resolved_event(WaterfallKind::Question),
        "dsh-question-resolved"
    );
}

#[test]
fn session_id_prefers_session_key_then_agent_fallback() {
    use aeroric_core::agents::dsh_host::approvals::waterfall_session_id;
    assert_eq!(
        waterfall_session_id(&json!({"sessionId":"s1","agentId":"a1"})).as_deref(),
        Some("s1")
    );
    assert_eq!(
        waterfall_session_id(&json!({"agentId":"a1"})).as_deref(),
        Some("a1")
    );
    assert_eq!(waterfall_session_id(&json!({"sessionId":7})), None);
    assert_eq!(waterfall_session_id(&json!({})), None);
}

#[test]
fn request_frame_stamps_ids_without_clobbering_session_payload() {
    use aeroric_core::agents::dsh_host::approvals::stamp_waterfall_frame;
    let frame = stamp_waterfall_frame(
        &json!({"type":"permission/request","sessionId":"mine"}),
        "ev-1",
        Some("gen-9".to_string()),
        Some("other".to_string()),
    );
    assert_eq!(frame["eventId"], json!("ev-1"));
    assert_eq!(frame["clientId"], json!("gen-9"));
    assert_eq!(
        frame["sessionId"],
        json!("mine"),
        "or_insert 语义：payload 自带的 sessionId 不被补位值覆盖"
    );
    // client_id 缺位不注入；sessionId 缺位则补。
    let frame = stamp_waterfall_frame(&json!({"a":1}), "ev-2", None, Some("s2".to_string()));
    assert!(frame.get("clientId").is_none());
    assert_eq!(frame["sessionId"], json!("s2"));
    // 非 object payload 原样返回（宿主原 `as_object_mut` 分支）。
    assert_eq!(
        stamp_waterfall_frame(&json!(null), "ev-3", None, None),
        json!(null)
    );
}

#[test]
fn registry_consumes_once_and_clear_resets_generation_scoped_state() {
    use aeroric_core::agents::dsh_host::approvals::{
        PendingWaterfall, WaterfallKind, WaterfallRegistry,
    };
    let mut reg = WaterfallRegistry::default();
    assert_eq!(reg.client_id(), None);
    reg.set_client_id("gen-1".to_string());
    reg.insert(
        "ev-1".to_string(),
        PendingWaterfall {
            kind: WaterfallKind::Approval,
            session_id: Some("s1".to_string()),
        },
    );
    // take-and-consume：第一次 remove 拿到，第二次 None——网络重放不得二次批准。
    let first = reg.remove("ev-1").expect("first consume");
    assert!(matches!(first.kind, WaterfallKind::Approval));
    assert_eq!(first.session_id.as_deref(), Some("s1"));
    assert!(reg.remove("ev-1").is_none(), "重放必须拿不到");
    // 幂等兜底路由：unknown eventId 时 reply 路径读 all_pending().next()。
    reg.insert(
        "ev-2".to_string(),
        PendingWaterfall {
            kind: WaterfallKind::Question,
            session_id: None,
        },
    );
    let pending = reg.all_pending();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].session_id.is_none());
    // 载体重连：client_id 与 pending 同代清空（默认值的两半一起归零）。
    reg.clear();
    assert_eq!(reg.client_id(), None);
    assert!(reg.all_pending().is_empty());
}
