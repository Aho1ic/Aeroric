//! 手机远程连接(Mobile Remote)模块入口。
//!
//! 架构见 docs/mobile-remote-vibe-coding-plan-2026-07-28.md。M0 范围:
//! 局域网 WS 服务 + QR 配对 + 设备注册表 + 窄面 RPC + 事件桥。
//! 服务默认关闭,由设置页开关控制;开关与端口持久化在
//! `~/.aeroric/remote-config.json`(不进 settings.json,避免搅动大结构)。
//!
//! P8-03 刀一：纯逻辑半边（protocol/crypto/orca_crypto/terminal_frames/
//! terminal_hub/event_log/agent_keymap/auth）已上收 core `remote`，这里
//! re-export 保住 `super::` 旧路径；宿主只剩 WS 服务器、Tauri 事件桥、
//! 命令壳、RPC dispatch 与审计落点。

mod agent_config_rpc;
mod audit;
mod bridge;
mod events_bridge;
mod files_rpc;
mod relay_client;
mod session_push;
mod tasks_rpc;
mod terminal_streams;

// 刀一上收 core 的九个模块，按旧名 re-export（`super::crypto` 等旧路径不变）。
pub(crate) use aeroric_core::remote::{agent_keymap, protocol, terminal_frames, terminal_hub};

pub(crate) use session_push::publish_session_appended;

#[cfg(test)]
mod protocol_golden_parity {
    /// 刀一随 protocol.rs 上收 core：v3 golden fixture 的 hello.capabilities
    /// 必须与宿主 rpc::rpc_capabilities_value() 逐字节一致（rpc dispatch 面
    /// 刀二上收前，capabilities 的权威值在宿主）。
    #[test]
    fn v3_golden_hello_capabilities_match_host_rpc_capabilities() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../packages/remote-contracts/fixtures/rpc-golden.json"
        ))
        .expect("shared golden fixture");
        assert_eq!(
            fixture["hello"]["capabilities"],
            aeroric_core::remote::rpc::rpc_capabilities_value()
        );
    }
}

#[cfg(test)]
mod lan_roundtrip_tests;

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime, State};

use aeroric_core::remote::services::RemoteHostHandle;
pub use aeroric_core::remote::state::RemoteState;
use aeroric_core::remote::state::{ensure_stopped as core_ensure_stopped, ListenerScope};

// ── 配置与身份面（P8-03 刀三上收 core，这里按旧名 re-export） ──────────────

pub use aeroric_core::remote::config::{
    normalize_public_endpoint, normalize_relay_url, save_config,
};
pub use aeroric_core::remote::identity::{
    host_name, lan_addresses, pairing_endpoints, selected_lan_ip, RemoteNetworkAddress,
};

// ── 全局状态（P8-03 刀三b：RemoteState 上收 core `remote::state`，这里薄包装） ──

/// 刀一：core `terminal_hub` 不探文件系统（G24 读取面一个点），宿主在此
/// 安装历史长度解析器；幂等——多次构造只装一次。
pub fn install_terminal_history_resolver() {
    use std::sync::Arc;
    aeroric_core::remote::terminal_hub::set_history_len_resolver(Arc::new(
        crate::storage::task_terminal_history_len,
    ));
}

/// 应用启动时调用(lib.rs setup):按持久化配置决定是否自动拉起服务。
pub fn init<R: Runtime>(app: AppHandle<R>) {
    let enabled = app
        .state::<std::sync::Arc<RemoteState>>()
        .config
        .lock()
        .enabled;
    if enabled {
        tauri::async_runtime::spawn(async move {
            let managed = app.state::<std::sync::Arc<RemoteState>>();
            let _lifecycle = managed.lifecycle.lock().await;
            // The setting may have changed while this startup task waited for
            // another control-plane operation.
            if !managed.config.lock().enabled {
                return;
            }
            if let Err(err) = ensure_started(&app).await {
                eprintln!("[remote] auto-start failed: {err}");
            }
        });
    }
}

async fn ensure_started<R: Runtime>(app: &AppHandle<R>) -> Result<u16, String> {
    // 生产路径必须经宿主 wrapper：事件桥（task-status/task-session listen_any）
    // 与 relay 出站连接挂在这里，core start_with_scope 只管 TCP。
    let state = app.state::<std::sync::Arc<RemoteState>>().inner().clone();
    let mut running = state.running.lock().await;
    if let Some(server) = running.as_ref() {
        return Ok(server.port);
    }
    let port = state.config.lock().port;
    let scope = aeroric_core::remote::state::required_listener_scope(&state);
    let server = bridge::start_with_host_services(app, port, scope).await?;
    let bound = server.port;
    *running = Some(server);
    Ok(bound)
}

async fn ensure_stopped<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<std::sync::Arc<RemoteState>>().inner().clone();
    core_ensure_stopped(&state).await;
}

// ── Tauri commands(桌面设置页专用) ─────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub running: bool,
    /// 是否实际监听在 LAN/公网接口。false 表示服务仅在 127.0.0.1，
    /// 生成配对二维码前不应把 LAN 地址当作可连端点展示或复制。
    pub network_exposed: bool,
    pub port: u16,
    pub lan_ip: Option<String>,
    pub lan_addresses: Vec<RemoteNetworkAddress>,
    pub online_count: usize,
    pub relay_url: Option<String>,
    pub relay_token: Option<String>,
    pub public_endpoints: Vec<String>,
    /// off | connecting | online | error:<msg>
    pub relay_state: String,
}

async fn current_status<R: Runtime>(app: &AppHandle<R>) -> RemoteStatus {
    let state = app.state::<std::sync::Arc<RemoteState>>();
    let (enabled, port, preferred_lan_ip, relay_url, relay_token, public_endpoints) = {
        let cfg = state.config.lock();
        (
            cfg.enabled,
            cfg.port,
            cfg.preferred_lan_ip.clone(),
            cfg.relay_url.clone(),
            cfg.relay_token.clone(),
            cfg.public_endpoints.clone(),
        )
    };
    let (running_port, network_exposed) = {
        let running = state.running.lock().await;
        let handle = running.as_ref().filter(|handle| handle.is_listening());
        (
            handle.map(|handle| handle.port),
            handle.is_some_and(|handle| handle.network_exposed()),
        )
    };
    let relay_state = state.relay_state.lock().clone();
    let lan_addresses = lan_addresses();
    let lan_ip = selected_lan_ip(preferred_lan_ip.as_deref(), &lan_addresses);
    RemoteStatus {
        enabled,
        running: running_port.is_some(),
        network_exposed,
        port: running_port.unwrap_or(port),
        lan_ip,
        lan_addresses,
        online_count: state.clients.online_count(),
        relay_url,
        relay_token,
        public_endpoints,
        relay_state,
    }
}

#[tauri::command]
pub async fn remote_select_lan_ip<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<RemoteState>>,
    lan_ip: String,
) -> Result<RemoteStatus, String> {
    let _lifecycle = state.lifecycle.lock().await;
    let lan_ip = lan_ip.trim();
    let addresses = lan_addresses();
    if !addresses.iter().any(|address| address.ip == lan_ip) {
        return Err(format!("Local IP is no longer available: {lan_ip}"));
    }
    {
        let mut cfg = state.config.lock();
        cfg.preferred_lan_ip = Some(lan_ip.to_string());
        save_config(&cfg)?;
    }
    Ok(current_status(&app).await)
}

#[tauri::command]
pub async fn remote_server_status<R: Runtime>(app: AppHandle<R>) -> Result<RemoteStatus, String> {
    let state = app.state::<std::sync::Arc<RemoteState>>();
    let _lifecycle = state.lifecycle.lock().await;
    Ok(current_status(&app).await)
}

#[tauri::command]
pub async fn remote_server_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<RemoteState>>,
    port: Option<u16>,
) -> Result<RemoteStatus, String> {
    let _lifecycle = state.lifecycle.lock().await;
    {
        let mut cfg = state.config.lock();
        if let Some(p) = port {
            if p < 1024 {
                return Err("Port must be >= 1024".to_string());
            }
            cfg.port = p;
        }
        cfg.enabled = true;
        save_config(&cfg)?;
    }
    ensure_started(&app).await?;
    Ok(current_status(&app).await)
}

#[tauri::command]
pub async fn remote_server_stop<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<RemoteState>>,
) -> Result<RemoteStatus, String> {
    let _lifecycle = state.lifecycle.lock().await;
    {
        let mut cfg = state.config.lock();
        cfg.enabled = false;
        save_config(&cfg)?;
    }
    ensure_stopped(&app).await;
    Ok(current_status(&app).await)
}

/// 更新公网访问配置(relay / 自定义公网地址)。服务在运行时重启以生效。
#[tauri::command]
pub async fn remote_update_config<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<RemoteState>>,
    relay_url: Option<String>,
    relay_token: Option<String>,
    public_endpoints: Option<Vec<String>>,
) -> Result<RemoteStatus, String> {
    let _lifecycle = state.lifecycle.lock().await;
    let was_running;
    {
        let mut cfg = state.config.lock();
        let mut next = cfg.clone();
        let relay_url = relay_url.map(|url| url.trim().to_string());
        match relay_url {
            Some(url) if url.is_empty() => {
                next.relay_url = None;
                next.relay_token = None;
            }
            Some(url) => {
                next.relay_url = Some(normalize_relay_url(&url)?);
            }
            None => {}
        }
        if let Some(token) = relay_token {
            let token = token.trim().to_string();
            next.relay_token = (!token.is_empty()).then_some(token);
        }
        if let Some(endpoints) = public_endpoints {
            let mut cleaned = Vec::new();
            for endpoint in endpoints {
                if endpoint.trim().is_empty() {
                    continue;
                }
                let endpoint = normalize_public_endpoint(&endpoint)?;
                if !cleaned.contains(&endpoint) {
                    cleaned.push(endpoint);
                }
            }
            next.public_endpoints = cleaned;
        }
        if next.relay_url.is_some() && next.relay_token.is_none() {
            return Err("Relay token is required when a relay URL is configured".to_string());
        }
        save_config(&next)?;
        was_running = next.enabled;
        *cfg = next;
    }
    // relay 生命周期挂在服务上:重启使新配置生效
    if was_running {
        ensure_stopped(&app).await;
        ensure_started(&app).await?;
    }
    Ok(current_status(&app).await)
}

/// 生成一次性 invite 并组装配对深链(前端渲染成 QR)。
#[tauri::command]
pub async fn remote_create_invite<R: Runtime>(
    app: AppHandle<R>,
) -> Result<serde_json::Value, String> {
    let state = app.state::<std::sync::Arc<RemoteState>>();
    let _lifecycle = state.lifecycle.lock().await;
    create_invite_for_addresses_locked(&app, lan_addresses()).await
}

/// Validate endpoints, widen the listener when a LAN endpoint needs it, and
/// issue the token under one control-plane lock. Injecting addresses makes the
/// no-interface failure path deterministic in tests.
#[cfg(test)]
async fn create_invite_for_addresses<R: Runtime>(
    app: &AppHandle<R>,
    addresses: Vec<RemoteNetworkAddress>,
) -> Result<serde_json::Value, String> {
    let state = app.state::<std::sync::Arc<RemoteState>>();
    let _lifecycle = state.lifecycle.lock().await;
    create_invite_for_addresses_locked(app, addresses).await
}

/// Requires `RemoteState::lifecycle`. Do not move token creation after the
/// lock is dropped: a concurrent stop/configuration restart would otherwise
/// invalidate the listener between validation and QR creation.
async fn create_invite_for_addresses_locked<R: Runtime>(
    app: &AppHandle<R>,
    addresses: Vec<RemoteNetworkAddress>,
) -> Result<serde_json::Value, String> {
    let state = app.state::<std::sync::Arc<RemoteState>>().inner().clone();
    let bridge: std::sync::Arc<dyn RemoteHostHandle> =
        std::sync::Arc::new(bridge::RemoteHostBridge::new(app.clone()));
    let mut running = state.running.lock().await;
    let Some(mut server) = running.take() else {
        return Err("Remote server is not running".to_string());
    };

    let result = async {
        let config = state.config.lock().clone();
        let host_id = state.keys.host_id();
        let listener_port = server.port;
        let pairing_endpoints = pairing_endpoints(listener_port, &addresses, &config, &host_id);
        if pairing_endpoints.endpoints.is_empty() {
            // Do this before replacing the listener. A failed QR request must
            // retain a local-only server instead of widening it pointlessly.
            return Err("No LAN address detected and no public endpoints configured".to_string());
        }

        if pairing_endpoints.includes_lan {
            // The LAN URL was built from this exact handle. Keeping it out of
            // `running` until the token exists prevents stop/restart from
            // making a successfully returned QR point at a stale listener.
            server
                .ensure_listener_scope(&state, &bridge, ListenerScope::Network)
                .await?;
            if !server.network_exposed() || server.port != listener_port {
                return Err("Remote server could not verify its network listener".to_string());
            }
        }
        if !server.is_listening() {
            return Err("Remote server is not running".to_string());
        }

        #[allow(clippy::expect_used, reason = "上文已对 endpoints.is_empty() 早返回")]
        let primary = pairing_endpoints
            .endpoints
            .first()
            .cloned()
            .expect("checked for a non-empty pairing endpoint list");
        // Keep this last: every fallible listener/endpoint step above is done,
        // so no failed request consumes a single-use token.
        let invite = state.auth.lock().create_invite()?;
        audit::log("invite-created", json!({}));
        let offer = json!({
            "v": protocol::PROTOCOL_VERSION,
            "endpoints": pairing_endpoints.endpoints,
            "invite": invite,
            "hostName": host_name(),
            "hostId": host_id,
            // 桌面静态公钥:手机首连 pinning,E2EE 握手的信任根
            "publicKey": state.keys.public_b64(),
            // Optional additive negotiation. Older phones ignore this field
            // and continue using the legacy single-step pairing flow.
            "pairingConfirmationVersion": protocol::PAIRING_CONFIRMATION_V1,
        });
        let code = URL_SAFE_NO_PAD.encode(offer.to_string());
        Ok(json!({
            "pairingUrl": format!("aeroric://pair?code={code}"),
            "endpoint": primary,
            "expiresInSeconds": 600,
        }))
    }
    .await;

    // A LAN endpoint may have widened the listener before token generation
    // fails (for example, if the CSPRNG is unavailable). Never retain that
    // broader bind after an unsuccessful QR request: restore exactly the
    // scope required by the current device/configuration policy. If recovery
    // itself fails, shutting down is safer than leaving a possibly-wide
    // listener running.
    if result.is_err() {
        let required_scope = aeroric_core::remote::state::required_listener_scope(&state);
        if let Err(scope_error) = server
            .ensure_listener_scope(&state, &bridge, required_scope)
            .await
        {
            #[allow(clippy::expect_used, reason = "上一行 if result.is_err() 已保证")]
            let original_error = result.expect_err("checked invite creation failure");
            server.shutdown_and_wait(&state).await;
            return Err(format!(
                "{original_error}; additionally failed to restore the remote listener scope: {scope_error}"
            ));
        }
    }

    if server.is_listening() {
        *running = Some(server);
    } else {
        // Both wide bind and loopback recovery failed. Tear down the rest of
        // the server lifecycle before a later clean retry.
        server.shutdown_and_wait(&state).await;
    }
    result
}

#[cfg(test)]
mod address_tests {
    use super::{
        normalize_public_endpoint, normalize_relay_url, selected_lan_ip, RemoteNetworkAddress,
    };
    use aeroric_core::remote::identity::address_priority;
    use std::net::Ipv4Addr;

    fn address(interface_name: &str, ip: &str) -> RemoteNetworkAddress {
        RemoteNetworkAddress {
            interface_name: interface_name.to_string(),
            ip: ip.to_string(),
        }
    }

    #[test]
    fn regular_lan_addresses_rank_before_tunnels_and_proxy_benchmark_ranges() {
        assert!(
            address_priority(Ipv4Addr::new(192, 168, 1, 10), "en0")
                < address_priority(Ipv4Addr::new(100, 125, 106, 127), "utun4")
        );
        assert!(
            address_priority(Ipv4Addr::new(100, 125, 106, 127), "utun4")
                < address_priority(Ipv4Addr::new(198, 18, 0, 1), "utun1024")
        );
    }

    #[test]
    fn saved_available_address_wins_over_automatic_order() {
        let addresses = vec![address("en0", "192.168.1.10"), address("utun5", "10.0.0.2")];
        assert_eq!(
            selected_lan_ip(Some("10.0.0.2"), &addresses).as_deref(),
            Some("10.0.0.2")
        );
        assert_eq!(
            selected_lan_ip(Some("198.18.0.1"), &addresses).as_deref(),
            Some("192.168.1.10")
        );
    }

    #[test]
    fn relay_urls_require_tls_except_on_loopback() {
        assert_eq!(
            normalize_relay_url("wss://relay.example.test/").unwrap(),
            "wss://relay.example.test"
        );
        assert!(normalize_relay_url("ws://relay.example.test").is_err());
        assert!(normalize_relay_url("ws://192.168.1.2:6791").is_err());
        assert_eq!(
            normalize_relay_url("ws://127.0.0.1:6791/").unwrap(),
            "ws://127.0.0.1:6791"
        );
        assert_eq!(
            normalize_relay_url("ws://[::1]:6791").unwrap(),
            "ws://[::1]:6791"
        );
    }

    #[test]
    fn websocket_urls_reject_credentials_queries_and_invalid_schemes() {
        assert!(normalize_relay_url("wss://user:secret@relay.example.test").is_err());
        assert!(normalize_relay_url("wss://relay.example.test?token=secret").is_err());
        assert!(normalize_relay_url("https://relay.example.test").is_err());
        assert_eq!(
            normalize_public_endpoint("ws://100.64.0.2:6790/").unwrap(),
            "ws://100.64.0.2:6790"
        );
    }
}

#[cfg(test)]
mod network_exposure_tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use tauri::Manager;

    use super::{
        create_invite_for_addresses, ensure_started, ensure_stopped, revoke_device_for_test,
        RemoteNetworkAddress, RemoteState,
    };
    use crate::remote::bridge;
    use aeroric_core::remote::auth::AuthOutcome;
    use aeroric_core::remote::services::RemoteHostHandle;
    use aeroric_core::remote::state::abort_unconfirmed_pairing;

    fn address(ip: &str) -> RemoteNetworkAddress {
        RemoteNetworkAddress {
            interface_name: "test0".to_string(),
            ip: ip.to_string(),
        }
    }

    fn pair_test_device(state: &RemoteState) -> String {
        let invite = state.auth.lock().create_invite().expect("create invite");
        match state.auth.lock().authenticate(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            Some(&invite),
            None,
            Some("Test phone"),
        ) {
            Ok(AuthOutcome::Paired { device_id, .. }) => device_id,
            _ => panic!("expected paired device"),
        }
    }

    #[test]
    fn fresh_server_binds_loopback_then_widens_before_pairing() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle
                .state::<std::sync::Arc<RemoteState>>()
                .config
                .lock()
                .port = 0;
            let initial_port = ensure_started(&handle)
                .await
                .expect("start loopback server");

            {
                let state = handle.state::<std::sync::Arc<RemoteState>>();
                let running = state.running.lock().await;
                let server = running.as_ref().expect("server handle");
                assert_eq!(server.port, initial_port);
                assert_eq!(
                    server.bound_addr().expect("bound address").ip(),
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    "a fresh server must not accept LAN connections before a pairing action"
                );
                assert!(!server.network_exposed());
            }

            let invite = create_invite_for_addresses(&handle, vec![address("192.168.1.10")])
                .await
                .expect("create invite after widening listener");
            assert_eq!(
                invite["endpoint"],
                format!("ws://192.168.1.10:{initial_port}"),
                "the QR must retain the port from the verified replacement listener"
            );

            {
                let state = handle.state::<std::sync::Arc<RemoteState>>();
                let running = state.running.lock().await;
                let server = running.as_ref().expect("server handle after widen");
                assert_eq!(
                    server.bound_addr().expect("bound address").ip(),
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                );
                assert!(server.network_exposed());
            }
            // Exercise the actual replacement listener, rather than only its
            // bookkeeping: an all-interface bind must still accept loopback.
            assert!(
                tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, initial_port))
                    .await
                    .is_ok()
            );

            // A second request is serialized by the lifecycle guard and sees
            // the already widened listener instead of replacing it again.
            create_invite_for_addresses(&handle, vec![address("192.168.1.10")])
                .await
                .expect("idempotent second invite");
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn failed_invite_without_endpoints_keeps_loopback_listener() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle
                .state::<std::sync::Arc<RemoteState>>()
                .config
                .lock()
                .port = 0;
            ensure_started(&handle)
                .await
                .expect("start loopback server");

            let error = create_invite_for_addresses(&handle, Vec::new())
                .await
                .expect_err("an invite without a LAN, relay, or public endpoint must fail");
            assert!(error.contains("No LAN address detected"));

            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let running = state.running.lock().await;
            let server = running
                .as_ref()
                .expect("loopback server retained after failure");
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "a failed endpoint check must not widen or tear down the listener"
            );
            assert!(!server.network_exposed());
            drop(running);
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn failed_invite_after_widening_restores_loopback_listener() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle
                .state::<std::sync::Arc<RemoteState>>()
                .config
                .lock()
                .port = 0;
            let port = ensure_started(&handle)
                .await
                .expect("start loopback server");
            handle
                .state::<std::sync::Arc<RemoteState>>()
                .auth
                .lock()
                .set_invite_failure(Some("simulated CSPRNG unavailable".to_string()));

            let error = create_invite_for_addresses(&handle, vec![address("192.168.1.10")])
                .await
                .expect_err("injected token-generation failure must reach the caller");
            assert!(error.contains("simulated CSPRNG unavailable"));

            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let running = state.running.lock().await;
            let server = running
                .as_ref()
                .expect("server remains available locally after failed invite");
            assert_eq!(server.port, port);
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "a post-widening invite failure must not retain a LAN listener"
            );
            assert!(!server.network_exposed());
            drop(running);
            assert!(
                tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_ok(),
                "the recovered loopback listener must still accept local connections"
            );
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn invite_creation_waits_for_the_lifecycle_transaction() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle
                .state::<std::sync::Arc<RemoteState>>()
                .config
                .lock()
                .port = 0;
            ensure_started(&handle)
                .await
                .expect("start loopback server");

            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let lifecycle = state.lifecycle.lock().await;
            let pending_invite =
                create_invite_for_addresses(&handle, vec![address("192.168.1.10")]);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), pending_invite)
                    .await
                    .is_err(),
                "an invite must not interleave with a stop or configuration transaction"
            );
            drop(lifecycle);

            create_invite_for_addresses(&handle, vec![address("192.168.1.10")])
                .await
                .expect("invite succeeds once the control-plane transaction completes");
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn paired_device_reconnects_over_network_after_restart() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let invite = state.auth.lock().create_invite().expect("create invite");
            assert!(state
                .auth
                .lock()
                .authenticate(
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
                    Some(&invite),
                    None,
                    Some("Existing phone"),
                )
                .is_ok());
            state.config.lock().port = 0;

            ensure_started(&handle)
                .await
                .expect("start server for paired device");
            let running = state.running.lock().await;
            let server = running.as_ref().expect("server handle");
            assert!(server.network_exposed());
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                "a paired device must be able to reconnect without creating a new QR"
            );
            drop(running);
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn revoking_last_device_downscopes_listener_to_loopback() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let device_id = pair_test_device(&state);
            state.config.lock().port = 0;
            ensure_started(&handle)
                .await
                .expect("start network listener for paired device");

            revoke_device_for_test(&handle, &device_id)
                .await
                .expect("revoke last device");

            let running = state.running.lock().await;
            let server = running.as_ref().expect("server remains available locally");
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "removing the final paired device must close the network listener"
            );
            assert!(!server.network_exposed());
            drop(running);
            assert!(state.auth.lock().devices().is_empty());
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn aborting_an_unconfirmed_pairing_downscopes_to_loopback() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let device_id = pair_test_device(&state);
            state.config.lock().port = 0;
            ensure_started(&handle)
                .await
                .expect("start network listener for pending paired device");

            {
                let state = handle
                    .state::<std::sync::Arc<RemoteState>>()
                    .inner()
                    .clone();
                let bridge: std::sync::Arc<dyn RemoteHostHandle> =
                    std::sync::Arc::new(bridge::RemoteHostBridge::new(handle.clone()));
                abort_unconfirmed_pairing(&state, &bridge, &device_id).await;
            }

            assert!(state.auth.lock().devices().is_empty());
            let running = state.running.lock().await;
            let server = running.as_ref().expect("local server remains available");
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "rolling back an unconfirmed final pairing must remove LAN exposure"
            );
            assert!(!server.network_exposed());
            drop(running);
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn revoking_last_device_keeps_network_listener_for_public_endpoint() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<std::sync::Arc<RemoteState>>();
            let device_id = pair_test_device(&state);
            {
                let mut config = state.config.lock();
                config.port = 0;
                config.public_endpoints = vec!["wss://tunnel.example.test".to_string()];
            }
            ensure_started(&handle)
                .await
                .expect("start network listener for paired device");

            revoke_device_for_test(&handle, &device_id)
                .await
                .expect("revoke last device with public endpoint configured");

            let running = state.running.lock().await;
            let server = running
                .as_ref()
                .expect("public endpoint keeps server running");
            assert_eq!(
                server.bound_addr().expect("bound address").ip(),
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                "an explicit public endpoint remains a network-bind opt-in"
            );
            assert!(server.network_exposed());
            drop(running);
            ensure_stopped(&handle).await;
        });
    }

    #[test]
    fn configured_direct_public_endpoint_is_an_explicit_wide_bind_opt_in() {
        let app = tauri::test::mock_app();
        app.manage(std::sync::Arc::new(RemoteState::new_in_memory()));
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<std::sync::Arc<RemoteState>>();
            {
                let mut config = state.config.lock();
                config.port = 0;
                config.public_endpoints = vec!["wss://tunnel.example.test".to_string()];
            }

            ensure_started(&handle)
                .await
                .expect("start server for configured tunnel");
            let running = state.running.lock().await;
            assert!(
                running.as_ref().expect("server handle").network_exposed(),
                "a user-configured direct endpoint must not become a dead URL after restart"
            );
            drop(running);
            ensure_stopped(&handle).await;
        });
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeviceView {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub online: bool,
}

#[tauri::command]
pub fn remote_list_devices(state: State<'_, RemoteState>) -> Result<Vec<RemoteDeviceView>, String> {
    let online = state.clients.online_device_ids();
    let auth = state.auth.lock();
    Ok(auth
        .devices()
        .iter()
        .map(|d| RemoteDeviceView {
            id: d.id.clone(),
            name: d.name.clone(),
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
            online: online.contains(&d.id),
        })
        .collect())
}

#[tauri::command]
pub async fn remote_revoke_device<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<RemoteState>>,
    device_id: String,
) -> Result<(), String> {
    let _lifecycle = state.lifecycle.lock().await;
    revoke_device_locked(&app, &device_id).await
}

/// Test hook for the same transactional revoke path without persisting user
/// configuration through the Tauri command wrapper.
#[cfg(test)]
async fn revoke_device_for_test<R: Runtime>(
    app: &AppHandle<R>,
    device_id: &str,
) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<RemoteState>>();
    let _lifecycle = state.lifecycle.lock().await;
    revoke_device_locked(app, device_id).await
}

/// Requires `RemoteState::lifecycle`. Keep auth -> client-registry ordering:
/// connection registration takes the same order, closing the auth-to-register
/// revoke race without holding any synchronous mutex across an await.
async fn revoke_device_locked<R: Runtime>(
    app: &AppHandle<R>,
    device_id: &str,
) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<RemoteState>>().inner().clone();
    let existed = {
        let mut auth = state.auth.lock();
        let existed = auth.revoke(device_id)?;
        if existed {
            state.clients.disconnect_device(device_id);
        }
        existed
    };
    if !existed {
        return Err("Device not found".to_string());
    }

    // A failed rebind may restore the old (wider) listener. In that case the
    // reconciler closes the server rather than retaining a network listener
    // after the last device was revoked. Revocation itself has already been
    // persisted, so return success and let the refreshed status show a safely
    // stopped service instead of falsely claiming the revoke was rolled back.
    {
        let bridge: std::sync::Arc<dyn RemoteHostHandle> =
            std::sync::Arc::new(bridge::RemoteHostBridge::new(app.clone()));
        if let Err(error) =
            aeroric_core::remote::state::reconcile_listener_scope_locked(&state, &bridge).await
        {
            eprintln!("[remote] server closed after revoke scope reconciliation failed: {error}");
            if state.audit_enabled {
                audit::log(
                    "listener-closed-after-revoke",
                    json!({ "deviceId": device_id, "error": error }),
                );
            }
        }
    }
    if state.audit_enabled {
        audit::log("device-revoked", json!({ "deviceId": device_id }));
    }
    Ok(())
}

#[tauri::command]
pub fn remote_complete_task_request(
    state: State<'_, Arc<RemoteState>>,
    request_id: String,
    accepted: bool,
    task_id: Option<String>,
    error: Option<String>,
    task: Option<serde_json::Value>,
) -> Result<(), String> {
    let result = if accepted {
        Ok(json!({ "accepted": true, "taskId": task_id, "task": task }))
    } else {
        Err(error.unwrap_or_else(|| "Desktop rejected the task request".to_string()))
    };
    state.task_requests.resolve(&request_id, result)
}
