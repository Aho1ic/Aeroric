use super::*;
use crate::sessions::live::Decision;
use serde_json::json;

#[test]
fn turns_start_and_end_with_their_reason() {
    let (updates, signal) = map_notification(
        "turn/started",
        &json!({"threadId": "th", "turn": {"id": "u1", "status": "inProgress", "items": []}}),
    );
    assert_eq!(
        updates,
        vec![SessionUpdateKind::TurnStarted {
            turn_id: "u1".into()
        }]
    );
    assert_eq!(signal, Some(TurnSignal::Started("u1".into())));

    for (status, reason) in [
        ("completed", TurnEndReason::Completed),
        ("interrupted", TurnEndReason::Interrupted),
        ("failed", TurnEndReason::Failed),
    ] {
        let (_, signal) = map_notification(
            "turn/completed",
            &json!({"threadId": "th", "turn": {"id": "u1", "status": status, "items": [],
            "error": if status == "failed" { json!({"message": "boom"}) } else { Value::Null }}}),
        );
        let error = (status == "failed").then(|| "boom".to_string());
        assert_eq!(
            signal,
            Some(TurnSignal::Ended {
                turn_id: "u1".into(),
                reason,
                error
            })
        );
    }
}

#[test]
fn text_and_reasoning_stream_as_chunks() {
    let (updates, _) = map_notification(
        "item/agentMessage/delta",
        &json!({"threadId": "th", "turnId": "u1", "itemId": "m1", "delta": "你好"}),
    );
    assert_eq!(
        updates,
        vec![SessionUpdateKind::MessageChunk {
            message_id: "m1".into(),
            role: "assistant".into(),
            text: "你好".into()
        }]
    );
    let (updates, _) = map_notification(
        "item/reasoning/summaryTextDelta",
        &json!({"itemId": "r1", "delta": "想想", "summaryIndex": 0, "threadId": "th", "turnId": "u1"}),
    );
    assert!(matches!(
        &updates[0],
        SessionUpdateKind::ThoughtChunk { summary: true, .. }
    ));
    let (updates, _) = map_notification(
        "item/completed",
        &json!({"threadId": "th", "turnId": "u1", "completedAtMs": 1,
        "item": {"type": "agentMessage", "id": "m1", "text": "你好"}}),
    );
    assert_eq!(
        updates,
        vec![SessionUpdateKind::MessageCompleted {
            message_id: "m1".into(),
            text: Some("你好".into())
        }]
    );
}

#[test]
fn commands_become_tool_calls_with_streamed_output() {
    let item = json!({"type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/w", "status": "inProgress"});
    let (started, _) = map_notification(
        "item/started",
        &json!({"item": item, "threadId": "th", "turnId": "u1", "startedAtMs": 1}),
    );
    assert!(
        matches!(&started[0], SessionUpdateKind::ToolCall { kind: ToolKind::Command, title, .. } if title == "cargo test")
    );
    let (delta, _) = map_notification(
        "item/commandExecution/outputDelta",
        &json!({"itemId": "c1", "delta": "ok\n", "threadId": "th", "turnId": "u1"}),
    );
    assert!(
        matches!(&delta[0], SessionUpdateKind::ToolCallUpdate { status: ToolStatus::Running, output_delta: Some(d), .. } if d == "ok\n")
    );
    let done = json!({"type": "commandExecution", "id": "c1", "command": "cargo test", "status": "failed", "exitCode": 101});
    let (completed, _) = map_notification(
        "item/completed",
        &json!({"item": done, "threadId": "th", "turnId": "u1", "completedAtMs": 2}),
    );
    assert!(matches!(
        &completed[0],
        SessionUpdateKind::ToolCallUpdate {
            status: ToolStatus::Failed,
            ..
        }
    ));
}

#[test]
fn plan_usage_and_errors() {
    let (plan, _) = map_notification(
        "turn/plan/updated",
        &json!({"threadId": "th", "turnId": "u1",
        "plan": [{"step": "读代码", "status": "completed"}, {"step": "改", "status": "inProgress"}]}),
    );
    assert_eq!(
        plan,
        vec![SessionUpdateKind::Plan {
            entries: vec![
                PlanEntry {
                    text: "读代码".into(),
                    status: "completed".into()
                },
                PlanEntry {
                    text: "改".into(),
                    status: "inProgress".into()
                },
            ]
        }]
    );
    let (usage, _) = map_notification(
        "thread/tokenUsage/updated",
        &json!({"threadId": "th", "turnId": "u1", "tokenUsage": {
        "last": {"inputTokens": 1200, "outputTokens": 30, "cachedInputTokens": 0, "reasoningOutputTokens": 0, "totalTokens": 1230},
        "total": {"inputTokens": 5000, "outputTokens": 400, "cachedInputTokens": 0, "reasoningOutputTokens": 0, "totalTokens": 5400},
        "modelContextWindow": 200000}}),
    );
    assert_eq!(
        usage,
        vec![SessionUpdateKind::Usage {
            context_used: Some(1200),
            context_limit: Some(200000),
            input_tokens: Some(5000),
            output_tokens: Some(400),
            cached_input_tokens: Some(0)
        }]
    );
    let (error, _) = map_notification(
        "error",
        &json!({"threadId": "th", "turnId": "u1", "willRetry": true, "error": {"message": "rate limited"}}),
    );
    assert_eq!(
        error,
        vec![SessionUpdateKind::Error {
            message: "rate limited".into(),
            recoverable: true
        }]
    );
}

#[test]
fn unknown_notifications_are_quiet_but_unknown_items_are_kept() {
    for method in [
        "thread/realtime/started",
        "item/commandExecution/terminalInteraction",
        "thread/goal/cleared",
    ] {
        let (updates, _) = map_notification(method, &json!({"x": 1}));
        assert!(
            updates.is_empty(),
            "{method} is bookkeeping, not 未识别的内容"
        );
    }
    let (updates, _) = map_notification(
        "item/started",
        &json!({"item": {"type": "hologram", "id": "h"}}),
    );
    assert!(matches!(&updates[0], SessionUpdateKind::Unknown { .. }));
    let (quiet, _) = map_notification("thread/status/changed", &json!({}));
    assert!(quiet.is_empty(), "known bookkeeping is silent, not unknown");
}

#[test]
fn approvals_map_both_ways() {
    let params = json!({"threadId": "th", "turnId": "u1", "itemId": "c1", "startedAtMs": 1, "command": "rm -rf build"});
    let request =
        map_approval_request("item/commandExecution/requestApproval", "7", &params).unwrap();
    assert!(
        matches!(&request, SessionUpdateKind::PermissionRequest { id, kind, summary, tool_call_id: Some(t), .. }
        if id == "7" && kind == "command" && summary == "rm -rf build" && t == "c1")
    );
    assert!(map_approval_request("item/tool/call", "8", &params).is_none());

    let method = "item/commandExecution/requestApproval";
    assert_eq!(
        approval_response(method, &params, &Decision::AllowOnce),
        json!({"decision": "accept"})
    );
    assert_eq!(
        approval_response(method, &params, &Decision::AllowForSession),
        json!({"decision": "acceptForSession"})
    );
    assert_eq!(
        approval_response(
            method,
            &params,
            &Decision::Deny {
                reason: Some("不要删".into())
            }
        ),
        json!({"decision": "decline"})
    );

    let perms = json!({"permissions": {"network": {"enabled": true}}, "itemId": "p1"});
    assert_eq!(
        approval_response(
            "item/permissions/requestApproval",
            &perms,
            &Decision::AllowForSession
        ),
        json!({"permissions": {"network": {"enabled": true}}, "scope": "session"})
    );
    assert_eq!(
        approval_response(
            "item/permissions/requestApproval",
            &perms,
            &Decision::Deny { reason: None }
        ),
        json!({"permissions": {}})
    );
}

#[test]
fn policies_and_version_gate() {
    assert_eq!(codex_policy("ask").sandbox_mode, "read-only");
    assert_eq!(
        codex_policy("auto_edit").sandbox_policy,
        json!({"type": "workspaceWrite"})
    );
    assert_eq!(codex_policy("full_access").approval, "never");
    assert!(version_supported("codex-cli 0.162.1").is_ok());
    assert!(version_supported("codex-cli 1.0.0").is_ok());
    assert!(version_supported("codex-cli 0.161.9")
        .unwrap_err()
        .contains("0.162.0"));
    assert!(version_supported("garbage").is_err());
}

#[test]
fn codex_methods_exist_in_the_snapshot() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/codex/app-server-0.162.1");
    let methods = |file: &str| -> Vec<String> {
        let schema: Value =
            serde_json::from_slice(&std::fs::read(dir.join(file)).expect(file)).unwrap();
        schema["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| {
                v.pointer("/properties/method/enum/0")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    };
    let client = methods("ClientRequest.json");
    for method in CLIENT_METHODS {
        assert!(
            client.iter().any(|m| m == method),
            "{method} missing from snapshot"
        );
    }
    let server = methods("ServerRequest.json");
    for method in SERVER_REQUESTS {
        assert!(
            server.iter().any(|m| m == method),
            "{method} missing from snapshot"
        );
    }
    let notifications = methods("ServerNotification.json");
    for method in [
        "turn/started",
        "turn/completed",
        "item/started",
        "item/completed",
        "item/agentMessage/delta",
        "item/commandExecution/outputDelta",
        "turn/plan/updated",
        "thread/tokenUsage/updated",
        "error",
    ] {
        assert!(
            notifications.iter().any(|m| m == method),
            "{method} missing from snapshot"
        );
    }
}

#[test]
fn thread_goal_notifications_are_bookkeeping() {
    for method in ["thread/goal/updated", "thread/goal/cleared"] {
        let (updates, _) = map_notification(method, &json!({"threadId": "th"}));
        assert!(
            updates.is_empty(),
            "{method} is not shown as unknown content"
        );
    }
}
