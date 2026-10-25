//! Line-delimited JSON-RPC over a [`Transport`] (the `codex app-server` stdio
//! process, or an in-memory peer in tests). Responses complete the matching
//! request; notifications and server-initiated requests arrive on `inbound`.
//! When the transport closes, pending requests fail and `inbound` ends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

/// Two line channels: what we write, what the peer writes.
pub struct Transport {
    pub outgoing: mpsc::UnboundedSender<String>,
    pub incoming: mpsc::UnboundedReceiver<String>,
}

impl Transport {
    /// An in-memory pair: the transport and the peer's ends.
    pub fn pair() -> (
        Transport,
        mpsc::UnboundedReceiver<String>,
        mpsc::UnboundedSender<String>,
    ) {
        let (to_peer, from_client) = mpsc::unbounded_channel();
        let (to_client, from_peer) = mpsc::unbounded_channel();
        (
            Transport {
                outgoing: to_peer,
                incoming: from_peer,
            },
            from_client,
            to_client,
        )
    }

    /// Spawn a child process speaking line JSON on stdin/stdout. The child is
    /// killed when the returned guard drops.
    pub fn spawn(mut command: tokio::process::Command) -> std::io::Result<(Transport, ChildGuard)> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        let (outgoing, mut to_write) = mpsc::unbounded_channel::<String>();
        let (to_read, incoming) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            while let Some(line) = to_write.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if to_read.send(line).is_err() {
                    break;
                }
            }
        });
        Ok((
            Transport { outgoing, incoming },
            ChildGuard {
                child: Some(child),
                cleanup: None,
            },
        ))
    }
}

pub struct ChildGuard {
    child: Option<tokio::process::Child>,
    cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl ChildGuard {
    /// Run `cleanup` once the process is gone (killed here, or already exited):
    /// e.g. revoke the credentials it was started with.
    pub fn with_cleanup(mut self, cleanup: impl FnOnce() + Send + 'static) -> Self {
        self.cleanup = Some(Box::new(cleanup));
        self
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inbound {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RpcError {
    #[error("transport closed")]
    Closed,
    #[error("{message} ({code})")]
    Remote { code: i64, message: String },
}

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>>;

pub struct RpcClient {
    outgoing: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicI64,
}

impl RpcClient {
    /// Start reading; returns the client and the inbound stream.
    pub fn start(transport: Transport) -> (Arc<RpcClient>, mpsc::UnboundedReceiver<Inbound>) {
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
                let method = message
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                match (method, message.get("id")) {
                    (Some(method), Some(id)) => {
                        let _ = inbound_tx.send(Inbound::Request {
                            id: id.clone(),
                            method,
                            params,
                        });
                    }
                    (Some(method), None) => {
                        let _ = inbound_tx.send(Inbound::Notification { method, params });
                    }
                    (None, Some(id)) => {
                        let Some(id) = id.as_i64() else { continue };
                        let Some(waiter) = reader_pending.lock().remove(&id) else {
                            continue;
                        };
                        let result = match message.get("error") {
                            Some(error) => Err(RpcError::Remote {
                                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                                message: error
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("error")
                                    .to_string(),
                            }),
                            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = waiter.send(result);
                    }
                    (None, None) => {}
                }
            }
            // Closed: fail everything still waiting; dropping inbound_tx ends the stream.
            for (_, waiter) in reader_pending.lock().drain() {
                let _ = waiter.send(Err(RpcError::Closed));
            }
        });
        (
            Arc::new(RpcClient {
                outgoing,
                pending,
                next_id: AtomicI64::new(1),
            }),
            inbound_rx,
        )
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let line =
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        if self.outgoing.send(line).is_err() {
            self.pending.lock().remove(&id);
            return Err(RpcError::Closed);
        }
        rx.await.unwrap_or(Err(RpcError::Closed))
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string();
        self.outgoing.send(line).map_err(|_| RpcError::Closed)
    }

    /// Answer a server-initiated request.
    pub fn respond(&self, id: &Value, result: Value) -> Result<(), RpcError> {
        let line = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
        self.outgoing.send(line).map_err(|_| RpcError::Closed)
    }

    /// Refuse a server-initiated request this client does not handle.
    pub fn respond_error(&self, id: &Value, code: i64, message: &str) -> Result<(), RpcError> {
        let line = json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            .to_string();
        self.outgoing.send(line).map_err(|_| RpcError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_get_their_own_response_and_the_rest_is_inbound() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = RpcClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request("thread/start", json!({"cwd": "/w"})).await }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(sent["method"], "thread/start");
        to_client.send(json!({"jsonrpc": "2.0", "method": "thread/started", "params": {"thread": {"id": "th"}}}).to_string()).unwrap();
        to_client.send(json!({"jsonrpc": "2.0", "id": 99, "method": "item/commandExecution/requestApproval", "params": {}}).to_string()).unwrap();
        to_client
            .send(
                json!({"jsonrpc": "2.0", "id": sent["id"], "result": {"thread": {"id": "th"}}})
                    .to_string(),
            )
            .unwrap();
        assert_eq!(call.await.unwrap().unwrap()["thread"]["id"], "th");
        assert!(
            matches!(inbound.recv().await, Some(Inbound::Notification { method, .. }) if method == "thread/started")
        );
        assert!(
            matches!(inbound.recv().await, Some(Inbound::Request { method, .. }) if method == "item/commandExecution/requestApproval")
        );
        client
            .respond(&json!(99), json!({"decision": "accept"}))
            .unwrap();
        let answer: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        assert_eq!(
            answer,
            json!({"jsonrpc": "2.0", "id": 99, "result": {"decision": "accept"}})
        );
    }

    #[tokio::test]
    async fn remote_errors_and_closing_fail_requests() {
        let (transport, mut from_client, to_client) = Transport::pair();
        let (client, mut inbound) = RpcClient::start(transport);
        let call = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request("turn/start", json!({})).await }
        });
        let sent: Value = serde_json::from_str(&from_client.recv().await.unwrap()).unwrap();
        to_client
            .send(
                json!({"id": sent["id"], "error": {"code": -32602, "message": "bad params"}})
                    .to_string(),
            )
            .unwrap();
        assert_eq!(
            call.await.unwrap(),
            Err(RpcError::Remote {
                code: -32602,
                message: "bad params".into()
            })
        );

        let pending = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.request("turn/start", json!({})).await }
        });
        let _ = from_client.recv().await;
        drop(to_client);
        assert_eq!(pending.await.unwrap(), Err(RpcError::Closed));
        assert_eq!(
            inbound.recv().await,
            None,
            "inbound ends when the transport closes"
        );
    }

    #[tokio::test]
    async fn the_guard_kills_the_process_then_runs_its_cleanup_once() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (_transport, guard) = Transport::spawn(tokio::process::Command::new("cat")).unwrap();
        let guard = guard.with_cleanup(move || tx.send("revoked").unwrap());
        drop(guard);
        assert_eq!(rx.try_recv(), Ok("revoked"));
        assert!(rx.try_recv().is_err(), "once");
    }
}
