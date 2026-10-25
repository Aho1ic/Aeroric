use super::*;
use crate::sessions::live::update::TurnEndReason;
use serde_json::json;

fn types(updates: &[SessionUpdateKind]) -> Vec<String> {
    updates
        .iter()
        .map(|u| {
            serde_json::to_value(u).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn init_carries_the_session_id() {
    let mut m = ClaudeMapper::default();
    let (updates, signal) = m.map(
        &json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-x"}),
    );
    assert!(updates.is_empty());
    assert_eq!(
        signal,
        Some(ClaudeSignal::SessionStarted {
            session_id: "s1".into(),
            model: Some("claude-x".into())
        })
    );
}

#[test]
fn streamed_text_and_thinking_are_not_repeated_by_the_full_message() {
    let mut m = ClaudeMapper::default();
    m.map(&json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "msg1"}}}));
    let (thinking, _) = m.map(&json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": "想"}}}));
    let (text, _) = m.map(&json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "你好"}}}));
    assert_eq!(
        thinking,
        vec![SessionUpdateKind::ThoughtChunk {
            id: "msg1.thinking".into(),
            text: "想".into(),
            summary: false
        }]
    );
    assert_eq!(
        text,
        vec![SessionUpdateKind::MessageChunk {
            message_id: "msg1".into(),
            role: "assistant".into(),
            text: "你好".into()
        }]
    );
    let (full, _) = m.map(
        &json!({"type": "assistant", "message": {"id": "msg1", "content": [
        {"type": "thinking", "thinking": "想"}, {"type": "text", "text": "你好"}]}}),
    );
    assert_eq!(
        full,
        vec![SessionUpdateKind::MessageCompleted {
            message_id: "msg1".into(),
            text: Some("你好".into())
        }]
    );
}

#[test]
fn unstreamed_messages_arrive_whole() {
    let mut m = ClaudeMapper::default();
    let (updates, _) = m.map(&json!({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "全文"}]}}));
    assert_eq!(types(&updates), ["messageChunk", "messageCompleted"]);
}

#[test]
fn tools_become_cards_and_results_close_them() {
    let mut m = ClaudeMapper::default();
    let (calls, _) = m.map(
        &json!({"type": "assistant", "message": {"id": "m", "content": [
        {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls -la"}},
        {"type": "tool_use", "id": "t2", "name": "Edit", "input": {"file_path": "/w/a.rs"}},
        {"type": "tool_use", "id": "t3", "name": "mcp__github__issue", "input": {}}]}}),
    );
    assert_eq!(
        calls[0],
        SessionUpdateKind::ToolCall {
            id: "t1".into(),
            kind: ToolKind::Command,
            title: "ls -la".into(),
            input: json!({"command": "ls -la"})
        }
    );
    assert!(
        matches!(&calls[1], SessionUpdateKind::ToolCall { kind: ToolKind::FileChange, title, .. } if title == "Edit /w/a.rs")
    );
    assert!(matches!(
        &calls[2],
        SessionUpdateKind::ToolCall {
            kind: ToolKind::Mcp,
            ..
        }
    ));
    let (results, _) = m.map(&json!({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": "a\nb"},
        {"type": "tool_result", "tool_use_id": "t2", "content": [{"type": "text", "text": "no such file"}], "is_error": true}]}}));
    assert_eq!(
        results[0],
        SessionUpdateKind::ToolCallUpdate {
            id: "t1".into(),
            status: ToolStatus::Completed,
            output_delta: None,
            result: Some(json!({"output": "a\nb"}))
        }
    );
    assert!(matches!(
        &results[1],
        SessionUpdateKind::ToolCallUpdate {
            status: ToolStatus::Failed,
            ..
        }
    ));
}

#[test]
fn todo_write_is_the_plan_and_its_result_is_silent() {
    let mut m = ClaudeMapper::default();
    let (plan, _) = m.map(&json!({"type": "assistant", "message": {"id": "m", "content": [{"type": "tool_use", "id": "td", "name": "TodoWrite",
        "input": {"todos": [{"content": "读", "status": "completed"}, {"content": "改", "status": "in_progress"}, {"content": "测", "status": "pending"}]}}]}}));
    assert_eq!(
        plan,
        vec![SessionUpdateKind::Plan {
            entries: vec![
                PlanEntry {
                    text: "读".into(),
                    status: "completed".into()
                },
                PlanEntry {
                    text: "改".into(),
                    status: "inProgress".into()
                },
                PlanEntry {
                    text: "测".into(),
                    status: "pending".into()
                }
            ]
        }]
    );
    let (result, _) = m.map(&json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "td", "content": "ok"}]}}));
    assert!(result.is_empty());
}

#[test]
fn subagent_tasks_and_their_inner_stream() {
    let mut m = ClaudeMapper::default();
    let (started, _) = m.map(&json!({"type": "system", "subtype": "task_started", "task_id": "k1", "description": "检索代码", "uuid": "u", "session_id": "s"}));
    assert_eq!(
        started,
        vec![SessionUpdateKind::Subagent {
            id: "k1".into(),
            title: "检索代码".into(),
            status: "running".into()
        }]
    );
    let (done, _) = m.map(&json!({"type": "system", "subtype": "task_notification", "task_id": "k1", "status": "completed", "summary": "找到 3 处"}));
    assert!(
        matches!(&done[0], SessionUpdateKind::Subagent { status, .. } if status == "completed")
    );
    let (inner, _) = m.map(&json!({"type": "assistant", "parent_tool_use_id": "tk", "message": {"id": "x", "content": [{"type": "text", "text": "inner"}]}}));
    assert!(
        inner.is_empty(),
        "a subagent's own messages stay in its card"
    );
}

#[test]
fn a_result_ends_the_turn_with_running_totals() {
    let mut m = ClaudeMapper::default();
    let (first, signal) = m.map(
        &json!({"type": "result", "subtype": "success", "is_error": false, "session_id": "s",
        "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5}}),
    );
    assert_eq!(
        signal,
        Some(ClaudeSignal::TurnResult {
            failed: false,
            error: None
        })
    );
    assert_eq!(
        first,
        vec![SessionUpdateKind::Usage {
            context_used: None,
            context_limit: None,
            input_tokens: Some(100),
            output_tokens: Some(5)
        }]
    );
    let (second, failed) = m.map(&json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "session_id": "s",
        "usage": {"input_tokens": 1, "output_tokens": 1}, "errors": ["boom"]}));
    assert_eq!(
        failed,
        Some(ClaudeSignal::TurnResult {
            failed: true,
            error: Some("boom".into())
        })
    );
    assert!(matches!(
        &second[0],
        SessionUpdateKind::Usage {
            input_tokens: Some(101),
            output_tokens: Some(6),
            ..
        }
    ));
    let _ = TurnEndReason::Completed;
}

#[test]
fn unknown_messages_are_kept() {
    let mut m = ClaudeMapper::default();
    let (updates, _) = m.map(&json!({"type": "future_thing", "x": 1}));
    assert!(
        matches!(&updates[0], SessionUpdateKind::Unknown { kind, .. } if kind == "future_thing")
    );
    let (system, _) = m.map(&json!({"type": "system", "subtype": "brand_new"}));
    let (estimate, _) = m.map(
        &json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 3, "estimated_tokens_delta": 1}),
    );
    assert!(estimate.is_empty(), "a per-token estimate is not an update");
    assert!(
        matches!(&system[0], SessionUpdateKind::Unknown { kind, .. } if kind == "system/brand_new")
    );
    assert!(m.map(&json!({"type": "rate_limit_event"})).0.is_empty());
}

#[test]
fn approvals_map_both_ways() {
    let request = json!({"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "rm -rf build"}, "tool_use_id": "t9",
        "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "rm:*"}], "behavior": "allow", "destination": "localSettings"}]});
    assert_eq!(
        map_permission_request("r1", &request),
        SessionUpdateKind::PermissionRequest {
            id: "r1".into(),
            tool_call_id: Some("t9".into()),
            kind: "Bash".into(),
            summary: "rm -rf build".into(),
            options: vec!["allowOnce".into(), "allowForSession".into(), "deny".into()],
        }
    );
    let plain = json!({"tool_name": "Write", "input": {"file_path": "/w/a"}});
    assert!(
        matches!(map_permission_request("r2", &plain), SessionUpdateKind::PermissionRequest { options, .. } if options == ["allowOnce", "deny"])
    );

    assert_eq!(
        permission_response(&request, &Decision::AllowOnce),
        json!({"behavior": "allow", "updatedInput": {"command": "rm -rf build"}})
    );
    let session = permission_response(&request, &Decision::AllowForSession);
    assert_eq!(session["updatedPermissions"][0]["destination"], "session");
    assert_eq!(
        session["updatedPermissions"][0]["rules"][0]["ruleContent"],
        "rm:*"
    );
    assert_eq!(
        permission_response(
            &request,
            &Decision::Deny {
                reason: Some("别删".into())
            }
        ),
        json!({"behavior": "deny", "message": "别删"})
    );
    assert_eq!(
        permission_response(&request, &Decision::Deny { reason: None })["message"],
        "用户拒绝了这一步"
    );
}

#[test]
fn modes_and_version_gate() {
    assert_eq!(claude_permission_mode("ask"), "default");
    assert_eq!(claude_permission_mode("auto_edit"), "acceptEdits");
    assert_eq!(claude_permission_mode("full_access"), "bypassPermissions");
    assert!(version_supported("2.1.299 (Claude Code)").is_ok());
    assert!(version_supported("3.0.0 (Claude Code)").is_ok());
    assert!(version_supported("2.0.9 (Claude Code)")
        .unwrap_err()
        .contains("请升级"));
    assert!(version_supported("garbage").is_err());
}
