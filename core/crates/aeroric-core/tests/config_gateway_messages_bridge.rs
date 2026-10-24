//! B1 4c end to end: a Claude task pinned to an agent configuration whose only
//! channel speaks Chat Completions. The router must send Chat upstream and hand
//! Claude Code a Messages-shaped reply, buffered and streamed.
//!
//! Its own test binary because it sets process globals (data dir, secret store).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use aeroric_core::agent_connections::model::ConnectionsDocument;
use aeroric_core::agent_connections::store;
use aeroric_core::context::MemorySecrets;
use aeroric_core::local_router::{
    task_tokens, LocalRouterState, RouterRuntimeConfig, RouterUpstreams, ROUTER_TOKEN_HEADER,
};
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::any;
use axum::{Json, Router};
use serde_json::{json, Value};

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// A Chat Completions upstream: JSON for `stream: false`, SSE chunks otherwise.
async fn chat_upstream() -> (SocketAddr, Seen) {
    let seen: Seen = Arc::default();
    let record = Arc::clone(&seen);
    let app = Router::new().fallback(any(move |uri: Uri, Json(body): Json<Value>| {
        let record = Arc::clone(&record);
        async move {
            record
                .lock()
                .unwrap()
                .push((uri.path().to_string(), body.clone()));
            if body["stream"].as_bool() == Some(true) {
                let chunks = [
                    json!({"id":"c1","model":"ds-chat","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}),
                    json!({"id":"c1","model":"ds-chat","choices":[{"index":0,"delta":{"content":"lo"}}]}),
                    json!({"id":"c1","model":"ds-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                           "usage":{"prompt_tokens":3,"completion_tokens":2}}),
                ];
                let mut sse = String::new();
                for chunk in chunks {
                    sse.push_str(&format!("data: {chunk}\n\n"));
                }
                sse.push_str("data: [DONE]\n\n");
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    sse,
                )
                    .into_response()
            } else {
                Json(json!({
                    "id": "c1", "object": "chat.completion", "model": "ds-chat",
                    "choices": [{ "index": 0, "finish_reason": "stop",
                                  "message": { "role": "assistant", "content": "Hello" } }],
                    "usage": { "prompt_tokens": 3, "completion_tokens": 2 }
                }))
                .into_response()
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (address, seen)
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn send(base_url: &str, token: &str, stream: bool) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base_url}/claude/v1/messages"))
        .header(ROUTER_TOKEN_HEADER, token)
        .json(&json!({
            "model": "deepseek-v4.1-flash",
            "max_tokens": 64,
            "stream": stream,
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn claude_reaches_a_chat_only_channel_through_the_bridge() {
    let root = std::env::temp_dir().join(format!("aeroric-gw-msg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    aeroric_core::settings::configure_data_dir(&root);
    let secrets = Arc::new(MemorySecrets::default());
    aeroric_core::settings::secret_fields::configure_secret_store(secrets.clone());

    let (chat, seen) = chat_upstream().await;
    let doc: ConnectionsDocument = serde_json::from_value(json!({
        "version": 1,
        "connections": [{
            "id": "chat-only", "label": "chat-only", "kind": "api_key", "vendor": "other",
            "base_url": format!("http://{chat}/v1"),
            "protocols": ["chat_completions"],
            "models": [{ "upstream_id": "ds-chat", "mapped_name": "deepseek-v4.1-flash" }]
        }],
        "pools": [],
        "configs": [{
            "id": "claude-chat", "label": "claude-chat", "harness": "claude",
            "target": { "kind": "connection", "connection_id": "chat-only" },
            "default_model": "deepseek-v4.1-flash"
        }]
    }))
    .unwrap();
    store::save(&store::document_path(&root), &doc).unwrap();
    store::set_credential(secrets.as_ref(), "chat-only", "sk-test").unwrap();

    let router = LocalRouterState::with_database_path(root.join("usage.sqlite3"));
    let info = router
        .start(RouterRuntimeConfig::new(
            "127.0.0.1",
            unused_port(),
            false,
            RouterUpstreams::default(),
        ))
        .await
        .unwrap();
    let token = task_tokens::issue("e2e-messages", Some("claude-chat".into()));

    // Buffered: Chat goes upstream, a Messages reply comes back.
    let response = send(&info.base_url, &token, false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reply: Value = response.json().await.unwrap();
    {
        let seen = seen.lock().unwrap();
        let (path, body) = seen.last().expect("upstream was called");
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(body["model"], "ds-chat", "upstream id, not the mapped name");
        let messages = body["messages"].as_array().expect("chat messages");
        assert!(
            messages
                .iter()
                .any(|m| m["role"] == "user" && m.to_string().contains("hi")),
            "{body}"
        );
    }
    assert_eq!(reply["type"], "message", "{reply}");
    assert_eq!(reply["role"], "assistant");
    assert_eq!(reply["content"][0]["type"], "text");
    assert_eq!(reply["content"][0]["text"], "Hello");
    assert_eq!(reply["stop_reason"], "end_turn");

    // Streamed: Chat chunks become Anthropic SSE events.
    let response = send(&info.base_url, &token, true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.text().await.unwrap();
    for event in [
        "message_start",
        "content_block_start",
        "content_block_delta",
        "content_block_stop",
        "message_delta",
        "message_stop",
    ] {
        assert!(text.contains(event), "missing {event} in:\n{text}");
    }
    assert!(text.contains("Hel") && text.contains("lo"), "{text}");
    assert!(
        !text.contains("chat.completion"),
        "raw Chat chunks leaked:\n{text}"
    );

    task_tokens::revoke("e2e-messages");
    router.stop().await.unwrap();
    std::fs::remove_dir_all(&root).ok();
}
