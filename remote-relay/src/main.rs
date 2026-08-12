//! Aeroric 自托管盲转发 relay。
//!
//! 职责刻意最小:按 hostId 撮合「桌面出站控制连接」与「手机接入连接」,
//! 撮合成功后对两条 WS 连接逐帧转发。业务流量是端到端加密的
//! (X25519 + ChaCha20-Poly1305,密钥只在手机与桌面),relay 无法解密,
//! 也不落任何盘。
//!
//! 配置(环境变量):
//! - `RELAY_PORT`:监听端口,默认 6791。
//! - `RELAY_TOKEN`:必填。桌面注册必须携带相同 token(手机接入无需
//!   token——能否用起来取决于 E2EE 握手,relay 只做撮合)。
//!
//! 协议见 remote-protocol crate;部署指南见仓库 docs/remote-public-access.md。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use remote_protocol::{parse_route, HostToRelay, RelayRoute, RelayToHost, RELAY_PROTOCOL_VERSION};

/// 与桌面端 remote/server.rs 一致的单条消息上限。
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_ACTIVE_CONNECTIONS: usize = 512;
const MAX_PENDING_CONNECTIONS: usize = 256;
const MAX_PENDING_PER_HOST: usize = 32;
const CLIENT_CONNECT_RATE_LIMIT: u32 = 12;
const CLIENT_CONNECT_RATE_WINDOW: Duration = Duration::from_secs(10);
const CLIENT_RATE_LIMIT_RETENTION: Duration = Duration::from_secs(10 * 60);
const MAX_CLIENT_RATE_LIMIT_ENTRIES: usize = 4096;
const HOST_CONTROL_QUEUE_CAPACITY: usize = 64;
const WEBSOCKET_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
/// 通知桌面后等它拨数据连接的窗口。
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(25);
const CONTROL_IDLE_DISCONNECT: Duration = Duration::from_secs(75);

type Ws = WebSocketStream<TcpStream>;

#[derive(Clone)]
struct RelayConfig {
    token: String,
}

#[derive(Default)]
struct Registry {
    /// hostId → 控制连接出口(投递 ClientConnected)。
    hosts: Mutex<HashMap<String, mpsc::Sender<Message>>>,
    /// connId → 把桌面数据连接交回手机接入任务的信道。
    pending: Mutex<HashMap<String, PendingConnection>>,
    /// 来源 IP → 手机接入速率窗口。只限制无 token 的 `/connect/:hostId`。
    client_rate_limits: Mutex<HashMap<IpAddr, ClientRateLimit>>,
}

struct PendingConnection {
    host_id: String,
    sender: oneshot::Sender<Ws>,
}

struct ClientRateLimit {
    window_started: Instant,
    attempts: u32,
    last_seen: Instant,
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("RELAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6791);
    let token = require_relay_token(std::env::var("RELAY_TOKEN").ok())
        .unwrap_or_else(|error| panic!("{error}"));
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("failed to bind 0.0.0.0:{port}: {e}"));
    eprintln!("[relay] listening on 0.0.0.0:{port} (host auth: token)");
    serve(listener, RelayConfig { token }).await;
}

fn require_relay_token(token: Option<String>) -> Result<String, String> {
    token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            "RELAY_TOKEN is required and must be non-empty; refusing to start an open relay"
                .to_string()
        })
}

async fn serve(listener: TcpListener, config: RelayConfig) {
    let registry = Arc::new(Registry::default());
    let connection_slots = Arc::new(Semaphore::new(MAX_ACTIVE_CONNECTIONS));
    loop {
        let Ok((stream, peer)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = connection_slots.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let registry = registry.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let _permit = permit;
            handle_connection(stream, peer, registry, config).await;
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<Registry>,
    config: RelayConfig,
) {
    let ws_config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    };
    let mut path = String::new();
    let mut client_ip = peer.ip();
    // tungstenite's required handshake error type is intentionally large; the
    // callback only accepts the successful response and never constructs it.
    #[allow(clippy::result_large_err)]
    let callback = |req: &Request, resp: Response| {
        path = req.uri().path().to_string();
        client_ip = rate_limit_source_ip(peer.ip(), req);
        Ok(resp)
    };
    let Ok(Ok(ws)) = tokio::time::timeout(
        WEBSOCKET_HANDSHAKE_TIMEOUT,
        tokio_tungstenite::accept_hdr_async_with_config(stream, callback, Some(ws_config)),
    )
    .await
    else {
        return;
    };
    match parse_route(&path) {
        Some(RelayRoute::HostControl) => host_control(ws, registry, config).await,
        Some(RelayRoute::ClientConnect { host_id }) => {
            client_connect(ws, host_id, peer, client_ip, registry).await
        }
        Some(RelayRoute::HostData { conn_id }) => host_data(ws, conn_id, registry),
        None => {
            // 未知路径:直接关闭(不给探测者任何信息)
        }
    }
}

fn control_text(msg: &RelayToHost) -> Message {
    Message::Text(serde_json::to_string(msg).expect("relay control message serializes"))
}

/// 桌面控制连接:注册 → 持续投递 ClientConnected,ping 保活。
async fn host_control(mut ws: Ws, registry: Arc<Registry>, config: RelayConfig) {
    // 首条消息必须是 Register
    let first = match tokio::time::timeout(REGISTER_TIMEOUT, ws.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            let _ = ws.close(None).await;
            return;
        }
    };
    let register = serde_json::from_str::<HostToRelay>(&first);
    let (host_id, token) = match register {
        Ok(HostToRelay::Register { v, host_id, token }) if v == RELAY_PROTOCOL_VERSION => {
            (host_id, token)
        }
        Ok(HostToRelay::Register { v, .. }) => {
            let _ = ws
                .send(control_text(&RelayToHost::Error {
                    message: format!("unsupported relay protocol v{v}"),
                }))
                .await;
            let _ = ws.close(None).await;
            return;
        }
        Err(_) => {
            let _ = ws
                .send(control_text(&RelayToHost::Error {
                    message: "first message must be register".to_string(),
                }))
                .await;
            let _ = ws.close(None).await;
            return;
        }
    };
    if token.as_deref() != Some(config.token.as_str()) {
        let _ = ws
            .send(control_text(&RelayToHost::Error {
                message: "invalid relay token".to_string(),
            }))
            .await;
        let _ = ws.close(None).await;
        return;
    }
    if host_id.is_empty() || host_id.len() > 128 {
        let _ = ws
            .send(control_text(&RelayToHost::Error {
                message: "invalid host id".to_string(),
            }))
            .await;
        let _ = ws.close(None).await;
        return;
    }

    let (tx, mut rx) = mpsc::channel::<Message>(HOST_CONTROL_QUEUE_CAPACITY);
    if !try_register_host(&registry, &host_id, tx.clone()) {
        let _ = ws
            .send(control_text(&RelayToHost::Error {
                message: "host id is already registered".to_string(),
            }))
            .await;
        let _ = ws.close(None).await;
        return;
    }
    if ws
        .send(control_text(&RelayToHost::Registered))
        .await
        .is_err()
    {
        cleanup_host(&registry, &host_id, &tx);
        return;
    }
    eprintln!("[relay] host registered: {host_id}");

    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(CONTROL_PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_inbound = tokio::time::Instant::now();
    loop {
        tokio::select! {
            outbound = rx.recv() => {
                match outbound {
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
                    Some(Ok(Message::Ping(payload))) => {
                        last_inbound = tokio::time::Instant::now();
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {
                        last_inbound = tokio::time::Instant::now();
                    }
                }
            }
            _ = ping.tick() => {
                if last_inbound.elapsed() > CONTROL_IDLE_DISCONNECT {
                    break;
                }
                if sink.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
        }
    }
    cleanup_host(&registry, &host_id, &tx);
    eprintln!("[relay] host disconnected: {host_id}");
}

fn try_register_host(registry: &Registry, host_id: &str, tx: mpsc::Sender<Message>) -> bool {
    let mut hosts = registry.hosts.lock().unwrap();
    if hosts.contains_key(host_id) {
        return false;
    }
    hosts.insert(host_id.to_string(), tx);
    true
}

fn cleanup_host(registry: &Registry, host_id: &str, tx: &mpsc::Sender<Message>) {
    let mut hosts = registry.hosts.lock().unwrap();
    if hosts
        .get(host_id)
        .is_some_and(|current| current.same_channel(tx))
    {
        hosts.remove(host_id);
    }
}

/// 手机接入:通知桌面拨数据连接,等到后逐帧对接。
async fn client_connect(
    ws: Ws,
    host_id: String,
    peer: SocketAddr,
    client_ip: IpAddr,
    registry: Arc<Registry>,
) {
    if !try_acquire_client_connect(&registry, client_ip, Instant::now()) {
        let mut ws = ws;
        let _ = ws.close(None).await;
        return;
    }
    let Some(host_tx) = registry.hosts.lock().unwrap().get(&host_id).cloned() else {
        let mut ws = ws;
        let _ = ws.close(None).await;
        return;
    };
    let conn_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<Ws>();
    if !try_insert_pending(&registry, &host_id, &conn_id, tx) {
        let mut ws = ws;
        let _ = ws.close(None).await;
        return;
    }
    let notify = control_text(&RelayToHost::ClientConnected {
        conn_id: conn_id.clone(),
        peer: Some(peer.to_string()),
    });
    if host_tx.try_send(notify).is_err() {
        registry.pending.lock().unwrap().remove(&conn_id);
        let mut ws = ws;
        let _ = ws.close(None).await;
        return;
    }
    match tokio::time::timeout(DIAL_TIMEOUT, rx).await {
        Ok(Ok(host_ws)) => splice(ws, host_ws).await,
        _ => {
            registry.pending.lock().unwrap().remove(&conn_id);
            let mut ws = ws;
            let _ = ws.close(None).await;
        }
    }
}

fn rate_limit_source_ip(peer: IpAddr, request: &Request) -> IpAddr {
    // 部署文档中的 TLS 反代与 relay 同机；仅对 loopback 反代信任来源头，
    // 避免公网直连客户端伪造 X-Forwarded-For 绕过限流。
    if !peer.is_loopback() {
        return peer;
    }
    ["x-forwarded-for", "x-real-ip"]
        .iter()
        .filter_map(|name| request.headers().get(*name))
        .filter_map(|value| value.to_str().ok())
        // 取本机反代追加的最右一项，不信任客户端预先伪造的左侧链。
        .filter_map(|value| value.rsplit(',').next())
        .find_map(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

fn try_acquire_client_connect(registry: &Registry, peer: IpAddr, now: Instant) -> bool {
    let mut limits = registry.client_rate_limits.lock().unwrap();
    if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES {
        limits.retain(|_, entry| now.duration_since(entry.last_seen) < CLIENT_RATE_LIMIT_RETENTION);
        if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES && !limits.contains_key(&peer) {
            if let Some(oldest) = limits
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(address, _)| *address)
            {
                limits.remove(&oldest);
            }
        }
    }

    let entry = limits.entry(peer).or_insert(ClientRateLimit {
        window_started: now,
        attempts: 0,
        last_seen: now,
    });
    entry.last_seen = now;
    if now.duration_since(entry.window_started) >= CLIENT_CONNECT_RATE_WINDOW {
        entry.window_started = now;
        entry.attempts = 0;
    }
    if entry.attempts >= CLIENT_CONNECT_RATE_LIMIT {
        return false;
    }
    entry.attempts += 1;
    true
}

/// 桌面数据连接:交给等待中的手机接入任务(由它执行 splice)。
fn host_data(ws: Ws, conn_id: String, registry: Arc<Registry>) {
    let Some(pending) = registry.pending.lock().unwrap().remove(&conn_id) else {
        // 无人等待(超时/伪造 connId):直接丢弃连接
        return;
    };
    let _ = pending.sender.send(ws);
}

fn try_insert_pending(
    registry: &Registry,
    host_id: &str,
    conn_id: &str,
    sender: oneshot::Sender<Ws>,
) -> bool {
    let mut pending = registry.pending.lock().unwrap();
    if pending.len() >= MAX_PENDING_CONNECTIONS
        || pending
            .values()
            .filter(|entry| entry.host_id == host_id)
            .count()
            >= MAX_PENDING_PER_HOST
    {
        return false;
    }
    pending.insert(
        conn_id.to_string(),
        PendingConnection {
            host_id: host_id.to_string(),
            sender,
        },
    );
    true
}

/// 双向逐帧盲转发,任一侧断开即两侧收尾。
async fn splice(a: Ws, b: Ws) {
    let (mut a_sink, mut a_stream) = a.split();
    let (mut b_sink, mut b_stream) = b.split();
    let a_to_b = async {
        while let Some(Ok(msg)) = a_stream.next().await {
            let closing = matches!(msg, Message::Close(_));
            if b_sink.send(msg).await.is_err() || closing {
                break;
            }
        }
        let _ = b_sink.close().await;
    };
    let b_to_a = async {
        while let Some(Ok(msg)) = b_stream.next().await {
            let closing = matches!(msg, Message::Close(_));
            if a_sink.send(msg).await.is_err() || closing {
                break;
            }
        }
        let _ = a_sink.close().await;
    };
    tokio::join!(a_to_b, b_to_a);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    async fn spawn_relay(config: RelayConfig) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, config));
        port
    }

    fn test_config() -> RelayConfig {
        RelayConfig {
            token: "s3cret".to_string(),
        }
    }

    async fn next_msg<S>(ws: &mut WebSocketStream<S>) -> Message
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .expect("timed out")
                .expect("stream ended")
                .expect("frame error");
            match msg {
                Message::Ping(_) | Message::Pong(_) => continue,
                other => return other,
            }
        }
    }

    fn parse_control(msg: &Message) -> RelayToHost {
        let Message::Text(text) = msg else {
            panic!("expected text control message, got {msg:?}")
        };
        serde_json::from_str(text).expect("control json")
    }

    #[test]
    fn pending_connections_are_bounded_per_host() {
        let registry = Registry::default();
        for index in 0..MAX_PENDING_PER_HOST {
            let (sender, _receiver) = oneshot::channel::<Ws>();
            assert!(try_insert_pending(
                &registry,
                "host-1",
                &format!("conn-{index}"),
                sender,
            ));
        }
        let (sender, _receiver) = oneshot::channel::<Ws>();
        assert!(!try_insert_pending(
            &registry,
            "host-1",
            "one-too-many",
            sender,
        ));
    }

    #[test]
    fn client_connections_are_rate_limited_per_source_ip() {
        let registry = Registry::default();
        let now = Instant::now();
        let first: IpAddr = "203.0.113.10".parse().unwrap();
        let second: IpAddr = "203.0.113.11".parse().unwrap();

        for _ in 0..CLIENT_CONNECT_RATE_LIMIT {
            assert!(try_acquire_client_connect(&registry, first, now));
        }
        assert!(!try_acquire_client_connect(&registry, first, now));
        assert!(try_acquire_client_connect(&registry, second, now));
        assert!(try_acquire_client_connect(
            &registry,
            first,
            now + CLIENT_CONNECT_RATE_WINDOW,
        ));
    }

    #[test]
    fn forwarded_client_ip_is_trusted_only_from_a_loopback_proxy() {
        let request = Request::builder()
            .uri("/connect/host")
            .header("x-forwarded-for", "203.0.113.10, 198.51.100.30")
            .body(())
            .unwrap();
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        let public: IpAddr = "198.51.100.20".parse().unwrap();

        assert_eq!(
            rate_limit_source_ip(loopback, &request),
            "198.51.100.30".parse::<IpAddr>().unwrap()
        );
        assert_eq!(rate_limit_source_ip(public, &request), public);
    }

    #[test]
    fn relay_token_is_required() {
        assert!(require_relay_token(None).is_err());
        assert!(require_relay_token(Some("   ".to_string())).is_err());
        assert_eq!(
            require_relay_token(Some(" secret ".to_string())).unwrap(),
            "secret"
        );
    }

    #[tokio::test]
    async fn relay_matches_host_and_client_and_splices_frames() {
        let port = spawn_relay(test_config()).await;

        // 桌面注册
        let (mut host, _) = connect_async(format!("ws://127.0.0.1:{port}/host"))
            .await
            .unwrap();
        host.send(Message::Text(
            serde_json::to_string(&HostToRelay::Register {
                v: RELAY_PROTOCOL_VERSION,
                host_id: "host-1".to_string(),
                token: Some("s3cret".to_string()),
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        assert_eq!(
            parse_control(&next_msg(&mut host).await),
            RelayToHost::Registered
        );

        // 手机接入 → 先发一帧(relay 应缓存在 WS 层直至撮合完成)
        let client_task = tokio::spawn(async move {
            let (mut client, _) = connect_async(format!("ws://127.0.0.1:{port}/connect/host-1"))
                .await
                .unwrap();
            client
                .send(Message::Text("hello-from-phone".to_string()))
                .await
                .unwrap();
            client.send(Message::Binary(vec![1, 2, 3])).await.unwrap();
            let echo = next_msg(&mut client).await;
            assert_eq!(echo, Message::Binary(vec![9, 9]));
        });

        // 桌面收 ClientConnected → 拨数据连接
        let RelayToHost::ClientConnected { conn_id, peer } =
            parse_control(&next_msg(&mut host).await)
        else {
            panic!("expected client_connected")
        };
        assert!(peer.is_some());
        let (mut data, _) = connect_async(format!("ws://127.0.0.1:{port}/data/{conn_id}"))
            .await
            .unwrap();
        assert_eq!(
            next_msg(&mut data).await,
            Message::Text("hello-from-phone".to_string())
        );
        assert_eq!(next_msg(&mut data).await, Message::Binary(vec![1, 2, 3]));
        data.send(Message::Binary(vec![9, 9])).await.unwrap();
        client_task.await.unwrap();
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let port = spawn_relay(test_config()).await;
        let (mut host, _) = connect_async(format!("ws://127.0.0.1:{port}/host"))
            .await
            .unwrap();
        host.send(Message::Text(
            serde_json::to_string(&HostToRelay::Register {
                v: RELAY_PROTOCOL_VERSION,
                host_id: "host-1".to_string(),
                token: Some("wrong".to_string()),
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        let RelayToHost::Error { message } = parse_control(&next_msg(&mut host).await) else {
            panic!("expected error")
        };
        assert!(message.contains("invalid relay token"));
    }

    #[tokio::test]
    async fn duplicate_host_registration_is_rejected_without_replacing_the_first() {
        let port = spawn_relay(test_config()).await;
        let register = serde_json::to_string(&HostToRelay::Register {
            v: RELAY_PROTOCOL_VERSION,
            host_id: "host-1".to_string(),
            token: Some("s3cret".to_string()),
        })
        .unwrap();

        let (mut first, _) = connect_async(format!("ws://127.0.0.1:{port}/host"))
            .await
            .unwrap();
        first.send(Message::Text(register.clone())).await.unwrap();
        assert_eq!(
            parse_control(&next_msg(&mut first).await),
            RelayToHost::Registered
        );

        let (mut duplicate, _) = connect_async(format!("ws://127.0.0.1:{port}/host"))
            .await
            .unwrap();
        duplicate.send(Message::Text(register)).await.unwrap();
        let RelayToHost::Error { message } = parse_control(&next_msg(&mut duplicate).await) else {
            panic!("expected duplicate registration error")
        };
        assert!(message.contains("already registered"));

        let (mut client, _) = connect_async(format!("ws://127.0.0.1:{port}/connect/host-1"))
            .await
            .unwrap();
        let message = parse_control(&next_msg(&mut first).await);
        assert!(matches!(message, RelayToHost::ClientConnected { .. }));
        let _ = client.close(None).await;
    }

    #[tokio::test]
    async fn client_to_unknown_host_is_closed() {
        let port = spawn_relay(test_config()).await;
        let (mut client, _) = connect_async(format!("ws://127.0.0.1:{port}/connect/ghost"))
            .await
            .unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match client.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "connection should be closed promptly");
    }

    #[tokio::test]
    async fn forged_data_conn_id_is_dropped() {
        let port = spawn_relay(test_config()).await;
        let (mut data, _) = connect_async(format!("ws://127.0.0.1:{port}/data/forged"))
            .await
            .unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match data.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "forged data connection should be dropped");
    }
}
