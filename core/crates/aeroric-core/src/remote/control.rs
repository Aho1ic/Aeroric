//! 远程控制面 helper（P8-03 刀五自宿主 mod.rs 命令体上收）。
//!
//! status / config 更新 / LAN 选择 / 邀请签发 / 设备列表与撤销——全部是
//! `&RemoteState`＋handle 上的纯状态操作，宿主（src-tauri Tauri 命令）与
//! 无头（aeroric-host dispatcher 的 `remote.*` 方法）共用同一实现。

use serde_json::{json, Value};

use crate::remote::config::{normalize_public_endpoint, normalize_relay_url, save_config};
use crate::remote::identity::{host_name, pairing_endpoints, selected_lan_ip};
use crate::remote::protocol;
use crate::remote::services::RemoteHostHandle;
use crate::remote::state::{reconcile_listener_scope_locked, RemoteState};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// 设置页状态卡（原 `RemoteStatus`，序列化形状 camelCase 不变）。
pub async fn current_status(state: &RemoteState) -> Value {
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
    let addresses = crate::remote::identity::lan_addresses();
    let lan_ip = selected_lan_ip(preferred_lan_ip.as_deref(), &addresses);
    let lan_addresses: Vec<Value> = addresses
        .iter()
        .map(|address| json!({ "interfaceName": address.interface_name, "ip": address.ip }))
        .collect();
    json!({
        "enabled": enabled,
        "running": running_port.is_some(),
        "networkExposed": network_exposed,
        "port": running_port.unwrap_or(port),
        "lanIp": lan_ip,
        "lanAddresses": lan_addresses,
        "onlineCount": state.clients.online_count(),
        "relayUrl": relay_url,
        "relayToken": relay_token,
        "publicEndpoints": public_endpoints,
        "relayState": state.relay_state.lock().clone(),
    })
}

/// `remote.updateConfig`：校验 → 落盘 → 服务在运行时重启以生效。
pub async fn update_config(
    state: &std::sync::Arc<RemoteState>,
    handle: &std::sync::Arc<dyn RemoteHostHandle>,
    relay_url: Option<String>,
    relay_token: Option<String>,
    public_endpoints: Option<Vec<String>>,
) -> Result<Value, String> {
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
    if was_running {
        crate::remote::state::ensure_stopped(state).await;
        crate::remote::state::ensure_started(state, handle).await?;
    }
    Ok(current_status(state).await)
}

/// `remote.selectLanIp`：地址仍在才落盘。
pub async fn select_lan_ip(state: &RemoteState, lan_ip: String) -> Result<Value, String> {
    let _lifecycle = state.lifecycle.lock().await;
    let lan_ip = lan_ip.trim().to_string();
    let addresses = crate::remote::identity::lan_addresses();
    if !addresses.iter().any(|address| address.ip == lan_ip) {
        return Err(format!("Local IP is no longer available: {lan_ip}"));
    }
    {
        let mut cfg = state.config.lock();
        cfg.preferred_lan_ip = Some(lan_ip.clone());
        save_config(&cfg)?;
    }
    Ok(current_status(state).await)
}

/// `remote.server.start`：端口校验＋启用＋拉起（宿主 wrapper 负责事件桥）。
pub async fn server_start(
    state: &std::sync::Arc<RemoteState>,
    _handle: &std::sync::Arc<dyn RemoteHostHandle>,
    start: impl AsyncFnOnce() -> Result<(), String>,
    port: Option<u16>,
) -> Result<Value, String> {
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
    start().await?;
    Ok(current_status(state).await)
}

/// `remote.server.stop`：停用并停机。
pub async fn server_stop(
    state: &std::sync::Arc<RemoteState>,
    stop: impl AsyncFnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let _lifecycle = state.lifecycle.lock().await;
    {
        let mut cfg = state.config.lock();
        cfg.enabled = false;
        save_config(&cfg)?;
    }
    stop().await?;
    Ok(current_status(state).await)
}

/// 生成一次性 invite 并组装配对深链（原 `create_invite_for_addresses_locked`）。
/// Requires `RemoteState::lifecycle` 已被调用方持有。
pub async fn create_invite_for_addresses(
    state: &std::sync::Arc<RemoteState>,
    handle: &std::sync::Arc<dyn RemoteHostHandle>,
    addresses: &[crate::remote::identity::RemoteNetworkAddress],
) -> Result<Value, String> {
    let mut running = state.running.lock().await;
    let Some(mut server) = running.take() else {
        return Err("Remote server is not running".to_string());
    };

    let result = async {
        let config = state.config.lock().clone();
        let host_id = state.keys.host_id();
        let listener_port = server.port;
        let endpoints = pairing_endpoints(listener_port, addresses, &config, &host_id);
        if endpoints.endpoints.is_empty() {
            return Err("No LAN address detected and no public endpoints configured".to_string());
        }

        if endpoints.includes_lan {
            server
                .ensure_listener_scope(state, handle, crate::remote::state::ListenerScope::Network)
                .await?;
            if !server.network_exposed() || server.port != listener_port {
                return Err("Remote server could not verify its network listener".to_string());
            }
        }
        if !server.is_listening() {
            return Err("Remote server is not running".to_string());
        }

        #[allow(clippy::expect_used, reason = "上文已对 endpoints.is_empty() 早返回")]
        let primary = endpoints
            .endpoints
            .first()
            .cloned()
            .expect("checked for a non-empty pairing endpoint list");
        let invite = state.auth.lock().create_invite()?;
        handle.audit_log("invite-created", json!({}));
        let offer = json!({
            "v": protocol::PROTOCOL_VERSION,
            "endpoints": endpoints.endpoints,
            "invite": invite,
            "hostName": host_name(),
            "hostId": host_id,
            "publicKey": state.keys.public_b64(),
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

    if result.is_err() {
        let required_scope = crate::remote::state::required_listener_scope(state);
        if let Err(scope_error) = server
            .ensure_listener_scope(state, handle, required_scope)
            .await
        {
            #[allow(clippy::expect_used, reason = "上一行 if result.is_err() 已保证")]
            let original_error = result.expect_err("checked invite creation failure");
            server.shutdown_and_wait(state).await;
            return Err(format!(
                "{original_error}; additionally failed to restore the remote listener scope: {scope_error}"
            ));
        }
    }

    if server.is_listening() {
        *running = Some(server);
    } else {
        server.shutdown_and_wait(state).await;
    }
    result
}

/// `remote.listDevices`：设备表＋在线标记。
pub fn list_devices(state: &RemoteState) -> Result<Value, String> {
    let online = state.clients.online_device_ids();
    let auth = state.auth.lock();
    let devices: Vec<Value> = auth
        .devices()
        .iter()
        .map(|device| {
            json!({
                "id": device.id,
                "name": device.name,
                "createdAt": device.created_at,
                "lastSeenAt": device.last_seen_at,
                "online": online.contains(&device.id),
            })
        })
        .collect();
    Ok(json!(devices))
}

/// `remote.revokeDevice`：撤销＋客户端断开＋listener 收窄（调用方持有 lifecycle）。
pub async fn revoke_device(
    state: &std::sync::Arc<RemoteState>,
    handle: &std::sync::Arc<dyn RemoteHostHandle>,
    device_id: &str,
) -> Result<(), String> {
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
    if let Err(error) = reconcile_listener_scope_locked(state, handle).await {
        eprintln!("[remote] server closed after revoke scope reconciliation failed: {error}");
        if state.audit_enabled {
            handle.audit_log(
                "listener-closed-after-revoke",
                json!({ "deviceId": device_id, "error": error }),
            );
        }
    }
    // 撤销必须连带停掉推送给这台设备 —— 否则被撤销的设备仍然会收到「任务完成」
    // 之类的内容提醒(Q7/§D AP6「撤销后停止新增推送」)。core 只负责**说清楚**
    // 哪台设备失去了权限,真正的解绑(本地绑定表 + 自托管发送端)归宿主。
    match handle.device_revoked(device_id).await {
        Ok(detail) => {
            if state.audit_enabled {
                handle.audit_log(
                    "device-push-revoked",
                    json!({ "deviceId": device_id, "detail": detail }),
                );
            }
        }
        Err(error) => {
            // 推送没停成不阻断撤销(权限已经收回,连接已断开),但必须留痕:
            // 「撤销了但还会收到通知」是用户有权知道的状态。
            eprintln!("[remote] could not stop push for the revoked device {device_id}: {error}");
            if state.audit_enabled {
                handle.audit_log(
                    "device-push-revoke-failed",
                    json!({ "deviceId": device_id, "error": error }),
                );
            }
        }
    }
    if state.audit_enabled {
        handle.audit_log("device-revoked", json!({ "deviceId": device_id }));
    }
    Ok(())
}
