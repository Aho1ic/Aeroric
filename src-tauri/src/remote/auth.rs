//! 远程设备注册表、一次性 invite 与认证限流。
//!
//! 安全模型:
//! - invite:桌面端生成的一次性配对令牌,TTL 10 分钟,配对成功即作废。
//! - device token:配对成功后签发的长期令牌,手机持有明文,磁盘只存 SHA-256。
//! - 注册表 `~/.aeroric/remote-devices.json` 走 atomic_write_private(0600)。
//! - 未认证连接按对端 IP 指数退避限流,防爆破。

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::{aeroric_dir, atomic_write_private};

const INVITE_TTL: Duration = Duration::from_secs(10 * 60);
/// 同时存活的 invite 上限,防止无限累积。
const MAX_PENDING_INVITES: usize = 8;
/// 触发退避前允许的连续失败次数。
const THROTTLE_FREE_FAILURES: u32 = 3;
const THROTTLE_MAX_DELAY: Duration = Duration::from_secs(300);

// ── Token 基础设施 ───────────────────────────────────────────────────────────

/// 32 字节 CSPRNG → base64url(无填充)。
pub fn generate_token() -> Result<String, String> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).map_err(|e| format!("CSPRNG unavailable: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── 设备注册表 ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceEntry {
    pub id: String,
    pub name: String,
    /// 长期 device token 的 SHA-256 hex;明文只在配对响应中出现一次。
    #[serde(rename = "tokenHash")]
    pub token_hash: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastSeenAt")]
    pub last_seen_at: i64,
}

fn devices_path() -> Result<std::path::PathBuf, String> {
    Ok(aeroric_dir()?.join("remote-devices.json"))
}

pub fn load_devices() -> Vec<DeviceEntry> {
    let Ok(path) = devices_path() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_devices(devices: &[DeviceEntry]) -> Result<(), String> {
    let path = devices_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(devices).map_err(|e| e.to_string())?;
    atomic_write_private(&path, &raw)
}

// ── 认证状态机 ───────────────────────────────────────────────────────────────

struct PendingInvite {
    expires_at: Instant,
}

#[derive(Default)]
struct ThrottleEntry {
    failures: u32,
    blocked_until: Option<Instant>,
}

/// 由 RemoteState 持有(parking_lot Mutex 包裹),所有方法都要求短临界区。
pub struct AuthStore {
    devices: Vec<DeviceEntry>,
    /// key = invite token 的 SHA-256 hex。
    invites: HashMap<String, PendingInvite>,
    throttle: HashMap<IpAddr, ThrottleEntry>,
    /// false 仅用于单测,避免污染真实注册表文件。
    persist: bool,
    #[cfg(test)]
    persist_failure: Option<String>,
}

pub enum AuthOutcome {
    /// invite 配对成功:新设备已入表并落盘,携带一次性下发的明文 token。
    Paired {
        device_id: String,
        device_name: String,
        device_token: String,
    },
    /// device token 验证成功。
    Authenticated { device_id: String },
}

impl AuthStore {
    pub fn load() -> Self {
        Self {
            devices: load_devices(),
            invites: HashMap::new(),
            throttle: HashMap::new(),
            persist: true,
            #[cfg(test)]
            persist_failure: None,
        }
    }

    /// 纯内存实例,集成测试用:绝不读写真实注册表文件。
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self {
            devices: Vec::new(),
            invites: HashMap::new(),
            throttle: HashMap::new(),
            persist: false,
            persist_failure: None,
        }
    }

    fn persist_device_list(&self, devices: &[DeviceEntry]) -> Result<(), String> {
        #[cfg(test)]
        if let Some(error) = &self.persist_failure {
            return Err(error.clone());
        }
        if !self.persist {
            return Ok(());
        }
        save_devices(devices)
    }

    fn persist_devices(&self) -> Result<(), String> {
        self.persist_device_list(&self.devices)
    }

    pub fn devices(&self) -> &[DeviceEntry] {
        &self.devices
    }

    /// 生成一次性 invite,返回明文 token(嵌入 QR)。
    pub fn create_invite(&mut self) -> Result<String, String> {
        self.prune_invites();
        if self.invites.len() >= MAX_PENDING_INVITES {
            // 挤掉最早过期的一条,保证「重新生成二维码」永远可用。
            if let Some(oldest) = self
                .invites
                .iter()
                .min_by_key(|(_, inv)| inv.expires_at)
                .map(|(k, _)| k.clone())
            {
                self.invites.remove(&oldest);
            }
        }
        let token = generate_token()?;
        self.invites.insert(
            hash_token(&token),
            PendingInvite {
                expires_at: Instant::now() + INVITE_TTL,
            },
        );
        Ok(token)
    }

    fn prune_invites(&mut self) {
        let now = Instant::now();
        self.invites.retain(|_, inv| inv.expires_at > now);
    }

    /// 认证入口。`invite`/`device_token` 二选一;成功前先过限流闸门。
    pub fn authenticate(
        &mut self,
        peer: IpAddr,
        invite: Option<&str>,
        device_token: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<AuthOutcome, String> {
        if let Some(wait) = self.throttle_wait(peer) {
            return Err(format!(
                "Too many failed attempts; retry in {}s",
                wait.as_secs().max(1)
            ));
        }

        let outcome = self.authenticate_inner(invite, device_token, device_name);
        match &outcome {
            Ok(_) => {
                self.throttle.remove(&peer);
            }
            Err(_) => self.record_failure(peer),
        }
        outcome
    }

    fn authenticate_inner(
        &mut self,
        invite: Option<&str>,
        device_token: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<AuthOutcome, String> {
        if let Some(token) = device_token {
            let hash = hash_token(token);
            let now = now_ms();
            if let Some(entry) = self.devices.iter_mut().find(|d| d.token_hash == hash) {
                entry.last_seen_at = now;
                let device_id = entry.id.clone();
                let _ = self.persist_devices();
                return Ok(AuthOutcome::Authenticated { device_id });
            }
            return Err("Unknown device token".to_string());
        }

        if let Some(invite_token) = invite {
            self.prune_invites();
            let hash = hash_token(invite_token);
            if self.invites.remove(&hash).is_none() {
                return Err("Invalid or expired invite".to_string());
            }
            let device_token = generate_token()?;
            let entry = DeviceEntry {
                id: uuid::Uuid::new_v4().to_string(),
                name: device_name
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or("Unnamed device")
                    .chars()
                    .take(64)
                    .collect(),
                token_hash: hash_token(&device_token),
                created_at: now_ms(),
                last_seen_at: now_ms(),
            };
            let (device_id, device_name) = (entry.id.clone(), entry.name.clone());
            self.devices.push(entry);
            self.persist_devices()?;
            return Ok(AuthOutcome::Paired {
                device_id,
                device_name,
                device_token,
            });
        }

        Err("auth requires invite or deviceToken".to_string())
    }

    /// 撤销设备;返回是否存在。调用方负责断开其在线连接。
    pub fn revoke(&mut self, device_id: &str) -> Result<bool, String> {
        let mut next = self.devices.clone();
        let before = next.len();
        next.retain(|d| d.id != device_id);
        if next.len() == before {
            return Ok(false);
        }
        // 先原子写入下一状态，再切换内存视图。写盘失败时 token 在内存和磁盘
        // 都仍有效，调用方会收到错误且不会误报“已撤销”。
        self.persist_device_list(&next)?;
        self.devices = next;
        Ok(true)
    }

    // ── 限流 ────────────────────────────────────────────────────────────────

    fn throttle_wait(&mut self, peer: IpAddr) -> Option<Duration> {
        let entry = self.throttle.get(&peer)?;
        let until = entry.blocked_until?;
        let now = Instant::now();
        (until > now).then(|| until - now)
    }

    fn record_failure(&mut self, peer: IpAddr) {
        // 防状态膨胀:超过 1024 个 IP 时清掉已解封的旧条目。
        if self.throttle.len() > 1024 {
            let now = Instant::now();
            self.throttle
                .retain(|_, e| e.blocked_until.is_some_and(|t| t > now));
        }
        let entry = self.throttle.entry(peer).or_default();
        entry.failures += 1;
        if entry.failures > THROTTLE_FREE_FAILURES {
            let exp = (entry.failures - THROTTLE_FREE_FAILURES).min(16);
            let delay = Duration::from_secs(2u64.saturating_pow(exp)).min(THROTTLE_MAX_DELAY);
            entry.blocked_until = Some(Instant::now() + delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn peer() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))
    }

    fn empty_store() -> AuthStore {
        AuthStore {
            devices: Vec::new(),
            invites: HashMap::new(),
            throttle: HashMap::new(),
            persist: false,
            persist_failure: None,
        }
    }

    #[test]
    fn token_roundtrip_is_url_safe() {
        let token = generate_token().unwrap();
        assert!(!token.contains('+') && !token.contains('/') && !token.contains('='));
        assert_eq!(hash_token(&token).len(), 64);
    }

    #[test]
    fn invite_is_single_use() {
        let mut store = empty_store();
        let invite = store.create_invite().unwrap();
        let first = store.authenticate_inner(Some(&invite), None, Some("Phone"));
        assert!(matches!(first, Ok(AuthOutcome::Paired { .. })));
        let second = store.authenticate_inner(Some(&invite), None, Some("Phone"));
        assert!(second.is_err());
    }

    #[test]
    fn bad_invite_rejected() {
        let mut store = empty_store();
        store.create_invite().unwrap();
        assert!(store
            .authenticate_inner(Some("bogus"), None, Some("Phone"))
            .is_err());
    }

    #[test]
    fn device_token_authenticates_after_pairing() {
        let mut store = empty_store();
        let invite = store.create_invite().unwrap();
        let Ok(AuthOutcome::Paired { device_token, .. }) =
            store.authenticate_inner(Some(&invite), None, Some("Phone"))
        else {
            panic!("pairing failed");
        };
        let auth = store.authenticate_inner(None, Some(&device_token), None);
        assert!(matches!(auth, Ok(AuthOutcome::Authenticated { .. })));
        assert!(store.authenticate_inner(None, Some("wrong"), None).is_err());
    }

    #[test]
    fn revoke_removes_device() {
        let mut store = empty_store();
        let invite = store.create_invite().unwrap();
        let Ok(AuthOutcome::Paired {
            device_id,
            device_token,
            ..
        }) = store.authenticate_inner(Some(&invite), None, Some("Phone"))
        else {
            panic!("pairing failed");
        };
        assert!(store.revoke(&device_id).unwrap());
        assert!(store
            .authenticate_inner(None, Some(&device_token), None)
            .is_err());
        assert!(!store.revoke(&device_id).unwrap());
    }

    #[test]
    fn revoke_persistence_failure_keeps_in_memory_device() {
        let mut store = empty_store();
        let invite = store.create_invite().unwrap();
        let Ok(AuthOutcome::Paired {
            device_id,
            device_token,
            ..
        }) = store.authenticate_inner(Some(&invite), None, Some("Phone"))
        else {
            panic!("pairing failed");
        };
        store.persist_failure = Some("disk full".to_string());

        assert_eq!(store.revoke(&device_id), Err("disk full".to_string()));
        assert!(store
            .authenticate_inner(None, Some(&device_token), None)
            .is_ok());
    }

    #[test]
    fn throttle_blocks_after_repeated_failures() {
        let mut store = empty_store();
        for _ in 0..THROTTLE_FREE_FAILURES + 1 {
            let _ = store.authenticate(peer(), Some("bogus"), None, None);
        }
        let blocked = store.authenticate(peer(), Some("bogus"), None, None);
        assert!(blocked.is_err_and(|e| e.contains("Too many failed attempts")));
    }

    #[test]
    fn success_resets_throttle() {
        let mut store = empty_store();
        for _ in 0..THROTTLE_FREE_FAILURES {
            let _ = store.authenticate(peer(), Some("bogus"), None, None);
        }
        let invite = store.create_invite().unwrap();
        let ok = store.authenticate(peer(), Some(&invite), None, Some("Phone"));
        assert!(ok.is_ok());
        assert!(!store.throttle.contains_key(&peer()));
    }
}
