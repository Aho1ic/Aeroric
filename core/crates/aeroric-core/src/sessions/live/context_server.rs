//! B4′ (2026-10-30): the Aeroric context MCP server each structured session
//! gets — `aeroric-cli mcp-context`, which calls back into the host with a
//! token that names one task and lasts as long as that task's session.
//!
//! Injected per start (Claude `--mcp-config`, Codex `thread/start` config),
//! never written into a shared config file, so two tasks cannot read each
//! other's context through it.

use std::collections::HashMap;

use parking_lot::Mutex;

/// The environment variable the server reads its token from (never argv).
pub const TOKEN_ENV: &str = "AERORIC_CONTEXT_TOKEN";
/// The server's name in the agent's MCP list.
pub const SERVER_NAME: &str = "aeroric_context";

/// How to start the context server for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextServer {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ContextServer {
    /// One MCP server entry, as both Claude's `mcpServers` and Codex's
    /// `mcp_servers` take it.
    pub fn entry(&self) -> serde_json::Value {
        let env: serde_json::Map<String, serde_json::Value> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        serde_json::json!({"command": self.command, "args": self.args, "env": env})
    }
}

/// Tokens by task: one live token per task, revoked when its session closes
/// or a new one is issued.
#[derive(Default)]
pub struct ContextTokens {
    inner: Mutex<Tokens>,
}

#[derive(Default)]
struct Tokens {
    by_token: HashMap<String, (String, String)>,
    by_task: HashMap<String, String>,
}

impl ContextTokens {
    /// A fresh token for `task_id` in `project_id`; the task's old one stops working.
    pub fn issue(&self, task_id: &str, project_id: &str) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut t = self.inner.lock();
        if let Some(old) = t.by_task.insert(task_id.to_string(), token.clone()) {
            t.by_token.remove(&old);
        }
        t.by_token
            .insert(token.clone(), (task_id.to_string(), project_id.to_string()));
        token
    }

    /// `(task_id, project_id)` for a live token.
    pub fn resolve(&self, token: &str) -> Option<(String, String)> {
        self.inner.lock().by_token.get(token).cloned()
    }

    pub fn revoke(&self, task_id: &str) {
        let mut t = self.inner.lock();
        if let Some(old) = t.by_task.remove(task_id) {
            t.by_token.remove(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_token_retires_the_old_one_and_revoke_ends_it() {
        let tokens = ContextTokens::default();
        let first = tokens.issue("t1", "p1");
        assert_eq!(tokens.resolve(&first), Some(("t1".into(), "p1".into())));
        let second = tokens.issue("t1", "p1");
        assert_eq!(tokens.resolve(&first), None);
        assert!(tokens.resolve(&second).is_some());
        tokens.revoke("t1");
        assert_eq!(tokens.resolve(&second), None);
        assert_eq!(tokens.resolve("made-up"), None);
    }

    #[test]
    fn the_entry_carries_the_token_in_env_not_args() {
        let server = ContextServer {
            command: "/a/aeroric-cli".into(),
            args: vec!["--data-dir".into(), "/d".into(), "mcp-context".into()],
            env: vec![(TOKEN_ENV.into(), "secret".into())],
        };
        let entry = server.entry();
        assert_eq!(entry["env"][TOKEN_ENV], "secret");
        assert!(!entry["args"].to_string().contains("secret"));
    }
}
