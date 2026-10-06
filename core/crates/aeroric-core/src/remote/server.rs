//! 远程 WS 服务:监听、E2EE 握手、认证、请求循环、在线客户端注册表。
//!
//! 生命周期:`start()` 绑定端口并 spawn accept loop,返回 ServerHandle;
//! `ServerHandle::shutdown()` 广播关闭信号,所有连接任务随之退出。
//! 每个连接:10s 内完成明文 hello → hello_ack 的 E2EE 握手(crypto.rs),
//! 再在 10s 内于加密通道内完成 auth(invite 配对或 device token),
//! 通过后进入请求循环;30s 心跳 Ping,75s 无入站即断开。
//! 会话主循环 `serve_ws` 对底层流泛型:LAN 直连(服务端 accept)与
//! relay 数据连接(客户端 connect)走同一条代码路径。

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use crate::remote::auth::AuthOutcome;
use crate::remote::protocol::{
    parse_auth_request, parse_request, select_rpc_version, RpcResponse, RpcVersion,
    PAIRING_CONFIRMATION_V1, PROTOCOL_VERSION,
};
use crate::remote::services::RemoteHostHandle;
use crate::remote::state::{
    abort_unconfirmed_pairing as state_abort_pairing, reconcile_listener_scope_for_current_policy,
    register_authenticated_client, ListenerScope, OutboundSender, RemoteState, ServerHandle,
};
use crate::remote::{crypto, orca_crypto, orca_rpc};

const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound sockets before the WebSocket upgrade/authentication completes. A
/// network peer must not be able to turn slow HTTP headers into unbounded tasks
/// and file-descriptor pressure.
const MAX_CONNECTIONS: usize = 256;
const HTTP_UPGRADE_TIMEOUT: Duration = Duration::from_secs(20);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_DISCONNECT: Duration = Duration::from_secs(75);
/// 单条消息上限;控制面为小 JSON、终端帧 ≤64KB+密文开销,1MB 足够且防滥用。
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// 每连接出口队列上限。终端消费者持续落后时主动断连，重连后由快照恢复，
/// 避免慢客户端无限占用桌面内存。
const OUTBOUND_QUEUE_CAPACITY: usize = 256;
/// Typed close reason for intentional host shutdown (user stops remote access
/// or quits the desktop). Mobile surfaces this as "电脑端已退出登录" instead of
/// a generic reconnect blurb. Device revoke and queue-full disconnects must
/// keep using a reason-less Close — those are not a host sign-out.
const HOST_SIGNED_OUT_CLOSE_REASON: &str = "signed-out";

fn host_signed_out_close() -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::Normal,
        reason: HOST_SIGNED_OUT_CLOSE_REASON.into(),
    }))
}

// ── 服务生命周期 ─────────────────────────────────────────────────────────────

/// 默认的低层启动入口保持全网监听，供 LAN roundtrip 测试和显式网络
/// 调用者使用。桌面远程功能通过 `start_with_scope` 选择最小暴露范围。
pub async fn start(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
    port: u16,
) -> Result<ServerHandle, String> {
    start_with_scope(state, handle, port, ListenerScope::Network).await
}

/// 绑定指定范围的端口并启动服务生命周期。事件桥（Tauri listen_any）与
/// relay 出站连接由宿主 wrapper 挂接：core 只管 TCP listener 与连接循环。
pub async fn start_with_scope(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
    port: u16,
    scope: ListenerScope,
) -> Result<ServerHandle, String> {
    let listener = bind_listener(scope, port).await?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (listener_shutdown, listener_shutdown_rx) = watch::channel(false);
    let listener_stopped = spawn_accept_loop(
        Arc::clone(&state),
        Arc::clone(&handle),
        listener,
        shutdown_rx,
        listener_shutdown_rx,
    );

    Ok(ServerHandle {
        port: bound_addr.port(),
        bound_addr: Some(bound_addr),
        listener_scope: scope,
        shutdown: shutdown_tx,
        listener_shutdown,
        listener_stopped: Some(listener_stopped),
        post_shutdown: None,
    })
}

pub(crate) async fn bind_listener(scope: ListenerScope, port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(scope.bind_addr(port))
        .await
        .map_err(|e| format!("Failed to bind port {port}: {e}"))
}

pub(crate) fn spawn_accept_loop(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
    listener: TcpListener,
    shutdown: watch::Receiver<bool>,
    listener_shutdown: watch::Receiver<bool>,
) -> oneshot::Receiver<()> {
    let (stopped_tx, stopped_rx) = oneshot::channel();
    tokio::spawn(async move {
        accept_loop(state, handle, listener, shutdown, listener_shutdown).await;
        let _ = stopped_tx.send(());
    });
    stopped_rx
}

async fn accept_loop(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
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
                let conn_state = Arc::clone(&state);
                let conn_handle = Arc::clone(&handle);
                let conn_shutdown = shutdown.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    handle_connection(conn_state, conn_handle, stream, peer.ip(), conn_shutdown)
                        .await;
                });
            }
        }
    }
}

// ── 单连接处理 ───────────────────────────────────────────────────────────────

async fn handle_connection(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
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
    serve_ws(state, handle, ws, peer, ConnectionContext::Direct, shutdown).await;
}
pub enum ConnectionContext {
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
    provisional_pairing_id: Option<String>,
    rpc_version: RpcVersion,
}

async fn abort_pending_pairing(
    state: &Arc<RemoteState>,
    handle: &Arc<dyn RemoteHostHandle>,
    authentication: &AuthenticatedFirstMessage,
) {
    if let Some(pairing_id) = authentication.provisional_pairing_id.as_deref() {
        state.auth.lock().abort_pairing(pairing_id);
    } else if authentication.paired_device_name.is_some() {
        state_abort_pairing(state, handle, &authentication.device_id).await;
    }
}

/// Publish the pairing only after the peer has received its encrypted device
/// token. A concurrent revoke may already have removed the device; in that
/// case there is no successful pairing left to announce.
fn commit_pending_pairing(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    peer: IpAddr,
    authentication: &AuthenticatedFirstMessage,
) {
    let Some(device_name) = authentication.paired_device_name.as_deref() else {
        return;
    };
    if !state.auth.lock().contains_device(&authentication.device_id) {
        return;
    }
    if state.audit_enabled {
        handle.audit_log(
            "device-paired",
            json!({
                "peer": peer.to_string(),
                "deviceId": authentication.device_id,
                "deviceName": device_name,
            }),
        );
    }
    // 通知设置页:配对成功(二维码视图切到完成态 + 刷新设备列表)。
    handle.emit(
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
async fn commit_pending_pairing_unless_stopped(
    state: &Arc<RemoteState>,
    handle: &dyn RemoteHostHandle,
    peer: IpAddr,
    authentication: &AuthenticatedFirstMessage,
    shutdown: &watch::Receiver<bool>,
) -> bool {
    if authentication.paired_device_name.is_none() {
        return true;
    }
    let _lifecycle = state.lifecycle.lock().await;
    if *shutdown.borrow() {
        return false;
    }
    commit_pending_pairing(state, handle, peer, authentication);
    true
}

/// Wait for the v1 confirmation on the same encrypted connection. Persistence
/// failures are reported without dropping the in-memory reservation, so the
/// peer may retry while the bounded authentication window remains open.
async fn await_pairing_confirmation<S>(
    state: &RemoteState,
    sink: &mut SplitSink<WebSocketStream<S>, Message>,
    stream: &mut SplitStream<WebSocketStream<S>>,
    session: &mut NegotiatedSession,
    authentication: &AuthenticatedFirstMessage,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let expected_pairing_id = authentication.provisional_pairing_id.as_deref()?;
    let deadline = tokio::time::Instant::now() + AUTH_TIMEOUT;
    loop {
        let received = tokio::select! {
            biased;
            _ = shutdown.changed() => return None,
            received = tokio::time::timeout_at(deadline, stream.next()) => received,
        };
        let message = match received {
            Ok(Some(Ok(message))) => message,
            _ => return None,
        };
        if let Message::Ping(payload) = message {
            if sink.send(Message::Pong(payload)).await.is_err() {
                return None;
            }
            continue;
        }
        let text = match session.decrypt_message(&message) {
            Ok(DecryptedMessage::Control(text)) => text,
            _ => return None,
        };
        let req = match parse_auth_request(&text) {
            Ok(req) => req,
            Err(error) => {
                let reply = RpcResponse::malformed(RpcVersion::V2, error).to_json();
                if !send_encrypted(sink, session, crypto::KIND_CTRL, reply.as_bytes()).await {
                    return None;
                }
                continue;
            }
        };
        let pairing_id = req.params.get("pairingId").and_then(Value::as_str);
        let confirmation_version = req
            .params
            .get("pairingConfirmationVersion")
            .and_then(Value::as_u64);
        if req.method != "pairing.confirm"
            || pairing_id != Some(expected_pairing_id)
            || confirmation_version != Some(u64::from(PAIRING_CONFIRMATION_V1))
        {
            let reply =
                RpcResponse::failure(RpcVersion::V2, req.id, "Invalid pairing confirmation")
                    .to_json();
            if !send_encrypted(sink, session, crypto::KIND_CTRL, reply.as_bytes()).await {
                return None;
            }
            continue;
        }

        let confirmation = {
            let _lifecycle = state.lifecycle.lock().await;
            if *shutdown.borrow() {
                return None;
            }
            // guard 绑成局部变量,使其先于 `state` 释放。
            let mut auth = state.auth.lock();
            auth.confirm_pairing(expected_pairing_id)
        };
        match confirmation {
            Ok((device_id, _)) if device_id == authentication.device_id => {
                return Some(
                    RpcResponse::success(RpcVersion::V2, req.id, json!({ "confirmed": true }))
                        .to_json(),
                );
            }
            Ok(_) => return None,
            Err(error) => {
                let reply = RpcResponse::failure(RpcVersion::V2, req.id, error).to_json();
                if !send_encrypted(sink, session, crypto::KIND_CTRL, reply.as_bytes()).await {
                    return None;
                }
            }
        }
    }
}

/// 会话主循环:E2EE 握手 → 认证 → 请求循环。LAN 与 relay 数据连接共用。
pub async fn serve_ws<S>(
    state: Arc<RemoteState>,
    handle: Arc<dyn RemoteHostHandle>,
    ws: WebSocketStream<S>,
    peer: IpAddr,
    context: ConnectionContext,
    mut shutdown: watch::Receiver<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = ws.split();
    let audit_enabled = state.audit_enabled;
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
    let mut session = match respond_hello(&state, &hello, &context) {
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
                handle.audit_log("handshake-rejected", json!({ "peer": peer.to_string() }));
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
        let _lifecycle = state.lifecycle.lock().await;
        (!*shutdown.borrow()).then(|| {
            authenticate_first_message(
                &state,
                handle.as_ref(),
                peer,
                &first_ctrl,
                orca_transcript_hash.as_deref(),
            )
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
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    let confirmation_reply = if authentication.provisional_pairing_id.is_some() {
        // Hand provisional credentials to the phone while the invite and
        // device remain memory-only. The phone must durably stage them before
        // it sends the next encrypted control request.
        if !send_encrypted_until_shutdown(
            &mut sink,
            &mut session,
            crypto::KIND_CTRL,
            authentication.reply.as_bytes(),
            &mut shutdown,
        )
        .await
        {
            abort_pending_pairing(&state, &handle, &authentication).await;
            return;
        }
        let reply = await_pairing_confirmation(
            &state,
            &mut sink,
            &mut stream,
            &mut session,
            &authentication,
            &mut shutdown,
        )
        .await;
        let Some(reply) = reply else {
            abort_pending_pairing(&state, &handle, &authentication).await;
            let _ = sink.close().await;
            return;
        };
        Some(reply)
    } else {
        None
    };

    // A newly paired device is the only authentication outcome that can widen
    // the listener. In the confirmation flow this happens only after the
    // durable device commit above; existing device-token authentication cannot
    // change listener policy.
    if authentication.paired_device_name.is_some()
        && reconcile_listener_scope_for_current_policy(&state, &handle)
            .await
            .is_err()
    {
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }
    if *shutdown.borrow() {
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    // Register before the final reply (legacy auth result or confirmation
    // ACK), while holding the same auth lock used by revocation. This closes
    // the otherwise reachable auth-to-register window.
    let clients = state.clients.clone();
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOUND_QUEUE_CAPACITY);
    let (disconnect_tx, mut disconnect_rx) = watch::channel(false);
    let outbound = OutboundSender::new(tx, disconnect_tx);
    if *shutdown.borrow() {
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }
    let client_id = register_authenticated_client(
        &state,
        &authentication.device_id,
        authentication.rpc_version,
        outbound.clone(),
    );
    let Some(client_id) = client_id else {
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    };
    // A revoke can win immediately after registration. Check its disconnect
    // signal before reporting the final commit to the peer.
    if *shutdown.borrow() || *disconnect_rx.borrow() {
        clients.unregister(client_id);
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    let final_reply = confirmation_reply
        .as_deref()
        .unwrap_or(authentication.reply.as_str());
    if !send_encrypted_until_shutdown(
        &mut sink,
        &mut session,
        crypto::KIND_CTRL,
        final_reply.as_bytes(),
        &mut shutdown,
    )
    .await
    {
        clients.unregister(client_id);
        abort_pending_pairing(&state, &handle, &authentication).await;
        return;
    }

    let pairing_committed = commit_pending_pairing_unless_stopped(
        &state,
        handle.as_ref(),
        peer,
        &authentication,
        &shutdown,
    )
    .await;
    if !pairing_committed {
        clients.unregister(client_id);
        abort_pending_pairing(&state, &handle, &authentication).await;
        let _ = sink.close().await;
        return;
    }

    if *shutdown.borrow() || *disconnect_rx.borrow() {
        clients.unregister(client_id);
        let _ = sink.close().await;
        return;
    }

    // ── 已注册，进入请求循环 ──
    let mut terminal = handle.terminal_streams(outbound);

    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_inbound = Instant::now();

    loop {
        if *shutdown.borrow() {
            let _ = sink.send(host_signed_out_close()).await;
            break;
        }
        if *disconnect_rx.borrow() {
            let _ = sink.send(Message::Close(None)).await;
            break;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                let _ = sink.send(host_signed_out_close()).await;
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
                                    orca_rpc::handle(&state, &handle.desktop_version(), &text).await
                                } else {
                                    handle_request(&state, handle.as_ref(), &text, authentication.rpc_version).await
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
fn respond_hello(
    state: &RemoteState,
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
fn authenticate_first_message(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
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
        let outcome = state
            .auth
            .lock()
            .authenticate(peer, None, Some(device_token), None);
        return match outcome {
            Ok(AuthOutcome::Authenticated { device_id }) => {
                if state.audit_enabled {
                    handle.audit_log(
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
                    provisional_pairing_id: None,
                    rpc_version: RpcVersion::V2,
                })
            }
            Ok(AuthOutcome::Paired { .. } | AuthOutcome::ProvisionalPaired { .. }) => {
                Err(json!({ "type": "e2ee_error", "error": { "code": "bad_auth" } }).to_string())
            }
            Err(error) => {
                if state.audit_enabled {
                    handle.audit_log(
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
    let req = match parse_auth_request(raw) {
        Ok(req) => req,
        Err(err) => return Err(RpcResponse::malformed(RpcVersion::V2, err).to_json()),
    };
    if req.method != "auth" {
        return Err(
            RpcResponse::failure(RpcVersion::V2, req.id, "First message must be auth").to_json(),
        );
    }
    let rpc_version = select_rpc_version(&req.params);
    let invite = req.params.get("invite").and_then(Value::as_str);
    let device_token = req.params.get("deviceToken").and_then(Value::as_str);
    let device_name = req.params.get("deviceName").and_then(Value::as_str);
    let pairing_confirmation = req
        .params
        .get("pairingConfirmationVersion")
        .and_then(Value::as_u64)
        == Some(u64::from(PAIRING_CONFIRMATION_V1));

    let outcome = if pairing_confirmation && device_token.is_none() {
        match invite {
            Some(invite) => state.auth.lock().begin_pairing(peer, invite, device_name),
            None => Err("Pairing confirmation requires an invite".to_string()),
        }
    } else {
        state
            .auth
            .lock()
            .authenticate(peer, invite, device_token, device_name)
    };
    let audit_enabled = state.audit_enabled;

    let host = json!({
        "name": crate::remote::identity::host_name(),
        "version": handle.desktop_version(),
        "platform": std::env::consts::OS,
    });

    match outcome {
        Ok(AuthOutcome::Paired {
            device_id,
            device_name,
            device_token,
        }) => {
            let reply = RpcResponse::success(
                RpcVersion::V2,
                req.id,
                json!({
                    "deviceId": device_id,
                    "deviceToken": device_token,
                    "host": host,
                    "rpcVersion": rpc_version.as_u32(),
                    "rpcVersions": [3, 2],
                    "capabilities": crate::remote::rpc::rpc_capabilities_value(),
                }),
            );
            Ok(AuthenticatedFirstMessage {
                device_id,
                reply: reply.to_json(),
                paired_device_name: Some(device_name),
                provisional_pairing_id: None,
                rpc_version,
            })
        }
        Ok(AuthOutcome::ProvisionalPaired {
            device_id,
            device_name,
            device_token,
            pairing_id,
        }) => {
            let reply = RpcResponse::success(
                RpcVersion::V2,
                req.id,
                json!({
                    "deviceId": device_id,
                    "deviceToken": device_token,
                    "pairingId": pairing_id,
                    "pairingConfirmationVersion": PAIRING_CONFIRMATION_V1,
                    "host": host,
                    "rpcVersion": rpc_version.as_u32(),
                    "rpcVersions": [3, 2],
                    "capabilities": crate::remote::rpc::rpc_capabilities_value(),
                }),
            );
            Ok(AuthenticatedFirstMessage {
                device_id,
                reply: reply.to_json(),
                paired_device_name: Some(device_name),
                provisional_pairing_id: Some(pairing_id),
                rpc_version,
            })
        }
        Ok(AuthOutcome::Authenticated { device_id, .. }) => {
            if audit_enabled {
                handle.audit_log(
                    "auth-ok",
                    json!({ "peer": peer.to_string(), "deviceId": device_id }),
                );
            }
            let reply = RpcResponse::success(
                RpcVersion::V2,
                req.id,
                json!({
                    "deviceId": device_id,
                    "host": host,
                    "rpcVersion": rpc_version.as_u32(),
                    "rpcVersions": [3, 2],
                    "capabilities": crate::remote::rpc::rpc_capabilities_value(),
                }),
            );
            Ok(AuthenticatedFirstMessage {
                device_id,
                reply: reply.to_json(),
                paired_device_name: None,
                provisional_pairing_id: None,
                rpc_version,
            })
        }
        Err(err) => {
            if audit_enabled {
                handle.audit_log(
                    "auth-failed",
                    json!({ "peer": peer.to_string(), "error": err }),
                );
            }
            Err(RpcResponse::failure(RpcVersion::V2, req.id, err).to_json())
        }
    }
}

async fn handle_request(
    state: &RemoteState,
    handle: &dyn RemoteHostHandle,
    raw: &str,
    rpc_version: RpcVersion,
) -> String {
    let req = match parse_request(raw, rpc_version) {
        Ok(req) => req,
        Err(err) => return RpcResponse::malformed(rpc_version, err).to_json(),
    };
    // 已认证连接不允许再发 auth(避免把配对逻辑暴露在长连接里)。
    if req.method == "auth" {
        return RpcResponse::failure(rpc_version, req.id, "Already authenticated").to_json();
    }
    match crate::remote::rpc::dispatch(state, handle, &req.method, req.params).await {
        Ok(result) => RpcResponse::success(rpc_version, req.id, result).to_json(),
        Err(err) => RpcResponse::failure(rpc_version, req.id, err).to_json(),
    }
}
