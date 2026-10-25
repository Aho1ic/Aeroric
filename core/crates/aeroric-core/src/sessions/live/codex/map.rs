//! `codex app-server` → [`SessionUpdateKind`] (ADR-043). Pure functions: the
//! driver feeds them every notification / server request and appends the
//! result. Field names follow the snapshot in `contracts/codex/app-server-0.162.1`.

use serde_json::{json, Value};

use crate::sessions::live::update::{
    PlanEntry, SessionUpdateKind, ToolKind, ToolStatus, TurnEndReason,
};

/// What a turn-level notification means for the task, besides the updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnSignal {
    Started(String),
    Ended {
        turn_id: String,
        reason: TurnEndReason,
        error: Option<String>,
    },
}

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn string(value: &Value, key: &str) -> String {
    s(value, key).unwrap_or_default().to_string()
}

fn u64_at(value: &Value, path: &[&str]) -> Option<u64> {
    path.iter().try_fold(value, |v, key| v.get(key))?.as_u64()
}

/// Notification → updates (+ a turn signal when it starts or ends a turn).
pub fn map_notification(
    method: &str,
    params: &Value,
) -> (Vec<SessionUpdateKind>, Option<TurnSignal>) {
    use SessionUpdateKind as K;
    let one = |kind| (vec![kind], None);
    match method {
        "turn/started" => {
            let turn_id = params
                .pointer("/turn/id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            (
                vec![K::TurnStarted {
                    turn_id: turn_id.clone(),
                }],
                Some(TurnSignal::Started(turn_id)),
            )
        }
        "turn/completed" => {
            let turn = params.get("turn").cloned().unwrap_or(Value::Null);
            let turn_id = string(&turn, "id");
            let reason = match s(&turn, "status") {
                Some("interrupted") => TurnEndReason::Interrupted,
                Some("failed") => TurnEndReason::Failed,
                _ => TurnEndReason::Completed,
            };
            let error = turn
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string);
            (
                vec![K::TurnEnded {
                    turn_id: turn_id.clone(),
                    reason,
                    error: error.clone(),
                }],
                Some(TurnSignal::Ended {
                    turn_id,
                    reason,
                    error,
                }),
            )
        }
        "item/agentMessage/delta" => one(K::MessageChunk {
            message_id: string(params, "itemId"),
            role: "assistant".into(),
            text: string(params, "delta"),
        }),
        "item/reasoning/summaryTextDelta" => one(K::ThoughtChunk {
            id: string(params, "itemId"),
            text: string(params, "delta"),
            summary: true,
        }),
        "item/reasoning/textDelta" => one(K::ThoughtChunk {
            id: string(params, "itemId"),
            text: string(params, "delta"),
            summary: false,
        }),
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            one(K::ToolCallUpdate {
                id: string(params, "itemId"),
                status: ToolStatus::Running,
                output_delta: Some(string(params, "delta")),
                result: None,
            })
        }
        "item/started" => match params.get("item") {
            Some(item) => (map_item(item, false), None),
            None => (vec![unknown(method, params)], None),
        },
        "item/completed" => match params.get("item") {
            Some(item) => (map_item(item, true), None),
            None => (vec![unknown(method, params)], None),
        },
        "turn/plan/updated" => one(K::Plan {
            entries: params
                .get("plan")
                .and_then(Value::as_array)
                .map(|steps| {
                    steps
                        .iter()
                        .map(|step| PlanEntry {
                            text: string(step, "step"),
                            status: string(step, "status"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "thread/tokenUsage/updated" => one(K::Usage {
            context_used: u64_at(params, &["tokenUsage", "last", "inputTokens"]),
            context_limit: u64_at(params, &["tokenUsage", "modelContextWindow"]),
            input_tokens: u64_at(params, &["tokenUsage", "total", "inputTokens"]),
            output_tokens: u64_at(params, &["tokenUsage", "total", "outputTokens"]),
        }),
        "error" => one(K::Error {
            message: params
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Codex 报告了错误")
                .to_string(),
            recoverable: params
                .get("willRetry")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "warning" | "configWarning" | "deprecationNotice" | "guardianWarning" => one(K::Error {
            message: s(params, "message")
                .map(str::to_string)
                .unwrap_or_else(|| params.to_string()),
            recoverable: true,
        }),
        // Bookkeeping the session page does not render; not "unknown".
        "thread/started"
        | "thread/status/changed"
        | "thread/settings/updated"
        | "serverRequest/resolved"
        | "item/plan/delta"
        | "turn/diff/updated"
        | "item/fileChange/patchUpdated"
        | "item/reasoning/summaryPartAdded"
        | "account/rateLimits/updated"
        | "mcpServer/startupStatus/updated"
        | "thread/name/updated" => (vec![], None),
        _ => one(unknown(method, params)),
    }
}

fn unknown(kind: &str, payload: &Value) -> SessionUpdateKind {
    SessionUpdateKind::Unknown {
        kind: kind.to_string(),
        payload: payload.clone(),
    }
}

/// A thread item becomes a tool call (started) / tool update or message (completed).
fn map_item(item: &Value, completed: bool) -> Vec<SessionUpdateKind> {
    use SessionUpdateKind as K;
    let id = string(item, "id");
    let status = |v: &Value| match s(v, "status") {
        Some("completed") => ToolStatus::Completed,
        Some("failed") => ToolStatus::Failed,
        Some("declined") => ToolStatus::Declined,
        _ if completed => ToolStatus::Completed,
        _ => ToolStatus::Running,
    };
    let tool = |kind: ToolKind, title: String, input: Value, result: Option<Value>| {
        if completed {
            vec![K::ToolCallUpdate {
                id: id.clone(),
                status: status(item),
                output_delta: None,
                result,
            }]
        } else {
            vec![K::ToolCall {
                id: id.clone(),
                kind,
                title,
                input,
            }]
        }
    };
    match s(item, "type") {
        Some("agentMessage") if completed => vec![K::MessageCompleted {
            message_id: id.clone(),
            text: s(item, "text").map(str::to_string),
        }],
        Some("agentMessage") | Some("reasoning") | Some("userMessage") | Some("plan")
        | Some("hookPrompt") => vec![],
        Some("commandExecution") => tool(
            ToolKind::Command,
            string(item, "command"),
            json!({"command": item.get("command"), "cwd": item.get("cwd")}),
            Some(
                json!({"exitCode": item.get("exitCode"), "output": item.get("aggregatedOutput"), "durationMs": item.get("durationMs")}),
            ),
        ),
        Some("fileChange") => tool(
            ToolKind::FileChange,
            item.get("changes")
                .and_then(Value::as_array)
                .map(|changes| {
                    changes
                        .iter()
                        .filter_map(|c| s(c, "path"))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "修改文件".into()),
            json!({"changes": item.get("changes")}),
            None,
        ),
        Some("mcpToolCall") => tool(
            ToolKind::Mcp,
            format!("{}.{}", string(item, "server"), string(item, "tool")),
            item.get("arguments").cloned().unwrap_or(Value::Null),
            Some(json!({"result": item.get("result"), "error": item.get("error")})),
        ),
        Some("dynamicToolCall") => tool(
            ToolKind::Other,
            string(item, "tool"),
            item.get("arguments").cloned().unwrap_or(Value::Null),
            Some(json!({"success": item.get("success"), "content": item.get("contentItems")})),
        ),
        Some("webSearch") => tool(
            ToolKind::WebSearch,
            string(item, "query"),
            json!({"query": item.get("query")}),
            Some(json!({"results": item.get("results")})),
        ),
        Some("collabAgentToolCall") | Some("subAgentActivity") => tool(
            ToolKind::Subagent,
            s(item, "prompt")
                .or_else(|| s(item, "agentPath"))
                .unwrap_or("子 Agent")
                .chars()
                .take(80)
                .collect(),
            item.clone(),
            None,
        ),
        Some("contextCompaction") if completed => vec![K::Error {
            message: "上下文已压缩".into(),
            recoverable: true,
        }],
        Some("contextCompaction")
        | Some("enteredReviewMode")
        | Some("exitedReviewMode")
        | Some("sleep") => vec![],
        _ => vec![unknown("item", item)],
    }
}

/// Server-initiated approval → a permission request the user answers.
pub fn map_approval_request(
    method: &str,
    request_id: &str,
    params: &Value,
) -> Option<SessionUpdateKind> {
    let (kind, summary) = match method {
        "item/commandExecution/requestApproval" => (
            "command",
            s(params, "command")
                .map(str::to_string)
                .or_else(|| s(params, "reason").map(str::to_string))
                .unwrap_or_else(|| "运行命令".into()),
        ),
        "item/fileChange/requestApproval" => (
            "fileChange",
            s(params, "reason")
                .map(str::to_string)
                .unwrap_or_else(|| "修改文件".into()),
        ),
        "item/permissions/requestApproval" => (
            "permissions",
            s(params, "reason")
                .map(str::to_string)
                .unwrap_or_else(|| "申请额外权限".into()),
        ),
        _ => return None,
    };
    Some(SessionUpdateKind::PermissionRequest {
        id: request_id.to_string(),
        tool_call_id: s(params, "itemId").map(str::to_string),
        kind: kind.into(),
        summary,
        options: vec!["allowOnce".into(), "allowForSession".into(), "deny".into()],
    })
}

/// The user's choice, as Codex wants it for that request kind.
pub fn approval_response(
    method: &str,
    params: &Value,
    decision: &crate::sessions::live::Decision,
) -> Value {
    use crate::sessions::live::Decision;
    if method == "item/permissions/requestApproval" {
        let requested = params
            .get("permissions")
            .cloned()
            .unwrap_or_else(|| json!({}));
        return match decision {
            Decision::AllowOnce => json!({"permissions": requested, "scope": "turn"}),
            Decision::AllowForSession => json!({"permissions": requested, "scope": "session"}),
            Decision::Deny { .. } => json!({"permissions": {}}),
        };
    }
    let wire = match decision {
        Decision::AllowOnce => "accept",
        Decision::AllowForSession => "acceptForSession",
        Decision::Deny { .. } => "decline",
    };
    json!({ "decision": wire })
}

/// Aeroric permission tier → Codex per-thread / per-turn policy. Same meaning
/// as the PTY flags in `agents::launch::codex_permission_args`.
#[derive(Clone, Debug, PartialEq)]
pub struct CodexPolicy {
    pub approval: &'static str,
    pub sandbox_mode: &'static str,
    pub sandbox_policy: Value,
}

pub fn codex_policy(permission_mode: &str) -> CodexPolicy {
    match permission_mode {
        "full_access" => CodexPolicy {
            approval: "never",
            sandbox_mode: "danger-full-access",
            sandbox_policy: json!({"type": "dangerFullAccess"}),
        },
        "auto_edit" => CodexPolicy {
            approval: "on-request",
            sandbox_mode: "workspace-write",
            sandbox_policy: json!({"type": "workspaceWrite"}),
        },
        _ => CodexPolicy {
            approval: "on-request",
            sandbox_mode: "read-only",
            sandbox_policy: json!({"type": "readOnly"}),
        },
    }
}

/// `codex --version` → is it new enough for the structured driver?
pub fn version_supported(output: &str) -> Result<(), String> {
    const MINIMUM: (u64, u64) = (0, 162);
    let version = output
        .split_whitespace()
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("无法识别 Codex 版本：{}", output.trim()))?;
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
            "Codex {version} 太旧，原生会话需要 0.{}.0 或更新版本，请升级 Codex",
            MINIMUM.1
        ))
    }
}

/// Methods the driver calls or answers; checked against the schema snapshot.
pub const CLIENT_METHODS: &[&str] = &[
    "initialize",
    "thread/start",
    "thread/resume",
    "thread/unsubscribe",
    "turn/start",
    "turn/interrupt",
    "turn/steer",
    "thread/compact/start",
    "review/start",
];
pub const SERVER_REQUESTS: &[&str] = &[
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
];

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
