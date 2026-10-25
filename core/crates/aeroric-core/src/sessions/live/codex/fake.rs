//! Test support: a scripted in-process `codex app-server` peer.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::rpc::Transport;
use super::server::Launcher;

const WAIT: Duration = Duration::from_secs(5);

/// The server's side of one launched process.
pub struct Peer {
    from_client: mpsc::UnboundedReceiver<String>,
    to_client: Option<mpsc::UnboundedSender<String>>,
}

impl Peer {
    pub async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(WAIT, self.from_client.recv())
            .await
            .expect("the client sent nothing")
            .expect("the client closed");
        serde_json::from_str(&line).expect("a JSON line")
    }

    pub async fn expect(&mut self, method: &str) -> Value {
        let message = self.next().await;
        assert_eq!(message["method"], method, "unexpected message {message}");
        message
    }

    fn send(&self, message: Value) {
        self.to_client
            .as_ref()
            .expect("peer closed")
            .send(message.to_string())
            .expect("client gone");
    }

    pub fn reply(&self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn request(&self, id: i64, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    pub async fn handshake(&mut self) {
        let initialize = self.expect("initialize").await;
        assert_eq!(initialize["params"]["clientInfo"]["name"], "aeroric");
        self.reply(&initialize, json!({"userAgent": "fake"}));
        self.expect("initialized").await;
    }

    /// The process exits.
    pub fn close(&mut self) {
        self.to_client = None;
    }
}

/// Every launch hands the test a new [`Peer`].
pub fn launcher() -> (Launcher, mpsc::UnboundedReceiver<Peer>) {
    let (peers_tx, peers) = mpsc::unbounded_channel();
    let launcher: Launcher = Arc::new(move || {
        let (transport, from_client, to_client) = Transport::pair();
        let _ = peers_tx.send(Peer {
            from_client,
            to_client: Some(to_client),
        });
        Ok((transport, None))
    });
    (launcher, peers)
}

pub async fn launched(peers: &mut mpsc::UnboundedReceiver<Peer>) -> Peer {
    tokio::time::timeout(WAIT, peers.recv())
        .await
        .expect("nothing was launched")
        .expect("launcher gone")
}

/// Wait (bounded) until `condition` holds.
pub async fn until(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
