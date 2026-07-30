//! 手机远程连接(Mobile Remote)模块入口。
//!
//! 架构见 docs/mobile-remote-vibe-coding-plan-2026-07-28.md。M0 范围:
//! 局域网 WS 服务 + QR 配对 + 设备注册表 + 窄面 RPC + 事件桥。
//! 服务默认关闭,由设置页开关控制;开关与端口持久化在
//! `~/.aeroric/remote-config.json`(不进 settings.json,避免搅动大结构)。

mod agent_keymap;
mod audit;
mod auth;
mod crypto;
mod event_log;
mod events_bridge;
mod files_rpc;
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

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::storage::{aeroric_dir, atomic_write};
use auth::AuthStore;
use event_log::EventLog;
use server::{ClientRegistry, ServerHandle};
use tasks_rpc::{ApprovalRegistry, TaskRequestBroker};
use terminal_streams::TerminalLeaseRegistry;

pub const DEFAULT_PORT: u16 = 6790;

// ── 配置持久化 ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub port: u16,
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
    let Ok(raw) = std::fs::read_to_string(path) else {
        return RemoteConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_config(config: &RemoteConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    atomic_write(&path, &raw)
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
        }
    }
}

/// 应用启动时调用(lib.rs setup):按持久化配置决定是否自动拉起服务。
pub fn init<R: Runtime>(app: AppHandle<R>) {
    let enabled = app.state::<RemoteState>().config.lock().enabled;
    if enabled {
        tauri::async_runtime::spawn(async move {
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
    let handle = server::start(app.clone(), port).await?;
    let bound = handle.port;
    *running = Some(handle);
    Ok(bound)
}

async fn ensure_stopped<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<RemoteState>();
    let mut running = state.running.lock().await;
    if let Some(handle) = running.take() {
        handle.shutdown(app);
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

/// 主出口网卡的局域网 IP。UDP connect 不发包,仅让内核选路由。
fn lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

// ── Tauri commands(桌面设置页专用) ─────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub lan_ip: Option<String>,
    pub online_count: usize,
    pub relay_url: Option<String>,
    pub relay_token: Option<String>,
    pub public_endpoints: Vec<String>,
    /// off | connecting | online | error:<msg>
    pub relay_state: String,
}

async fn current_status<R: Runtime>(app: &AppHandle<R>) -> RemoteStatus {
    let state = app.state::<RemoteState>();
    let (enabled, port, relay_url, relay_token, public_endpoints) = {
        let cfg = state.config.lock();
        (
            cfg.enabled,
            cfg.port,
            cfg.relay_url.clone(),
            cfg.relay_token.clone(),
            cfg.public_endpoints.clone(),
        )
    };
    let running_port = state.running.lock().await.as_ref().map(|h| h.port);
    let relay_state = state.relay_state.lock().clone();
    RemoteStatus {
        enabled,
        running: running_port.is_some(),
        port: running_port.unwrap_or(port),
        lan_ip: lan_ip(),
        online_count: state.clients.online_count(),
        relay_url,
        relay_token,
        public_endpoints,
        relay_state,
    }
}

#[tauri::command]
pub async fn remote_server_status<R: Runtime>(app: AppHandle<R>) -> Result<RemoteStatus, String> {
    Ok(current_status(&app).await)
}

#[tauri::command]
pub async fn remote_server_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, RemoteState>,
    port: Option<u16>,
) -> Result<RemoteStatus, String> {
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
    let running_port = state.running.lock().await.as_ref().map(|h| h.port);
    let Some(port) = running_port else {
        return Err("Remote server is not running".to_string());
    };
    // 候选地址:LAN 直连 → 自定义公网地址 → relay(手机端按序/竞速试连)
    let mut endpoints = Vec::new();
    if let Some(ip) = lan_ip() {
        endpoints.push(format!("ws://{ip}:{port}"));
    }
    let (relay_endpoint, extra_endpoints) = {
        let cfg = state.config.lock();
        let relay = cfg
            .relay_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .map(|u| remote_protocol::client_connect_url(u, &state.keys.host_id()));
        (relay, cfg.public_endpoints.clone())
    };
    endpoints.extend(extra_endpoints);
    if let Some(relay) = relay_endpoint {
        endpoints.push(relay);
    }
    if endpoints.is_empty() {
        return Err("No LAN address detected and no public endpoints configured".to_string());
    }
    let primary = endpoints[0].clone();
    let invite = state.auth.lock().create_invite()?;
    audit::log("invite-created", json!({}));
    let offer = json!({
        "v": protocol::PROTOCOL_VERSION,
        "endpoints": endpoints,
        "invite": invite,
        "hostName": host_name(),
        "hostId": state.keys.host_id(),
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
pub fn remote_revoke_device(
    state: State<'_, RemoteState>,
    device_id: String,
) -> Result<(), String> {
    let existed = state.auth.lock().revoke(&device_id)?;
    if !existed {
        return Err("Device not found".to_string());
    }
    audit::log("device-revoked", json!({ "deviceId": device_id }));
    state.clients.disconnect_device(&device_id);
    Ok(())
}

#[tauri::command]
pub fn remote_complete_task_request(
    state: State<'_, RemoteState>,
    request_id: String,
    accepted: bool,
    task_id: Option<String>,
    error: Option<String>,
) -> Result<(), String> {
    let result = if accepted {
        Ok(json!({ "accepted": true, "taskId": task_id }))
    } else {
        Err(error.unwrap_or_else(|| "Desktop rejected the task request".to_string()))
    };
    state.task_requests.resolve(&request_id, result)
}
