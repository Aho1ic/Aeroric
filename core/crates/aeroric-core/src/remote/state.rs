//! 远程服务的共享状态与服务句柄（P8-03 刀三b 自宿主上收）。
//!
//! `RemoteState` 收敛"谁在线、谁在配对、配置是什么、服务起没起"：
//! - `auth`/`keys`/`event_log`/`config` 等纯状态半边（宿主与 headless 共用）；
//! - `running`/`lifecycle` 是服务生命周期半边：`ServerHandle` 在 core 只有
//!   watcher 与 listener 管理，Tauri 事件桥的注销经 `post_shutdown` 钩子由
//!   宿主在启动时注入（core 不知 Tauri）。
//! - 宿主服务（PTY/存储/git/agent 配置）经 [`crate::remote::services::RemoteHostHandle`]
//!   注入，core 只持 trait 对象。

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};

use crate::remote::auth::AuthStore;
use crate::remote::config::{load_config, RemoteConfig};
use crate::remote::crypto;
use crate::remote::event_log::EventLog;
use crate::remote::protocol::{push_json, RpcVersion};
use crate::remote::services::RemoteHostHandle;

// ── 在线客户端注册表（自宿主 server.rs） ─────────────────────────────────────

/// 每连接出口队列上限。终端消费者持续落后时主动断连，重连后由快照恢复，
/// 避免慢客户端无限占用桌面内存。
pub const OUTBOUND_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct OutboundSender {
    sender: mpsc::Sender<Message>,
    disconnect: watch::Sender<bool>,
}

/// tungstenite 的 `Message` 在 core 以 re-export 形态出现（server.rs 也用它）。
pub use tokio_tungstenite::tungstenite::protocol::Message;

impl OutboundSender {
    pub(crate) fn new(sender: mpsc::Sender<Message>, disconnect: watch::Sender<bool>) -> Self {
        Self { sender, disconnect }
    }

    /// 非阻塞排队。队列满说明客户端已明显落后，断开后让其重连并重新快照。
    /// Err(()) 是刻意的布尔化信号（调用方只区分通/断），不携带负载。
    #[allow(clippy::result_unit_err)]
    pub fn send(&self, message: Message) -> Result<(), ()> {
        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                let _ = self.disconnect.send(true);
                Err(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
        }
    }

    pub(crate) fn disconnect(&self) {
        let _ = self.disconnect.send(true);
    }
}

pub struct ClientHandle {
    pub device_id: String,
    rpc_version: RpcVersion,
    sender: OutboundSender,
}

#[derive(Default)]
pub struct ClientRegistry {
    next_id: AtomicU64,
    inner: Mutex<HashMap<u64, ClientHandle>>,
}

impl ClientRegistry {
    pub fn register(
        &self,
        device_id: String,
        rpc_version: RpcVersion,
        sender: OutboundSender,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().insert(
            id,
            ClientHandle {
                device_id,
                rpc_version,
                sender,
            },
        );
        id
    }

    pub fn unregister(&self, client_id: u64) {
        self.inner.lock().remove(&client_id);
    }

    /// 事件桥按每条连接协商出的版本编码，避免 v2/v3 客户端相互污染。
    pub fn broadcast_push(&self, event: &str, seq: Option<u64>, data: &Value) {
        let clients = self.inner.lock();
        for handle in clients.values() {
            let text = push_json(handle.rpc_version, event, seq, data);
            let _ = handle.sender.send(Message::Text(text));
        }
    }

    /// 撤销设备时立即断开其全部连接(发送 Close 并丢弃 sender)。
    pub fn disconnect_device(&self, device_id: &str) {
        let clients = self.inner.lock();
        for handle in clients.values() {
            if handle.device_id == device_id {
                handle.sender.disconnect();
            }
        }
    }

    pub fn online_count(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn online_device_ids(&self) -> Vec<String> {
        let clients = self.inner.lock();
        clients.values().map(|h| h.device_id.clone()).collect()
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

/// Atomically gate online registration on the device still existing in the
/// auth store. Device revocation takes these locks in the same order
/// (auth → registry), so either revocation wins and no client is added, or
/// registration wins and revocation observes and disconnects that client.
pub(crate) fn register_authenticated_client(
    state: &RemoteState,
    device_id: &str,
    rpc_version: RpcVersion,
    sender: OutboundSender,
) -> Option<u64> {
    let auth = state.auth.lock();
    auth.contains_device(device_id).then(|| {
        state
            .clients
            .register(device_id.to_string(), rpc_version, sender)
    })
}

// ── LAN listener 可见范围（自宿主 server.rs） ────────────────────────────────

/// LAN listener 的可见范围。
///
/// 新安装且尚未配对时只在 loopback 上监听；生成配对二维码是显式的
/// 网络暴露动作。已有设备或用户配置的直连公网入口则直接使用全网绑定，
/// 以免重启后破坏既有连接。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListenerScope {
    Loopback,
    Network,
}

impl ListenerScope {
    pub fn bind_addr(self, port: u16) -> SocketAddr {
        let ip = match self {
            Self::Loopback => Ipv4Addr::LOCALHOST,
            Self::Network => Ipv4Addr::UNSPECIFIED,
        };
        SocketAddr::from((ip, port))
    }

    pub fn network_exposed(self) -> bool {
        matches!(self, Self::Network)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Loopback => "local-only",
            Self::Network => "network",
        }
    }
}

// ── 服务句柄（de-tauri 化：桥注销经 post_shutdown 钩子） ─────────────────────

pub struct ServerHandle {
    pub port: u16,
    /// OS 已确认的实际绑定地址。监听器恢复失败时为 None，避免把一个
    /// 已失效的服务继续报告为 running。
    pub(crate) bound_addr: Option<SocketAddr>,
    pub(crate) listener_scope: ListenerScope,
    pub(crate) shutdown: watch::Sender<bool>,
    /// 仅控制 LAN listener；relay、事件桥和已建立连接仍使用 `shutdown`。
    pub(crate) listener_shutdown: watch::Sender<bool>,
    pub(crate) listener_stopped: Option<oneshot::Receiver<()>>,
    /// 宿主在启动时注入的收尾钩子（事件桥 unlisten）。core 不知 Tauri。
    pub(crate) post_shutdown: Option<Box<dyn FnOnce() + Send>>,
}

impl ServerHandle {
    pub fn network_exposed(&self) -> bool {
        self.listener_scope.network_exposed() && self.bound_addr.is_some()
    }

    pub fn is_listening(&self) -> bool {
        self.bound_addr.is_some()
    }

    /// 测试断言绑定地址用（宿主集成测试跨 crate 调用）。
    #[doc(hidden)]
    pub fn bound_addr(&self) -> Option<SocketAddr> {
        self.bound_addr
    }

    /// 测试用：同步停机（宿主集成测试跨 crate 调用）。
    #[doc(hidden)]
    pub fn shutdown(mut self, state: &RemoteState) {
        self.signal_shutdown(state);
    }

    pub fn signal_shutdown(&mut self, state: &RemoteState) {
        let _ = self.shutdown.send(true);
        let _ = self.listener_shutdown.send(true);
        if let Some(hook) = self.post_shutdown.take() {
            hook();
        }
        state.clients.clear();
    }

    /// 完整停机并等待 listener 释放端口。控制面 rebind 需要这个确认，
    /// 否则同端口的 0.0.0.0 绑定可能与刚关闭的 loopback listener 竞争。
    pub async fn shutdown_and_wait(mut self, state: &RemoteState) {
        self.signal_shutdown(state);
        self.wait_for_listener_stop().await;
    }

    /// 将 LAN listener 原子地换到所需范围。
    ///
    /// 只替换 TCP accept loop，保持 relay、事件桥、认证表和既有连接不变。
    /// 目标绑定失败时尽力恢复原先范围；调用方可据此决定保留还是完整关闭
    /// 服务，避免撤销最后一个设备后意外继续保留广域监听。
    pub async fn ensure_listener_scope(
        &mut self,
        state: &Arc<RemoteState>,
        handle: &Arc<dyn RemoteHostHandle>,
        scope: ListenerScope,
    ) -> Result<(), String> {
        if self.is_listening() && self.listener_scope == scope {
            return Ok(());
        }

        let port = self.port;
        let previous_scope = self.listener_scope;
        self.stop_listener().await;
        match self
            .replace_listener(Arc::clone(state), Arc::clone(handle), scope, port)
            .await
        {
            Ok(()) => Ok(()),
            Err(scope_error) => {
                let recovery = self
                    .replace_listener(Arc::clone(state), Arc::clone(handle), previous_scope, port)
                    .await;
                match recovery {
                    Ok(()) => Err(format!(
                        "Failed to change the remote listener to {}: {scope_error}. The {} listener was restored.",
                        scope.label(),
                        previous_scope.label(),
                    )),
                    Err(recovery_error) => Err(format!(
                        "Failed to change the remote listener to {}: {scope_error}. The {} listener could not be restored: {recovery_error}",
                        scope.label(),
                        previous_scope.label(),
                    )),
                }
            }
        }
    }

    async fn stop_listener(&mut self) {
        let _ = self.listener_shutdown.send(true);
        self.wait_for_listener_stop().await;
        self.bound_addr = None;
    }

    async fn wait_for_listener_stop(&mut self) {
        if let Some(stopped) = self.listener_stopped.take() {
            let _ = stopped.await;
        }
    }

    async fn replace_listener(
        &mut self,
        state: Arc<RemoteState>,
        handle: Arc<dyn RemoteHostHandle>,
        scope: ListenerScope,
        port: u16,
    ) -> Result<(), String> {
        let listener = crate::remote::server::bind_listener(scope, port).await?;
        let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;
        let (listener_shutdown, listener_shutdown_rx) = watch::channel(false);
        let listener_stopped = crate::remote::server::spawn_accept_loop(
            Arc::clone(&state),
            Arc::clone(&handle),
            listener,
            self.shutdown.subscribe(),
            listener_shutdown_rx,
        );

        self.port = bound_addr.port();
        self.bound_addr = Some(bound_addr);
        self.listener_scope = scope;
        self.listener_shutdown = listener_shutdown;
        self.listener_stopped = Some(listener_stopped);
        Ok(())
    }

    pub fn set_post_shutdown(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.post_shutdown = Some(hook);
    }

    pub fn shutdown_sender(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
}

// ── 审批/任务请求注册表（自宿主 tasks_rpc.rs） ───────────────────────────────

/// 一次性任务请求的 oneshot 中转（刀二起 create/resume 原生化，broker 仅
/// 保留给 complete_task_request 命令兼容面与测试）。
pub struct TaskRequestBroker {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
}

impl Default for TaskRequestBroker {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl TaskRequestBroker {
    #[doc(hidden)]
    pub fn register(&self) -> (String, oneshot::Receiver<Result<Value, String>>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id.clone(), tx);
        (request_id, rx)
    }

    pub fn resolve(&self, request_id: &str, result: Result<Value, String>) -> Result<(), String> {
        let sender = self
            .pending
            .lock()
            .remove(request_id)
            .ok_or_else(|| "Task request expired or was already completed".to_string())?;
        sender
            .send(result)
            .map_err(|_| "Task request is no longer waiting".to_string())
    }

    #[doc(hidden)]
    pub fn cancel(&self, request_id: &str) {
        self.pending.lock().remove(request_id);
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub request_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Default)]
pub struct ApprovalRegistry {
    pending: Mutex<HashMap<String, ApprovalEntry>>,
}

#[derive(Clone)]
pub struct ApprovalEntry {
    pub request: ApprovalRequest,
    pub agent: String,
}

impl ApprovalRegistry {
    pub fn register(
        &self,
        task_id: &str,
        agent: &str,
        kind: &str,
        tool_name: Option<&str>,
    ) -> ApprovalRequest {
        let request = ApprovalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            kind: kind.to_string(),
            tool_name: tool_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        };
        self.pending.lock().insert(
            task_id.to_string(),
            ApprovalEntry {
                request: request.clone(),
                agent: agent.to_string(),
            },
        );
        request
    }

    pub fn clear(&self, task_id: &str) {
        self.pending.lock().remove(task_id);
    }

    pub fn get(&self, task_id: &str) -> Option<ApprovalRequest> {
        self.pending
            .lock()
            .get(task_id)
            .map(|entry| entry.request.clone())
    }

    pub fn take_matching(&self, task_id: &str, request_id: &str) -> Result<ApprovalEntry, String> {
        let mut pending = self.pending.lock();
        let Some(entry) = pending.get(task_id) else {
            return Err("Task is no longer waiting for an approval".to_string());
        };
        if entry.request.request_id != request_id {
            return Err("Approval request is stale".to_string());
        }
        pending
            .remove(task_id)
            .ok_or_else(|| "Approval request disappeared before it could be consumed".to_string())
    }

    pub fn restore(&self, task_id: String, entry: ApprovalEntry) {
        self.pending.lock().insert(task_id, entry);
    }
}

// ── 终端租约注册表（自宿主 terminal_streams.rs） ─────────────────────────────

const DEFAULT_DESKTOP_PTY_SIZE: (u16, u16) = (220, 50);

#[derive(Default)]
pub struct TerminalLeaseRegistry {
    inner: Mutex<HashMap<String, TerminalLease>>,
}

struct TerminalLease {
    subscribers: usize,
    original_size: Option<(u16, u16)>,
}

impl TerminalLeaseRegistry {
    pub fn acquire(&self, task_id: &str, current_size: Option<(u16, u16)>) {
        let mut leases = self.inner.lock();
        let lease = leases.entry(task_id.to_string()).or_insert(TerminalLease {
            subscribers: 0,
            original_size: Some(current_size.unwrap_or(DEFAULT_DESKTOP_PTY_SIZE)),
        });
        lease.subscribers += 1;
    }

    pub fn release(&self, task_id: &str) -> Option<(u16, u16)> {
        let mut leases = self.inner.lock();
        let lease = leases.get_mut(task_id)?;
        lease.subscribers = lease.subscribers.saturating_sub(1);
        if lease.subscribers > 0 {
            return None;
        }
        leases.remove(task_id).and_then(|lease| lease.original_size)
    }

    /// 测试断言订阅数用（宿主集成测试跨 crate 调用）。
    #[doc(hidden)]
    pub fn subscriber_count(&self, task_id: &str) -> usize {
        self.inner
            .lock()
            .get(task_id)
            .map(|lease| lease.subscribers)
            .unwrap_or(0)
    }
}

// ── RemoteState（自宿主 mod.rs） ─────────────────────────────────────────────

pub struct RemoteState {
    pub auth: Mutex<AuthStore>,
    /// 桌面初始化完成闸（刀二：替代 React 的 startupReadyRef）。
    /// 宿主 setup 尾置位；置位前的 task.create/resume 请求等待而非失败。
    pub ready: Arc<std::sync::OnceLock<()>>,
    /// 按 projectId 串行的任务变更队列（刀二：G05 的 host 侧等价判决）。
    pub task_queues: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub clients: Arc<ClientRegistry>,
    pub terminal_leases: Arc<TerminalLeaseRegistry>,
    pub approvals: Arc<ApprovalRegistry>,
    pub task_requests: Arc<TaskRequestBroker>,
    /// 推送事件日志:重连 watermark 补发源。
    pub event_log: Arc<EventLog>,
    pub config: Mutex<RemoteConfig>,
    /// 静态 X25519 密钥对(E2EE 握手与 relay hostId 的根)。
    pub keys: crypto::StaticKeys,
    /// 内存态(测试)不写审计日志。
    pub audit_enabled: bool,
    /// relay 客户端状态:off | connecting | online | error:<msg>。
    pub relay_state: Arc<Mutex<String>>,
    /// tokio Mutex:start/stop 需要跨 await 持锁,防止并发启动竞态。
    pub running: tokio::sync::Mutex<Option<ServerHandle>>,
    /// 串行化会改变监听器、端点配置或邀请码的控制面操作。邀请码必须在
    /// 已验证的 listener 仍为当前 listener 时创建，不能与 stop/restart
    /// 或配置重启交错。
    pub lifecycle: tokio::sync::Mutex<()>,
}

impl RemoteState {
    /// 桌面初始化完成。幂等。
    pub fn mark_ready(&self) {
        let _ = self.ready.set(());
    }

    // Default 语义不成立：new() 会读盘载入密钥对与配置，空壳默认值会掩盖载入失败。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        // 挂在整条 `let` 上：`expect` 是下面闭包的尾表达式，属性只能上提到这里。
        #[allow(clippy::expect_used, reason = "CSPRNG 不可用属不可恢复")]
        let keys = crypto::StaticKeys::load_or_create().unwrap_or_else(|err| {
            eprintln!("[remote] keypair load failed, using in-memory keys: {err}");
            crypto::StaticKeys::ephemeral().expect("CSPRNG unavailable")
        });
        Self {
            ready: Arc::new(std::sync::OnceLock::new()),
            task_queues: Mutex::new(HashMap::new()),
            auth: Mutex::new(AuthStore::load()),
            clients: Arc::new(ClientRegistry::default()),
            terminal_leases: Arc::new(TerminalLeaseRegistry::default()),
            approvals: Arc::new(ApprovalRegistry::default()),
            task_requests: Arc::new(TaskRequestBroker::default()),
            event_log: Arc::new(EventLog::default()),
            config: Mutex::new(load_config()),
            keys,
            audit_enabled: true,
            relay_state: Arc::new(Mutex::new("off".to_string())),
            running: tokio::sync::Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }

    /// 集成测试用:内存 AuthStore + 默认配置,不触碰任何真实文件。
    /// mock 环境没有真实桌面启动闸：默认就绪。
    #[doc(hidden)]
    pub fn new_in_memory() -> Self {
        let ready = Arc::new(std::sync::OnceLock::new());
        let _ = ready.set(());
        Self {
            ready,
            task_queues: Mutex::new(HashMap::new()),
            auth: Mutex::new(AuthStore::in_memory()),
            clients: Arc::new(ClientRegistry::default()),
            terminal_leases: Arc::new(TerminalLeaseRegistry::default()),
            approvals: Arc::new(ApprovalRegistry::default()),
            task_requests: Arc::new(TaskRequestBroker::default()),
            event_log: Arc::new(EventLog::default()),
            config: Mutex::new(RemoteConfig::default()),
            keys: crypto::StaticKeys::ephemeral().expect("CSPRNG unavailable"),
            audit_enabled: false,
            relay_state: Arc::new(Mutex::new("off".to_string())),
            running: tokio::sync::Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }
}

// ── 生命周期与监听范围策略（自宿主 mod.rs；宿主句柄 → (state, handle)） ─────

/// 应用启动时调用：按持久化配置决定是否自动拉起服务。
pub async fn init_async(state: &Arc<RemoteState>, handle: &Arc<dyn RemoteHostHandle>) {
    let enabled = state.config.lock().enabled;
    if enabled {
        let state = Arc::clone(state);
        let handle = Arc::clone(handle);
        tokio::spawn(async move {
            let _lifecycle = state.lifecycle.lock().await;
            // The setting may have changed while this startup task waited for
            // another control-plane operation.
            if !state.config.lock().enabled {
                return;
            }
            if let Err(err) = ensure_started(&state, &handle).await {
                eprintln!("[remote] auto-start failed: {err}");
            }
        });
    }
}

pub async fn ensure_started(
    state: &Arc<RemoteState>,
    handle: &Arc<dyn RemoteHostHandle>,
) -> Result<u16, String> {
    let mut running = state.running.lock().await;
    if let Some(server) = running.as_ref() {
        return Ok(server.port);
    }
    let port = state.config.lock().port;
    let scope = required_listener_scope(state);
    let server =
        crate::remote::server::start_with_scope(Arc::clone(state), Arc::clone(handle), port, scope)
            .await?;
    let bound = server.port;
    *running = Some(server);
    Ok(bound)
}

pub async fn ensure_stopped(state: &Arc<RemoteState>) {
    let mut running = state.running.lock().await;
    if let Some(server) = running.take() {
        server.shutdown_and_wait(state).await;
    }
}

/// Devices are the only implicit reason to keep a LAN listener open. A
/// configured direct public endpoint is an explicit opt-in; relay-only access
/// is outbound and therefore does not require a wide local TCP bind.
pub fn required_listener_scope(state: &RemoteState) -> ListenerScope {
    let has_public_endpoint = !state.config.lock().public_endpoints.is_empty();
    let has_paired_device = !state.auth.lock().devices().is_empty();
    if has_paired_device || has_public_endpoint {
        ListenerScope::Network
    } else {
        ListenerScope::Loopback
    }
}

/// Acquire the control-plane lock before reconciling a listener after a
/// successfully authenticated device. This also makes an in-flight pairing
/// observe a concurrent revoke before it is registered as online.
pub async fn reconcile_listener_scope_for_current_policy(
    state: &Arc<RemoteState>,
    handle: &Arc<dyn RemoteHostHandle>,
) -> Result<(), String> {
    let _lifecycle = state.lifecycle.lock().await;
    reconcile_listener_scope_locked(state, handle).await
}

/// Requires `RemoteState::lifecycle`. A rebind failure is resolved by closing
/// the complete server rather than leaving a restored listener at a wider
/// scope than the current device/configuration policy permits.
pub async fn reconcile_listener_scope_locked(
    state: &Arc<RemoteState>,
    handle: &Arc<dyn RemoteHostHandle>,
) -> Result<(), String> {
    let required_scope = required_listener_scope(state);
    let mut running = state.running.lock().await;
    let Some(mut server) = running.take() else {
        return Ok(());
    };

    match server
        .ensure_listener_scope(state, handle, required_scope)
        .await
    {
        Ok(()) if server.is_listening() => {
            *running = Some(server);
            Ok(())
        }
        Ok(()) => {
            server.shutdown_and_wait(state).await;
            Err("Remote server listener stopped unexpectedly while changing scope".to_string())
        }
        Err(error) => {
            // `ensure_listener_scope` may have restored the previous scope.
            // Do not retain that possibly wider listener after a revoke.
            server.shutdown_and_wait(state).await;
            Err(error)
        }
    }
}

/// Remove a device that was persisted while handling an invite but could not
/// be committed to the peer (for example, shutdown/rebind/register/reply
/// failure). The operation is idempotent with a concurrent user revoke and
/// uses the same lifecycle → auth → registry lock ordering as normal revoke.
pub async fn abort_unconfirmed_pairing(
    state: &Arc<RemoteState>,
    handle: &Arc<dyn RemoteHostHandle>,
    device_id: &str,
) {
    let _lifecycle = state.lifecycle.lock().await;
    let removed = {
        let mut auth = state.auth.lock();
        match auth.revoke(device_id) {
            Ok(removed) => {
                if removed {
                    state.clients.disconnect_device(device_id);
                }
                removed
            }
            Err(error) => {
                eprintln!("[remote] could not roll back unconfirmed pairing {device_id}: {error}");
                return;
            }
        }
    };
    if !removed {
        return;
    }
    if let Err(error) = reconcile_listener_scope_locked(state, handle).await {
        eprintln!("[remote] server closed after rolling back unconfirmed pairing: {error}");
    }
}
