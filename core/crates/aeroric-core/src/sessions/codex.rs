//! Codex 落盘格式的逐行解析。迁移自宿主 `session/parse.rs` 的 Codex 半边，
//! 内容一行未改；附件抽取共用 [`crate::sessions::parser::attachment_from_value`]。
//!
//! Codex 的两条聚合规则随原文保留：assistant 侧的连续块并入上一条
//! （[`append_assistant_content`]），user 侧重复文本去重并入
//! （[`append_codex_user_message`]）——它们正是
//! `parse_session_messages_preserves_codex_tool_output_for_handoff` 依赖的行为。
//!
//! 2026-11-08 起多了一处：Codex 原生多代理的调用显示成子代理卡（见
//! [`multi_agent_label`]）。回合结束后界面用这份历史替换实时内容，不在这里
//! 认出来，子代理就只剩加密参数的原始调用。

use serde_json::Value;

use crate::sessions::live::codex::map::{looks_encrypted, multi_agent_action, subagent_name};
use crate::sessions::parser::attachment_from_value;
use crate::sessions::SessionContent;
use crate::sessions::SessionMessage;

pub fn parse_codex_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages: Vec<SessionMessage> = Vec::new();

    for line in lines {
        parse_codex_session_line(line, &mut messages);
    }

    messages
}

pub fn parse_codex_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let payload = val.get("payload");

    match event_type {
        "event_msg" => {
            let payload_type = payload
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if payload_type == "user_message" {
                let text = payload
                    .and_then(|p| p.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    append_codex_user_message(
                        messages,
                        vec![SessionContent::Text {
                            text: text.to_string(),
                        }],
                    );
                }
            } else if payload_type == "item_completed" {
                if let Some(item) = payload.and_then(|p| p.get("item")) {
                    note_subagent_item(item, messages);
                }
            }
        }
        "response_item" => {
            let payload_type = payload
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match payload_type {
                "message" => {
                    let role = payload
                        .and_then(|p| p.get("role"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !matches!(role, "assistant" | "user") {
                        return;
                    }
                    let parts: Vec<SessionContent> = payload
                        .and_then(|p| p.get("content"))
                        .and_then(|v| v.as_array())
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(|b| {
                                    let t = b.get("type").and_then(|v| v.as_str())?;
                                    if matches!(t, "output_text" | "input_text" | "text") {
                                        let text = b.get("text").and_then(|v| v.as_str())?;
                                        // AGENTS.md / environment context arrive as the
                                        // first "user" message; they are not the prompt.
                                        if role == "user" && is_injected_context(text) {
                                            return None;
                                        }
                                        if !text.trim().is_empty() {
                                            return Some(SessionContent::Text {
                                                text: text.to_string(),
                                            });
                                        }
                                    }
                                    if matches!(t, "input_image" | "image" | "attachment") {
                                        return attachment_from_value(b);
                                    }
                                    None
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if !parts.is_empty() {
                        if role == "assistant" {
                            if let Some(last) =
                                messages.last_mut().filter(|m| m.role == "assistant")
                            {
                                last.content.extend(parts);
                            } else {
                                messages.push(SessionMessage {
                                    role: "assistant".to_string(),
                                    content: parts,
                                    message_id: None,
                                });
                            }
                        } else {
                            append_codex_user_message(messages, parts);
                        }
                    }
                }
                "reasoning" => {
                    let thinking = payload
                        .and_then(|item| item.get("summary").or_else(|| item.get("content")))
                        .map(|summary| match summary {
                            serde_json::Value::String(text) => text.clone(),
                            serde_json::Value::Array(items) => items
                                .iter()
                                .filter_map(|item| {
                                    item.get("text").and_then(serde_json::Value::as_str)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            other => serde_json::to_string_pretty(other).unwrap_or_default(),
                        })
                        .unwrap_or_default();
                    if !thinking.trim().is_empty() {
                        append_assistant_content(messages, SessionContent::Thinking { thinking });
                    }
                }
                "agent_message" => {
                    if let Some(payload) = payload.filter(|p| p.get("author").is_some()) {
                        return note_subagent_message(payload, messages);
                    }
                    let text = payload
                        .and_then(|item| item.get("message").or_else(|| item.get("text")))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if !text.trim().is_empty() {
                        append_assistant_content(
                            messages,
                            SessionContent::Text {
                                text: text.to_string(),
                            },
                        );
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let call_id = payload
                        .and_then(|p| p.get("call_id").or_else(|| p.get("id")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = payload
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(if payload_type == "custom_tool_call" {
                            "custom_tool"
                        } else {
                            "tool"
                        })
                        .to_string();
                    let raw_value = payload.and_then(|p| {
                        p.get("arguments")
                            .or_else(|| p.get("input"))
                            .or_else(|| p.get("payload"))
                    });
                    let namespace = payload
                        .and_then(|p| p.get("namespace"))
                        .and_then(|v| v.as_str());
                    if let Some(label) = multi_agent_label(&name, namespace) {
                        let input = subagent_call_input(&name, label, raw_value);
                        return append_assistant_content(
                            messages,
                            SessionContent::ToolUse {
                                id: call_id,
                                name: SUBAGENT_TOOL.to_string(),
                                input,
                            },
                        );
                    }
                    let input = match raw_value {
                        Some(serde_json::Value::String(raw)) => {
                            serde_json::from_str::<serde_json::Value>(raw)
                                .ok()
                                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                                .unwrap_or_else(|| raw.clone())
                        }
                        value => json_value_to_display(value),
                    };
                    append_assistant_content(
                        messages,
                        SessionContent::ToolUse {
                            id: call_id,
                            name,
                            input,
                        },
                    );
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let id = payload
                        .and_then(|item| item.get("call_id").or_else(|| item.get("id")))
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let raw = payload.and_then(|item| {
                        item.get("output")
                            .or_else(|| item.get("result"))
                            .or_else(|| item.get("content"))
                    });
                    let output = spawn_started(messages, &id, raw)
                        .unwrap_or_else(|| json_value_to_display(raw));
                    // An output record means the call finished, even an empty
                    // one (a delivered `send_message`); only an anonymous
                    // empty one says nothing.
                    if !id.is_empty() || !output.trim().is_empty() {
                        messages.push(SessionMessage {
                            role: "user".to_string(),
                            content: vec![SessionContent::ToolResult { id, output }],
                            message_id: None,
                        });
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// 2026-11-08: Codex's own multi-agent tools. The rollout keeps them as plain
// function calls (v1 namespace `multi_agent_v1`, v2 `collaboration`, a v2
// spawn's message encrypted); the history shows each as a subagent card and
// the spawn's card collects the child's start, end and final report.

/// The card's tool name: Claude's subagent tool is `Agent`, so both
/// harnesses' subagents read (and are iconed) alike.
const SUBAGENT_TOOL: &str = "Agent";

/// What a multi-agent call does, if it is one.
fn multi_agent_label(name: &str, namespace: Option<&str>) -> Option<&'static str> {
    let ours = matches!(
        namespace,
        Some("collaboration" | "multi_agent_v1" | "multi_agent_v2")
    );
    // Names this common are someone else's tool unless namespaced as ours.
    if !ours && matches!(name, "wait" | "send_message" | "sendMessage") {
        return None;
    }
    multi_agent_action(name)
}

type Fields = serde_json::Map<String, Value>;

fn subagent_call_input(tool: &str, label: &str, raw: Option<&Value>) -> String {
    let parsed = match raw {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        other => other.cloned(),
    };
    let mut fields = match parsed {
        Some(Value::Object(fields)) => fields,
        _ => Fields::new(),
    };
    if fields
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(looks_encrypted)
    {
        fields.remove("message");
    }
    fields.insert("tool".into(), Value::from(tool));
    fields.insert("description".into(), Value::from(label));
    if let Some(name) = fields
        .get("task_name")
        .and_then(Value::as_str)
        .and_then(subagent_name)
    {
        name_spawn(&mut fields, &name);
    }
    serde_json::to_string_pretty(&Value::Object(fields)).unwrap_or_default()
}

fn name_spawn(fields: &mut Fields, name: &str) {
    fields.insert("name".into(), Value::from(name));
    fields.insert("description".into(), Value::from(format!("子代理 {name}")));
}

/// The latest spawn card `matches` picks: `edit` its arguments; its id and title.
fn edit_spawn(
    messages: &mut [SessionMessage],
    matches: impl Fn(&str, &Fields) -> bool,
    edit: impl FnOnce(&mut Fields),
) -> Option<(String, String)> {
    for message in messages.iter_mut().rev() {
        for part in message.content.iter_mut().rev() {
            let SessionContent::ToolUse { id, name, input } = part else {
                continue;
            };
            if name != SUBAGENT_TOOL {
                continue;
            }
            let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(input) else {
                continue;
            };
            if fields.get("tool").and_then(Value::as_str) != Some("spawn_agent")
                || !matches(id, &fields)
            {
                continue;
            }
            edit(&mut fields);
            let title = fields
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("子代理")
                .to_string();
            *input = serde_json::to_string_pretty(&Value::Object(fields)).unwrap_or_default();
            return Some((id.clone(), title));
        }
    }
    None
}

fn append_to_result(messages: &mut [SessionMessage], id: &str, line: &str) {
    for message in messages.iter_mut().rev() {
        for part in &mut message.content {
            if let SessionContent::ToolResult { id: result, output } = part {
                if result == id {
                    output.push('\n');
                    output.push_str(line);
                    return;
                }
            }
        }
    }
}

/// A spawn's output (v2 `{"task_name"}`, v1 `{"agent_id","nickname"}`) names
/// the child; its card's result starts with that it started.
fn spawn_started(messages: &mut [SessionMessage], id: &str, raw: Option<&Value>) -> Option<String> {
    let parsed = match raw? {
        Value::String(text) => serde_json::from_str::<Value>(text).ok()?,
        other => other.clone(),
    };
    let path = parsed.get("task_name").and_then(Value::as_str);
    let name = path.and_then(subagent_name).or_else(|| {
        parsed
            .get("nickname")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if path.is_none() && parsed.get("agent_id").is_none() {
        return None;
    }
    let (_, title) = edit_spawn(
        messages,
        |card, _| card == id,
        |fields| {
            if let Some(path) = path {
                fields.insert("agent".into(), Value::from(path));
            }
            if let Some(thread) = parsed.get("agent_id").and_then(Value::as_str) {
                fields.insert("thread".into(), Value::from(thread));
            }
            if let Some(name) = &name {
                name_spawn(fields, name);
            }
        },
    )?;
    Some(format!("{title} 开始"))
}

/// v2 `SubAgentActivity` (the child's thread and path, its end) and v1's
/// spawn `CollabAgentToolCall` (its thread) as the rollout records them.
fn note_subagent_item(item: &Value, messages: &mut [SessionMessage]) {
    let field = |key: &str| item.get(key).and_then(Value::as_str);
    match (field("type"), field("kind")) {
        (Some("SubAgentActivity"), Some("started")) => {
            let (Some(card), Some(thread)) = (field("id"), field("agent_thread_id")) else {
                return;
            };
            edit_spawn(
                messages,
                |id, _| id == card,
                |fields| {
                    fields.insert("thread".into(), Value::from(thread));
                    if let Some(path) = field("agent_path") {
                        fields.insert("agent".into(), Value::from(path));
                    }
                },
            );
        }
        (Some("SubAgentActivity"), Some(kind @ ("completed" | "interrupted"))) => {
            let Some(thread) = field("agent_thread_id") else {
                return;
            };
            let is_child = |_: &str, fields: &Fields| {
                fields.get("thread").and_then(Value::as_str) == Some(thread)
            };
            if let Some((card, title)) = edit_spawn(messages, is_child, |_| {}) {
                let word = if kind == "completed" {
                    "完成"
                } else {
                    "中断"
                };
                append_to_result(messages, &card, &format!("{title} {word}"));
            }
        }
        (Some("CollabAgentToolCall"), _) if field("tool") == Some("spawn_agent") => {
            let thread = item
                .get("receiver_thread_ids")
                .and_then(Value::as_array)
                .and_then(|ids| ids.first())
                .and_then(Value::as_str);
            if let (Some(card), Some(thread)) = (field("id"), thread) {
                edit_spawn(
                    messages,
                    |id, _| id == card,
                    |fields| {
                        fields.insert("thread".into(), Value::from(thread));
                    },
                );
            }
        }
        _ => {}
    }
}

/// A v2 child's message to its parent ("Message Type: FINAL_ANSWER … Payload:
/// …") is the report line on its spawn card. Messages between agents are not
/// the conversation, so one without a card is not shown.
fn note_subagent_message(payload: &Value, messages: &mut [SessionMessage]) {
    let Some(author) = payload.get("author").and_then(Value::as_str) else {
        return;
    };
    let text = payload
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let Some((head, body)) = text.split_once("Payload:") else {
        return;
    };
    let body = body.trim();
    if body.is_empty() {
        return;
    }
    let from_child =
        |_: &str, fields: &Fields| fields.get("agent").and_then(Value::as_str) == Some(author);
    if let Some((card, _)) = edit_spawn(messages, from_child, |_| {}) {
        let word = if head.contains("FINAL_ANSWER") {
            "回报"
        } else {
            "消息"
        };
        append_to_result(messages, &card, &format!("{word}：{body}"));
    }
}

pub fn append_assistant_content(messages: &mut Vec<SessionMessage>, part: SessionContent) {
    if let Some(last) = messages
        .last_mut()
        .filter(|message| message.role == "assistant")
    {
        last.content.push(part);
    } else {
        messages.push(SessionMessage {
            role: "assistant".to_string(),
            content: vec![part],
            message_id: None,
        });
    }
}

/// Text the agent injects as a "user" message rather than the user typing it:
/// wrapped blocks (`<environment_context>`, command wrappers) and Codex's
/// `# AGENTS.md instructions for …` project instructions.
pub fn is_injected_context(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with('<') || text.starts_with("# AGENTS.md instructions")
}

pub fn append_codex_user_message(
    messages: &mut Vec<SessionMessage>,
    mut content: Vec<SessionContent>,
) {
    let duplicate_text = messages
        .last()
        .filter(|message| message.role == "user")
        .map(|message| {
            content.iter().any(|candidate| {
                let SessionContent::Text {
                    text: candidate_text,
                } = candidate
                else {
                    return false;
                };
                message.content.iter().any(
                    |existing| matches!(existing, SessionContent::Text { text } if text == candidate_text),
                )
            })
        })
        .unwrap_or(false);

    if duplicate_text {
        if let Some(last) = messages.last_mut() {
            content.retain(|candidate| match candidate {
                SessionContent::Text { text: candidate } => !last
                    .content
                    .iter()
                    .any(|existing| matches!(existing, SessionContent::Text { text } if text == candidate)),
                _ => true,
            });
            last.content.extend(content);
        }
        return;
    }
    messages.push(SessionMessage {
        role: "user".to_string(),
        content,
        message_id: None,
    });
}

pub fn json_value_to_display(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
        None => String::new(),
    }
}
