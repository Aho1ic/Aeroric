//! ACP `session/update` notifications → session updates. The driver keeps
//! what changes its own state (config options, the turn's end); everything
//! else lands here. Updates that only drive the agent's own UI are dropped on
//! purpose; anything unknown is kept as `Unknown` (INV-12).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::dialect::Dialect;
use crate::sessions::live::omp::map::tool_title;
use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, ToolKind, ToolStatus};

/// Updates that carry nothing for the session page.
const IGNORED: &[&str] = &[
    "user_message_chunk",
    "available_commands_update",
    "current_mode_update",
    "config_option_update",
    "session_info_update",
];

/// A tool call as the agent announced it (approvals are summarised from it).
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub input: Value,
}

pub struct AcpMapper {
    tool_kind: fn(&str, Option<&str>) -> ToolKind,
    /// The assistant message being streamed: its id and text so far.
    open: Option<(String, String)>,
    /// Ids for chunks that come without a `messageId`.
    unnamed: u64,
    tools: HashMap<String, Tool>,
    /// Tool calls whose approval the user refused: their failure is a decline.
    declined: HashSet<String>,
    /// Output shown so far per tool call: ACP resends a call's whole
    /// `content` on each update, the page appends deltas.
    shown: HashMap<String, String>,
}

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The text of one ACP content block (`{type: "text", text}`).
fn block_text(block: &Value) -> Option<&str> {
    (s(block, "type") == Some("text"))
        .then(|| s(block, "text"))
        .flatten()
}

/// A tool call's `content` as output text: text blocks as they are, a diff as
/// the file it changed.
fn tool_output(content: &Value) -> Option<String> {
    let parts: Vec<String> = content
        .as_array()?
        .iter()
        .filter_map(|item| match s(item, "type") {
            Some("content") => item.get("content").and_then(block_text).map(str::to_string),
            Some("diff") => s(item, "path").map(|path| format!("改动 {path}")),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn tool_status(status: Option<&str>) -> ToolStatus {
    match status {
        Some("completed") => ToolStatus::Completed,
        Some("failed") => ToolStatus::Failed,
        _ => ToolStatus::Running,
    }
}

impl AcpMapper {
    pub fn new(dialect: &Dialect) -> Self {
        Self {
            tool_kind: dialect.tool_kind,
            open: None,
            unnamed: 0,
            tools: HashMap::new(),
            declined: HashSet::new(),
            shown: HashMap::new(),
        }
    }

    pub fn tool(&self, id: &str) -> Option<&Tool> {
        self.tools.get(id)
    }

    /// The user refused this call's approval.
    pub fn declined(&mut self, id: &str) {
        self.declined.insert(id.to_string());
    }

    /// The streamed message ends (a tool call, another message, the turn's end).
    pub fn finish(&mut self) -> Vec<SessionUpdateKind> {
        self.open
            .take()
            .map(|(message_id, text)| SessionUpdateKind::MessageCompleted {
                message_id,
                text: (!text.is_empty()).then_some(text),
            })
            .into_iter()
            .collect()
    }

    fn message_id(&mut self, update: &Value) -> String {
        match s(update, "messageId") {
            Some(id) => id.to_string(),
            None => match &self.open {
                Some((id, _)) => id.clone(),
                None => {
                    self.unnamed += 1;
                    format!("acp-message-{}", self.unnamed)
                }
            },
        }
    }

    pub fn update(&mut self, update: &Value) -> Vec<SessionUpdateKind> {
        use SessionUpdateKind as K;
        let kind = s(update, "sessionUpdate").unwrap_or("");
        if IGNORED.contains(&kind) {
            return Vec::new();
        }
        match kind {
            "agent_message_chunk" => {
                let Some(text) = update.get("content").and_then(block_text) else {
                    return Vec::new();
                };
                let id = self.message_id(update);
                let mut updates = Vec::new();
                if self.open.as_ref().is_some_and(|(open, _)| open != &id) {
                    updates.extend(self.finish());
                }
                self.open
                    .get_or_insert_with(|| (id.clone(), String::new()))
                    .1
                    .push_str(text);
                updates.push(K::MessageChunk {
                    message_id: id,
                    role: "assistant".into(),
                    text: text.to_string(),
                });
                updates
            }
            "agent_thought_chunk" => {
                let Some(text) = update.get("content").and_then(block_text) else {
                    return Vec::new();
                };
                let id = s(update, "messageId")
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("acp-thought-{}", self.unnamed));
                vec![K::ThoughtChunk {
                    id: format!("{id}-thinking"),
                    text: text.to_string(),
                    summary: false,
                }]
            }
            "tool_call" => {
                let mut updates = self.finish();
                let id = s(update, "toolCallId").unwrap_or_default().to_string();
                let name = s(update, "title").unwrap_or("tool").to_string();
                let input = update.get("rawInput").cloned().unwrap_or(Value::Null);
                updates.push(K::ToolCall {
                    id: id.clone(),
                    kind: (self.tool_kind)(&name, s(update, "kind")),
                    title: tool_title(&name, &input),
                    input: input.clone(),
                });
                // An agent may announce a call that already finished.
                let status = s(update, "status");
                if matches!(status, Some("completed" | "failed")) {
                    updates.push(self.tool_update(&id, update));
                }
                self.tools.insert(id, Tool { name, input });
                updates
            }
            "tool_call_update" => {
                let id = s(update, "toolCallId").unwrap_or_default().to_string();
                if let Some(input) = update.get("rawInput") {
                    if let Some(tool) = self.tools.get_mut(&id) {
                        tool.input = input.clone();
                    }
                }
                vec![self.tool_update(&id, update)]
            }
            "plan" => vec![K::Plan {
                entries: update
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| {
                        Some(PlanEntry {
                            text: s(entry, "content")?.to_string(),
                            status: match s(entry, "status").unwrap_or("pending") {
                                "in_progress" => "inProgress",
                                other => other,
                            }
                            .to_string(),
                        })
                    })
                    .collect(),
            }],
            "usage_update" => vec![K::Usage {
                context_used: update.get("used").and_then(Value::as_u64),
                context_limit: update.get("size").and_then(Value::as_u64),
                input_tokens: None,
                output_tokens: None,
                cached_input_tokens: None,
            }],
            _ => vec![K::Unknown {
                kind: format!("acp/{kind}"),
                payload: update.clone(),
            }],
        }
    }

    fn tool_update(&mut self, id: &str, update: &Value) -> SessionUpdateKind {
        let mut status = tool_status(s(update, "status"));
        if status == ToolStatus::Failed && self.declined.contains(id) {
            status = ToolStatus::Declined;
        }
        let output = update
            .get("content")
            .and_then(tool_output)
            .and_then(|full| {
                let shown = self.shown.entry(id.to_string()).or_default();
                let delta = match full.strip_prefix(shown.as_str()) {
                    Some(rest) => rest.to_string(),
                    None if shown.is_empty() => full.clone(),
                    None => format!("\n{full}"),
                };
                *shown = full;
                (!delta.is_empty()).then_some(delta)
            });
        SessionUpdateKind::ToolCallUpdate {
            id: id.to_string(),
            status,
            output_delta: output,
            result: None,
        }
    }
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
