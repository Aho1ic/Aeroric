//! Per-task router tokens (Q23): one random token per running task.
//!
//! The host issues a token when it launches a task through the gateway and revokes
//! it when the task exits. The router only accepts requests carrying a live token
//! (or the configured access token), on loopback too: without this any local
//! process could spend the user's upstream credentials through the router.
//!
//! Process-global like `LISTENING`, because the router and the executor live in
//! the same host process and must agree on one table.

use std::collections::HashMap;

use parking_lot::RwLock;

/// What a token grants: which task and which agent configuration it routes for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskGrant {
    pub task_id: String,
    pub agent_config_id: Option<String>,
}

static TOKENS: RwLock<Option<HashMap<String, TaskGrant>>> = RwLock::new(None);

/// 64 hex characters from two v4 UUIDs (OS randomness via `getrandom`).
fn mint() -> String {
    format!(
        "aeroric-task-{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Issue a fresh token for `task_id`. A task holds at most one: re-issuing (a
/// resume or a new generation) revokes the previous token first.
pub fn issue(task_id: &str, agent_config_id: Option<String>) -> String {
    let token = mint();
    let mut guard = TOKENS.write();
    let table = guard.get_or_insert_with(HashMap::new);
    table.retain(|_, grant| grant.task_id != task_id);
    table.insert(
        token.clone(),
        TaskGrant {
            task_id: task_id.to_string(),
            agent_config_id,
        },
    );
    token
}

/// Revoke every token of `task_id`. Idempotent.
pub fn revoke(task_id: &str) {
    if let Some(table) = TOKENS.write().as_mut() {
        table.retain(|_, grant| grant.task_id != task_id);
    }
}

/// The grant behind a presented credential, if it is a live task token.
pub fn lookup(credential: &str) -> Option<TaskGrant> {
    let credential = credential.trim();
    if !credential.starts_with("aeroric-task-") {
        return None;
    }
    TOKENS
        .read()
        .as_ref()
        .and_then(|table| table.get(credential).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_lookup_revoke() {
        let token = issue("t-tokens-1", Some("cfg".into()));
        assert_eq!(token.len(), "aeroric-task-".len() + 64);
        let grant = lookup(&token).expect("live token");
        assert_eq!(grant.task_id, "t-tokens-1");
        assert_eq!(grant.agent_config_id.as_deref(), Some("cfg"));
        revoke("t-tokens-1");
        assert!(lookup(&token).is_none());
    }

    #[test]
    fn reissue_replaces_the_previous_token() {
        let first = issue("t-tokens-2", None);
        let second = issue("t-tokens-2", None);
        assert_ne!(first, second);
        assert!(lookup(&first).is_none());
        assert!(lookup(&second).is_some());
        revoke("t-tokens-2");
    }

    #[test]
    fn unknown_or_foreign_credentials_are_rejected() {
        assert!(lookup("sk-upstream-key").is_none());
        assert!(lookup("aeroric-task-not-issued").is_none());
    }
}
