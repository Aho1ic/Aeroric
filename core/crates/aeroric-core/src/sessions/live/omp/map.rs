//! omp RPC events → session updates (pure). The driver handles the frames
//! that change its own state (`prompt_result`, `extension_ui_request`,
//! `ready`) before calling [`OmpMapper::map`]; everything else lands here.
//! Frames that only drive omp's own terminal (status lines, widgets, command
//! lists, cache warming) are dropped on purpose; anything not listed is kept
//! as `Unknown` (INV-12).

use std::collections::HashMap;

use serde_json::Value;

use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, ToolKind, ToolStatus};

/// What one frame became.
#[derive(Debug, PartialEq)]
pub enum Mapped {
    Updates(Vec<SessionUpdateKind>),
    /// Terminal decoration or bookkeeping: nothing to show.
    Ignored,
}

/// Frames that carry nothing for the session page (Q10 exception, user-approved).
const IGNORED: &[&str] = &[
    "agent_start",
    "agent_end",
    "turn_start",
    "turn_end",
    "message_start",
    "tool_execution_update",
    "tool_stream_update",
    "queue_update",
    "available_commands_update",
    "session_settled",
    "cache_warming_start",
    "cache_warming_end",
    "todo_reminder",
    "todo_auto_clear",
    "ttsr_triggered",
    "goal_updated",
    "irc_message",
    "subagent_event",
    "btw_delta",
    "btw_record",
    "live_phase",
    "live_levels",
    "live_transcript",
    "live_end",
    "ready",
];

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn tool_kind(name: &str) -> ToolKind {
    match name {
        "bash" | "eval" => ToolKind::Command,
        "edit" | "write" | "ast_edit" | "apply_patch" | "multi_edit" | "notebook_edit" => {
            ToolKind::FileChange
        }
        "task" => ToolKind::Subagent,
        "web_search" | "fetch" | "browser" => ToolKind::WebSearch,
        name if name.starts_with("mcp__") => ToolKind::Mcp,
        _ => ToolKind::Other,
    }
}

/// A one-line card title: the command, the path, else the tool name.
pub fn tool_title(name: &str, args: &Value) -> String {
    let field = [
        "command",
        "path",
        "file_path",
        "pattern",
        "query",
        "url",
        "description",
    ]
    .iter()
    .find_map(|key| s(args, key).filter(|text| !text.trim().is_empty()));
    match field {
        Some(text) => text.lines().next().unwrap_or(text).to_string(),
        None => name.to_string(),
    }
}

/// omp todo phases → plan entries (`in_progress` → `inProgress`, the agent's
/// other states kept as they are).
pub fn plan_from_todos(phases: &Value) -> Vec<PlanEntry> {
    phases
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|phase| {
            phase
                .get("tasks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|task| {
            let text = s(&task, "content")?.to_string();
            let status = match s(&task, "status").unwrap_or("pending") {
                "in_progress" => "inProgress",
                other => other,
            }
            .to_string();
            Some(PlanEntry { text, status })
        })
        .collect()
}

#[derive(Default)]
pub struct OmpMapper {
    /// Text streamed so far per message (for `MessageCompleted`).
    text: HashMap<String, String>,
}

impl OmpMapper {
    pub fn map(&mut self, frame: &Value) -> Mapped {
        use SessionUpdateKind as K;
        let kind = s(frame, "type").unwrap_or("");
        // `advisor_*`: the advisor runtime's own bookkeeping (cost, notes state).
        if IGNORED.contains(&kind) || kind.starts_with("advisor_") {
            return Mapped::Ignored;
        }
        let updates = match kind {
            "message_update" => {
                let id = s(frame, "messageId").unwrap_or("message").to_string();
                let event = frame
                    .get("assistantMessageEvent")
                    .cloned()
                    .unwrap_or(Value::Null);
                let delta = s(&event, "delta").unwrap_or("").to_string();
                match s(&event, "type") {
                    Some("text_delta") if !delta.is_empty() => {
                        self.text.entry(id.clone()).or_default().push_str(&delta);
                        vec![K::MessageChunk {
                            message_id: id,
                            role: "assistant".into(),
                            text: delta,
                        }]
                    }
                    Some("thinking_delta") if !delta.is_empty() => vec![K::ThoughtChunk {
                        id: format!("{id}-thinking"),
                        text: delta,
                        summary: false,
                    }],
                    _ => return Mapped::Ignored,
                }
            }
            "message_end" => {
                let message = frame.get("message").cloned().unwrap_or(Value::Null);
                if s(&message, "role") != Some("assistant") {
                    return Mapped::Ignored;
                }
                let id = s(frame, "messageId").unwrap_or("message").to_string();
                let streamed = self.text.remove(&id);
                let text: String = message
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|block| s(block, "type") == Some("text"))
                    .filter_map(|block| s(block, "text"))
                    .collect();
                let mut updates = vec![K::MessageCompleted {
                    message_id: id,
                    text: (!text.is_empty()).then_some(text).or(streamed),
                }];
                if let Some(usage) = message.get("usage") {
                    let input = usage.get("input").and_then(Value::as_u64);
                    let output = usage.get("output").and_then(Value::as_u64);
                    if input.unwrap_or(0) + output.unwrap_or(0) > 0 {
                        updates.push(K::Usage {
                            context_used: None,
                            context_limit: None,
                            input_tokens: input,
                            output_tokens: output,
                            cached_input_tokens: None,
                        });
                    }
                }
                updates
            }
            "tool_execution_start" => {
                let name = s(frame, "toolName").unwrap_or("tool");
                let args = frame.get("args").cloned().unwrap_or(Value::Null);
                vec![K::ToolCall {
                    id: s(frame, "toolCallId").unwrap_or_default().to_string(),
                    kind: tool_kind(name),
                    title: tool_title(name, &args),
                    input: args,
                }]
            }
            "tool_execution_end" => vec![K::ToolCallUpdate {
                id: s(frame, "toolCallId").unwrap_or_default().to_string(),
                status: if frame.get("isError").and_then(Value::as_bool) == Some(true) {
                    ToolStatus::Failed
                } else {
                    ToolStatus::Completed
                },
                output_delta: None,
                result: frame.get("result").cloned(),
            }],
            "subagent_lifecycle" | "subagent_progress" => {
                let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
                let progress = payload.get("progress").cloned().unwrap_or(Value::Null);
                let id = s(&payload, "id")
                    .or_else(|| s(&progress, "id"))
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "subagent-{}",
                            payload.get("index").and_then(Value::as_u64).unwrap_or(0)
                        )
                    });
                let agent = s(&payload, "agent").unwrap_or("子 Agent");
                let what = s(&payload, "description")
                    .or_else(|| s(&progress, "description"))
                    .or_else(|| s(&payload, "task"));
                let status = match s(&payload, "status").or_else(|| s(&progress, "status")) {
                    Some("started" | "running" | "pending") | None => "running",
                    Some("completed") => "completed",
                    Some("failed") => "failed",
                    Some("aborted") => "cancelled",
                    Some(other) => other,
                };
                let detail = s(&progress, "currentToolIntent")
                    .or_else(|| s(&progress, "lastIntent"))
                    .map(str::to_string);
                vec![K::Subagent {
                    id,
                    title: match what {
                        Some(what) => format!("{agent} · {}", what.lines().next().unwrap_or(what)),
                        None => agent.to_string(),
                    },
                    status: status.to_string(),
                    detail,
                }]
            }
            "notice" => vec![K::Notice {
                level: s(frame, "level").unwrap_or("info").to_string(),
                message: s(frame, "message").unwrap_or_default().to_string(),
            }],
            "auto_compaction_start" => vec![notice("info", "正在压缩上下文…")],
            "auto_compaction_end" => vec![notice("info", "上下文已自动压缩")],
            "auto_retry_start" => vec![notice("warning", "请求失败，omp 正在自动重试…")],
            "auto_retry_end" => return Mapped::Ignored,
            "extension_error" => vec![notice(
                "warning",
                &format!("omp 插件出错：{}", s(frame, "error").unwrap_or("未知错误")),
            )],
            "model_changed" => vec![K::ConfigChanged {
                model: frame
                    .get("model")
                    .and_then(|model| s(model, "id").or_else(|| model.as_str()))
                    .map(str::to_string),
                permission: None,
                effort: None,
            }],
            "thinking_level_changed" => vec![K::ConfigChanged {
                model: None,
                permission: None,
                effort: s(frame, "level").map(str::to_string),
            }],
            _ => vec![K::Unknown {
                kind: format!("omp/{kind}"),
                payload: frame.clone(),
            }],
        };
        Mapped::Updates(updates)
    }
}

fn notice(level: &str, message: &str) -> SessionUpdateKind {
    SessionUpdateKind::Notice {
        level: level.into(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
