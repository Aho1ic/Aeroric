//! Session ID extraction for routed requests.
//!
//! Mirrors CC Switch's `extract_session_id()` so the local router can correlate
//! requests that belong to the same conversation, which makes usage tracking
//! and debugging far more useful. Only Claude and Codex agents are supported;
//! the routine falls back to a freshly generated UUID v4 when no identifier is
//! present so every request carries a stable session id.

use crate::local_router::RouterAgent;
use axum::http::HeaderMap;
use serde_json::Value;
use uuid::Uuid;

/// Header sent by Claude Code carrying its per-conversation session id.
const CLAUDE_SESSION_HEADER: &str = "x-claude-code-session-id";
/// Headers sent by Codex carrying its per-conversation session id.
const CODEX_SESSION_HEADERS: &[&str] = &["session_id", "x-session-id"];

/// Extract a session id for the given agent, falling back to a generated UUID
/// when the request carries none. `body` is the parsed JSON request body (may
/// be `None` when the request is not JSON).
pub(crate) fn extract_session_id(
    agent: RouterAgent,
    headers: &HeaderMap,
    body: Option<&Value>,
) -> String {
    match agent {
        RouterAgent::Claude => extract_header(headers, CLAUDE_SESSION_HEADER)
            .or_else(|| {
                body.and_then(|body| {
                    body.pointer("/metadata/user_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
            })
            .unwrap_or_else(new_session_id),
        RouterAgent::Codex => extract_codex_session(headers, body).unwrap_or_else(new_session_id),
    }
}

fn extract_codex_session(headers: &HeaderMap, body: Option<&Value>) -> Option<String> {
    for header in CODEX_SESSION_HEADERS {
        if let Some(value) = extract_header(headers, header) {
            return Some(value);
        }
    }
    body.and_then(|body| {
        body.pointer("/metadata/session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn extract_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn extracts_claude_session_from_header() {
        let headers = headers(&[("x-claude-code-session-id", "claude-session-123")]);
        let id = extract_session_id(RouterAgent::Claude, &headers, None);
        assert_eq!(id, "claude-session-123");
    }

    #[test]
    fn extracts_claude_session_from_metadata_user_id() {
        let body = json!({"metadata": {"user_id": "claude-user-456"}});
        let id = extract_session_id(RouterAgent::Claude, &HeaderMap::new(), Some(&body));
        assert_eq!(id, "claude-user-456");
    }

    #[test]
    fn extracts_codex_session_from_metadata() {
        let body = json!({"metadata": {"session_id": "codex-session-789"}});
        let id = extract_session_id(RouterAgent::Codex, &HeaderMap::new(), Some(&body));
        assert_eq!(id, "codex-session-789");
    }

    #[test]
    fn extracts_codex_session_from_header() {
        let headers = headers(&[("x-session-id", "codex-header-000")]);
        let id = extract_session_id(RouterAgent::Codex, &headers, None);
        assert_eq!(id, "codex-header-000");
    }

    #[test]
    fn generates_uuid_fallback() {
        let id = extract_session_id(RouterAgent::Claude, &HeaderMap::new(), None);
        Uuid::parse_str(&id).expect("fallback should be a valid UUID v4");
    }

    #[test]
    fn ignores_empty_header_values() {
        let headers = headers(&[("x-claude-code-session-id", "   ")]);
        let id = extract_session_id(RouterAgent::Claude, &headers, None);
        assert_ne!(id, "   ");
        assert!(!id.trim().is_empty());
    }
}
