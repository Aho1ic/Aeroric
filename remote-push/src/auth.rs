//! 发送端认证:per-user 凭据、HMAC-SHA256 请求签名、nonce 重放防护与限流。
//!
//! 为什么不引入账号体系(Q10「优先复用设备信任关系,不默认增加账号」):
//! 发送端是**用户自托管**的,「用户」就是这份部署的所有者派生出的一台台设备。
//! 宿主在配对后向发送端 `/v1/enroll` 领一份 per-user 凭据(id + 32 字节随机
//! 密钥),存进系统钥匙串;此后每次请求用 HMAC-SHA256 签名该请求的
//! 方法/路径/时间戳/nonce/完整体(格式唯一定义在 `remote_protocol::signature_string`)。
//!
//! 三条硬约束:
//! 1. **密钥不在客户端之间流动**。凭据只在宿主钥匙串与发送端各存一份;
//!    手机上跑的是 E2EE,`push.register` 只上报 APNs device token。
//! 2. **失败文案不给预言机**。未知凭据与签名不符返回同一个 `Unauthorized`
//!    (与 core `remote::auth` 的既有口径一致)。
//! 3. **重放必须被拒**。nonce 只在时间窗内保留,超窗即淘汰(表有界),同一
//!    nonce 第二次出现直接 `Replay`。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 32 字节凭据密钥的十六进制长度(64 字符);测试断言明文长度用。
#[cfg(test)]
const SECRET_HEX_LEN: usize = 64;
/// nonce 表上限。到顶就按时间淘汰最旧的一批,不让攻击者撑爆内存。
const MAX_TRACKED_NONCES: usize = 4096;
/// 单凭据限流表的取值上限。
const MAX_RATE_ENTRIES: usize = 1024;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
struct UserCredential {
    id: String,
    /// 密钥的 SHA-256 摘要(hex)。明文只在 enroll 响应里出现一次——与
    /// `remote-devices.json` 存 device token 摘要同形,不存明文。
    secret_hash: String,
}

impl UserCredential {
    fn hash(secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// `credentials.json` 的磁盘形状。`version` 让将来的迁移有落脚点。
#[derive(Serialize, Deserialize, Default)]
struct CredentialFile {
    version: u32,
    users: Vec<UserCredential>,
}

/// per-user 凭据表。可由 `PUSH_CREDENTIALS_FILE` 落盘;`None` = 内存态。
pub struct CredentialStore {
    users: BTreeMap<String, UserCredential>,
    path: Option<std::path::PathBuf>,
}

impl CredentialStore {
    pub fn in_memory() -> Self {
        Self {
            users: BTreeMap::new(),
            path: None,
        }
    }

    /// 从文件加载;文件不存在视为空表(首次部署的正常状态)。
    pub fn load(path: Option<std::path::PathBuf>) -> Result<Self, String> {
        let mut store = Self {
            users: BTreeMap::new(),
            path,
        };
        if let Some(file) = &store.path {
            if file.exists() {
                let raw = std::fs::read_to_string(file)
                    .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                let parsed: CredentialFile = serde_json::from_str(&raw)
                    .map_err(|e| format!("cannot parse {}: {e}", file.display()))?;
                for user in parsed.users {
                    store.users.insert(user.id.clone(), user);
                }
            }
        }
        Ok(store)
    }

    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// 铸造一份新凭据;明文只在这里返回一次。调用方必须经管理口令保护。
    pub fn enroll(&mut self, user_hint: Option<&str>) -> Result<(String, String), String> {
        let secret = random_hex(32)?;
        let id = match user_hint.map(str::trim).filter(|hint| !hint.is_empty()) {
            Some(hint) => sanitize_user(hint)?,
            None => format!("u_{}", random_hex(8)?),
        };
        self.users.insert(
            id.clone(),
            UserCredential {
                id: id.clone(),
                secret_hash: UserCredential::hash(&secret),
            },
        );
        self.persist()?;
        Ok((id, secret))
    }

    /// 删除一份凭据(部署方撤销某台宿主时用)。
    pub fn revoke_user(&mut self, user: &str) -> Result<bool, String> {
        let removed = self.users.remove(user).is_some();
        if removed {
            self.persist()?;
        }
        Ok(removed)
    }

    /// 用调用方给的明文铸造一把**指定 id** 的凭据(测试与本地联调用:
    /// 让断言能自己算签名)。生产路径只走 [`CredentialStore::enroll`]。
    pub fn seed(&mut self, user: &str, secret: &str) -> Result<(), String> {
        let id = sanitize_user(user)?;
        self.users.insert(
            id.clone(),
            UserCredential {
                id,
                secret_hash: UserCredential::hash(secret),
            },
        );
        self.persist()
    }

    /// 校验签名。**顺序固定**:先查凭据(未知即 `Unauthorized`),再比对 HMAC;
    /// 两条失败路径返回同一个错误,不给「这个 user 存在吗」的预言机。
    pub fn verify(&self, user: &str, signature_hex: &str, message: &str) -> bool {
        let Some(credential) = self.users.get(user) else {
            return false;
        };
        let Some(expected) = hmac_hex(&credential.secret_hash, message) else {
            return false;
        };
        constant_time_eq(expected.as_bytes(), signature_hex.as_bytes())
    }

    fn persist(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = CredentialFile {
            version: 1,
            users: self.users.values().cloned().collect(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        write_private(path, &raw)
    }
}

/// 用凭据摘要作 HMAC 密钥,而不是明文密钥——落盘与内存里都只有摘要。
fn hmac_hex(secret_hash: &str, message: &str) -> Option<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_hash.as_bytes()).ok()?;
    mac.update(message.as_bytes());
    Some(hex::encode(mac.finalize().into_bytes()))
}

/// 客户端侧同款签名:凭据明文 → 摘要 → HMAC。宿主与发送端共用这段逻辑的
/// **形状**(core 侧另有一份注释指明同源);格式串本身来自
/// `remote_protocol::signature_string`,不在这里重写。
pub fn sign(secret: &str, message: &str) -> String {
    // 落盘存的是摘要,所以签名密钥也必须是摘要:两侧对同一 secret 得到同值。
    let digest = {
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        hex::encode(hasher.finalize())
    };
    hmac_hex(&digest, message).unwrap_or_default()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.is_empty() || left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |acc, (l, r)| acc | (l ^ r))
        == 0
}

fn sanitize_user(hint: &str) -> Result<String, String> {
    let cleaned: String = hint
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        return Err("user id becomes empty after sanitizing".to_string());
    }
    Ok(cleaned)
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| format!("CSPRNG unavailable: {e}"))?;
    Ok(hex::encode(buf))
}

/// 私密文件写入:0600 + 临时文件 + rename(与 core `atomic_write_private` 同口径)。
pub(crate) fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temp, contents).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot chmod {}: {e}", temp.display()))?;
    }
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("cannot rename into {}: {e}", path.display())
    })
}

/// nonce 重放防护。**有界**:只保留窗口内的 nonce,超窗与超量都淘汰。
///
/// 为什么要按时间淘汰而不是「永久记账」:没有账号体系也就没有可靠的全局时钟
/// 与清理周期,永久记账会被一个不断换 nonce 的调用方撑爆;窗口外的重放本来
/// 也会被签名时间戳判死,所以记账窗口与签名窗口取同一个值。
pub struct ReplayGuard {
    seen: Mutex<HashMap<String, Instant>>,
    window: Duration,
}

impl ReplayGuard {
    pub fn new(window: Duration) -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
            window,
        }
    }

    /// `true` = 首次见到(放行);`false` = 重放(拒绝)。
    pub fn observe(&self, nonce: &str) -> bool {
        let now = Instant::now();
        let mut seen = self.seen.lock();
        seen.retain(|_, at| now.duration_since(*at) < self.window);
        if seen.len() >= MAX_TRACKED_NONCES {
            // 到顶:淘汰最旧的一半。窗口内的记账宁可少留几条(重放仍会被
            // 时间戳判死),也不能让内存无界增长。
            let mut ages: Vec<Instant> = seen.values().copied().collect();
            ages.sort_unstable();
            let cutoff = ages[ages.len() / 2];
            seen.retain(|_, at| *at >= cutoff);
        }
        if seen.contains_key(nonce) {
            return false;
        }
        seen.insert(nonce.to_string(), now);
        true
    }

    pub fn tracked(&self) -> usize {
        self.seen.lock().len()
    }
}

/// 每凭据每分钟的请求上限。滑动窗口的简化实现:固定窗口计数 + 窗口翻转。
pub struct RateLimiter {
    /// 原子而不是普通 `u32`:集成测试要在启动服务**之后**压低上限以验证 429 与
    /// `retry-after`(不必打满 60 次请求),用原子就不需要任何 unsafe 别名。
    limit: AtomicU32,
    window: Duration,
    counters: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit: AtomicU32::new(limit),
            window,
            counters: Mutex::new(HashMap::new()),
        }
    }

    /// 改上限。生产路径只在构造时设定;测试用它验证限流拒绝与退避头。
    pub fn set_limit(&self, limit: u32) {
        self.limit.store(limit, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u32 {
        self.limit.load(Ordering::Relaxed)
    }

    /// `Ok(())` = 放行;`Err(retry_after_ms)` = 超限。
    pub fn check(&self, user: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut counters = self.counters.lock();
        if counters.len() > MAX_RATE_ENTRIES {
            let window = self.window;
            counters.retain(|_, (started, _)| now.duration_since(*started) < window);
        }
        let entry = counters.entry(user.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > self.limit.load(Ordering::Relaxed) {
            let elapsed = now.duration_since(entry.0);
            let remaining = self.window.saturating_sub(elapsed).as_millis() as u64;
            return Err(remaining.max(1));
        }
        Ok(())
    }
}

/// 时间戳窗口:拒绝明显过期/未来的请求(挡重放的第二道闸)。
pub fn timestamp_in_window(issued_at_ms: i64, now_ms: i64, window_ms: i64) -> bool {
    let delta = (now_ms - issued_at_ms).abs();
    delta <= window_ms
}

/// 当前毫秒时间戳。与 core 的时钟口径一致(UTC epoch 毫秒)。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// 供 `main.rs` 的启动检查使用:密钥 PEM 必须能解析成 ES256 私钥,
/// 否则发送端**拒绝启动**(而不是启动后在第一条推送才发现)。
pub fn validate_key_pem(pem: &str) -> Result<(), String> {
    crate::apns::ProviderKey::from_pem(pem).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enroll_stores_only_a_digest_and_verifies_signatures() {
        let mut store = CredentialStore::in_memory();
        let (user, secret) = store.enroll(Some("home-mac")).unwrap();
        assert_eq!(user, "home-mac");
        assert_eq!(secret.len(), SECRET_HEX_LEN);
        assert_eq!(store.user_count(), 1);

        let message = remote_protocol::signature_string("POST", "/v1/push", 1, "n", "{}");
        let signature = sign(&secret, &message);
        assert!(store.verify(&user, &signature, &message));

        // 篡改体/换路径/nonce 变化都必须失败。
        assert!(!store.verify(
            &user,
            &signature,
            &remote_protocol::signature_string("POST", "/v1/push", 1, "n", "{\"a\":2}")
        ));
        assert!(!store.verify(
            &user,
            &signature,
            &remote_protocol::signature_string("POST", "/v1/bind", 1, "n", "{}")
        ));

        // 未知 user 与错签名返回同一个布尔,不给枚举预言机。
        assert!(!store.verify("nobody", &signature, &message));
        assert!(!store.verify(&user, &"00".repeat(32), &message));
    }

    #[test]
    fn credentials_persist_as_private_json_without_plaintext() {
        let dir = std::env::temp_dir().join(format!("aeroric-push-creds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("credentials.json");

        let (user, secret) = {
            let mut store = CredentialStore::load(Some(path.clone())).unwrap();
            store.enroll(Some("u1")).unwrap()
        };
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(&secret), "plaintext secret on disk: {raw}");
        assert!(raw.contains("secret_hash"), "{raw}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "credentials file must be private");
        }

        // 重新加载后签名仍然通过(摘要可复现)。
        let reloaded = CredentialStore::load(Some(path.clone())).unwrap();
        let message = remote_protocol::signature_string("POST", "/v1/bind", 2, "n2", "{}");
        assert!(reloaded.verify(&user, &sign(&secret, &message), &message));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replay_guard_rejects_the_second_use_and_stays_bounded() {
        let guard = ReplayGuard::new(Duration::from_millis(50));
        assert!(guard.observe("a"));
        assert!(!guard.observe("a"));
        assert!(guard.observe("b"));
        // 窗口过后同一 nonce 重新可用(此时时间戳闸门仍然拦得住真重放)。
        std::thread::sleep(Duration::from_millis(60));
        assert!(guard.observe("a"));
        assert!(guard.tracked() <= 2);
    }

    #[test]
    fn rate_limiter_counts_per_user_and_reports_retry_after() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check("u1").is_ok());
        assert!(limiter.check("u1").is_ok());
        let retry = limiter.check("u1").unwrap_err();
        assert!(retry > 0 && retry <= 60_000, "retry_after_ms={retry}");
        // 另一个凭据不受影响:限流必须按 user 隔离。
        assert!(limiter.check("u2").is_ok());
    }

    #[test]
    fn timestamp_window_is_symmetric_and_bounded() {
        let now = 1_700_000_000_000;
        assert!(timestamp_in_window(now, now, 60_000));
        assert!(timestamp_in_window(now - 59_000, now, 60_000));
        assert!(!timestamp_in_window(now - 61_000, now, 60_000));
        assert!(!timestamp_in_window(now + 61_000, now, 60_000));
    }

    #[test]
    fn user_hints_are_sanitized_and_never_blank() {
        let mut store = CredentialStore::in_memory();
        let (user, _) = store.enroll(Some("../../etc/passwd")).unwrap();
        assert_eq!(user, "etcpasswd");
        assert!(store.enroll(Some("!!!")).is_err());
        let (generated, _) = store.enroll(None).unwrap();
        assert!(generated.starts_with("u_"));
        assert!(store.revoke_user(&generated).unwrap());
        assert!(!store.revoke_user(&generated).unwrap());
    }

    #[test]
    fn sign_matches_the_core_side_fixed_vectors() {
        // 与 core `remote::push::sign_with_secret` 的固定向量成对:两边都改才会
        // 一起变红。宿主算签名、发送端验签名,口径必须是同一个。
        assert_eq!(
            sign(
                "secret-a",
                &remote_protocol::push_signature_message(
                    "owner",
                    &["dev-a".to_string(), "dev-b".to_string()],
                    remote_protocol::PushKind::ApprovalPending,
                    42,
                    "ab12cd34ef56:approval",
                    1_700_000_000_000,
                    "nonce-z",
                )
            ),
            "874b92a0641412b75469e2d767142e93fcbe9c2a2689462cdae3db1fd868e5b9"
        );
        assert_eq!(
            sign(
                "secret-a",
                &remote_protocol::bind_signature_message(
                    "owner",
                    "dev-1",
                    "ios",
                    "aabbccdd",
                    1_700_000_000_000,
                    "bind-nonce"
                )
            ),
            "c5e0dd4278b08f9d65d49dddea4d4d85eeb8250d2662390033329622739c6e69"
        );
    }

    #[test]
    fn validate_key_pem_refuses_garbage() {
        assert!(validate_key_pem("not a pem").is_err());
    }
}
