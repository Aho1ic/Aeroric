//! E15 (2026-11-21): which MCP server of a session did not come up, how,
//! and after how long — said in the session instead of a bare timeout.
//!
//! Two harnesses report their MCP servers' start:
//! - Codex: `mcpServer/startupStatus/updated` `{name, status, error?,
//!   failureReason?}` per server, `starting` → `ready` | `failed` |
//!   `cancelled` (snapshots in `contracts/codex/`);
//! - Claude: `system/init` carries `mcp_servers: [{name, status}]`,
//!   `connected` | `pending` | `failed` | `needs-auth`, on every turn.
//!
//! Neither says which step (start, `initialize`, `tools/list`) a failure
//! was in, beyond Codex's error text; that text is quoted as it is.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::context_server::SERVER_NAME;
use super::update::SessionUpdateKind;

/// A start this slow is said even when it succeeded: the turn waited on it.
pub const SLOW_START: Duration = Duration::from_secs(5);

/// "Aeroric Context 工具（aeroric_context）" for our own server, the name
/// otherwise.
pub fn label(name: &str) -> String {
    if name == SERVER_NAME {
        format!("Aeroric Context 工具（{name}）")
    } else {
        name.to_string()
    }
}

/// What a failed start costs, for our own server: the tools it carries.
fn consequence(name: &str) -> &'static str {
    if name == SERVER_NAME {
        "；这一轮用不了记忆、委派、黑板等 Aeroric 工具"
    } else {
        ""
    }
}

fn notice(level: &str, message: String) -> SessionUpdateKind {
    SessionUpdateKind::Notice {
        level: level.into(),
        message,
    }
}

fn seconds(elapsed: Duration) -> String {
    let tenths = elapsed.as_millis() / 100;
    if tenths % 10 == 0 {
        format!("{} 秒", tenths / 10)
    } else {
        format!("{}.{} 秒", tenths / 10, tenths % 10)
    }
}

/// Codex's per-server start, timed from its `starting`. Each outcome is
/// said once per start; a server that starts again is timed afresh.
#[derive(Debug, Default)]
pub struct CodexStartups {
    started: HashMap<String, Instant>,
}

impl CodexStartups {
    /// The notice for one `mcpServer/startupStatus/updated`, if any.
    pub fn observe(&mut self, params: &Value, now: Instant) -> Option<SessionUpdateKind> {
        let name = params.get("name").and_then(Value::as_str)?;
        let status = params.get("status").and_then(Value::as_str)?;
        if status == "starting" {
            self.started.insert(name.to_string(), now);
            return None;
        }
        let elapsed = self
            .started
            .remove(name)
            .map(|at| now.saturating_duration_since(at));
        let after = elapsed
            .map(|e| format!("（启动 {}后）", seconds(e)))
            .unwrap_or_default();
        let who = label(name);
        match status {
            "ready" => elapsed.filter(|e| *e >= SLOW_START).map(|e| {
                notice(
                    "info",
                    format!("MCP 服务 {who} 用了 {}才准备好", seconds(e)),
                )
            }),
            "failed" => {
                let reauth = params.get("failureReason").and_then(Value::as_str)
                    == Some("reauthenticationRequired");
                let error = params
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|e| !e.is_empty());
                let why = match (reauth, error) {
                    (true, Some(error)) => format!("：需要重新登录这个服务（{error}）"),
                    (true, None) => "：需要重新登录这个服务".to_string(),
                    (false, Some(error)) => format!("：{error}"),
                    (false, None) => "，Codex 没有给出原因".to_string(),
                };
                Some(notice(
                    "error",
                    format!("MCP 服务 {who} 没起来{after}{why}{}", consequence(name)),
                ))
            }
            "cancelled" => Some(notice(
                "warning",
                format!("MCP 服务 {who} 的启动被取消了{after}{}", consequence(name)),
            )),
            _ => None,
        }
    }
}

/// Claude's `mcp_servers` from each `system/init`: a server that failed or
/// needs a login is said once per session, again only after it connected.
#[derive(Debug, Default)]
pub struct ClaudeStartups {
    said: HashSet<(String, String)>,
}

impl ClaudeStartups {
    pub fn observe(&mut self, init: &Value) -> Vec<SessionUpdateKind> {
        let Some(servers) = init.get("mcp_servers").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for server in servers {
            let (Some(name), Some(status)) = (
                server.get("name").and_then(Value::as_str),
                server.get("status").and_then(Value::as_str),
            ) else {
                continue;
            };
            if status == "connected" {
                self.said.retain(|(said, _)| said != name);
                continue;
            }
            let (level, what) = match status {
                "failed" => ("error", "没连上（Claude 报告 failed）"),
                "needs-auth" => ("warning", "需要先登录（Claude 报告 needs-auth）"),
                _ => continue,
            };
            if self.said.insert((name.to_string(), status.to_string())) {
                out.push(notice(
                    level,
                    format!("MCP 服务 {} {what}{}", label(name), consequence(name)),
                ));
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "mcp_startup_tests.rs"]
mod tests;
