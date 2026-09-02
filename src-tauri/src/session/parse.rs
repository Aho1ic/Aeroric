//! 会话 JSONL 的解析:一行文本 → `SessionMessage`。
//!
//! 从 `session.rs` 整块搬出来,内容一行没改。两家 agent 的落盘格式不同,
//! 所以有两套平行的解析器(`parse_claude_session_line` / `parse_codex_session_line`),
//! 由 `parse_session_lines` 按 `is_codex` 分派。
//!
//! 全是纯函数,不读盘 —— 调用方负责把文件读成 `&[String]` 再递进来。
//! `SessionMessage` / `SessionContent` 两个类型留在父模块,靠 `use super::*;` 取。

use super::*;

pub(crate) fn parse_session_lines(
    lines: &[String],
    is_codex: bool,
    messages: &mut Vec<SessionMessage>,
) {
    for line in lines {
        parse_session_line(line, is_codex, messages);
    }
}

pub(super) fn parse_session_line(line: &str, is_codex: bool, messages: &mut Vec<SessionMessage>) {
    if is_codex {
        parse_codex_session_line(line, messages);
    } else {
        parse_claude_session_line(line, messages);
    }
}

pub(super) fn parse_claude_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages = Vec::new();

    for line in lines {
        parse_claude_session_line(line, &mut messages);
    }

    messages
}

pub(super) fn parse_claude_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if msg_type == "attachment" {
        if let Some(content) = attachment_from_value(
            val.get("attachment")
                .or_else(|| val.get("payload"))
                .unwrap_or(&val),
        ) {
            messages.push(SessionMessage {
                role: "user".to_string(),
                content: vec![content],
                message_id: None,
            });
        }
        return;
    }
    let Some(message) = val.get("message") else {
        return;
    };

    match msg_type {
        "user" => {
            let parts = claude_user_content(message.get("content"));
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "user".to_string(),
                    content: parts,
                    message_id: None,
                });
            }
        }
        "assistant" => {
            let parts = message
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| claude_assistant_blocks(arr))
                .unwrap_or_default();
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "assistant".to_string(),
                    content: parts,
                    message_id: None,
                });
            }
        }
        _ => {}
    }
}

pub(super) fn claude_user_content(content: Option<&serde_json::Value>) -> Vec<SessionContent> {
    match content {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
            vec![SessionContent::Text { text: s.clone() }]
        }
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(
                |block| match block.get("type").and_then(|value| value.as_str()) {
                    Some("text") => {
                        let text = block
                            .get("text")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        (!text.trim().is_empty()).then(|| SessionContent::Text {
                            text: text.to_string(),
                        })
                    }
                    Some("tool_result") => {
                        let id = block
                            .get("tool_use_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string();
                        let output = match block.get("content") {
                            Some(serde_json::Value::String(text)) => text.clone(),
                            Some(serde_json::Value::Array(items)) => items
                                .iter()
                                .filter_map(|item| {
                                    item.get("text")
                                        .and_then(|value| value.as_str())
                                        .map(ToOwned::to_owned)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
                            None => String::new(),
                        };
                        (!output.trim().is_empty())
                            .then_some(SessionContent::ToolResult { id, output })
                    }
                    Some("image") | Some("document") | Some("attachment") => {
                        attachment_from_value(block)
                    }
                    _ => None,
                },
            )
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn claude_assistant_blocks(blocks: &[serde_json::Value]) -> Vec<SessionContent> {
    let mut parts = Vec::new();
    for block in blocks {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        parts.push(SessionContent::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block
                    .get("input")
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_default();
                parts.push(SessionContent::ToolUse { id, name, input });
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|v| v.as_str()) {
                    if !thinking.trim().is_empty() {
                        parts.push(SessionContent::Thinking {
                            thinking: thinking.to_string(),
                        });
                    }
                }
            }
            Some("image") | Some("document") | Some("attachment") => {
                if let Some(attachment) = attachment_from_value(block) {
                    parts.push(attachment);
                }
            }
            _ => {}
        }
    }
    parts
}

pub(super) fn json_value_to_display(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
        None => String::new(),
    }
}

pub(super) fn attachment_from_value(value: &serde_json::Value) -> Option<SessionContent> {
    let source_value = value.get("source").unwrap_or(value);
    let media_type = value
        .get("media_type")
        .or_else(|| value.get("mediaType"))
        .or_else(|| source_value.get("media_type"))
        .or_else(|| source_value.get("mediaType"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let name = value
        .get("name")
        .or_else(|| value.get("file_name"))
        .or_else(|| value.get("filename"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("attachment")
        .to_string();
    let source = source_value
        .get("url")
        .or_else(|| source_value.get("image_url"))
        .or_else(|| source_value.get("path"))
        .or_else(|| source_value.get("file_path"))
        .or_else(|| source_value.get("filePath"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            source_value
                .get("data")
                .and_then(serde_json::Value::as_str)
                .map(|data| format!("data:{media_type};base64,{data}"))
        })?;
    Some(SessionContent::Attachment {
        name,
        media_type,
        source,
    })
}

pub(super) fn append_assistant_content(messages: &mut Vec<SessionMessage>, part: SessionContent) {
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

pub(super) fn append_codex_user_message(
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

pub(super) fn parse_codex_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages: Vec<SessionMessage> = Vec::new();

    for line in lines {
        parse_codex_session_line(line, &mut messages);
    }

    messages
}

pub(super) fn parse_codex_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
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
