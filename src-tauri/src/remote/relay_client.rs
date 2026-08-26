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
        let exit = run_control(&app, &relay_url, &relay_token, &mut shutdown).await;
        match &exit {
            ControlExit::Shutdown => break,
            ControlExit::Registered(err) | ControlExit::Failed(err) => {
                set_relay_state(&app, &format!("error:{err}"));
            }
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = backoff_after(&exit, backoff);
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

/// 一次控制连接结束后,下一次重连该等多久。
///
/// 注册成功过说明 relay 地址与 token 都是对的,断开更像网络抖动 → 退避复位,
/// 手机侧尽快恢复可达。连注册都没成功(地址错、token 错、relay 没起来)则继续
/// 指数退避到上限,避免对一个坏配置每秒敲一次。
fn backoff_after(exit: &ControlExit, current: Duration) -> Duration {
    match exit {
        ControlExit::Registered(_) => INITIAL_BACKOFF,
        _ => (current * 2).min(MAX_BACKOFF),
    }
}

/// 把 relay 报的客户端地址解析成 IP。
///
/// 这个 IP 会喂给认证限流(`auth` 的失败节流)与审计日志,所以解析口径本身就是
/// 安全边界的一部分:全都退化成同一个地址,就等于把所有 relay 客户端并进一个
/// 限流桶,一个人试错会锁住其他人。解析不出来时用 `0.0.0.0` 归并,是"宁可并到
/// 一起限流,也不放行"的选择。
fn peer_ip_from_relay(peer: Option<&str>) -> IpAddr {
    peer.and_then(|p| {
        p.parse::<SocketAddr>()
            .map(|addr| addr.ip())
            .or_else(|_| p.parse::<IpAddr>())
            .ok()
    })
    .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
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
                            dial_data_connection(
                                app,
                                relay_url,
                                conn_id,
                                peer,
                                host_id.clone(),
                                shutdown.clone(),
                            );
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
    host_id: String,
    shutdown: watch::Receiver<bool>,
) {
    let app = app.clone();
    let url = host_data_url(relay_url, &conn_id);
    // relay 报告的客户端地址用于认证限流与审计;解析失败按 0.0.0.0 归并限流
    let peer_ip = peer_ip_from_relay(peer.as_deref());
    tauri::async_runtime::spawn(async move {
        match connect_async_with_config(&url, Some(ws_config()), false).await {
            Ok((ws, _)) => {
                super::server::serve_ws(
                    app,
                    ws,
                    peer_ip,
                    super::server::ConnectionContext::Relay { host_id },
                    shutdown,
                )
                .await;
            }
            Err(err) => {
                eprintln!("[remote] relay data dial failed: {err}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_connection_resets_the_backoff() {
        // 注册成功过说明地址与 token 都对,断开是网络抖动:必须复位,否则一次长断线
        // 之后手机要等到 60s 才可能重新连上。
        let exit = ControlExit::Registered("relay connection lost".to_string());
        assert_eq!(backoff_after(&exit, MAX_BACKOFF), INITIAL_BACKOFF);
        assert_eq!(backoff_after(&exit, INITIAL_BACKOFF), INITIAL_BACKOFF);
    }

    #[test]
    fn a_never_registered_connection_backs_off_exponentially_up_to_the_cap() {
        // 地址错 / token 错 / relay 没起来时不能每秒敲一次。
        let exit = ControlExit::Failed("connect failed".to_string());
        let mut backoff = INITIAL_BACKOFF;
        let mut seen = vec![backoff];
        for _ in 0..10 {
            backoff = backoff_after(&exit, backoff);
            seen.push(backoff);
        }
        assert_eq!(
            &seen[..7],
            &[
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(16),
                Duration::from_secs(32),
                Duration::from_secs(60),
            ],
            "got {seen:?}"
        );
        assert_eq!(
            backoff, MAX_BACKOFF,
            "must saturate, not overflow past the cap"
        );
    }

    #[test]
    fn distinct_relay_peers_get_distinct_rate_limit_buckets() {
        // 这是本函数存在的理由:peer_ip 会喂给认证失败节流。全都退化成同一个 IP
        // 就等于把所有 relay 客户端并进一个限流桶,一个人试错会锁住其他人。
        assert_eq!(
            peer_ip_from_relay(Some("1.2.3.4:5678")),
            "1.2.3.4".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            peer_ip_from_relay(Some("1.2.3.4")),
            "1.2.3.4".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            peer_ip_from_relay(Some("[::1]:443")),
            "::1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            peer_ip_from_relay(Some("2001:db8::1")),
            "2001:db8::1".parse::<IpAddr>().unwrap()
        );
        assert_ne!(
            peer_ip_from_relay(Some("1.2.3.4:1")),
            peer_ip_from_relay(Some("1.2.3.5:1")),
            "同一端口不同主机不能并进一个桶"
        );
    }

    #[test]
    fn an_unusable_peer_address_falls_back_to_a_shared_bucket() {
        // 解析不出来时"宁可并到一起限流,也不放行":返回 0.0.0.0 而不是放弃限流。
        let shared = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        assert_eq!(peer_ip_from_relay(None), shared);
        assert_eq!(peer_ip_from_relay(Some("")), shared);
        assert_eq!(peer_ip_from_relay(Some("not-an-address")), shared);
        assert_eq!(peer_ip_from_relay(Some("1.2.3.4:notaport")), shared);
        assert_eq!(peer_ip_from_relay(Some("999.1.1.1")), shared);
    }

    #[test]
    fn relay_connections_carry_the_same_message_cap_as_lan_connections() {
        // relay 与 LAN 直连共用 serve_ws,上限不一致会让同一台手机经 relay 与经
        // 局域网表现不同(一边收得下、一边直接断),排查起来完全没有线索。
        let config = ws_config();
        assert_eq!(config.max_message_size, Some(1024 * 1024));
        assert_eq!(config.max_frame_size, Some(1024 * 1024));
        assert_eq!(MAX_MESSAGE_BYTES, 1024 * 1024);
    }
}
