//! 手机远程连接(Mobile Remote)模块入口。
//!
//! 架构见 docs/mobile-remote-vibe-coding-plan-2026-07-28.md。M0 范围:
//! 局域网 WS 服务 + QR 配对 + 设备注册表 + 窄面 RPC + 事件桥。
//! 服务默认关闭,由设置页开关控制;开关与端口持久化在
//! `~/.aeroric/remote-config.json`(不进 settings.json,避免搅动大结构)。

mod agent_config_rpc;
mod agent_keymap;
mod audit;
mod auth;
mod crypto;
mod event_log;
mod events_bridge;
mod files_rpc;
mod orca_crypto;
mod orca_rpc;
mod protocol;
mod relay_client;
mod rpc;
mod server;
mod session_push;
mod tasks_rpc;
mod terminal_frames;
pub(crate) mod terminal_hub;
mod terminal_streams;

pub(crate) use session_push::publish_session_appended;

#[cfg(test)]
mod lan_roundtrip_tests;

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::storage::{aeroric_dir, atomic_write_private, ensure_private_file_permissions};
use auth::AuthStore;
use event_log::EventLog;
use server::{ClientRegistry, ListenerScope, ServerHandle};
use tasks_rpc::{ApprovalRegistry, TaskRequestBroker};
use terminal_streams::TerminalLeaseRegistry;

pub const DEFAULT_PORT: u16 = 6790;

// ── 配置持久化 ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub port: u16,
    /// 配对二维码优先公布的本机 IPv4 地址；它只影响 QR 中的 LAN 候选
    /// 地址，不决定 listener 的绑定范围。
    #[serde(default)]
    pub preferred_lan_ip: Option<String>,
    /// 自托管 relay 基址(ws:// 或 wss://),空 = 不用 relay。
    #[serde(default)]
    pub relay_url: Option<String>,
    /// relay 部署方设置的共享口令(RELAY_TOKEN)。
    #[serde(default)]
    pub relay_token: Option<String>,
    /// 额外公网地址(Tailscale/frp/cloudflared 等),写入配对 QR。
    #[serde(default)]
    pub public_endpoints: Vec<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            preferred_lan_ip: None,
            relay_url: None,
            relay_token: None,
            public_endpoints: Vec::new(),
        }
    }
}

fn config_path() -> Result<std::path::PathBuf, String> {
    Ok(aeroric_dir()?.join("remote-config.json"))
}

fn load_config() -> RemoteConfig {
    let Ok(path) = config_path() else {
        return RemoteConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return RemoteConfig::default();
    };
    let _ = ensure_private_file_permissions(&path);
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_config(config: &RemoteConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    atomic_write_private(&path, &raw)
}

// ── 全局状态 ─────────────────────────────────────────────────────────────────

pub struct RemoteState {
    pub(crate) auth: parking_lot::Mutex<AuthStore>,
    pub(crate) clients: Arc<ClientRegistry>,
    pub(crate) terminal_leases: Arc<TerminalLeaseRegistry>,
    pub(crate) approvals: Arc<ApprovalRegistry>,
    pub(crate) task_requests: Arc<TaskRequestBroker>,
    /// 推送事件日志:重连 watermark 补发源。
    pub(crate) event_log: Arc<EventLog>,
    pub(crate) config: parking_lot::Mutex<RemoteConfig>,
    /// 静态 X25519 密钥对(E2EE 握手与 relay hostId 的根)。
    pub(crate) keys: crypto::StaticKeys,
    /// 内存态(测试)不写审计日志。
    pub(crate) audit_enabled: bool,
    /// relay 客户端状态:off | connecting | online | error:<msg>。
    pub(crate) relay_state: Arc<parking_lot::Mutex<String>>,
    /// tokio Mutex:start/stop 需要跨 await 持锁,防止并发启动竞态。
    running: tokio::sync::Mutex<Option<ServerHandle>>,
    /// 串行化会改变监听器、端点配置或邀请码的控制面操作。邀请码必须在
    /// 已验证的 listener 仍为当前 listener 时创建，不能与 stop/restart
    /// 或配置重启交错。
    lifecycle: tokio::sync::Mutex<()>,
}

impl RemoteState {
    pub fn new() -> Self {
        let keys = crypto::StaticKeys::load_or_create().unwrap_or_else(|err| {
            eprintln!("[remote] keypair load failed, using in-memory keys: {err}");
            crypto::StaticKeys::ephemeral().expect("CSPRNG unavailable")
        });
        Self {
            auth: parking_lot::Mutex::new(AuthStore::load()),
            clients: Arc::new(ClientRegistry::default()),
            terminal_leases: Arc::new(TerminalLeaseRegistry::default()),
            approvals: Arc::new(ApprovalRegistry::default()),
            task_requests: Arc::new(TaskRequestBroker::default()),
            event_log: Arc::new(EventLog::default()),
            config: parking_lot::Mutex::new(load_config()),
            keys,
            audit_enabled: true,
            relay_state: Arc::new(parking_lot::Mutex::new("off".to_string())),
            running: tokio::sync::Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }

    /// 集成测试用:内存 AuthStore + 默认配置,不触碰任何真实文件。
    #[cfg(test)]
    pub(crate) fn new_in_memory() -> Self {
        Self {
            auth: parking_lot::Mutex::new(AuthStore::in_memory()),
            clients: Arc::new(ClientRegistry::default()),
            terminal_leases: Arc::new(TerminalLeaseRegistry::default()),
            approvals: Arc::new(ApprovalRegistry::default()),
            task_requests: Arc::new(TaskRequestBroker::default()),
            event_log: Arc::new(EventLog::default()),
            config: parking_lot::Mutex::new(RemoteConfig::default()),
            keys: crypto::StaticKeys::ephemeral().expect("CSPRNG unavailable"),
            audit_enabled: false,
            relay_state: Arc::new(parking_lot::Mutex::new("off".to_string())),
            running: tokio::sync::Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }
}

/// 应用启动时调用(lib.rs setup):按持久化配置决定是否自动拉起服务。
pub fn init<R: Runtime>(app: AppHandle<R>) {
    let enabled = app.state::<RemoteState>().config.lock().enabled;
    if enabled {
        tauri::async_runtime::spawn(async move {
            let state = app.state::<RemoteState>();
            let _lifecycle = state.lifecycle.lock().await;
            // The setting may have changed while this startup task waited for
            // another control-plane operation.
            if !state.config.lock().enabled {
                return;
            }
            if let Err(err) = ensure_started(&app).await {
                eprintln!("[remote] auto-start failed: {err}");
            }
        });
    }
}

async fn ensure_started<R: Runtime>(app: &AppHandle<R>) -> Result<u16, String> {
    let state = app.state::<RemoteState>();
    let mut running = state.running.lock().await;
    if let Some(handle) = running.as_ref() {
        return Ok(handle.port);
    }
    let port = state.config.lock().port;
    let scope = required_listener_scope(&state);
    let handle = server::start_with_scope(app.clone(), port, scope).await?;
    let bound = handle.port;
    *running = Some(handle);
    Ok(bound)
}

/// Devices are the only implicit reason to keep a LAN listener open. A
/// configured direct public endpoint is an explicit opt-in; relay-only access
/// is outbound and therefore does not require a wide local TCP bind.
fn required_listener_scope(state: &RemoteState) -> ListenerScope {
    let has_public_endpoint = !state.config.lock().public_endpoints.is_empty();
    let has_paired_device = !state.auth.lock().devices().is_empty();
    if has_paired_device || has_public_endpoint {
        ListenerScope::Network
    } else {
        ListenerScope::Loopback
    }
}

async fn ensure_stopped<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<RemoteState>();
    let mut running = state.running.lock().await;
    if let Some(handle) = running.take() {
        handle.shutdown_and_wait(app).await;
    }
}

/// Acquire the control-plane lock before reconciling a listener after a
/// successfully authenticated device. This also makes an in-flight pairing
/// observe a concurrent revoke before it is registered as online.
pub(crate) async fn reconcile_listener_scope_for_current_policy<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    let state = app.state::<RemoteState>();
    let _lifecycle = state.lifecycle.lock().await;
    reconcile_listener_scope_locked(app).await
}

/// Requires `RemoteState::lifecycle`. A rebind failure is resolved by closing
/// the complete server rather than leaving a restored listener at a wider
/// scope than the current device/configuration policy permits.
async fn reconcile_listener_scope_locked<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<RemoteState>();
    let required_scope = required_listener_scope(&state);
    let mut running = state.running.lock().await;
    let Some(mut handle) = running.take() else {
        return Ok(());
    };

    match handle
        .ensure_listener_scope(app.clone(), required_scope)
        .await
    {
        Ok(()) if handle.is_listening() => {
            *running = Some(handle);
            Ok(())
        }
        Ok(()) => {
            handle.shutdown_and_wait(app).await;
            Err("Remote server listener stopped unexpectedly while changing scope".to_string())
        }
        Err(error) => {
            // `ensure_listener_scope` may have restored the previous scope.
            // Do not retain that possibly wider listener after a revoke.
            handle.shutdown_and_wait(app).await;
            Err(error)
        }
    }
}

/// Remove a device that was persisted while handling an invite but could not
/// be committed to the peer (for example, shutdown/rebind/register/reply
/// failure). The operation is idempotent with a concurrent user revoke and
/// uses the same lifecycle → auth → registry lock ordering as normal revoke.
pub(crate) async fn abort_unconfirmed_pairing<R: Runtime>(app: &AppHandle<R>, device_id: &str) {
    let state = app.state::<RemoteState>();
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
    if let Err(error) = reconcile_listener_scope_locked(app).await {
        eprintln!("[remote] server closed after rolling back unconfirmed pairing: {error}");
    }
}

// ── 辅助:主机名 / 局域网 IP ─────────────────────────────────────────────────

pub(crate) fn host_name() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a valid writable byte array and its length is the
        // exact capacity passed to libc. `gethostname` writes at most that
        // many bytes; the result is treated as bytes until the NUL terminator
        // is found and then validated as UTF-8 below.
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if ok == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if let Ok(name) = std::str::from_utf8(&buf[..end]) {
                if !name.is_empty() {
                    return name.trim_end_matches(".local").to_string();
                }
            }
        }
        "Mac".to_string()
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNetworkAddress {
    pub interface_name: String,
    pub ip: String,
}

fn address_priority(ip: Ipv4Addr, interface_name: &str) -> (u8, u8, String, u32) {
    let [a, b, _, _] = ip.octets();
    let range_priority = if a == 192 && b == 168 {
        0
    } else if a == 172 && (16..=31).contains(&b) {
        1
    } else if a == 10 {
        2
    } else if a == 100 && (64..=127).contains(&b) {
        3
    } else if a == 198 && (18..=19).contains(&b) {
        // RFC 2544 benchmark ranges are commonly used by transparent proxies.
        9
    } else if ip.is_link_local() {
        10
    } else {
        4
    };
    let interface_priority = if interface_name.starts_with("utun")
        || interface_name.starts_with("tun")
        || interface_name.starts_with("tap")
        || interface_name.starts_with("wg")
    {
        1
    } else {
        0
    };
    (
        range_priority,
        interface_priority,
        interface_name.to_ascii_lowercase(),
        u32::from(ip),
    )
}

/// 返回所有可用于手机直连的本机 IPv4，并将常规 LAN 地址排在隧道/代理地址之前。
fn lan_addresses() -> Vec<RemoteNetworkAddress> {
    let mut seen = HashSet::new();
    let mut addresses = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| {
            let IpAddr::V4(ip) = interface.ip() else {
                return None;
            };
            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                return None;
            }
            if !seen.insert(ip) {
                return None;
            }
            Some((
                address_priority(ip, &interface.name),
                RemoteNetworkAddress {
                    interface_name: interface.name,
                    ip: ip.to_string(),
                },
            ))
        })
        .collect::<Vec<_>>();
    addresses.sort_by(|left, right| left.0.cmp(&right.0));
    addresses.into_iter().map(|(_, address)| address).collect()
}

fn selected_lan_ip(
    preferred_lan_ip: Option<&str>,
    addresses: &[RemoteNetworkAddress],
) -> Option<String> {
    preferred_lan_ip
        .and_then(|preferred| {
            addresses
                .iter()
                .find(|address| address.ip == preferred)
                .map(|address| address.ip.clone())
        })
        .or_else(|| addresses.first().map(|address| address.ip.clone()))
}

/// 生成配对码时要公布的端点快照。
///
/// `includes_lan` 不是仅用于展示：它表示 QR 中包含一个依赖本机 TCP
/// listener 的地址，因此在签发邀请码前必须确认 listener 已宽绑定。相反，
/// 只有 relay / 用户配置的公网地址时无需为了配对而扩大本地监听范围。
#[derive(Debug, PartialEq, Eq)]
struct PairingEndpoints {
    endpoints: Vec<String>,
    includes_lan: bool,
}

fn pairing_endpoints(
    port: u16,
    addresses: &[RemoteNetworkAddress],
    config: &RemoteConfig,
    host_id: &str,
) -> PairingEndpoints {
    let mut endpoints = Vec::new();
    let lan_ip = selected_lan_ip(config.preferred_lan_ip.as_deref(), addresses);
    let includes_lan = lan_ip.is_some();
    if let Some(ip) = lan_ip {
        endpoints.push(format!("ws://{ip}:{port}"));
    }
    endpoints.extend(config.public_endpoints.iter().cloned());
    if let Some(relay_url) = config
        .relay_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        endpoints.push(remote_protocol::client_connect_url(relay_url, host_id));
    }
    PairingEndpoints {
        includes_lan,
        endpoints,
    }
}

/// `hello` 用的实时主机身份:hostId / hostName / 当前可直连的候选地址。
///
/// 与 `remote_create_invite` 同一套取法(LAN 直连 → 自定义公网地址 → relay),
/// 区别是把**全部** LAN 地址都带上(preferred 排最前),让手机端能在换网段后
/// 刷新已保存主机的地址,而不是新建一条记录。服务未运行时端口取配置值。
pub(crate) async fn live_identity<R: Runtime>(app: &AppHandle<R>) -> serde_json::Value {
    let state = app.state::<RemoteState>();
    let running_port = state.running.lock().await.as_ref().map(|h| h.port);
    let addresses = lan_addresses();
    let (port, preferred_lan_ip, relay_url, public_endpoints) = {
        let cfg = state.config.lock();
        (
            running_port.unwrap_or(cfg.port),
            cfg.preferred_lan_ip.clone(),
            cfg.relay_url
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .map(str::to_string),
            cfg.public_endpoints.clone(),
        )
    };
    let mut endpoints = Vec::new();
    if let Some(ip) = selected_lan_ip(preferred_lan_ip.as_deref(), &addresses) {
        endpoints.push(format!("ws://{ip}:{port}"));
    }
    for address in &addresses {
        let endpoint = format!("ws://{}:{port}", address.ip);
        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }
    let lan_endpoints = endpoints.clone();
    endpoints.extend(public_endpoints);
    if let Some(relay) = relay_url {
        endpoints.push(remote_protocol::client_connect_url(
            &relay,
            &state.keys.host_id(),
        ));
    }
    json!({
        "hostId": state.keys.host_id(),
        "hostName": host_name(),
        "endpoints": endpoints,
        // 仅 LAN 的子集:手机端据此只替换旧的内网地址,保留用户手填的隧道地址
        "lanEndpoints": lan_endpoints,
    })
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
    let state = app.state::<RemoteState>();
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
    state: State<'_, RemoteState>,
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
    let state = app.state::<RemoteState>();
    let _lifecycle = state.lifecycle.lock().await;
    Ok(current_status(&app).await)
}

#[tauri::command]
pub async fn remote_server_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, RemoteState>,
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
    state: State<'_, RemoteState>,
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
    state: State<'_, RemoteState>,
    relay_url: Option<String>,
    relay_token: Option<String>,
    public_endpoints: Option<Vec<String>>,
) -> Result<RemoteStatus, String> {
    let _lifecycle = state.lifecycle.lock().await;
    fn valid_ws_url(url: &str) -> bool {
        url.starts_with("ws://") || url.starts_with("wss://")
    }
    let was_running;
    {
        let mut cfg = state.config.lock();
        let relay_url = relay_url.map(|u| u.trim().trim_end_matches('/').to_string());
        match relay_url {
            Some(url) if url.is_empty() => cfg.relay_url = None,
            Some(url) => {
                if !valid_ws_url(&url) {
                    return Err("Relay URL must start with ws:// or wss://".to_string());
                }
                cfg.relay_url = Some(url);
            }
            None => {}
        }
        if let Some(token) = relay_token {
            let token = token.trim().to_string();
            cfg.relay_token = (!token.is_empty()).then_some(token);
        }
        if let Some(endpoints) = public_endpoints {
            let mut cleaned = Vec::new();
            for endpoint in endpoints {
                let endpoint = endpoint.trim().trim_end_matches('/').to_string();
                if endpoint.is_empty() {
                    continue;
                }
                if !valid_ws_url(&endpoint) {
                    return Err(format!(
                        "Endpoint must start with ws:// or wss://: {endpoint}"
                    ));
                }
                if !cleaned.contains(&endpoint) {
                    cleaned.push(endpoint);
                }
            }
            cfg.public_endpoints = cleaned;
        }
        save_config(&cfg)?;
        was_running = cfg.enabled;
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
    let state = app.state::<RemoteState>();
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
    let state = app.state::<RemoteState>();
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
    let state = app.state::<RemoteState>();
    let mut running = state.running.lock().await;
    let Some(mut handle) = running.take() else {
        return Err("Remote server is not running".to_string());
    };

    let result = async {
        let config = state.config.lock().clone();
        let host_id = state.keys.host_id();
        let listener_port = handle.port;
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
            handle
                .ensure_listener_scope(app.clone(), ListenerScope::Network)
                .await?;
            if !handle.network_exposed() || handle.port != listener_port {
                return Err("Remote server could not verify its network listener".to_string());
            }
        }
        if !handle.is_listening() {
            return Err("Remote server is not running".to_string());
        }

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
        let required_scope = required_listener_scope(&state);
        if let Err(scope_error) = handle
            .ensure_listener_scope(app.clone(), required_scope)
            .await
        {
            let original_error = result.expect_err("checked invite creation failure");
            handle.shutdown_and_wait(app).await;
            return Err(format!(
                "{original_error}; additionally failed to restore the remote listener scope: {scope_error}"
            ));
        }
    }

    if handle.is_listening() {
        *running = Some(handle);
    } else {
        // Both wide bind and loopback recovery failed. Tear down the rest of
        // the server lifecycle before a later clean retry.
        handle.shutdown_and_wait(app).await;
    }
    result
}

#[cfg(test)]
mod address_tests {
    use super::{address_priority, selected_lan_ip, RemoteNetworkAddress};
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
}

#[cfg(test)]
mod network_exposure_tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use tauri::Manager;

    use super::auth::AuthOutcome;
    use super::{
        abort_unconfirmed_pairing, create_invite_for_addresses, ensure_started, ensure_stopped,
        revoke_device_for_test, RemoteNetworkAddress, RemoteState,
    };

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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle.state::<RemoteState>().config.lock().port = 0;
            let initial_port = ensure_started(&handle)
                .await
                .expect("start loopback server");

            {
                let state = handle.state::<RemoteState>();
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
                let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle.state::<RemoteState>().config.lock().port = 0;
            ensure_started(&handle)
                .await
                .expect("start loopback server");

            let error = create_invite_for_addresses(&handle, Vec::new())
                .await
                .expect_err("an invite without a LAN, relay, or public endpoint must fail");
            assert!(error.contains("No LAN address detected"));

            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle.state::<RemoteState>().config.lock().port = 0;
            let port = ensure_started(&handle)
                .await
                .expect("start loopback server");
            handle
                .state::<RemoteState>()
                .auth
                .lock()
                .set_invite_failure(Some("simulated CSPRNG unavailable".to_string()));

            let error = create_invite_for_addresses(&handle, vec![address("192.168.1.10")])
                .await
                .expect_err("injected token-generation failure must reach the caller");
            assert!(error.contains("simulated CSPRNG unavailable"));

            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            handle.state::<RemoteState>().config.lock().port = 0;
            ensure_started(&handle)
                .await
                .expect("start loopback server");

            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
            let device_id = pair_test_device(&state);
            state.config.lock().port = 0;
            ensure_started(&handle)
                .await
                .expect("start network listener for pending paired device");

            abort_unconfirmed_pairing(&handle, &device_id).await;

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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
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
        app.manage(RemoteState::new_in_memory());
        let handle = app.handle().clone();

        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
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
    state: State<'_, RemoteState>,
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
    let state = app.state::<RemoteState>();
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
    let state = app.state::<RemoteState>();
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
    if let Err(error) = reconcile_listener_scope_locked(app).await {
        eprintln!("[remote] server closed after revoke scope reconciliation failed: {error}");
        if state.audit_enabled {
            audit::log(
                "listener-closed-after-revoke",
                json!({ "deviceId": device_id, "error": error }),
            );
        }
    }
    if state.audit_enabled {
        audit::log("device-revoked", json!({ "deviceId": device_id }));
    }
    Ok(())
}

#[tauri::command]
pub fn remote_complete_task_request(
    state: State<'_, RemoteState>,
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
