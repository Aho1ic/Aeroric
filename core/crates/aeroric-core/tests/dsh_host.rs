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
