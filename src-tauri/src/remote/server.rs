//! 远程 WS 服务:监听、E2EE 握手、认证、请求循环、在线客户端注册表。
//!
//! 生命周期:`start()` 绑定端口并 spawn accept loop,返回 ServerHandle;
//! `ServerHandle::shutdown()` 广播关闭信号,所有连接任务随之退出。
//! 每个连接:10s 内完成明文 hello → hello_ack 的 E2EE 握手(crypto.rs),
//! 再在 10s 内于加密通道内完成 auth(invite 配对或 device token),
//! 通过后进入请求循环;30s 心跳 Ping,75s 无入站即断开。
//! 会话主循环 `serve_ws` 对底层流泛型:LAN 直连(服务端 accept)与
//! relay 数据连接(客户端 connect)走同一条代码路径。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use super::auth::AuthOutcome;
use super::protocol::{parse_request, RpcResponse, PROTOCOL_VERSION};
use super::{audit, crypto, orca_crypto, orca_rpc, rpc, RemoteState};

const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound sockets before the WebSocket upgrade/authentication completes. A
/// network peer must not be able to turn slow HTTP headers into unbounded tasks
/// and file-descriptor pressure.
const MAX_CONNECTIONS: usize = 256;
const HTTP_UPGRADE_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_DISCONNECT: Duration = Duration::from_secs(75);
/// 单条消息上限;控制面为小 JSON、终端帧 ≤64KB+密文开销,1MB 足够且防滥用。
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// 每连接出口队列上限。终端消费者持续落后时主动断连，重连后由快照恢复，
/// 避免慢客户端无限占用桌面内存。
const OUTBOUND_QUEUE_CAPACITY: usize = 256;

// ── 在线客户端注册表 ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct OutboundSender {
    sender: mpsc::Sender<Message>,
    disconnect: watch::Sender<bool>,
}

impl OutboundSender {
    fn new(sender: mpsc::Sender<Message>, disconnect: watch::Sender<bool>) -> Self {
        Self { sender, disconnect }
    }

    /// 非阻塞排队。队列满说明客户端已明显落后，断开后让其重连并重新快照。
    pub(crate) fn send(&self, message: Message) -> Result<(), ()> {
        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                let _ = self.disconnect.send(true);
                Err(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
        }
    }

    fn disconnect(&self) {
        let _ = self.disconnect.send(true);
    }
}

pub struct ClientHandle {
    pub device_id: String,
    sender: OutboundSender,
}

#[derive(Default)]
pub struct ClientRegistry {
    next_id: AtomicU64,
    inner: Mutex<HashMap<u64, ClientHandle>>,
}

impl ClientRegistry {
    fn register(&self, device_id: String, sender: OutboundSender) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .lock()
            .insert(id, ClientHandle { device_id, sender });
        id
    }

    fn unregister(&self, client_id: u64) {
        self.inner.lock().remove(&client_id);
    }

    /// 事件桥用:向所有在线客户端推送 text 帧。
    pub fn broadcast_text(&self, text: &str) {
        let clients = self.inner.lock();
        for handle in clients.values() {
            let _ = handle.sender.send(Message::Text(text.to_string()));
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

    fn clear(&self) {
        self.inner.lock().clear();
    }
}

/// Atomically gate online registration on the device still existing in the
/// auth store. Device revocation takes these locks in the same order
/// (auth → registry), so either revocation wins and no client is added, or
/// registration wins and revocation observes and disconnects that client.
fn register_authenticated_client(
    state: &RemoteState,
    device_id: &str,
    sender: OutboundSender,
) -> Option<u64> {
    let auth = state.auth.lock();
    auth.contains_device(device_id)
        .then(|| state.clients.register(device_id.to_string(), sender))
}

// ── 服务生命周期 ─────────────────────────────────────────────────────────────

/// LAN listener 的可见范围。
///
/// 新安装且尚未配对时只在 loopback 上监听；生成配对二维码是显式的
/// 网络暴露动作。已有设备或用户配置的直连公网入口则直接使用全网绑定，
/// 以免重启后破坏既有连接。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ListenerScope {
    Loopback,
    Network,
}

impl ListenerScope {
    fn bind_addr(self, port: u16) -> SocketAddr {
        let ip = match self {
            Self::Loopback => Ipv4Addr::LOCALHOST,
            Self::Network => Ipv4Addr::UNSPECIFIED,
        };
        SocketAddr::from((ip, port))
    }

    pub(crate) fn network_exposed(self) -> bool {
        matches!(self, Self::Network)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Loopback => "local-only",
            Self::Network => "network",
        }
    }
}

pub struct ServerHandle {
    pub port: u16,
    /// OS 已确认的实际绑定地址。监听器恢复失败时为 None，避免把一个
    /// 已失效的服务继续报告为 running。
    bound_addr: Option<SocketAddr>,
    listener_scope: ListenerScope,
    shutdown: watch::Sender<bool>,
    /// 仅控制 LAN listener；relay、事件桥和已建立连接仍使用 `shutdown`。
    listener_shutdown: watch::Sender<bool>,
    listener_stopped: Option<oneshot::Receiver<()>>,
    listener_ids: Vec<tauri::EventId>,
}

impl ServerHandle {
    pub(crate) fn network_exposed(&self) -> bool {
        self.listener_scope.network_exposed() && self.bound_addr.is_some()
    }

    pub(crate) fn is_listening(&self) -> bool {
        self.bound_addr.is_some()
    }

    #[cfg(test)]
    pub(crate) fn bound_addr(&self) -> Option<SocketAddr> {
        self.bound_addr
    }

    fn signal_shutdown<R: Runtime>(&self, app: &AppHandle<R>) {
        let _ = self.shutdown.send(true);
        let _ = self.listener_shutdown.send(true);
        for id in &self.listener_ids {
            app.unlisten(*id);
        }
        app.state::<RemoteState>().clients.clear();
    }

    #[cfg(test)]
    pub fn shutdown<R: Runtime>(self, app: &AppHandle<R>) {
        self.signal_shutdown(app);
    }

    /// 完整停机并等待 listener 释放端口。控制面 rebind 需要这个确认，
    /// 否则同端口的 0.0.0.0 绑定可能与刚关闭的 loopback listener 竞争。
    pub async fn shutdown_and_wait<R: Runtime>(mut self, app: &AppHandle<R>) {
        self.signal_shutdown(app);
        self.wait_for_listener_stop().await;
    }

    /// 将 LAN listener 原子地换到所需范围。
    ///
    /// 只替换 TCP accept loop，保持 relay、事件桥、认证表和既有连接不变。
    /// 目标绑定失败时尽力恢复原先范围；调用方可据此决定保留还是完整关闭
    /// 服务，避免撤销最后一个设备后意外继续保留广域监听。
    pub(crate) async fn ensure_listener_scope<R: Runtime>(
        &mut self,
        app: AppHandle<R>,
        scope: ListenerScope,
    ) -> Result<(), String> {
        if self.is_listening() && self.listener_scope == scope {
            return Ok(());
        }

        let port = self.port;
        let previous_scope = self.listener_scope;
        self.stop_listener().await;
        match self.replace_listener(app.clone(), scope, port).await {
            Ok(()) => Ok(()),
            Err(scope_error) => {
                let recovery = self.replace_listener(app, previous_scope, port).await;
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

    async fn replace_listener<R: Runtime>(
        &mut self,
        app: AppHandle<R>,
        scope: ListenerScope,
        port: u16,
    ) -> Result<(), String> {
        let listener = bind_listener(scope, port).await?;
        let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;
        let (listener_shutdown, listener_shutdown_rx) = watch::channel(false);
        let listener_stopped = spawn_accept_loop(
            app,
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
}

/// 默认的低层启动入口保持全网监听，供 LAN roundtrip 测试和显式网络
/// 调用者使用。桌面远程功能通过 `start_with_scope` 选择最小暴露范围。
#[cfg(test)]
pub async fn start<R: Runtime>(app: AppHandle<R>, port: u16) -> Result<ServerHandle, String> {
    start_with_scope(app, port, ListenerScope::Network).await
}

/// 绑定指定范围的端口并启动服务生命周期。
pub(crate) async fn start_with_scope<R: Runtime>(
    app: AppHandle<R>,
    port: u16,
    scope: ListenerScope,
) -> Result<ServerHandle, String> {
    let listener = bind_listener(scope, port).await?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (listener_shutdown, listener_shutdown_rx) = watch::channel(false);
    let listener_ids = super::events_bridge::attach(&app);

    // relay 出站注册与 WS 服务同生命周期(共享 shutdown)
    let (relay_url, relay_token) = {
        let state = app.state::<RemoteState>();
        let cfg = state.config.lock();
        (cfg.relay_url.clone(), cfg.relay_token.clone())
    };
    if let Some(relay_url) = relay_url.filter(|u| !u.trim().is_empty()) {
        match super::normalize_relay_url(&relay_url) {
            Ok(relay_url)
                if relay_token
                    .as_deref()
                    .is_some_and(|token| !token.trim().is_empty()) =>
            {
                super::relay_client::spawn(
                    app.clone(),
                    relay_url,
                    relay_token,
                    shutdown_rx.clone(),
                );
            }
            Ok(_) => {
                *app.state::<RemoteState>().relay_state.lock() =
                    "error:relay token is required".to_string();
            }
            Err(error) => {
                *app.state::<RemoteState>().relay_state.lock() = format!("error:{error}");
            }
        }
    }

    let listener_stopped =
        spawn_accept_loop(app.clone(), listener, shutdown_rx, listener_shutdown_rx);

    Ok(ServerHandle {
        port: bound_addr.port(),
        bound_addr: Some(bound_addr),
        listener_scope: scope,
        shutdown: shutdown_tx,
        listener_shutdown,
        listener_stopped: Some(listener_stopped),
        listener_ids,
    })
}

async fn bind_listener(scope: ListenerScope, port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(scope.bind_addr(port))
        .await
        .map_err(|e| format!("Failed to bind port {port}: {e}"))
}

fn spawn_accept_loop<R: Runtime>(
    app: AppHandle<R>,
    listener: TcpListener,
    shutdown: watch::Receiver<bool>,
    listener_shutdown: watch::Receiver<bool>,
) -> oneshot::Receiver<()> {
    let (stopped_tx, stopped_rx) = oneshot::channel();
    tauri::async_runtime::spawn(async move {
        accept_loop(app, listener, shutdown, listener_shutdown).await;
        let _ = stopped_tx.send(());
    });
    stopped_rx
}

async fn accept_loop<R: Runtime>(
    app: AppHandle<R>,
    listener: TcpListener,
    shutdown: watch::Receiver<bool>,
    listener_shutdown: watch::Receiver<bool>,
) {
    let connection_slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let mut shutdown_wait = shutdown.clone();
        let mut listener_shutdown_wait = listener_shutdown.clone();
        tokio::select! {
            _ = shutdown_wait.changed() => break,
            _ = listener_shutdown_wait.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                // 终端交互全是单字符小包:Nagle 会把它们攒到前一个包被 ACK 才发,
                // 与对端的延迟 ACK 叠加时每次按键最多多等约 40ms。远程链路
                // (WireGuard 等)RTT 本身就高,这段等待会直接体现为打字延迟。
                let _ = stream.set_nodelay(true);
                let Ok(slot) = connection_slots.clone().try_acquire_owned() else {
                    // Refuse excess sockets immediately; the peer can retry after
                    // an existing client finishes or is closed.
                    drop(stream);
                    continue;
                };
                let conn_app = app.clone();
                let conn_shutdown = shutdown.clone();
                tauri::async_runtime::spawn(async move {
                    let _slot = slot;
                    handle_connection(conn_app, stream, peer.ip(), conn_shutdown).await;
                });
            }
        }
    }
}

// ── 单连接处理 ───────────────────────────────────────────────────────────────

async fn handle_connection<R: Runtime>(
    app: AppHandle<R>,
    stream: TcpStream,
    peer: IpAddr,
    mut shutdown: watch::Receiver<bool>,
) {
    if *shutdown.borrow() {
        return;
    }
    let config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    };
    let upgraded = tokio::select! {
        biased;
        _ = shutdown.changed() => return,
        result = tokio::time::timeout(
            HTTP_UPGRADE_TIMEOUT,
            tokio_tungstenite::accept_async_with_config(stream, Some(config)),
        ) => match result {
            Ok(result) => result,
            Err(_) => return,
        },
    };
    let Ok(ws) = upgraded else {
        return;
    };
    if *shutdown.borrow() {
        return;
    }
    serve_ws(app, ws, peer, ConnectionContext::Direct, shutdown).await;
}

#[derive(Clone, Debug)]
pub(crate) enum ConnectionContext {
    Direct,
    Relay { host_id: String },
}

enum NegotiatedSession {
    Aeroric(crypto::SessionCrypto),
    Orca(orca_crypto::SessionCrypto),
}

enum DecryptedMessage {
    Control(String),
    Terminal(Vec<u8>),
    Unknown,
}

impl NegotiatedSession {
    fn is_orca(&self) -> bool {
        matches!(self, Self::Orca(_))
    }

    fn orca_transcript_hash(&self) -> Option<&str> {
        match self {
            Self::Aeroric(_) => None,
            Self::Orca(session) => Some(session.transcript_hash_b64()),
        }
    }

    fn decrypt_message(&mut self, message: &Message) -> Result<DecryptedMessage, String> {
        match self {
            Self::Aeroric(session) => match message {
                Message::Binary(bytes) => match session.decrypt_frame(bytes) {
                    Ok((crypto::KIND_CTRL, plain)) => String::from_utf8(plain)
                        .map(DecryptedMessage::Control)
                        .map_err(|_| "control payload is not UTF-8".to_string()),
                    Ok((crypto::KIND_TERMINAL, plain)) => Ok(DecryptedMessage::Terminal(plain)),
                    Ok(_) => Ok(DecryptedMessage::Unknown),
                    Err(error) => Err(error),
                },
                _ => Ok(DecryptedMessage::Unknown),
            },
            Self::Orca(session) => match message {
                Message::Text(text) => {
                    let plain = session.open_text_base64(text.as_ref())?;
                    String::from_utf8(plain)
                        .map(DecryptedMessage::Control)
                        .map_err(|_| "control payload is not UTF-8".to_string())
                }
                Message::Binary(bytes) => {
                    session.open_binary(bytes).map(DecryptedMessage::Terminal)
                }
                _ => Ok(DecryptedMessage::Unknown),
            },
        }
    }

    fn encrypt_message(&mut self, kind: u8, plain: &[u8]) -> Result<Message, String> {
        match self {
            Self::Aeroric(session) => session.encrypt_frame(kind, plain).map(Message::Binary),
            Self::Orca(session) if kind == crypto::KIND_CTRL => {
                let text = std::str::from_utf8(plain)
                    .map_err(|_| "control payload is not UTF-8".to_string())?;
                session.seal_text_base64(text).map(Message::Text)
            }
            Self::Orca(session) if kind == crypto::KIND_TERMINAL => {
                session.seal_binary(plain).map(Message::Binary)
            }
            Self::Orca(_) => Err("unknown Orca payload kind".to_string()),
        }
    }
}

/// 加密封包后发送;返回 false 表示连接已不可用。
async fn send_encrypted<S>(
    sink: &mut SplitSink<WebSocketStream<S>, Message>,
    session: &mut NegotiatedSession,
    kind: u8,
    plain: &[u8],
) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match session.encrypt_message(kind, plain) {
        Ok(frame) => sink.send(frame).await.is_ok(),
        Err(_) => false,
    }
}

/// The first successful auth reply may make a newly persisted pairing visible
/// to the UI. Unlike ordinary request replies, it must yield promptly to
/// shutdown so a stop cannot be delayed behind a blocked socket write.
async fn send_encrypted_until_shutdown<S>(
    sink: &mut SplitSink<WebSocketStream<S>, Message>,
    session: &mut NegotiatedSession,
    kind: u8,
    plain: &[u8],
    shutdown: &mut watch::Receiver<bool>,
) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if *shutdown.borrow() {
        return false;
    }
    let frame = match session.encrypt_message(kind, plain) {
        Ok(frame) => frame,
        Err(_) => return false,
    };
    tokio::select! {
        biased;
        _ = shutdown.changed() => false,
        result = sink.send(frame) => result.is_ok(),
    }
}

/// Authentication has persisted successfully, but a newly paired device is
/// not announced until its encrypted reply has reached the peer. Keeping the
/// distinction explicit lets every failure between persistence and that reply
/// remove only the device created by this connection.
struct AuthenticatedFirstMessage {
    device_id: String,
    reply: String,
    paired_device_name: Option<String>,
}

async fn abort_pending_pairing<R: Runtime>(
    app: &AppHandle<R>,
    authentication: &AuthenticatedFirstMessage,
) {
    if authentication.paired_device_name.is_some() {
        super::abort_unconfirmed_pairing(app, &authentication.device_id).await;
    }
}

/// Publish the pairing only after the peer has received its encrypted device
/// token. A concurrent revoke may already have removed the device; in that
/// case there is no successful pairing left to announce.
fn commit_pending_pairing<R: Runtime>(
    app: &AppHandle<R>,
    peer: IpAddr,
    authentication: &AuthenticatedFirstMessage,
) {
    let Some(device_name) = authentication.paired_device_name.as_deref() else {
        return;
    };
    let state = app.state::<RemoteState>();
    if !state.auth.lock().contains_device(&authentication.device_id) {
        return;
    }
    if state.audit_enabled {
        audit::log(
            "device-paired",
            json!({
                "peer": peer.to_string(),
                "deviceId": authentication.device_id,
                "deviceName": device_name,
            }),
        );
    }
    // 通知设置页:配对成功(二维码视图切到完成态 + 刷新设备列表)。
    let _ = app.emit(
        "remote-device-paired",
        json!({
            "deviceId": authentication.device_id,
            "deviceName": device_name,
        }),
    );
}

/// Linearize the visible pairing commit with server stop. `remote_server_stop`
/// takes `lifecycle` before sending shutdown, so after the auth reply either
/// this commit happens before stop starts or it observes the stop and leaves
/// the device for its caller to roll back.
async fn commit_pending_pairing_unless_stopped<R: Runtime>(
    app: &AppHandle<R>,
    peer: IpAddr,
    authentication: &AuthenticatedFirstMessage,
    shutdown: &watch::Receiver<bool>,
) -> bool {
    if authentication.paired_device_name.is_none() {
        return true;
    }
    let state = app.state::<RemoteState>();
    let _lifecycle = state.lifecycle.lock().await;
    if *shutdown.borrow() {
        return false;
    }
    commit_pending_pairing(app, peer, authentication);
    true
}

/// 会话主循环:E2EE 握手 → 认证 → 请求循环。LAN 与 relay 数据连接共用。
pub(crate) async fn serve_ws<R, S>(
    app: AppHandle<R>,
    ws: WebSocketStream<S>,
    peer: IpAddr,
    context: ConnectionContext,
    mut shutdown: watch::Receiver<bool>,
) where
    R: Runtime,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = ws.split();
    let audit_enabled = app.state::<RemoteState>().audit_enabled;
    if *shutdown.borrow() {
        let _ = sink.close().await;
        return;
    }

    // ── E2EE 握手:第一条明文 text 必须是 hello ──
    let hello = tokio::select! {
        biased;
        _ = shutdown.changed() => {
            let _ = sink.close().await;
            return;
        }
        received = tokio::time::timeout(AUTH_TIMEOUT, stream.next()) => match received {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                let _ = sink.close().await;
                return;
            }
        },
    };
    if *shutdown.borrow() {
        let _ = sink.close().await;
        return;
    }
    let mut session = match respond_hello(&app, &hello, &context) {
        Ok((ack, session)) => {
            if *shutdown.borrow() {
                let _ = sink.close().await;
                return;
            }
            if sink.send(Message::Text(ack)).await.is_err() {
                return;
            }
            if *shutdown.borrow() {
                let _ = sink.close().await;
                return;
            }
            session
        }
        Err(reply) => {
            if audit_enabled {
                audit::log("handshake-rejected", json!({ "peer": peer.to_string() }));
            }
            let _ = sink.send(Message::Text(reply)).await;
            let _ = sink.close().await;
            return;
        }
    };

    // ── 认证:第一条加密控制帧必须是 auth ──
    let first_ctrl = loop {
        let received = tokio::select! {
            biased;
            _ = shutdown.changed() => {
                let _ = sink.close().await;
                return;
            }
            received = tokio::time::timeout(AUTH_TIMEOUT, stream.next()) => received,
        };
        match received {
            Ok(Some(Ok(message @ Message::Binary(_))))
            | Ok(Some(Ok(message @ Message::Text(_)))) => match session.decrypt_message(&message) {
                Ok(DecryptedMessage::Control(text)) => break text,
                Ok(_) | Err(_) => {
                    let _ = sink.close().await;
                    return;
                }
            },
            Ok(Some(Ok(Message::Ping(payload)))) => {
                let _ = sink.send(Message::Pong(payload)).await;
            }
            _ => {
                let _ = sink.close().await;
                return;
            }
        }
    };
    if *shutdown.borrow() {
        let _ = sink.close().await;
        return;
    }
    // A server stop takes the same lifecycle lock before publishing its
    // shutdown signal. Holding it only for this synchronous auth transition
    // linearizes device persistence against stop without retaining a mutex
    // across network I/O or any other await.
    let orca_transcript_hash = session.orca_transcript_hash().map(str::to_owned);
    let authentication = {
        let state = app.state::<RemoteState>();
        let _lifecycle = state.lifecycle.lock().await;
        (!*shutdown.borrow()).then(|| {
            authenticate_first_message(&app, peer, &first_ctrl, orca_transcript_hash.as_deref())
        })
    };
    let Some(authentication) = authentication else {
        let _ = sink.close().await;
        return;
    };
    let authentication = match authentication {
        Ok(authentication) => authentication,
        Err(reply) => {
            let _ =
                send_encrypted(&mut sink, &mut session, crypto::KIND_CTRL, reply.as_bytes()).await;
            let _ = sink.close().await;
            return;
        }
    };
    if *shutdown.borrow() {
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    // A newly paired device is the only authentication outcome that can widen
    // the listener. Existing device-token authentication cannot change that
    // policy, so avoid an unnecessary rebind on every reconnect.
    if authentication.paired_device_name.is_some()
        && super::reconcile_listener_scope_for_current_policy(&app)
            .await
            .is_err()
    {
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }
    if *shutdown.borrow() {
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    // Register before awaiting the auth reply, while holding the same auth
    // lock used by revocation. This closes the otherwise reachable window in
    // which authentication succeeded, revocation found no registered client,
    // and this task subsequently registered a live session.
    let state = app.state::<RemoteState>();
    let clients = state.clients.clone();
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOUND_QUEUE_CAPACITY);
    let (disconnect_tx, mut disconnect_rx) = watch::channel(false);
    let outbound = OutboundSender::new(tx, disconnect_tx);
    if *shutdown.borrow() {
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }
    let client_id =
        register_authenticated_client(&state, &authentication.device_id, outbound.clone());
    let Some(client_id) = client_id else {
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    };
    // A revoke can win immediately after registration. Check its disconnect
    // signal before handing a newly paired device its token.
    if *shutdown.borrow() || *disconnect_rx.borrow() {
        clients.unregister(client_id);
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    if !send_encrypted_until_shutdown(
        &mut sink,
        &mut session,
        crypto::KIND_CTRL,
        authentication.reply.as_bytes(),
        &mut shutdown,
    )
    .await
    {
        clients.unregister(client_id);
        abort_pending_pairing(&app, &authentication).await;
        return;
    }

    let pairing_committed =
        commit_pending_pairing_unless_stopped(&app, peer, &authentication, &shutdown).await;
    if !pairing_committed {
        clients.unregister(client_id);
        abort_pending_pairing(&app, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    if *shutdown.borrow() || *disconnect_rx.borrow() {
        clients.unregister(client_id);
        let _ = sink.close().await;
        return;
    }

    // ── 已注册，进入请求循环 ──
    let mut terminal = super::terminal_streams::TerminalStreams::new(app.clone(), outbound);

    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_inbound = Instant::now();

    loop {
        if *shutdown.borrow() || *disconnect_rx.borrow() {
            let _ = sink.send(Message::Close(None)).await;
            break;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
            changed = disconnect_rx.changed() => {
                if changed.is_err() || *disconnect_rx.borrow() {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            }
            outbound = rx.recv() => {
                match outbound {
                    Some(Message::Close(frame)) => {
                        let _ = sink.send(Message::Close(frame)).await;
                        break;
                    }
                    // 注册表/事件桥投递明文(Text=控制 JSON,Binary=终端帧),
                    // 出口在此按连接各自的会话密钥加密
                    Some(Message::Text(text)) => {
                        if !send_encrypted(&mut sink, &mut session, crypto::KIND_CTRL, text.as_bytes()).await {
                            break;
                        }
                    }
                    Some(Message::Binary(bytes)) => {
                        if !send_encrypted(&mut sink, &mut session, crypto::KIND_TERMINAL, &bytes).await {
                            break;
                        }
                    }
                    Some(msg) => {
                        if sink.send(msg).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            inbound = stream.next() => {
                match inbound {
                    Some(Ok(message @ Message::Binary(_))) | Some(Ok(message @ Message::Text(_))) => {
                        last_inbound = Instant::now();
                        match session.decrypt_message(&message) {
                            Ok(DecryptedMessage::Control(text)) => {
                                let reply = if session.is_orca() {
                                    orca_rpc::handle(&app, &text).await
                                } else {
                                    handle_request(&app, &text).await
                                };
                                if !send_encrypted(&mut sink, &mut session, crypto::KIND_CTRL, reply.as_bytes()).await {
                                    break;
                                }
                            }
                            Ok(DecryptedMessage::Terminal(plain)) => terminal.handle_frame(&plain),
                            // Unknown Aeroric kinds remain forward-compatible;
                            // authenticated Orca kinds are fully enumerated by
                            // their WebSocket payload type.
                            Ok(DecryptedMessage::Unknown) => {}
                            Err(_) => break,
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        last_inbound = Instant::now();
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_inbound = Instant::now();
                    }
                    // 握手完成后不允许明文 text
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {
                        last_inbound = Instant::now();
                    }
                    Some(Err(_)) => break,
                }
            }
            _ = keepalive.tick() => {
                if last_inbound.elapsed() > IDLE_DISCONNECT {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
                if sink.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
        }
    }

    terminal.abort_all();
    clients.unregister(client_id);
}

fn hello_error(message: &str) -> String {
    json!({ "v": PROTOCOL_VERSION, "type": "hello_error", "error": message }).to_string()
}

/// 处理明文 hello。Ok=(hello_ack JSON, 会话密钥);Err=hello_error JSON。
fn respond_hello<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    context: &ConnectionContext,
) -> Result<(String, NegotiatedSession), String> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Err(hello_error("Malformed hello"));
    };
    let version = parsed.get("v").and_then(Value::as_u64).unwrap_or(0);
    let msg_type = parsed.get("type").and_then(Value::as_str).unwrap_or("");
    if msg_type == "e2ee_hello" && version == 2 {
        let expected = match context {
            ConnectionContext::Direct => orca_crypto::Context {
                transport: orca_crypto::Transport::Direct,
                relay_host_id: None,
            },
            ConnectionContext::Relay { host_id } => orca_crypto::Context {
                transport: orca_crypto::Transport::Relay,
                relay_host_id: Some(host_id.clone()),
            },
        };
        let state = app.state::<RemoteState>();
        let accept = orca_crypto::respond_handshake(&state.keys, raw, &expected)
            .map_err(|error| hello_error(&error))?;
        return Ok((accept.ready_json, NegotiatedSession::Orca(accept.session)));
    }
    if version != PROTOCOL_VERSION as u64 || msg_type != "hello" {
        return Err(hello_error(&format!(
            "Unsupported protocol (server speaks v{PROTOCOL_VERSION} with E2EE); update the mobile app"
        )));
    }
    let Some(client_pub) = parsed.get("pub").and_then(Value::as_str) else {
        return Err(hello_error("hello requires pub"));
    };
    let state = app.state::<RemoteState>();
    let accept = crypto::respond_handshake(&state.keys, client_pub).map_err(|e| hello_error(&e))?;
    let ack = json!({
        "v": PROTOCOL_VERSION,
        "type": "hello_ack",
        "pub": accept.server_eph_pub_b64,
        "confirm": accept.confirm_b64,
    });
    Ok((ack.to_string(), NegotiatedSession::Aeroric(accept.session)))
}

/// 处理首条 auth 消息。配对的落盘发生在这里，但其审计/UI 提交会等到
/// 加密 auth 回包成功送出后；此前失败由调用方按 device ID 回滚。
fn authenticate_first_message<R: Runtime>(
    app: &AppHandle<R>,
    peer: IpAddr,
    raw: &str,
    orca_transcript_hash: Option<&str>,
) -> Result<AuthenticatedFirstMessage, String> {
    if let Some(expected_hash) = orca_transcript_hash {
        let parsed: Value = serde_json::from_str(raw).map_err(|_| {
            json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string()
        })?;
        let Some(object) = parsed.as_object() else {
            return Err(
                json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string(),
            );
        };
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        if keys != ["deviceToken", "transcriptHashB64", "type", "v"]
            || parsed.get("type").and_then(Value::as_str) != Some("e2ee_auth")
            || parsed.get("v").and_then(Value::as_u64) != Some(2)
            || parsed.get("transcriptHashB64").and_then(Value::as_str) != Some(expected_hash)
        {
            return Err(
                json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string(),
            );
        }
        let Some(device_token) = parsed.get("deviceToken").and_then(Value::as_str) else {
            return Err(
                json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string(),
            );
        };
        let state = app.state::<RemoteState>();
        let outcome = state
            .auth
            .lock()
            .authenticate(peer, None, Some(device_token), None);
        return match outcome {
            Ok(AuthOutcome::Authenticated { device_id }) => {
                if state.audit_enabled {
                    audit::log(
                        "auth-ok",
                        json!({ "peer": peer.to_string(), "deviceId": device_id, "protocol": "orca" }),
                    );
                }
                Ok(AuthenticatedFirstMessage {
                    device_id,
                    reply: json!({
                        "type": "e2ee_authenticated",
                        "v": 2,
                        "transcriptHashB64": expected_hash,
                    })
                    .to_string(),
                    paired_device_name: None,
                })
            }
            Ok(AuthOutcome::Paired { .. }) => {
                Err(json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string())
            }
            Err(error) => {
                if state.audit_enabled {
                    audit::log(
                        "auth-failed",
                        json!({ "peer": peer.to_string(), "error": error, "protocol": "orca" }),
                    );
                }
                Err(json!({
                    "type": "e2ee_error",
                    "error": { "code": if error == "Unknown device token" { "unauthorized" } else { "bad_auth" } }
                })
                .to_string())
            }
        };
    }
    let req = match parse_request(raw) {
        Ok(req) => req,
        Err(err) => return Err(RpcResponse::failure(0, err).to_json()),
    };
    if req.method != "auth" {
        return Err(RpcResponse::failure(req.id, "First message must be auth").to_json());
    }
    let invite = req.params.get("invite").and_then(Value::as_str);
    let device_token = req.params.get("deviceToken").and_then(Value::as_str);
    let device_name = req.params.get("deviceName").and_then(Value::as_str);

    let state = app.state::<RemoteState>();
    let outcome = state
        .auth
        .lock()
        .authenticate(peer, invite, device_token, device_name);
    let audit_enabled = state.audit_enabled;

    let host = json!({
        "name": super::host_name(),
        "version": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
    });

    match outcome {
        Ok(AuthOutcome::Paired {
            device_id,
            device_name,
            device_token,
        }) => {
            let reply = RpcResponse::success(
                req.id,
                json!({
                    "deviceId": device_id,
                    "deviceToken": device_token,
                    "host": host,
                }),
            );
            Ok(AuthenticatedFirstMessage {
                device_id,
                reply: reply.to_json(),
                paired_device_name: Some(device_name),
            })
        }
        Ok(AuthOutcome::Authenticated { device_id, .. }) => {
            if audit_enabled {
                audit::log(
                    "auth-ok",
                    json!({ "peer": peer.to_string(), "deviceId": device_id }),
                );
            }
            let reply =
                RpcResponse::success(req.id, json!({ "deviceId": device_id, "host": host }));
            Ok(AuthenticatedFirstMessage {
                device_id,
                reply: reply.to_json(),
                paired_device_name: None,
            })
        }
        Err(err) => {
            if audit_enabled {
                audit::log(
                    "auth-failed",
                    json!({ "peer": peer.to_string(), "error": err }),
                );
            }
            Err(RpcResponse::failure(req.id, err).to_json())
        }
    }
}

async fn handle_request<R: Runtime>(app: &AppHandle<R>, raw: &str) -> String {
    let req = match parse_request(raw) {
        Ok(req) => req,
        Err(err) => return RpcResponse::failure(0, err).to_json(),
    };
    // 已认证连接不允许再发 auth(避免把配对逻辑暴露在长连接里)。
    if req.method == "auth" {
        return RpcResponse::failure(req.id, "Already authenticated").to_json();
    }
    match rpc::dispatch(app, &req.method, req.params).await {
        Ok(result) => RpcResponse::success(req.id, result).to_json(),
        Err(err) => RpcResponse::failure(req.id, err).to_json(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tauri::{Listener, Manager};
    use tokio_tungstenite::tungstenite::protocol::Role;

    use super::*;
    use crate::remote::auth::AuthOutcome;

    fn pair_test_device(state: &RemoteState) -> String {
        let invite = state.auth.lock().create_invite().expect("create invite");
        match state.auth.lock().authenticate(
            "127.0.0.1".parse().expect("loopback IP"),
            Some(&invite),
            None,
            Some("Test phone"),
        ) {
            Ok(AuthOutcome::Paired { device_id, .. }) => device_id,
            _ => panic!("expected paired test device"),
        }
    }

    #[test]
    fn outbound_queue_overflow_requests_disconnect() {
        let (tx, _rx) = mpsc::channel(1);
        let (disconnect_tx, disconnect_rx) = watch::channel(false);
        let outbound = OutboundSender::new(tx, disconnect_tx);

        assert!(outbound.send(Message::Text("first".to_string())).is_ok());
        assert!(outbound
            .send(Message::Text("overflow".to_string()))
            .is_err());
        assert!(*disconnect_rx.borrow());
    }

    #[test]
    fn revoke_cannot_leave_an_authenticated_device_registered() {
        // Revoke wins the auth→register window: the later registration gate
        // sees no device and cannot create an online session.
        let revoked_first = RemoteState::new_in_memory();
        let device_id = pair_test_device(&revoked_first);
        {
            let mut auth = revoked_first.auth.lock();
            assert!(auth.revoke(&device_id).expect("revoke device"));
            revoked_first.clients.disconnect_device(&device_id);
        }
        let (tx, _rx) = mpsc::channel(1);
        let (disconnect_tx, _disconnect_rx) = watch::channel(false);
        assert!(register_authenticated_client(
            &revoked_first,
            &device_id,
            OutboundSender::new(tx, disconnect_tx),
        )
        .is_none());
        assert_eq!(revoked_first.clients.online_count(), 0);

        // Registration wins first: revoke acquires the same lock order and
        // therefore sees the client and signals its connection to close.
        let registered_first = RemoteState::new_in_memory();
        let device_id = pair_test_device(&registered_first);
        let (tx, _rx) = mpsc::channel(1);
        let (disconnect_tx, disconnect_rx) = watch::channel(false);
        assert!(register_authenticated_client(
            &registered_first,
            &device_id,
            OutboundSender::new(tx, disconnect_tx),
        )
        .is_some());
        {
            let mut auth = registered_first.auth.lock();
            assert!(auth.revoke(&device_id).expect("revoke device"));
            registered_first.clients.disconnect_device(&device_id);
        }
        assert!(*disconnect_rx.borrow());
    }

    #[test]
    fn pairing_is_announced_only_after_reply_commit_and_rollback_is_id_specific() {
        let app = tauri::test::mock_app();
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();
        let pairing_events = Arc::new(AtomicUsize::new(0));
        let event_counter = pairing_events.clone();
        let _listener = handle.listen("remote-device-paired", move |_| {
            event_counter.fetch_add(1, Ordering::SeqCst);
        });

        tauri::async_runtime::block_on(async {
            let state = handle.state::<RemoteState>();
            let first_invite = state.auth.lock().create_invite().expect("first invite");
            let first_raw = json!({
                "v": PROTOCOL_VERSION,
                "id": 1,
                "method": "auth",
                "params": { "invite": first_invite, "deviceName": "Committed phone" },
            })
            .to_string();
            let first = authenticate_first_message(
                &handle,
                "127.0.0.1".parse().expect("loopback IP"),
                &first_raw,
                None,
            )
            .expect("pair first device");
            let first_id = first.device_id.clone();

            assert!(first.paired_device_name.is_some());
            assert_eq!(pairing_events.load(Ordering::SeqCst), 0);
            commit_pending_pairing(&handle, "127.0.0.1".parse().expect("loopback IP"), &first);
            assert_eq!(pairing_events.load(Ordering::SeqCst), 1);
            assert!(state.auth.lock().contains_device(&first_id));

            let second_invite = state.auth.lock().create_invite().expect("second invite");
            let second_raw = json!({
                "v": PROTOCOL_VERSION,
                "id": 2,
                "method": "auth",
                "params": { "invite": second_invite, "deviceName": "Rolled back phone" },
            })
            .to_string();
            let second = authenticate_first_message(
                &handle,
                "127.0.0.1".parse().expect("loopback IP"),
                &second_raw,
                None,
            )
            .expect("pair second device");
            let second_id = second.device_id.clone();
            assert_eq!(pairing_events.load(Ordering::SeqCst), 1);

            abort_pending_pairing(&handle, &second).await;
            let auth = state.auth.lock();
            assert!(auth.contains_device(&first_id));
            assert!(!auth.contains_device(&second_id));
            assert_eq!(pairing_events.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn shutdown_prevents_a_pending_pairing_from_being_committed() {
        let app = tauri::test::mock_app();
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();
        let pairing_events = Arc::new(AtomicUsize::new(0));
        let event_counter = pairing_events.clone();
        let _listener = handle.listen("remote-device-paired", move |_| {
            event_counter.fetch_add(1, Ordering::SeqCst);
        });

        tauri::async_runtime::block_on(async {
            let state = handle.state::<RemoteState>();
            let invite = state.auth.lock().create_invite().expect("invite");
            let raw = json!({
                "v": PROTOCOL_VERSION,
                "id": 1,
                "method": "auth",
                "params": { "invite": invite, "deviceName": "Stopped phone" },
            })
            .to_string();
            let authentication = authenticate_first_message(
                &handle,
                "127.0.0.1".parse().expect("loopback IP"),
                &raw,
                None,
            )
            .expect("pair device before the reply commit");
            let device_id = authentication.device_id.clone();
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            shutdown_tx.send(true).expect("shutdown receiver exists");

            assert!(
                !commit_pending_pairing_unless_stopped(
                    &handle,
                    "127.0.0.1".parse().expect("loopback IP"),
                    &authentication,
                    &shutdown_rx,
                )
                .await
            );
            assert_eq!(pairing_events.load(Ordering::SeqCst), 0);

            abort_pending_pairing(&handle, &authentication).await;
            assert!(!state.auth.lock().contains_device(&device_id));
        });
    }

    #[test]
    fn shutdown_interrupts_a_connection_waiting_for_hello() {
        let app = tauri::test::mock_app();
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let (server_io, _client_io) = tokio::io::duplex(1024);
            let ws = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let task = tauri::async_runtime::spawn(serve_ws(
                handle.clone(),
                ws,
                "127.0.0.1".parse().expect("loopback IP"),
                ConnectionContext::Direct,
                shutdown_rx,
            ));

            tokio::task::yield_now().await;
            shutdown_tx
                .send(true)
                .expect("connection still has shutdown receiver");
            tokio::time::timeout(Duration::from_millis(250), task)
                .await
                .expect("shutdown must cancel the hello wait")
                .expect("connection task must finish cleanly");

            let state = handle.state::<RemoteState>();
            assert_eq!(state.clients.online_count(), 0);
            assert!(state.auth.lock().devices().is_empty());
        });
    }
}
