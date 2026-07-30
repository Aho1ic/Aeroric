//! Aeroric 自托管盲转发 relay。
//!
//! 职责刻意最小:按 hostId 撮合「桌面出站控制连接」与「手机接入连接」,
//! 撮合成功后对两条 WS 连接逐帧转发。业务流量是端到端加密的
//! (X25519 + ChaCha20-Poly1305,密钥只在手机与桌面),relay 无法解密,
//! 也不落任何盘。
//!
//! 配置(环境变量):
//! - `RELAY_PORT`:监听端口,默认 6791。
//! - `RELAY_TOKEN`:可选。设置后桌面注册必须携带相同 token(手机接入
//!   无需 token——能否用起来取决于 E2EE 握手,relay 只做撮合)。
//!
//! 协议见 remote-protocol crate;部署指南见仓库 docs/remote-public-access.md。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use remote_protocol::{
    parse_route, HostToRelay, RelayRoute, RelayToHost, RELAY_PROTOCOL_VERSION,
};

/// 与桌面端 remote/server.rs 一致的单条消息上限。
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
/// 通知桌面后等它拨数据连接的窗口。
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(25);
const CONTROL_IDLE_DISCONNECT: Duration = Duration::from_secs(75);

type Ws = WebSocketStream<TcpStream>;

#[derive(Clone, Default)]
struct RelayConfig {
    token: Option<String>,
}

#[derive(Default)]
struct Registry {
    /// hostId → 控制连接出口(投递 ClientConnected)。
    hosts: Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>,
    /// connId → 把桌面数据连接交回手机接入任务的信道。
    pending: Mutex<HashMap<String, oneshot::Sender<Ws>>>,
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("RELAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6791);
    let token = std::env::var("RELAY_TOKEN").ok().filter(|t| !t.is_empty());
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("failed to bind 0.0.0.0:{port}: {e}"));
    eprintln!(
        "[relay] listening on 0.0.0.0:{port} (host auth: {})",
        if token.is_some() { "token" } else { "open" }
    );
    serve(listener, RelayConfig { token }).await;
}

async fn serve(listener: TcpListener, config: RelayConfig) {
    let registry = Arc::new(Registry::default());
    loop {
        let Ok((stream, peer)) = listener.accept().await else {
            continue;
        };
        let registry = registry.clone();
        let config = config.clone();
        tokio::spawn(async move {
            handle_connection(stream, peer.to_string(), registry, config).await;
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: String,
    registry: Arc<Registry>,
    config: RelayConfig,
) {
    let ws_config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    };
    let mut path = String::new();
    let callback = |req: &Request, resp: Response| {
        path = req.uri().path().to_string();
        Ok(resp)
    };
    let Ok(ws) =
        tokio_tungstenite::accept_hdr_async_with_config(stream, callback, Some(ws_config)).await
    else {
        return;
    };
    match parse_route(&path) {
        Some(RelayRoute::HostControl) => host_control(ws, registry, config).await,
        Some(RelayRoute::ClientConnect { host_id }) => {
            client_connect(ws, host_id, peer, registry).await
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
    if config.token != token {
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

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    // 同 hostId 重复注册按「顶掉旧连接」处理:旧控制连接的 rx 发送端被覆盖后,
    // 其循环在下一次投递失败/断连时退出,且退出清理不会误删新连接(same_channel 守卫)
    registry.hosts.lock().unwrap().insert(host_id.clone(), tx.clone());
    if ws.send(control_text(&RelayToHost::Registered)).await.is_err() {
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

fn cleanup_host(registry: &Registry, host_id: &str, tx: &mpsc::UnboundedSender<Message>) {
    let mut hosts = registry.hosts.lock().unwrap();
    if hosts.get(host_id).is_some_and(|current| current.same_channel(tx)) {
        hosts.remove(host_id);
    }
}

/// 手机接入:通知桌面拨数据连接,等到后逐帧对接。
async fn client_connect(ws: Ws, host_id: String, peer: String, registry: Arc<Registry>) {
    let Some(host_tx) = registry.hosts.lock().unwrap().get(&host_id).cloned() else {
        let mut ws = ws;
        let _ = ws.close(None).await;
        return;
    };
    let conn_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<Ws>();
    registry.pending.lock().unwrap().insert(conn_id.clone(), tx);
    let notify = control_text(&RelayToHost::ClientConnected {
        conn_id: conn_id.clone(),
        peer: Some(peer),
    });
    if host_tx.send(notify).is_err() {
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

/// 桌面数据连接:交给等待中的手机接入任务(由它执行 splice)。
fn host_data(ws: Ws, conn_id: String, registry: Arc<Registry>) {
    let Some(tx) = registry.pending.lock().unwrap().remove(&conn_id) else {
        // 无人等待(超时/伪造 connId):直接丢弃连接
        return;
    };
    let _ = tx.send(ws);
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

    #[tokio::test]
    async fn relay_matches_host_and_client_and_splices_frames() {
        let port = spawn_relay(RelayConfig { token: Some("s3cret".to_string()) }).await;

        // 桌面注册
        let (mut host, _) = connect_async(format!("ws://127.0.0.1:{port}/host")).await.unwrap();
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
        assert_eq!(parse_control(&next_msg(&mut host).await), RelayToHost::Registered);

        // 手机接入 → 先发一帧(relay 应缓存在 WS 层直至撮合完成)
        let client_task = tokio::spawn(async move {
            let (mut client, _) = connect_async(format!("ws://127.0.0.1:{port}/connect/host-1"))
                .await
                .unwrap();
            client.send(Message::Text("hello-from-phone".to_string())).await.unwrap();
            client
                .send(Message::Binary(vec![1, 2, 3]))
                .await
                .unwrap();
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
        assert_eq!(next_msg(&mut data).await, Message::Text("hello-from-phone".to_string()));
        assert_eq!(next_msg(&mut data).await, Message::Binary(vec![1, 2, 3]));
        data.send(Message::Binary(vec![9, 9])).await.unwrap();
        client_task.await.unwrap();
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let port = spawn_relay(RelayConfig { token: Some("s3cret".to_string()) }).await;
        let (mut host, _) = connect_async(format!("ws://127.0.0.1:{port}/host")).await.unwrap();
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
    async fn client_to_unknown_host_is_closed() {
        let port = spawn_relay(RelayConfig::default()).await;
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
        let port = spawn_relay(RelayConfig::default()).await;
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
