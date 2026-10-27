use serde_json::json;

use super::*;

fn updates(mapped: Mapped) -> Vec<SessionUpdateKind> {
    match mapped {
        Mapped::Updates(updates) => updates,
        Mapped::Ignored => panic!("ignored"),
    }
}

#[test]
fn streamed_text_and_thinking_then_the_completed_message_with_usage() {
    let mut m = OmpMapper::default();
    let chunk = updates(m.map(
        &json!({"type": "message_update", "messageId": "msg-2", "message": {"role": "assistant"},
        "assistantMessageEvent": {"type": "text_delta", "contentIndex": 0, "delta": "你好"}}),
    ));
    assert_eq!(
        chunk,
        vec![SessionUpdateKind::MessageChunk {
            message_id: "msg-2".into(),
            role: "assistant".into(),
            text: "你好".into()
        }]
    );
    let thought = updates(
        m.map(&json!({"type": "message_update", "messageId": "msg-2",
        "assistantMessageEvent": {"type": "thinking_delta", "delta": "想想"}})),
    );
    assert!(
        matches!(&thought[0], SessionUpdateKind::ThoughtChunk { id, .. } if id == "msg-2-thinking")
    );
    assert_eq!(m.map(&json!({"type": "message_update", "messageId": "msg-2", "assistantMessageEvent": {"type": "toolcall_delta", "delta": "{"}})), Mapped::Ignored);
    let end = updates(m.map(&json!({"type": "message_end", "messageId": "msg-2", "message": {"role": "assistant",
        "content": [{"type": "text", "text": "你好"}, {"type": "toolCall", "id": "c"}], "usage": {"input": 12, "output": 3}}})));
    assert_eq!(
        end[0],
        SessionUpdateKind::MessageCompleted {
            message_id: "msg-2".into(),
            text: Some("你好".into())
        }
    );
    assert_eq!(
        end[1],
        SessionUpdateKind::Usage {
            context_used: None,
            context_limit: None,
            input_tokens: Some(12),
            output_tokens: Some(3)
        }
    );
    assert_eq!(
        m.map(&json!({"type": "message_end", "messageId": "msg-1", "message": {"role": "user"}})),
        Mapped::Ignored
    );
}

#[test]
fn tools_map_to_cards_with_kinds_titles_and_outcomes() {
    let mut m = OmpMapper::default();
    let start = updates(m.map(&json!({"type": "tool_execution_start", "toolCallId": "call_1", "toolName": "bash", "args": {"command": "echo hi\nmore"}})));
    assert_eq!(
        start,
        vec![SessionUpdateKind::ToolCall {
            id: "call_1".into(),
            kind: ToolKind::Command,
            title: "echo hi".into(),
            input: json!({"command": "echo hi\nmore"})
        }]
    );
    let end = updates(m.map(&json!({"type": "tool_execution_end", "toolCallId": "call_1", "toolName": "bash", "result": {"content": []}, "isError": true})));
    assert!(matches!(
        &end[0],
        SessionUpdateKind::ToolCallUpdate {
            status: ToolStatus::Failed,
            ..
        }
    ));
    assert_eq!(tool_kind("write"), ToolKind::FileChange);
    assert_eq!(tool_kind("mcp__github__issue"), ToolKind::Mcp);
    assert_eq!(tool_title("read", &json!({"path": "src/a.rs"})), "src/a.rs");
    assert_eq!(tool_title("think", &json!({})), "think");
}

#[test]
fn subagents_notices_config_and_the_rest() {
    let mut m = OmpMapper::default();
    let sub = updates(m.map(&json!({"type": "subagent_progress", "payload": {"index": 0, "agent": "explore", "task": "读导入器",
        "progress": {"id": "sa1", "status": "running", "lastIntent": "读取 src/importer"}}})));
    assert_eq!(
        sub,
        vec![SessionUpdateKind::Subagent {
            id: "sa1".into(),
            title: "explore · 读导入器".into(),
            status: "running".into(),
            detail: Some("读取 src/importer".into())
        }]
    );
    let done = updates(m.map(&json!({"type": "subagent_lifecycle", "payload": {"id": "sa1", "agent": "explore", "status": "aborted", "index": 0}})));
    assert!(
        matches!(&done[0], SessionUpdateKind::Subagent { status, .. } if status == "cancelled")
    );
    assert!(
        matches!(&updates(m.map(&json!({"type": "notice", "level": "warning", "message": "x"})))[0], SessionUpdateKind::Notice { level, .. } if level == "warning")
    );
    assert!(
        matches!(&updates(m.map(&json!({"type": "model_changed", "model": {"id": "sonnet"}})))[0], SessionUpdateKind::ConfigChanged { model: Some(model), .. } if model == "sonnet")
    );
    assert_eq!(
        m.map(&json!({"type": "queue_update", "steering": []})),
        Mapped::Ignored
    );
    assert_eq!(
        m.map(&json!({"type": "advisor_cost_changed", "cost": 0})),
        Mapped::Ignored
    );
    assert!(
        matches!(&updates(m.map(&json!({"type": "future_frame", "x": 1})))[0], SessionUpdateKind::Unknown { kind, .. } if kind == "omp/future_frame")
    );
}

#[test]
fn todo_phases_become_the_plan() {
    let plan = plan_from_todos(
        &json!([{"name": "一", "tasks": [{"content": "读代码", "status": "completed"}, {"content": "改代码", "status": "in_progress"}]},
        {"name": "二", "tasks": [{"content": "测试", "status": "pending"}]}]),
    );
    assert_eq!(
        plan.iter()
            .map(|e| (e.text.as_str(), e.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("读代码", "completed"),
            ("改代码", "inProgress"),
            ("测试", "pending")
        ]
    );
}
