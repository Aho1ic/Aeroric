//! Model-list fetch for a connection. Catalog only: no inference, no balance (Q134).

use crate::settings::schema::AgentSetupKind;

use super::model::{Connection, WireProtocol};

/// Pick the auth-header order that matches the connection's protocols: Messages
/// upstreams expect `x-api-key` first, OpenAI-style ones expect a bearer token.
fn auth_kind(connection: &Connection) -> AgentSetupKind {
    if connection.protocols.contains(&WireProtocol::Messages)
        && !connection.protocols.contains(&WireProtocol::Responses)
        && !connection.protocols.contains(&WireProtocol::ChatCompletions)
    {
        AgentSetupKind::ClaudeCode
    } else {
        AgentSetupKind::Codex
    }
}

pub async fn fetch_models(connection: &Connection, credential: &str) -> Result<Vec<String>, String> {
    if connection.base_url.trim().is_empty() {
        return Err("connection has no base URL".to_string());
    }
    crate::settings::model_detect::fetch_connection_catalog(
        auth_kind(connection),
        &connection.base_url,
        credential,
        connection.use_global_proxy,
    )
    .await
}
