//! 桌面 → 自托管 relay 的出站注册客户端。
//!
//! 生命周期跟随远程服务:server::start 时若配置了 relay 则 spawn,
//! 共享服务的 shutdown watch;断线按指数退避重连(注册成功即复位)。
//! 收到 ClientConnected 后出站拨数据连接,复用 server::serve_ws ——
//! 手机经 relay 与经 LAN 直连走完全相同的 E2EE 握手 + 认证 + 请求循环,
//! relay 全程只见密文。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::watch;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};

use remote_protocol::{
    host_control_url, host_data_url, HostToRelay, RelayToHost, RELAY_PROTOCOL_VERSION,
};

use super::{audit, RemoteState};

const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_DISCONNECT: Duration = Duration::from_secs(75);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// 与 server.rs 一致的消息上限。
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

fn ws_config() -> WebSocketConfig {
    WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    }
}

fn set_relay_state<R: Runtime>(app: &AppHandle<R>, value: &str) {
    *app.state::<RemoteState>().relay_state.lock() = value.to_string();
}

/// 启动 relay 维护任务。`shutdown` 与 WS 服务共享:服务停,relay 停。
pub(crate) fn spawn<R: Runtime>(
    app: AppHandle<R>,
    relay_url: String,
    relay_token: Option<String>,
    shutdown: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        run(app, relay_url, relay_token, shutdown).await;
    });
}

async fn run<R: Runtime>(
    app: AppHandle<R>,
    relay_url: String,
    relay_token: Option<String>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        if *shutdown.borrow() {
            break;
        }
        match run_control(&app, &relay_url, &relay_token, &mut shutdown).await {
            ControlExit::Shutdown => break,
            ControlExit::Registered(err) => {
                // 注册成功过:视为网络抖动,退避复位
                backoff = INITIAL_BACKOFF;
                set_relay_state(&app, &format!("error:{err}"));
            }
            ControlExit::Failed(err) => {
                set_relay_state(&app, &format!("error:{err}"));
            }
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
    set_relay_state(&app, "off");
}

enum ControlExit {
    /// 服务停机,不再重连。
    Shutdown,
    /// 注册成功后断开(携错误说明)。
    Registered(String),
    /// 未完成注册即失败。
    Failed(String),
}

async fn run_control<R: Runtime>(
    app: &AppHandle<R>,
    relay_url: &str,
    relay_token: &Option<String>,
    shutdown: &mut watch::Receiver<bool>,
) -> ControlExit {
    set_relay_state(app, "connecting");
    let url = host_control_url(relay_url);
    let connect = tokio::select! {
        _ = shutdown.changed() => return ControlExit::Shutdown,
        result = connect_async_with_config(&url, Some(ws_config()), false) => result,
    };
    let ws = match connect {
        Ok((ws, _)) => ws,
        Err(err) => return ControlExit::Failed(format!("connect failed: {err}")),
    };
    let (mut sink, mut stream) = ws.split();

    let host_id = app.state::<RemoteState>().keys.host_id();
    let register = HostToRelay::Register {
        v: RELAY_PROTOCOL_VERSION,
        host_id: host_id.clone(),
        token: relay_token.clone(),
    };
    let payload = match serde_json::to_string(&register) {
        Ok(payload) => payload,
        Err(err) => return ControlExit::Failed(format!("serialize register: {err}")),
    };
    if sink.send(Message::Text(payload)).await.is_err() {
        return ControlExit::Failed("register send failed".to_string());
    }

    // 等 Registered 确认
    loop {
        let reply = match tokio::time::timeout(REGISTER_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(msg))) => msg,
            _ => return ControlExit::Failed("no register reply".to_string()),
        };
        match reply {
            Message::Text(text) => match serde_json::from_str::<RelayToHost>(&text) {
                Ok(RelayToHost::Registered) => break,
                Ok(RelayToHost::Error { message }) => return ControlExit::Failed(message),
                _ => return ControlExit::Failed("unexpected register reply".to_string()),
            },
            Message::Ping(payload) => {
                let _ = sink.send(Message::Pong(payload)).await;
            }
            _ => return ControlExit::Failed("unexpected register reply".to_string()),
        }
    }
    set_relay_state(app, "online");
    if app.state::<RemoteState>().audit_enabled {
        audit::log(
            "relay-registered",
            json!({ "relay": relay_url, "hostId": host_id }),
        );
    }

    // 控制循环:转发 ClientConnected → 拨数据连接
    let mut idle = tokio::time::interval(IDLE_DISCONNECT);
    idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_inbound = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                let _ = sink.close().await;
                return ControlExit::Shutdown;
            }
            inbound = stream.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        last_inbound = tokio::time::Instant::now();
                        if let Ok(RelayToHost::ClientConnected { conn_id, peer }) =
                            serde_json::from_str::<RelayToHost>(&text)
                        {
                            dial_data_connection(app, relay_url, conn_id, peer, shutdown.clone());
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        last_inbound = tokio::time::Instant::now();
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        return ControlExit::Registered("relay connection lost".to_string());
                    }
                    Some(Ok(_)) => {
                        last_inbound = tokio::time::Instant::now();
                    }
                }
            }
            _ = idle.tick() => {
                if last_inbound.elapsed() > IDLE_DISCONNECT {
                    let _ = sink.close().await;
                    return ControlExit::Registered("relay idle timeout".to_string());
                }
                if sink.send(Message::Ping(Vec::new())).await.is_err() {
                    return ControlExit::Registered("relay connection lost".to_string());
                }
            }
        }
    }
}

/// 为一个手机接入拨数据连接;成功后与 LAN 直连同路径(E2EE 握手 + 认证)。
fn dial_data_connection<R: Runtime>(
    app: &AppHandle<R>,
    relay_url: &str,
    conn_id: String,
    peer: Option<String>,
    shutdown: watch::Receiver<bool>,
) {
    let app = app.clone();
    let url = host_data_url(relay_url, &conn_id);
    // relay 报告的客户端地址用于认证限流与审计;解析失败按 0.0.0.0 归并限流
    let peer_ip: IpAddr = peer
        .as_deref()
        .and_then(|p| {
            p.parse::<SocketAddr>()
                .map(|addr| addr.ip())
                .or_else(|_| p.parse::<IpAddr>())
                .ok()
        })
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    tauri::async_runtime::spawn(async move {
        match connect_async_with_config(&url, Some(ws_config()), false).await {
            Ok((ws, _)) => {
                super::server::serve_ws(app, ws, peer_ip, shutdown).await;
            }
            Err(err) => {
                eprintln!("[remote] relay data dial failed: {err}");
            }
        }
    });
}
