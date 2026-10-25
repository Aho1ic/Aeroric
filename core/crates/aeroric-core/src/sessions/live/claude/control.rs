//! Claude Code's stdio control protocol (the Claude Agent SDK's wire): every
//! line is a JSON object. Our `control_request`s are matched to their
//! `control_response` by `request_id`; Claude's own `control_request`s (tool
//! permission, hooks, MCP) and every other message arrive on `inbound`. When
//! the transport closes, waiting requests fail and `inbound` ends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::sessions::live::codex::rpc::Transport;

#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeInbound {
    /// system / assistant / user / stream_event / result / … and
    /// `control_cancel_request`.
    Message(Value),
    /// Claude asks us something (`can_use_tool`, `hook_callback`, `mcp_message`).
    Request { request_id: String, request: Value },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ControlError {
    #[error("the Claude process is gone")]
    Closed,
    #[error("{0}")]
    Refused(String),
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, ControlError>>>>>;

pub struct ControlClient {
    outgoing: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
}

impl ControlClient {
    pub fn start(
        transport: Transport,
    ) -> (Arc<ControlClient>, mpsc::UnboundedReceiver<ClaudeInbound>) {
        let Transport {
            outgoing,
            mut incoming,
        } = transport;
        let pending: Pending = Arc::default();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let reader_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            while let Some(line) = incoming.recv().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match message.get("type").and_then(Value::as_str) {
                    Some("control_response") => {
                        let response = message.get("response").cloned().unwrap_or(Value::Null);
                        let Some(id) = response.get("request_id").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(waiter) = reader_pending.lock().remove(id) else {
                            continue;
                        };
                        let result = match response.get("subtype").and_then(Value::as_str) {
                            Some("error") => Err(ControlError::Refused(
                                response
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .unwrap_or("error")
                                    .to_string(),
                            )),
                            _ => Ok(response
                                .get("response")
                                .cloned()
                                .unwrap_or_else(|| json!({}))),
                        };
                        let _ = waiter.send(result);
                    }
                    Some("control_request") => {
                        let request_id = message
                            .get("request_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let request = message.get("request").cloned().unwrap_or(Value::Null);
                        let _ = inbound_tx.send(ClaudeInbound::Request {
                            request_id,
                            request,
                        });
                    }
                    _ => {
                        let _ = inbound_tx.send(ClaudeInbound::Message(message));
                    }
                }
            }
            for (_, waiter) in reader_pending.lock().drain() {
                let _ = waiter.send(Err(ControlError::Closed));
            }
        });
        (
            Arc::new(ControlClient {
                outgoing,
                pending,
                next_id: AtomicU64::new(1),
            }),
            inbound_rx,
        )
    }

    fn send(&self, message: Value) -> Result<(), ControlError> {
        self.outgoing
            .send(message.to_string())
            .map_err(|_| ControlError::Closed)
    }

    /// One of our control requests (`initialize`, `interrupt`, `set_model`, …).
    pub async fn request(&self, request: Value) -> Result<Value, ControlError> {
        let id = format!("aeroric_{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.clone(), tx);
        if self
            .send(json!({"type": "control_request", "request_id": id, "request": request}))
            .is_err()
        {
            self.pending.lock().remove(&id);
            return Err(ControlError::Closed);
        }
        rx.await.unwrap_or(Err(ControlError::Closed))
    }

    /// A user turn's message.
    pub fn send_user(&self, text: &str, session_id: Option<&str>) -> Result<(), ControlError> {
        self.send(json!({
            "type": "user",
            "message": {"role": "user", "content": text},
            "parent_tool_use_id": null,
            "session_id": session_id.unwrap_or("default"),
        }))
    }

    /// Answer one of Claude's control requests.
    pub fn respond(&self, request_id: &str, response: Value) -> Result<(), ControlError> {
        self.send(json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}}))
    }

    pub fn respond_error(&self, request_id: &str, error: &str) -> Result<(), ControlError> {
        self.send(json!({"type": "control_response", "response": {"subtype": "error", "request_id": request_id, "error": error}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_pair_by_id_and_everything_else_is_inbound() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = ControlClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request(json!({"subtype": "initialize"})).await }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(sent["type"], "control_request");
        assert_eq!(sent["request"]["subtype"], "initialize");
        let id = sent["request_id"].as_str().unwrap().to_string();
        to_client
            .send(json!({"type": "system", "subtype": "init", "session_id": "s"}).to_string())
            .unwrap();
        to_client.send(json!({"type": "control_request", "request_id": "c1", "request": {"subtype": "can_use_tool"}}).to_string()).unwrap();
        to_client.send(json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"commands": []}}}).to_string()).unwrap();
        assert_eq!(call.await.unwrap(), Ok(json!({"commands": []})));
        assert!(
            matches!(inbound.recv().await, Some(ClaudeInbound::Message(m)) if m["subtype"] == "init")
        );
        assert!(
            matches!(inbound.recv().await, Some(ClaudeInbound::Request { request_id, .. }) if request_id == "c1")
        );

        client.respond("c1", json!({"behavior": "allow"})).unwrap();
        let answer: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(
            answer,
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": "c1", "response": {"behavior": "allow"}}})
        );
        client.send_user("hi", Some("s")).unwrap();
        let user: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(user["message"], json!({"role": "user", "content": "hi"}));
    }

    #[tokio::test]
    async fn refusals_and_closing_fail_requests() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = ControlClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request(json!({"subtype": "set_model"})).await }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        to_client.send(json!({"type": "control_response", "response": {"subtype": "error", "request_id": sent["request_id"], "error": "no such model"}}).to_string()).unwrap();
        assert_eq!(
            call.await.unwrap(),
            Err(ControlError::Refused("no such model".into()))
        );
        let pending = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request(json!({"subtype": "interrupt"})).await }
        });
        let _ = from_client.recv().await;
        drop(to_client);
        assert_eq!(pending.await.unwrap(), Err(ControlError::Closed));
        assert_eq!(inbound.recv().await, None);
    }
}
