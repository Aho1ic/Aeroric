use super::*;
use crate::sessions::live::acp::dialect::DSH;
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

/// Frames as `dsh acp` 0.2.2-rc.1 sent them on 2026-11-13 (read-only sandbox,
/// a write that escalated, was approved, then a second one refused).
#[test]
fn a_recorded_dsh_turn_maps_to_cards() {
    let mut m = AcpMapper::new(&DSH);
    let thought = m.update(
        &json!({"sessionUpdate": "agent_thought_chunk", "messageId": "m1",
        "content": {"type": "text", "text": "Sandbox is read-only."}}),
    );
    assert_eq!(
        thought,
        vec![SessionUpdateKind::ThoughtChunk {
            id: "m1-thinking".into(),
            text: "Sandbox is read-only.".into(),
            summary: false
        }]
    );
    let usage = m.update(&json!({"sessionUpdate": "usage_update", "used": 9815, "size": 1048576}));
    assert!(matches!(
        usage[0],
        SessionUpdateKind::Usage {
            context_used: Some(9815),
            context_limit: Some(1048576),
            ..
        }
    ));
    let call = m.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "write",
        "kind": "other", "status": "in_progress",
        "rawInput": {"file_path": "/w/a.txt", "content": "hello", "sandbox_permissions": "workspace-write",
            "justification": "需要写入工作目录"}}));
    match &call[0] {
        SessionUpdateKind::ToolCall { kind, title, .. } => {
            assert_eq!(*kind, ToolKind::FileChange);
            assert_eq!(title, "/w/a.txt");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(m.tool("c1").unwrap().name, "write");
    let done = m.update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "Created file"}}]}),
    );
    assert_eq!(
        done,
        vec![SessionUpdateKind::ToolCallUpdate {
            id: "c1".into(),
            status: ToolStatus::Completed,
            output_delta: Some("Created file".into()),
            result: None
        }]
    );

    m.update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "c2", "title": "write", "kind": "other",
        "status": "in_progress", "rawInput": {"file_path": "/w/b.txt"}}),
    );
    m.declined("c2");
    let refused = m.update(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "c2", "status": "failed",
        "content": [{"type": "content", "content": {"type": "text", "text": "Error: the user rejected escalating"}}]}));
    assert!(matches!(
        refused[0],
        SessionUpdateKind::ToolCallUpdate {
            status: ToolStatus::Declined,
            ..
        }
    ));

    let first = m.update(
        &json!({"sessionUpdate": "agent_message_chunk", "messageId": "m9",
        "content": {"type": "text", "text": "已创建 a.txt，"}}),
    );
    assert_eq!(types(&first), ["messageChunk"]);
    m.update(
        &json!({"sessionUpdate": "agent_message_chunk", "messageId": "m9",
        "content": {"type": "text", "text": "b.txt 被拒绝。"}}),
    );
    assert_eq!(
        m.finish(),
        vec![SessionUpdateKind::MessageCompleted {
            message_id: "m9".into(),
            text: Some("已创建 a.txt，b.txt 被拒绝。".into())
        }]
    );
    assert!(m.finish().is_empty());
}

#[test]
fn a_tool_call_or_a_new_message_ends_the_streamed_one() {
    let mut m = AcpMapper::new(&DSH);
    m.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "先看看"}}));
    let call = m.update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "bash",
        "rawInput": {"command": "ls -la\npwd"}}),
    );
    assert_eq!(types(&call), ["messageCompleted", "toolCall"]);
    assert!(
        matches!(&call[1], SessionUpdateKind::ToolCall { kind: ToolKind::Command, title, .. } if title == "ls -la")
    );
    let a = m.update(&json!({"sessionUpdate": "agent_message_chunk", "messageId": "a", "content": {"type": "text", "text": "x"}}));
    let b = m.update(&json!({"sessionUpdate": "agent_message_chunk", "messageId": "b", "content": {"type": "text", "text": "y"}}));
    assert_eq!(types(&a), ["messageChunk"]);
    assert_eq!(types(&b), ["messageCompleted", "messageChunk"]);
}

#[test]
fn resent_tool_content_only_adds_what_is_new() {
    let mut m = AcpMapper::new(&DSH);
    m.update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "c", "title": "bash", "rawInput": {}}),
    );
    let line = |text: &str| {
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "c",
        "content": [{"type": "content", "content": {"type": "text", "text": text}}]})
    };
    let delta = |u: Vec<SessionUpdateKind>| match &u[0] {
        SessionUpdateKind::ToolCallUpdate {
            output_delta,
            status,
            ..
        } => {
            assert_eq!(*status, ToolStatus::Running);
            output_delta.clone()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(delta(m.update(&line("a"))).as_deref(), Some("a"));
    assert_eq!(delta(m.update(&line("a\nb"))).as_deref(), Some("\nb"));
    assert_eq!(delta(m.update(&line("a\nb"))), None);
    let diff = m.update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "d", "status": "completed",
        "content": [{"type": "diff", "path": "/w/x.rs", "oldText": "a", "newText": "b"}]}),
    );
    assert_eq!(delta_of(&diff), Some("改动 /w/x.rs".into()));
}

fn delta_of(u: &[SessionUpdateKind]) -> Option<String> {
    match &u[0] {
        SessionUpdateKind::ToolCallUpdate { output_delta, .. } => output_delta.clone(),
        _ => None,
    }
}

#[test]
fn plans_map_and_the_unknown_is_kept() {
    let mut m = AcpMapper::new(&DSH);
    let plan = m.update(&json!({"sessionUpdate": "plan", "entries": [
        {"content": "读代码", "priority": "high", "status": "completed"},
        {"content": "改代码", "priority": "high", "status": "in_progress"}]}));
    assert_eq!(
        plan,
        vec![SessionUpdateKind::Plan {
            entries: vec![
                PlanEntry {
                    text: "读代码".into(),
                    status: "completed".into()
                },
                PlanEntry {
                    text: "改代码".into(),
                    status: "inProgress".into()
                },
            ]
        }]
    );
    assert!(m
        .update(&json!({"sessionUpdate": "available_commands_update", "availableCommands": []}))
        .is_empty());
    assert!(m
        .update(&json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "hi"}}))
        .is_empty());
    let unknown = m.update(&json!({"sessionUpdate": "brand_new", "x": 1}));
    assert!(
        matches!(&unknown[0], SessionUpdateKind::Unknown { kind, .. } if kind == "acp/brand_new")
    );
}
