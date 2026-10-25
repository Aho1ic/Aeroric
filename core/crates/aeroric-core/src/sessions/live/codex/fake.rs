//! Test support: a scripted in-process `codex app-server` peer and a sink that
//! records what a driver reported.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::rpc::Transport;
use super::server::Launcher;
use crate::sessions::live::update::SessionUpdate;
use crate::sessions::live::LiveStatusSink;
use crate::tasks::{ProtocolFamily, TaskStatus};

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

    /// Nothing more arrives within a short wait.
    pub async fn silent(&mut self) {
        let next = tokio::time::timeout(Duration::from_millis(100), self.from_client.recv()).await;
        assert!(next.is_err(), "unexpected message {next:?}");
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

#[derive(Default)]
pub struct Sink {
    pub statuses: Mutex<Vec<(TaskStatus, Option<String>, u64)>>,
    pub sessions: Mutex<Vec<(String, Option<String>)>>,
    pub updates: Mutex<Vec<SessionUpdate>>,
}

impl Sink {
    pub fn last_status(&self) -> Option<TaskStatus> {
        self.statuses.lock().last().map(|(status, _, _)| *status)
    }

    pub fn statuses(&self) -> Vec<TaskStatus> {
        self.statuses
            .lock()
            .iter()
            .map(|(status, _, _)| *status)
            .collect()
    }

    pub fn update_types(&self) -> Vec<String> {
        self.updates
            .lock()
            .iter()
            .map(|u| {
                serde_json::to_value(u).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }
}

impl LiveStatusSink for Sink {
    fn status(
        &self,
        _project: &str,
        _task: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.statuses.lock().push((status, reason, generation));
    }

    fn session(
        &self,
        _project: &str,
        _task: &str,
        _family: ProtocolFamily,
        _agent: &str,
        id: &str,
        path: Option<&str>,
    ) {
        self.sessions
            .lock()
            .push((id.to_string(), path.map(str::to_string)));
    }

    fn update(&self, update: &SessionUpdate) {
        self.updates.lock().push(update.clone());
    }
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
