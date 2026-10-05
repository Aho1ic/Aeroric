//! 远程服务的配置持久化与端点校验（P8-03 刀三自宿主 `remote/mod.rs` 上收）。
//!
//! `remote-config.json` 不进 settings.json（避免搅动大结构）；落盘原语走
//! core `settings`（`aeroric_dir` 读序与宿主同：DATA_DIR → `AERORIC_HOME` →
//! 真实家）。端点校验逐字保持宿主语义：relay 必须显式带 token 且非本机
//! 地址走 wss；用户手填的直连端点允许 ws:// 内网地址（E2EE 载荷不依赖
//! 传输层加密）。

use serde::{Deserialize, Serialize};

use crate::settings::{
    aeroric_dir, atomic_write_private, ensure_private_dir, ensure_private_file_permissions,
};

pub const DEFAULT_PORT: u16 = 6790;

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

pub fn config_path() -> Result<std::path::PathBuf, String> {
    Ok(aeroric_dir()?.join("remote-config.json"))
}

pub fn normalize_ws_endpoint(
    value: &str,
    label: &str,
    allow_non_loopback_ws: bool,
) -> Result<String, String> {
    let value = value.trim();
    let url =
        url::Url::parse(value).map_err(|_| format!("{label} must be a valid WebSocket URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{label} must not contain embedded credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{label} must not contain a query or fragment"));
    }
    let host = url
        .host()
        .ok_or_else(|| format!("{label} must include a host"))?;
    let loopback = match host {
        url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    };
    match url.scheme() {
        "wss" => {}
        "ws" if allow_non_loopback_ws || loopback => {}
        "ws" => {
            return Err(format!(
                "{label} must use wss:// unless it connects to localhost"
            ));
        }
        _ => return Err(format!("{label} must use ws:// or wss://")),
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn normalize_relay_url(value: &str) -> Result<String, String> {
    normalize_ws_endpoint(value, "Relay URL", false)
}

pub fn normalize_public_endpoint(value: &str) -> Result<String, String> {
    // Direct endpoints carry the application's E2EE protocol and may
    // intentionally be private ws:// LAN/Tailscale addresses.
    normalize_ws_endpoint(value, "Endpoint", true)
}

pub fn load_config() -> RemoteConfig {
    let Ok(path) = config_path() else {
        return RemoteConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return RemoteConfig::default();
    };
    let _ = ensure_private_file_permissions(&path);
    let mut config = serde_json::from_str::<RemoteConfig>(&raw).unwrap_or_default();
    let relay_valid = config
        .relay_url
        .as_deref()
        .map(normalize_relay_url)
        .transpose()
        .ok()
        .flatten();
    let relay_token = config
        .relay_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    if config.relay_url.is_some() && (relay_valid.is_none() || relay_token.is_none()) {
        eprintln!("[remote] ignoring an insecure relay URL or a relay without a token");
        config.relay_url = None;
        config.relay_token = None;
    } else {
        config.relay_url = relay_valid;
        config.relay_token = relay_token;
    }
    config.public_endpoints = config
        .public_endpoints
        .into_iter()
        .filter_map(|endpoint| normalize_public_endpoint(&endpoint).ok())
        .collect();
    config
}

pub fn save_config(config: &RemoteConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    atomic_write_private(&path, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_requires_wss_off_loopback_and_a_token() {
        // 明文 ws 只允许本机回环；token 缺失由 load_config 丢弃。
        assert!(normalize_relay_url("ws://localhost:8080").is_ok());
        assert!(normalize_relay_url("ws://192.168.1.10:8080").is_err());
        assert!(normalize_relay_url("wss://relay.example.com").is_ok());
        assert!(normalize_relay_url("https://relay.example.com").is_err());
    }

    #[test]
    fn direct_endpoints_may_use_private_ws_but_reject_credentials() {
        assert!(normalize_public_endpoint("ws://192.168.1.10:6790").is_ok());
        assert!(
            normalize_public_endpoint("ws://user:pass@192.168.1.10:6790").is_err(),
            "端点不得内嵌凭据"
        );
    }

    #[test]
    fn default_config_is_disabled_on_the_default_port() {
        let config = RemoteConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.port, DEFAULT_PORT);
        assert!(config.public_endpoints.is_empty());
    }
}
