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
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use super::auth::AuthOutcome;
use super::protocol::{parse_request, RpcResponse, PROTOCOL_VERSION};
use super::{audit, crypto, rpc, RemoteState};

const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
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

// ── 服务生命周期 ─────────────────────────────────────────────────────────────

pub struct ServerHandle {
    pub port: u16,
    shutdown: watch::Sender<bool>,
    listener_ids: Vec<tauri::EventId>,
}

impl ServerHandle {
    pub fn shutdown<R: Runtime>(self, app: &AppHandle<R>) {
        let _ = self.shutdown.send(true);
        for id in self.listener_ids {
            app.unlisten(id);
        }
        app.state::<RemoteState>().clients.clear();
    }
}

/// 绑定端口并启动 accept loop。绑定失败(端口占用等)同步返回 Err。
pub async fn start<R: Runtime>(app: AppHandle<R>, port: u16) -> Result<ServerHandle, String> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("Failed to bind port {port}: {e}"))?;
    let bound_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let listener_ids = super::events_bridge::attach(&app);

    // relay 出站注册与 WS 服务同生命周期(共享 shutdown)
    let (relay_url, relay_token) = {
        let state = app.state::<RemoteState>();
        let cfg = state.config.lock();
        (cfg.relay_url.clone(), cfg.relay_token.clone())
    };
    if let Some(relay_url) = relay_url.filter(|u| !u.trim().is_empty()) {
        super::relay_client::spawn(app.clone(), relay_url, relay_token, shutdown_rx.clone());
    }

    let accept_app = app.clone();
    tauri::async_runtime::spawn(async move {
        accept_loop(accept_app, listener, shutdown_rx).await;
    });

    Ok(ServerHandle {
        port: bound_port,
        shutdown: shutdown_tx,
        listener_ids,
    })
}

async fn accept_loop<R: Runtime>(
    app: AppHandle<R>,
    listener: TcpListener,
    shutdown: watch::Receiver<bool>,
) {
    loop {
        let mut shutdown_wait = shutdown.clone();
        tokio::select! {
            _ = shutdown_wait.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let conn_app = app.clone();
                let conn_shutdown = shutdown.clone();
                tauri::async_runtime::spawn(async move {
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
    shutdown: watch::Receiver<bool>,
) {
    let config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    };
    let Ok(ws) = tokio_tungstenite::accept_async_with_config(stream, Some(config)).await else {
        return;
    };
    serve_ws(app, ws, peer, shutdown).await;
}

/// 加密封包后发送;返回 false 表示连接已不可用。
async fn send_encrypted<S>(
    sink: &mut SplitSink<WebSocketStream<S>, Message>,
    session: &mut crypto::SessionCrypto,
    kind: u8,
    plain: &[u8],
) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match session.encrypt_frame(kind, plain) {
        Ok(frame) => sink.send(Message::Binary(frame)).await.is_ok(),
        Err(_) => false,
    }
}

/// 会话主循环:E2EE 握手 → 认证 → 请求循环。LAN 与 relay 数据连接共用。
pub(crate) async fn serve_ws<R, S>(
    app: AppHandle<R>,
    ws: WebSocketStream<S>,
    peer: IpAddr,
    mut shutdown: watch::Receiver<bool>,
) where
    R: Runtime,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = ws.split();
    let audit_enabled = app.state::<RemoteState>().audit_enabled;

    // ── E2EE 握手:第一条明文 text 必须是 hello ──
    let hello = match tokio::time::timeout(AUTH_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            let _ = sink.close().await;
            return;
        }
    };
    let mut session = match respond_hello(&app, &hello) {
        Ok((ack, session)) => {
            if sink.send(Message::Text(ack)).await.is_err() {
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
        match tokio::time::timeout(AUTH_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => match session.decrypt_frame(&bytes) {
                Ok((crypto::KIND_CTRL, plain)) => match String::from_utf8(plain) {
                    Ok(text) => break text,
                    Err(_) => {
                        let _ = sink.close().await;
                        return;
                    }
                },
                _ => {
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
    let (device_id, auth_reply) = match authenticate_first_message(&app, peer, &first_ctrl) {
        Ok((device_id, reply)) => (device_id, reply),
        Err(reply) => {
            let _ =
                send_encrypted(&mut sink, &mut session, crypto::KIND_CTRL, reply.as_bytes()).await;
            let _ = sink.close().await;
            return;
        }
    };
    if !send_encrypted(
        &mut sink,
        &mut session,
        crypto::KIND_CTRL,
        auth_reply.as_bytes(),
    )
    .await
    {
        return;
    }

    // ── 注册进在线表,进入请求循环 ──
    let clients = app.state::<RemoteState>().clients.clone();
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOUND_QUEUE_CAPACITY);
    let (disconnect_tx, mut disconnect_rx) = watch::channel(false);
    let outbound = OutboundSender::new(tx, disconnect_tx);
    let client_id = clients.register(device_id, outbound.clone());
    let mut terminal = super::terminal_streams::TerminalStreams::new(app.clone(), outbound);

    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_inbound = Instant::now();

    loop {
        tokio::select! {
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
                    Some(Ok(Message::Binary(bytes))) => {
                        last_inbound = Instant::now();
                        match session.decrypt_frame(&bytes) {
                            Ok((crypto::KIND_CTRL, plain)) => {
                                let Ok(text) = String::from_utf8(plain) else { break };
                                let reply = handle_request(&app, &text).await;
                                if !send_encrypted(&mut sink, &mut session, crypto::KIND_CTRL, reply.as_bytes()).await {
                                    break;
                                }
                            }
                            Ok((crypto::KIND_TERMINAL, plain)) => terminal.handle_frame(&plain),
                            // 未知 kind:前向兼容,忽略
                            Ok(_) => {}
                            // 篡改/乱序/重放:立即断连
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
                    Some(Ok(Message::Text(_))) => break,
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
) -> Result<(String, crypto::SessionCrypto), String> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Err(hello_error("Malformed hello"));
    };
    let version = parsed.get("v").and_then(Value::as_u64).unwrap_or(0);
    let msg_type = parsed.get("type").and_then(Value::as_str).unwrap_or("");
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
    Ok((ack.to_string(), accept.session))
}

/// 处理首条 auth 消息。Ok=(device_id, 成功响应);Err=失败响应(回给对端后断开)。
fn authenticate_first_message<R: Runtime>(
    app: &AppHandle<R>,
    peer: IpAddr,
    raw: &str,
) -> Result<(String, String), String> {
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
            if audit_enabled {
                audit::log(
                    "device-paired",
                    json!({ "peer": peer.to_string(), "deviceId": device_id, "deviceName": device_name }),
                );
            }
            // 通知设置页:配对成功(二维码视图切到完成态 + 刷新设备列表)。
            let _ = app.emit(
                "remote-device-paired",
                json!({ "deviceId": device_id, "deviceName": device_name }),
            );
            let reply = RpcResponse::success(
                req.id,
                json!({
                    "deviceId": device_id,
                    "deviceToken": device_token,
                    "host": host,
                }),
            );
            Ok((device_id, reply.to_json()))
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
            Ok((device_id, reply.to_json()))
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
    use super::*;

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
}
