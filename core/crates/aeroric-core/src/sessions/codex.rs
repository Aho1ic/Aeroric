//! Codex 落盘格式的逐行解析。迁移自宿主 `session/parse.rs` 的 Codex 半边，
//! 内容一行未改；附件抽取共用 [`crate::sessions::parser::attachment_from_value`]。
//!
//! Codex 的两条聚合规则随原文保留：assistant 侧的连续块并入上一条
//! （[`append_assistant_content`]），user 侧重复文本去重并入
//! （[`append_codex_user_message`]）——它们正是
//! `parse_session_messages_preserves_codex_tool_output_for_handoff` 依赖的行为。

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
                    let output = json_value_to_display(payload.and_then(|item| {
                        item.get("output")
                            .or_else(|| item.get("result"))
                            .or_else(|| item.get("content"))
                    }));
                    if !output.trim().is_empty() {
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
