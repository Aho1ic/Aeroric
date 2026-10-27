//! Claude Code stream-json → [`SessionUpdateKind`] (ADR-043, N5). Message
//! shapes follow the Claude Agent SDK's parser. The mapper keeps the little
//! state the stream needs (which message is streaming, which tools are which,
//! running token totals); everything else is a pure function of the message.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, ToolKind, ToolStatus};
use crate::sessions::live::Decision;

/// What a message means for the session, besides its updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeSignal {
    /// `system/init`: the session id (also after `--resume`) and its settings.
    SessionStarted {
        session_id: String,
        model: Option<String>,
    },
    /// `result`: one turn is over.
    TurnResult { failed: bool, error: Option<String> },
}

#[derive(Default)]
pub struct ClaudeMapper {
    streaming: Option<String>,
    streamed_text: HashSet<String>,
    streamed_thought: HashSet<String>,
    completed_text: HashMap<String, String>,
    todo_tools: HashSet<String>,
    input_tokens: u64,
    output_tokens: u64,
    /// Prompt plus reply of the latest API call: what the window holds now.
    /// Fallback for when `get_context_usage` answers nothing (gateways).
    last_context: Option<u64>,
}

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn unknown(kind: &str, payload: &Value) -> SessionUpdateKind {
    SessionUpdateKind::Unknown {
        kind: kind.to_string(),
        payload: payload.clone(),
    }
}

/// Claude tool → card kind.
pub fn tool_kind(name: &str) -> ToolKind {
    match name {
        "Bash" | "BashOutput" | "KillShell" | "KillBash" => ToolKind::Command,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ToolKind::FileChange,
        "WebSearch" | "WebFetch" => ToolKind::WebSearch,
        "Task" | "Agent" => ToolKind::Subagent,
        name if name.starts_with("mcp__") => ToolKind::Mcp,
        _ => ToolKind::Other,
    }
}

/// The most telling argument of a tool call, for its one-line title.
pub fn tool_title(name: &str, input: &Value) -> String {
    let field = match name {
        "Bash" => "command",
        "Read" | "Edit" | "MultiEdit" | "Write" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        _ => "",
    };
    let detail = s(input, field)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default();
    if detail.is_empty() {
        name.to_string()
    } else if matches!(name, "Bash" | "Task" | "Agent") {
        detail.chars().take(160).collect()
    } else {
        format!("{name} {}", detail.chars().take(160).collect::<String>())
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| s(p, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn plan_from_todos(input: &Value) -> Vec<PlanEntry> {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .map(|todo| PlanEntry {
                    text: s(todo, "content").unwrap_or_default().to_string(),
                    status: match s(todo, "status") {
                        Some("in_progress") => "inProgress".into(),
                        Some(other) => other.into(),
                        None => "pending".into(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

impl ClaudeMapper {
    pub fn map(&mut self, message: &Value) -> (Vec<SessionUpdateKind>, Option<ClaudeSignal>) {
        // A subagent's own stream stays inside its card (its task_* events).
        if message
            .get("parent_tool_use_id")
            .is_some_and(|p| !p.is_null())
        {
            return (vec![], None);
        }
        match s(message, "type") {
            Some("system") => self.system(message),
            Some("stream_event") => (
                self.stream(message.get("event").unwrap_or(&Value::Null)),
                None,
            ),
            Some("assistant") => (self.assistant(message), None),
            Some("user") => (self.user(message), None),
            Some("result") => self.result(message),
            Some("rate_limit_event") | Some("keep_alive") => (vec![], None),
            Some("conversation_reset") => (
                vec![SessionUpdateKind::Error {
                    message: "会话已重置".into(),
                    recoverable: true,
                }],
                None,
            ),
            Some(kind) => (vec![unknown(kind, message)], None),
            None => (vec![unknown("message", message)], None),
        }
    }

    fn system(&mut self, message: &Value) -> (Vec<SessionUpdateKind>, Option<ClaudeSignal>) {
        use SessionUpdateKind as K;
        let task = |status: &str| K::Subagent {
            id: s(message, "task_id").unwrap_or_default().to_string(),
            title: s(message, "description")
                .or_else(|| s(message, "summary"))
                .unwrap_or("子 Agent")
                .to_string(),
            status: status.to_string(),
            detail: None,
        };
        match s(message, "subtype") {
            Some("init") => {
                let signal = s(message, "session_id").map(|id| ClaudeSignal::SessionStarted {
                    session_id: id.to_string(),
                    model: s(message, "model").map(str::to_string),
                });
                (vec![], signal)
            }
            Some("task_started") => (vec![task("running")], None),
            Some("task_progress") => (vec![task("running")], None),
            Some("task_notification") => (
                vec![task(s(message, "status").unwrap_or("completed"))],
                None,
            ),
            Some("task_updated") => {
                match message.pointer("/patch/status").and_then(Value::as_str) {
                    Some(status) => (vec![task(status)], None),
                    None => (vec![], None),
                }
            }
            Some("compact_boundary") => (
                vec![K::Error {
                    message: "上下文已压缩".into(),
                    recoverable: true,
                }],
                None,
            ),
            // `thinking_tokens` is a running estimate sent once per streamed
            // thinking token; the thinking itself arrives as thoughtChunk (N6a C3).
            Some("hook_started")
            | Some("hook_response")
            | Some("status")
            | Some("session_state_changed")
            | Some("thinking_tokens") => (vec![], None),
            Some(other) => (vec![unknown(&format!("system/{other}"), message)], None),
            None => (vec![unknown("system", message)], None),
        }
    }

    fn stream(&mut self, event: &Value) -> Vec<SessionUpdateKind> {
        use SessionUpdateKind as K;
        match s(event, "type") {
            Some("message_start") => {
                self.streaming = event
                    .pointer("/message/id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                vec![]
            }
            Some("content_block_delta") => {
                let Some(id) = self.streaming.clone() else {
                    return vec![];
                };
                let delta = event.get("delta").unwrap_or(&Value::Null);
                match s(delta, "type") {
                    Some("text_delta") => {
                        self.streamed_text.insert(id.clone());
                        vec![K::MessageChunk {
                            message_id: id,
                            role: "assistant".into(),
                            text: s(delta, "text").unwrap_or_default().to_string(),
                        }]
                    }
                    Some("thinking_delta") => {
                        self.streamed_thought.insert(id.clone());
                        vec![K::ThoughtChunk {
                            id: format!("{id}.thinking"),
                            text: s(delta, "thinking").unwrap_or_default().to_string(),
                            summary: false,
                        }]
                    }
                    _ => vec![],
                }
            }
            _ => vec![],
        }
    }

    fn assistant(&mut self, message: &Value) -> Vec<SessionUpdateKind> {
        use SessionUpdateKind as K;
        let id = message
            .pointer("/message/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let blocks = message
            .pointer("/message/content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(usage) = message.pointer("/message/usage") {
            let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            let total = count("input_tokens")
                + count("cache_read_input_tokens")
                + count("cache_creation_input_tokens")
                + count("output_tokens");
            if total > 0 {
                self.last_context = Some(total);
            }
        }
        let mut updates = Vec::new();
        for block in &blocks {
            match s(block, "type") {
                Some("text") => {
                    let text = s(block, "text").unwrap_or_default();
                    if !self.streamed_text.contains(&id) {
                        updates.push(K::MessageChunk {
                            message_id: id.clone(),
                            role: "assistant".into(),
                            text: text.into(),
                        });
                    }
                    let full = self.completed_text.entry(id.clone()).or_default();
                    full.push_str(text);
                    updates.push(K::MessageCompleted {
                        message_id: id.clone(),
                        text: Some(full.clone()),
                    });
                }
                Some("thinking") => {
                    if !self.streamed_thought.contains(&id) {
                        updates.push(K::ThoughtChunk {
                            id: format!("{id}.thinking"),
                            text: s(block, "thinking").unwrap_or_default().into(),
                            summary: false,
                        });
                    }
                }
                Some("redacted_thinking") => {}
                Some("tool_use") | Some("server_tool_use") => {
                    let tool_id = s(block, "id").unwrap_or_default().to_string();
                    let name = s(block, "name").unwrap_or_default();
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    if name == "TodoWrite" {
                        self.todo_tools.insert(tool_id);
                        updates.push(K::Plan {
                            entries: plan_from_todos(&input),
                        });
                    } else {
                        updates.push(K::ToolCall {
                            id: tool_id,
                            kind: tool_kind(name),
                            title: tool_title(name, &input),
                            input,
                        });
                    }
                }
                Some(kind) => updates.push(unknown(&format!("assistant/{kind}"), block)),
                None => {}
            }
        }
        updates
    }

    fn user(&mut self, message: &Value) -> Vec<SessionUpdateKind> {
        let blocks = message
            .pointer("/message/content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        blocks
            .iter()
            .filter(|block| s(block, "type") == Some("tool_result"))
            .filter_map(|block| {
                let id = s(block, "tool_use_id")?.to_string();
                if self.todo_tools.contains(&id) {
                    return None;
                }
                let failed = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Some(SessionUpdateKind::ToolCallUpdate {
                    id,
                    status: if failed {
                        ToolStatus::Failed
                    } else {
                        ToolStatus::Completed
                    },
                    output_delta: None,
                    result: Some(
                        json!({"output": text_of(block.get("content").unwrap_or(&Value::Null))}),
                    ),
                })
            })
            .collect()
    }

    fn result(&mut self, message: &Value) -> (Vec<SessionUpdateKind>, Option<ClaudeSignal>) {
        let usage = message.get("usage").unwrap_or(&Value::Null);
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        self.input_tokens += count("input_tokens")
            + count("cache_read_input_tokens")
            + count("cache_creation_input_tokens");
        self.output_tokens += count("output_tokens");
        let failed = message
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || s(message, "subtype").is_some_and(|subtype| subtype != "success");
        let error = failed.then(|| {
            message
                .get("errors")
                .and_then(Value::as_array)
                .map(|errors| {
                    errors
                        .iter()
                        .map(|e| {
                            e.as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| e.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("；")
                })
                .filter(|text| !text.is_empty())
                .or_else(|| s(message, "result").map(str::to_string))
                .unwrap_or_else(|| {
                    format!(
                        "Claude 这一轮失败了（{}）",
                        s(message, "subtype").unwrap_or("error")
                    )
                })
        });
        self.streaming = None;
        (
            vec![SessionUpdateKind::Usage {
                context_used: self.last_context,
                context_limit: None,
                input_tokens: Some(self.input_tokens),
                output_tokens: Some(self.output_tokens),
            }],
            Some(ClaudeSignal::TurnResult { failed, error }),
        )
    }
}

/// `can_use_tool` → the card the user answers. "Always allow in this session"
/// is offered only when Claude suggested rules to add (N5 D2).
pub fn map_permission_request(request_id: &str, request: &Value) -> SessionUpdateKind {
    let tool = s(request, "tool_name").unwrap_or("工具");
    let input = request.get("input").unwrap_or(&Value::Null);
    let summary = s(request, "title")
        .or_else(|| (tool == "Bash").then(|| s(input, "command")).flatten())
        .or_else(|| s(request, "description"))
        .map(str::to_string)
        .unwrap_or_else(|| tool_title(tool, input));
    let suggested = request
        .get("permission_suggestions")
        .and_then(Value::as_array)
        .is_some_and(|s| !s.is_empty());
    let mut options = vec!["allowOnce".to_string()];
    if suggested {
        options.push("allowForSession".into());
    }
    options.push("deny".into());
    SessionUpdateKind::PermissionRequest {
        id: request_id.to_string(),
        tool_call_id: s(request, "tool_use_id").map(str::to_string),
        kind: tool.to_string(),
        summary,
        options,
    }
}

/// The user's choice, as Claude's `can_use_tool` response.
pub fn permission_response(request: &Value, decision: &Decision) -> Value {
    let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
    match decision {
        Decision::AllowOnce => json!({"behavior": "allow", "updatedInput": input}),
        Decision::AllowForSession => {
            let updates: Vec<Value> = request
                .get("permission_suggestions")
                .and_then(Value::as_array)
                .map(|suggestions| {
                    suggestions
                        .iter()
                        .map(|suggestion| {
                            let mut suggestion = suggestion.clone();
                            if let Some(object) = suggestion.as_object_mut() {
                                object.insert("destination".into(), json!("session"));
                            }
                            suggestion
                        })
                        .collect()
                })
                .unwrap_or_default();
            if updates.is_empty() {
                json!({"behavior": "allow", "updatedInput": input})
            } else {
                json!({"behavior": "allow", "updatedInput": input, "updatedPermissions": updates})
            }
        }
        Decision::Deny { reason } => json!({
            "behavior": "deny",
            "message": reason.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "用户拒绝了这一步".into()),
        }),
    }
}

/// Aeroric permission tier → Claude permission mode (start flag and `set_permission_mode`).
pub fn claude_permission_mode(permission: &str) -> &'static str {
    match permission {
        "full_access" => "bypassPermissions",
        "auto_edit" => "acceptEdits",
        _ => "default",
    }
}

/// `claude --version` (`2.1.299 (Claude Code)`) → new enough for the structured driver?
pub fn version_supported(output: &str) -> Result<(), String> {
    const MINIMUM: (u64, u64) = (2, 1);
    let version = output
        .split_whitespace()
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("无法识别 Claude Code 版本：{}", output.trim()))?;
    let mut parts = version.split('.').map(|p| {
        p.trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<u64>()
            .unwrap_or(0)
    });
    let found = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    if found >= MINIMUM {
        Ok(())
    } else {
        Err(format!(
            "Claude Code {version} 太旧，原生会话需要 {}.{} 或更新版本，请升级 Claude Code",
            MINIMUM.0, MINIMUM.1
        ))
    }
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
