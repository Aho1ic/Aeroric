//! Claude 落盘格式的逐行解析。迁移自宿主 `session/parse.rs` 的 Claude 半边，
//! 内容一行未改；附件抽取共用 [`crate::sessions::parser::attachment_from_value`]。

use crate::sessions::parser::attachment_from_value;
use crate::sessions::SessionContent;
use crate::sessions::SessionMessage;

pub fn parse_claude_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages = Vec::new();

    for line in lines {
        parse_claude_session_line(line, &mut messages);
    }

    messages
}

pub fn parse_claude_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
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

pub fn claude_user_content(content: Option<&serde_json::Value>) -> Vec<SessionContent> {
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

pub fn claude_assistant_blocks(blocks: &[serde_json::Value]) -> Vec<SessionContent> {
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
