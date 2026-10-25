//! One shared `codex app-server` per agent (ADR-043 §5). Started on first use,
//! handshaken, and routing every notification / server request to the session
//! that owns its thread. When the process exits every routed session is told
//! (`SERVER_EXITED`) and the next `connect` starts a new one, unless it has
//! exited [`CRASH_LIMIT`] times within [`CRASH_WINDOW_MS`].

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::map::version_supported;
use super::rpc::{ChildGuard, Inbound, RpcClient, Transport};
use crate::sessions::live::now_ms;

/// Starts the process (or, in tests, an in-memory peer).
pub type Launcher = Arc<dyn Fn() -> Result<(Transport, Option<ChildGuard>), String> + Send + Sync>;

/// Synthetic notification a session receives when its server process is gone.
pub const SERVER_EXITED: &str = "aeroric/serverExited";
pub const CRASH_WINDOW_MS: u64 = 60_000;
pub const CRASH_LIMIT: usize = 3;
/// A server nobody has used for this long is stopped by [`CodexServer::stop_if_unused`].
pub const SERVER_IDLE_MS: u64 = 5 * 60_000;

/// A handshaken client and the process generation it talks to.
#[derive(Clone)]
pub struct Connection {
    pub client: Arc<RpcClient>,
    pub epoch: u64,
}

struct Running {
    connection: Connection,
    _guard: Option<ChildGuard>,
}

type Route = (u64, mpsc::UnboundedSender<Inbound>);

pub struct CodexServer {
    launcher: Launcher,
    running: tokio::sync::Mutex<Option<Running>>,
    routes: Mutex<HashMap<String, Route>>,
    crashes: Mutex<VecDeque<u64>>,
    epoch: AtomicU64,
    unused_since: Mutex<Option<u64>>,
}

impl CodexServer {
    pub fn new(launcher: Launcher) -> Arc<Self> {
        Arc::new(Self {
            launcher,
            running: tokio::sync::Mutex::new(None),
            routes: Mutex::default(),
            crashes: Mutex::default(),
            epoch: AtomicU64::new(0),
            unused_since: Mutex::new(None),
        })
    }

    /// The running server, starting and handshaking one if needed.
    pub async fn connect(self: &Arc<Self>) -> Result<Connection, String> {
        let mut running = self.running.lock().await;
        if let Some(running) = running.as_ref() {
            return Ok(running.connection.clone());
        }
        self.within_crash_budget(now_ms())?;
        let (transport, guard) = (self.launcher)()?;
        let (client, inbound) = RpcClient::start(transport);
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        tokio::spawn(route(Arc::downgrade(self), epoch, inbound));
        let info = json!({"clientInfo": {"name": "aeroric", "title": "Aeroric", "version": env!("CARGO_PKG_VERSION")}});
        if let Err(error) = client.request("initialize", info).await {
            self.record_crash(now_ms());
            return Err(format!("Codex 握手失败：{error}"));
        }
        let _ = client.notify("initialized", json!({}));
        let connection = Connection { client, epoch };
        *running = Some(Running {
            connection: connection.clone(),
            _guard: guard,
        });
        self.unused_since.lock().get_or_insert(now_ms());
        Ok(connection)
    }

    /// Route a thread's traffic from the server generation `epoch` to `inbox`.
    pub fn register(&self, thread_id: &str, epoch: u64, inbox: mpsc::UnboundedSender<Inbound>) {
        self.routes
            .lock()
            .insert(thread_id.to_string(), (epoch, inbox));
        *self.unused_since.lock() = None;
    }

    pub fn unregister(&self, thread_id: &str) {
        let mut routes = self.routes.lock();
        routes.remove(thread_id);
        if routes.is_empty() {
            *self.unused_since.lock() = Some(now_ms());
        }
    }

    /// Stop the process when no thread has been routed for [`SERVER_IDLE_MS`].
    pub async fn stop_if_unused(&self, now: u64) -> bool {
        if !self.routes.lock().is_empty() {
            return false;
        }
        let idle = self
            .unused_since
            .lock()
            .is_some_and(|since| now.saturating_sub(since) >= SERVER_IDLE_MS);
        if !idle {
            return false;
        }
        // Taking it out first makes the exit that follows an intended one.
        self.running.lock().await.take().is_some()
    }

    pub async fn is_running(&self) -> bool {
        self.running.lock().await.is_some()
    }

    fn within_crash_budget(&self, now: u64) -> Result<(), String> {
        let mut crashes = self.crashes.lock();
        while crashes
            .front()
            .is_some_and(|at| now.saturating_sub(*at) > CRASH_WINDOW_MS)
        {
            crashes.pop_front();
        }
        if crashes.len() >= CRASH_LIMIT {
            return Err(format!(
                "Codex 进程在 1 分钟内连续退出 {CRASH_LIMIT} 次，已停止自动重启；请检查 Codex 安装与登录后再试"
            ));
        }
        Ok(())
    }

    fn record_crash(&self, now: u64) {
        self.crashes.lock().push_back(now);
    }

    async fn exited(&self, epoch: u64) {
        let unexpected = {
            let mut running = self.running.lock().await;
            let current = running
                .as_ref()
                .is_some_and(|r| r.connection.epoch == epoch);
            if current {
                *running = None;
            }
            current
        };
        if unexpected {
            self.record_crash(now_ms());
        }
        let gone: Vec<_> = {
            let mut routes = self.routes.lock();
            let threads: Vec<String> = routes
                .iter()
                .filter(|(_, (e, _))| *e == epoch)
                .map(|(t, _)| t.clone())
                .collect();
            threads
                .into_iter()
                .filter_map(|t| routes.remove(&t))
                .collect()
        };
        for (_, inbox) in gone {
            let _ = inbox.send(Inbound::Notification {
                method: SERVER_EXITED.into(),
                params: Value::Null,
            });
        }
    }

    fn inbox_for(&self, message: &Inbound, epoch: u64) -> Option<mpsc::UnboundedSender<Inbound>> {
        let params = match message {
            Inbound::Notification { params, .. } | Inbound::Request { params, .. } => params,
        };
        let thread = params
            .get("threadId")
            .or_else(|| params.pointer("/thread/id"))
            .or_else(|| params.get("conversationId"))
            .and_then(Value::as_str)?;
        self.routes
            .lock()
            .get(thread)
            .filter(|(e, _)| *e == epoch)
            .map(|(_, inbox)| inbox.clone())
    }
}

async fn route(
    server: Weak<CodexServer>,
    epoch: u64,
    mut inbound: mpsc::UnboundedReceiver<Inbound>,
) {
    while let Some(message) = inbound.recv().await {
        let Some(server) = server.upgrade() else {
            return;
        };
        match server.inbox_for(&message, epoch) {
            Some(inbox) => {
                let _ = inbox.send(message);
            }
            // A request nobody owns would leave Codex waiting forever.
            None => {
                if let Inbound::Request { id, .. } = &message {
                    let running = server.running.lock().await;
                    if let Some(running) = running.as_ref().filter(|r| r.connection.epoch == epoch)
                    {
                        let _ = running.connection.client.respond_error(
                            id,
                            -32601,
                            "no Aeroric session owns this thread",
                        );
                    }
                }
            }
        }
    }
    if let Some(server) = server.upgrade() {
        server.exited(epoch).await;
    }
}

/// How the host starts the real process: the harness CLI with its fixed args
/// and the same environment layering a PTY launch gets (Q16).
#[derive(Clone, Debug, Default)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub unset_env: Vec<String>,
}

impl ProcessSpec {
    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);
        for key in &self.unset_env {
            command.env_remove(key);
        }
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }
}

/// `<program> --version` must pass [`version_supported`] (checked once), then
/// `<program> app-server` is spawned.
pub fn process_launcher(spec: ProcessSpec) -> Launcher {
    let checked = Arc::new(OnceLock::<()>::new());
    Arc::new(move || {
        if checked.get().is_none() {
            let output = spec
                .command()
                .arg("--version")
                .output()
                .map_err(|e| format!("无法启动 Codex（{}）：{e}", spec.program))?;
            version_supported(&String::from_utf8_lossy(&output.stdout))?;
            let _ = checked.set(());
        }
        let mut command = spec.command();
        command.arg("app-server").env("RUST_LOG", "error");
        Transport::spawn(tokio::process::Command::from(command))
            .map(|(transport, guard)| (transport, Some(guard)))
            .map_err(|e| format!("无法启动 Codex app-server（{}）：{e}", spec.program))
    })
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
