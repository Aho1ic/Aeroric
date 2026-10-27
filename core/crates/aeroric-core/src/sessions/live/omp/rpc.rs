//! omp's RPC framing: every stdout line is a JSON object. A `response` answers
//! the command with the same `id`; everything else (session events,
//! `prompt_result`, `extension_ui_request`, …) arrives on `inbound`. Protocol
//! v2 splits an oversized object into base64 `rpc_chunk` frames, reassembled
//! here. When the transport closes, waiting commands fail and `inbound` ends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::sessions::live::codex::rpc::Transport;

/// The largest object we reassemble from chunks (omp advertises 64 MiB).
const MAX_REASSEMBLED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OmpError {
    #[error("the omp process is gone")]
    Closed,
    #[error("{0}")]
    Refused(String),
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, OmpError>>>>>;

pub struct OmpClient {
    outgoing: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
}

#[derive(Default)]
struct Chunks {
    parts: HashMap<String, (usize, Vec<Option<String>>)>,
}

impl Chunks {
    /// A complete object once every part of `chunkId` has arrived.
    fn add(&mut self, frame: &Value) -> Option<Value> {
        let id = frame.get("chunkId")?.as_str()?.to_string();
        let index = frame.get("index")?.as_u64()? as usize;
        let count = frame.get("count")?.as_u64()? as usize;
        let bytes = frame.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        let data = frame.get("data")?.as_str()?.to_string();
        if count == 0 || index >= count || bytes > MAX_REASSEMBLED_BYTES {
            self.parts.remove(&id);
            return None;
        }
        let entry = self
            .parts
            .entry(id.clone())
            .or_insert_with(|| (bytes, vec![None; count]));
        if entry.1.len() != count {
            self.parts.remove(&id);
            return None;
        }
        entry.1[index] = Some(data);
        if entry.1.iter().any(Option::is_none) {
            return None;
        }
        let (_, parts) = self.parts.remove(&id)?;
        let mut joined = Vec::with_capacity(bytes);
        for part in parts.into_iter().flatten() {
            joined.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(part)
                    .ok()?,
            );
            if joined.len() > MAX_REASSEMBLED_BYTES {
                return None;
            }
        }
        serde_json::from_slice(&joined).ok()
    }
}

impl OmpClient {
    pub fn start(transport: Transport) -> (Arc<OmpClient>, mpsc::UnboundedReceiver<Value>) {
        let Transport {
            outgoing,
            mut incoming,
        } = transport;
        let pending: Pending = Arc::default();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let reader_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut chunks = Chunks::default();
            while let Some(line) = incoming.recv().await {
                let Ok(mut frame) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if frame.get("type").and_then(Value::as_str) == Some("rpc_chunk") {
                    match chunks.add(&frame) {
                        Some(whole) => frame = whole,
                        None => continue,
                    }
                }
                if frame.get("type").and_then(Value::as_str) == Some("response") {
                    let waiter = frame
                        .get("id")
                        .and_then(Value::as_str)
                        .and_then(|id| reader_pending.lock().remove(id));
                    if let Some(waiter) = waiter {
                        let result = if frame.get("success").and_then(Value::as_bool) == Some(true)
                        {
                            Ok(frame.get("data").cloned().unwrap_or_else(|| json!({})))
                        } else {
                            Err(OmpError::Refused(
                                frame
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .unwrap_or("omp refused the command")
                                    .to_string(),
                            ))
                        };
                        let _ = waiter.send(result);
                        continue;
                    }
                }
                let _ = inbound_tx.send(frame);
            }
            for (_, waiter) in reader_pending.lock().drain() {
                let _ = waiter.send(Err(OmpError::Closed));
            }
        });
        (
            Arc::new(OmpClient {
                outgoing,
                pending,
                next_id: AtomicU64::new(1),
            }),
            inbound_rx,
        )
    }

    /// A fresh command id (`prompt` results are matched to it later).
    pub fn next_id(&self) -> String {
        format!("aeroric_{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub fn send(&self, frame: Value) -> Result<(), OmpError> {
        self.outgoing
            .send(frame.to_string())
            .map_err(|_| OmpError::Closed)
    }

    /// Send `{type, ...fields}` and wait for its `response`.
    pub async fn command(&self, kind: &str, fields: Value) -> Result<Value, OmpError> {
        self.command_with_id(&self.next_id(), kind, fields).await
    }

    pub async fn command_with_id(
        &self,
        id: &str,
        kind: &str,
        fields: Value,
    ) -> Result<Value, OmpError> {
        let mut frame = json!({"id": id, "type": kind});
        if let (Some(target), Some(extra)) = (frame.as_object_mut(), fields.as_object()) {
            target.extend(extra.clone());
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.to_string(), tx);
        if self.send(frame).is_err() {
            self.pending.lock().remove(id);
            return Err(OmpError::Closed);
        }
        rx.await.unwrap_or(Err(OmpError::Closed))
    }

    /// Answer an `extension_ui_request`.
    pub fn answer_ui(&self, id: &str, answer: Value) -> Result<(), OmpError> {
        let mut frame = json!({"type": "extension_ui_response", "id": id});
        if let (Some(target), Some(extra)) = (frame.as_object_mut(), answer.as_object()) {
            target.extend(extra.clone());
        }
        self.send(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn responses_pair_by_id_and_chunks_reassemble() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = OmpClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.command("get_state", json!({})).await }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(sent["type"], "get_state");
        to_client
            .send(json!({"type": "agent_start"}).to_string())
            .unwrap();
        to_client
            .send(json!({"id": sent["id"], "type": "response", "command": "get_state", "success": true, "data": {"sessionId": "s1"}}).to_string())
            .unwrap();
        assert_eq!(call.await.unwrap(), Ok(json!({"sessionId": "s1"})));
        assert_eq!(inbound.recv().await, Some(json!({"type": "agent_start"})));

        let whole =
            json!({"type": "message_end", "messageId": "m1", "message": {"role": "assistant"}})
                .to_string();
        // omp splits the bytes and base64-encodes each part on its own.
        let bytes = whole.as_bytes();
        let (left, right) = bytes.split_at(bytes.len() / 2);
        for (index, part) in [left, right].into_iter().enumerate() {
            to_client
                .send(json!({"type": "rpc_chunk", "chunkId": "c1", "index": index, "count": 2,
                    "byteLength": bytes.len(), "data": base64::engine::general_purpose::STANDARD.encode(part)}).to_string())
                .unwrap();
        }
        let frame = inbound.recv().await.unwrap();
        assert_eq!(frame["messageId"], "m1");
    }

    #[tokio::test]
    async fn refusals_and_closing_fail_commands() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = OmpClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move {
                client
                    .command("set_model", json!({"provider": "x", "modelId": "y"}))
                    .await
            }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(sent["provider"], "x");
        to_client
            .send(json!({"id": sent["id"], "type": "response", "command": "set_model", "success": false, "error": "Model not found: x/y"}).to_string())
            .unwrap();
        assert_eq!(
            call.await.unwrap(),
            Err(OmpError::Refused("Model not found: x/y".into()))
        );
        let waiting = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.command("abort", json!({})).await }
        });
        let _ = from_client.recv().await;
        drop(to_client);
        assert_eq!(waiting.await.unwrap(), Err(OmpError::Closed));
        assert_eq!(inbound.recv().await, None);
        client.answer_ui("u1", json!({"value": "Approve"})).ok();
    }
}
