//! 推送平面的 **core 半边**(MIGRATION_PLAN §D AP6)。
//!
//! core 只做三件事,其余(APNs 私钥、HTTP/2、退避节奏)留给宿主与发送端:
//!
//! 1. **载荷策略**:`PushPayload` 是**唯一**能从任务事件构造推送载荷的路径。
//!    它按白名单产出字段,`kind` 决定客户端 bundle 的 `loc-key`。字段集合就是
//!    隐私边界 —— 任务标题、项目名、路径、命令、提示词**没有入口**,不是靠
//!    「记得别写进去」。
//! 2. **事件→推送的判决**:`PushPolicy` 把 `task-status` 载荷翻译成
//!    「要不要推 / 推哪一种」。只有 `done`/`failed`/`input_required` 三种值得
//!    打扰用户;`running`/`pending` 等状态变化频繁且无价值,明确不推。
//! 3. **发送端客户端**:`PushSenderClient` 按 `remote_protocol` 的签名格式签
//!    请求并解析回执(含 `invalidatedDeviceIds`,用于在自己的绑定表里同步失效)。
//!
//! 不做的事:core **不**持有 APNs device token(那是宿主的 `push.register` 从
//! 手机收到后直接转给发送端的),也**不**决定重试节奏 —— 发送端回执里的
//! `retryAfterMs` 由宿主解释。core 里没有网络重试循环,避免「静默无限重试」
//! 这种会让通知行为不可预测的东西。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use remote_protocol::{
    push_signature_message, PushAck, PushEnvelope, PushErrorCode, PushFailure, PushKind,
    PushRequest, PUSH_PROTOCOL_VERSION,
};

/// 一次 `task-status` 事件该不该推、推哪一种。
///
/// 依据 Q7「任务完成/失败/待审批」三类提醒;其余状态明确不推。**审批类尤其
/// 重要**:通知只说明「有审批在等」,批准动作必须回到已认证的 E2EE 连接做
/// (通知不带授权语义)。
pub fn push_kind_for_status(status: &str) -> Option<PushKind> {
    match status {
        "done" => Some(PushKind::TaskDone),
        "failed" => Some(PushKind::TaskFailed),
        // 宿主在 `input_required` 时才会带上审批信息,但状态本身已经足够说明
        // 「需要人来看一眼」;不带审批细节也是刻意的:审批详情走 E2EE。
        "input_required" => Some(PushKind::ApprovalPending),
        _ => None,
    }
}

/// 从 `task-status` 载荷构造推送信封。
///
/// `event_seq` 取宿主事件日志的 seq(调用方给),`collapse_key` 由任务 id 与
/// 类别拼成 —— **只含 id 与类别**,不含标题。任务 id 是本地 opaque 值,
/// 不是人类可读的业务描述;即便泄露也不含内容语义。
pub fn envelope_for(task_id: &str, kind: PushKind, event_seq: u64) -> PushEnvelope {
    let mut hasher = sha2::Sha256::new();
    {
        use sha2::Digest;
        hasher.update(task_id.as_bytes());
    }
    use sha2::Digest;
    let digest = hex_prefix(&hasher.finalize(), 8);
    PushEnvelope {
        v: PUSH_PROTOCOL_VERSION,
        kind,
        event_seq,
        // APNs 限制 64 字节;`<8 hex>:<tag>` 远小于它。
        collapse_key: format!("{digest}:{}", kind.collapse_tag()),
    }
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    let mut out = String::with_capacity(count * 2);
    for byte in bytes.iter().take(count) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 任务事件 → 推送判决的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushDecision {
    pub kind: PushKind,
    pub envelope: PushEnvelope,
}

/// 判决入口:给一条 `task-status` 载荷,返回要不要推。
///
/// 只认 `task_id` 与 `status` 两个字段 —— 即使上游载荷里带了标题(当前不带),
/// 这里也**不读**它,所以不可能顺着新增字段把它带进推送。
pub fn decide(payload: &Value, event_seq: u64) -> Option<PushDecision> {
    let task_id = payload.get("task_id").and_then(Value::as_str)?.trim();
    if task_id.is_empty() {
        return None;
    }
    let status = payload.get("status").and_then(Value::as_str)?;
    let kind = push_kind_for_status(status)?;
    Some(PushDecision {
        kind,
        envelope: envelope_for(task_id, kind, event_seq),
    })
}

// ── 设备绑定(宿主侧镜像) ────────────────────────────────────────────────────

/// 宿主侧的一条推送绑定。**不含** APNs device token:token 在发送端,
/// 宿主只记「这台设备注册过推送 + 它的平台」,用来决定要不要为它发请求。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushBinding {
    pub device_id: String,
    pub platform: String,
    pub registered_at_ms: i64,
    /// 发送端回执里报过失效(410/BadDeviceToken)。标记后不再作为推送目标,
    /// 但仍然保留记录,让 UI 能解释「这台设备的推送为什么没了」。
    pub invalidated: bool,
}

/// 宿主侧绑定表。落盘 `push-bindings.json`(0600),与 `remote-devices.json` 并列。
#[derive(Default)]
pub struct PushBindings {
    inner: Mutex<BTreeMap<String, PushBinding>>,
}

impl PushBindings {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册/更新。`registered_at_ms` 由调用方给(可测)。
    pub fn register(&self, device_id: &str, platform: &str, now_ms: i64) {
        self.inner.lock().insert(
            device_id.to_string(),
            PushBinding {
                device_id: device_id.to_string(),
                platform: platform.to_string(),
                registered_at_ms: now_ms,
                invalidated: false,
            },
        );
    }

    /// 撤销(用户在 Mac 上移除设备,或手机显式注销)。返回是否真的删掉了。
    pub fn unregister(&self, device_id: &str) -> bool {
        self.inner.lock().remove(device_id).is_some()
    }

    /// 发送端报失效时打标。返回是否改变了状态(用于只记一次审计)。
    pub fn mark_invalidated(&self, device_id: &str) -> bool {
        let mut inner = self.inner.lock();
        match inner.get_mut(device_id) {
            Some(binding) if !binding.invalidated => {
                binding.invalidated = true;
                true
            }
            _ => false,
        }
    }

    /// 当前可作为推送目标的设备 id(已注册且未被判失效)。
    pub fn active_device_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .values()
            .filter(|binding| !binding.invalidated)
            .map(|binding| binding.device_id.clone())
            .collect()
    }

    pub fn snapshot(&self) -> Vec<PushBinding> {
        self.inner.lock().values().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

// ── 发送端客户端 ─────────────────────────────────────────────────────────────

/// 发送端配置(宿主侧)。`url` 必须显式配置:产品方不提供默认发送端(Q10),
/// 所以「没配」= 推送不可用,而不是回落到某个官方地址。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushSenderConfig {
    pub url: String,
    /// per-user 凭据 id。
    pub user: String,
    /// per-user 凭据明文(宿主钥匙串里存的)。
    pub secret: String,
}

/// 推送请求的构造与回执解析。**纯逻辑**:真正的 HTTP 由宿主的 `PushHost` 端口做,
/// 这样 core 能对签名、裁剪、回执语义做单测,而不用起网络。
#[derive(Clone)]
pub struct PushSenderClient {
    config: PushSenderConfig,
}

impl PushSenderClient {
    pub fn new(config: PushSenderConfig) -> Result<Self, String> {
        // 发送端必须显式配置;非 loopback 必须是 https(与 relay 的 wss 规则同口径)。
        let trimmed = config.url.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            return Err("push sender URL is not configured".to_string());
        }
        if !trimmed.starts_with("https://") && !is_loopback_http(&trimmed) {
            return Err(
                "push sender URL must use https:// unless it points at localhost".to_string(),
            );
        }
        let mut config = config;
        config.url = trimmed;
        Ok(Self { config })
    }

    pub fn config(&self) -> &PushSenderConfig {
        &self.config
    }

    /// 构造 `/v1/push` 请求体(签名已算好)。`nonce` 由调用方给,便于测试与
    /// 「同一事件不重发同一 nonce」的幂等策略。
    pub fn build_push_request(
        &self,
        device_ids: &[String],
        envelope: &PushEnvelope,
        issued_at_ms: i64,
        nonce: &str,
    ) -> PushRequest {
        // 签名串由 remote-protocol 唯一构造(它已经把方法/路径/时间/nonce/规范体
        // 拼好),这里**不再**二次拼接 —— 重写格式就是给对方一个不一致的机会。
        let signature = sign_with_secret(
            &self.config.secret,
            &push_signature_message(
                &self.config.user,
                device_ids,
                envelope.kind,
                envelope.event_seq,
                &envelope.collapse_key,
                issued_at_ms,
                nonce,
            ),
        );
        PushRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: self.config.user.clone(),
            device_ids: device_ids.to_vec(),
            envelope: envelope.clone(),
            issued_at_ms,
            nonce: nonce.to_string(),
            signature,
        }
    }

    /// 构造 `/v1/bind` 请求体。`token` 为空串 = 注销。
    pub fn build_bind_request(
        &self,
        device_id: &str,
        platform: &str,
        token: &str,
        issued_at_ms: i64,
        nonce: &str,
    ) -> remote_protocol::PushBindRequest {
        let signature = sign_with_secret(
            &self.config.secret,
            &remote_protocol::bind_signature_message(
                &self.config.user,
                device_id,
                platform,
                token,
                issued_at_ms,
                nonce,
            ),
        );
        remote_protocol::PushBindRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: self.config.user.clone(),
            device_id: device_id.to_string(),
            platform: platform.to_string(),
            token: token.to_string(),
            issued_at_ms,
            nonce: nonce.to_string(),
            signature,
        }
    }
}

/// HMAC-SHA256(以凭据摘要为密钥)。
///
/// 密钥是**摘要**而不是明文,与发送端 `auth::sign` 同源:发送端落盘只存摘要,
/// 两侧对同一 secret 得出同一签名。这条约定必须成对修改 ——
/// `push_signature_matches_what_the_sender_recomputes` 与发送端的
/// `enroll_stores_only_a_digest_and_verifies_signatures` 一起钉住它。
fn sign_with_secret(secret: &str, message: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let digest = hex::encode(hasher.finalize());

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(digest.as_bytes()) else {
        // 摘要长度恒定,`new_from_slice` 只在密钥长度为 0 时失败,而 SHA-256
        // 摘要永远 64 个 hex 字符;真发生了也宁可返回空签名让服务端拒绝,
        // 而不是 panic 掉宿主进程。
        return String::new();
    };
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    host == "localhost"
        || host == "127.0.0.1"
        || host == "[::1]"
        || host.starts_with("127.0.0.1:")
        || host.starts_with("localhost:")
        || host.starts_with("[::1]:")
}

/// 回执解释:把发送端的应答翻译成宿主该做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushOutcome {
    pub accepted: usize,
    /// 需要标记失效的设备(从绑定表剔除)。
    pub invalidated_device_ids: Vec<String>,
    /// 建议的退避(毫秒)。宿主据此**不再**连发,而不是自己重试。
    pub retry_after_ms: Option<u64>,
}

/// 解析发送端回执。区分「成功回执」与「错误回执」,并把错误码映射成宿主可读的
/// 不重试/重试判决。
pub fn interpret_response(status: u16, body: &str) -> Result<PushOutcome, PushDiagnostic> {
    if (200..300).contains(&status) {
        let ack: PushAck = serde_json::from_str(body).map_err(|error| PushDiagnostic {
            code: PushErrorCode::BadRequest,
            message: format!("sender returned an unreadable ack: {error}"),
            retryable: false,
        })?;
        return Ok(PushOutcome {
            accepted: ack.accepted,
            invalidated_device_ids: ack.invalidated_device_ids,
            retry_after_ms: ack.retry_after_ms,
        });
    }
    match serde_json::from_str::<PushFailure>(body) {
        Ok(failure) => Err(PushDiagnostic {
            code: failure.code,
            message: failure.message,
            retryable: failure.retryable,
        }),
        Err(_) => Err(PushDiagnostic {
            code: PushErrorCode::Upstream,
            // 不把原始体透传:可能是反向代理的 HTML,对用户无意义且可能含路径。
            message: format!("sender returned HTTP {status} with an unrecognized body"),
            retryable: status >= 500,
        }),
    }
}

/// 面向上层的推送诊断。**只**含稳定错误码与一句原因,不含请求体/凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushDiagnostic {
    pub code: PushErrorCode,
    pub message: String,
    pub retryable: bool,
}

// ── 自托管 relay 诊断(配置面纯逻辑) ─────────────────────────────────────────

/// relay 诊断中**配置侧**的判决(连通性探测由宿主做,见 `remote.relayDiagnostics`)。
///
/// 拆成两半的理由:方案校验是纯逻辑、可单测且与网络无关;「连得上吗」必须真发
/// 一次 WS 握手,那段归宿主。
pub fn relay_scheme_problem(url: &str) -> Option<String> {
    let url = url.trim();
    // 空 URL 不是「方案问题」:它表达的是「没用 relay」。把两者混为一谈会让
    // 诊断把一个完全合法的配置(只有 LAN 直连)报成配置错误 —— 而 Q10 明确
    // 允许「用户自托管 relay 或可信直连网络」二选一。
    if url.is_empty() {
        return None;
    }
    if url.starts_with("wss://") {
        return None;
    }
    if url.starts_with("ws://") {
        let host = url
            .trim_start_matches("ws://")
            .split(['/', ':'])
            .next()
            .unwrap_or_default();
        if host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1" {
            return None;
        }
        return Some(
            "non-loopback relay URLs must use wss:// (plain ws:// exposes metadata to the network)"
                .to_string(),
        );
    }
    Some("relay URL must start with wss:// (or ws:// for localhost testing)".to_string())
}

// ── 落盘 ─────────────────────────────────────────────────────────────────────

/// 宿主推送配置的持久化形状(`push-config.json`)。
///
/// 刻意**不**把 per-user 凭据明文写进来:那是钥匙串的东西(与 SSH 密码、
/// DBX 凭据同一策略)。这里只存发送端地址与凭据 id。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushConfigFile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sender_url: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
}

/// per-user 推送凭据在系统钥匙串里的键名。
///
/// 与 SSH 密码 / DBX 凭据同一策略:配置只存 **id** 与地址,密钥明文进钥匙串,
/// 不进 `push-config.json`(理由与那些凭据相同:配置文件会被同步/备份)。
pub const PUSH_CREDENTIAL_KEY: &str = "push-sender-credential";

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("push-config.json")
}

/// 读取;缺失或坏文件都回默认(未启用),不 panic、不猜。
pub fn load_config(data_dir: &Path) -> PushConfigFile {
    let Ok(raw) = std::fs::read_to_string(config_path(data_dir)) else {
        return PushConfigFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// 保存。权限与 core 其它私密文件同口径。
pub fn save_config(data_dir: &Path, config: &PushConfigFile) -> Result<(), String> {
    let path = config_path(data_dir);
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&path, &raw)
}

/// 退避上限:即便发送端建议一个很大的值,宿主也不会算出一个荒谬的等待。
pub const MAX_RETRY_AFTER_MS: u64 = 15 * 60 * 1000;

/// 发送端**没有**给 `retryAfterMs` 时的默认退避。
///
/// 这条不是可有可无的默认值:发送端回一个可重试的诊断(如 APNs 503)却没说
/// 等多久时,如果宿主不设窗口,下一条任务事件就会立刻再打一次 APNs —— 正是
/// 「不重试」要避免的连发。`None` 的含义是「发送端没有要求退避」,而可重试
/// 失败本身就要求退避,所以这里必须补一个非零默认。
pub const DEFAULT_BACKOFF_MS: u64 = 5_000;

/// 把发送端建议的退避夹到合理范围。
pub fn clamp_retry_after(suggested: Option<u64>) -> Option<Duration> {
    suggested.map(|ms| Duration::from_millis(ms.clamp(1_000, MAX_RETRY_AFTER_MS)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_three_statuses_are_worth_a_notification() {
        assert_eq!(push_kind_for_status("done"), Some(PushKind::TaskDone));
        assert_eq!(push_kind_for_status("failed"), Some(PushKind::TaskFailed));
        assert_eq!(
            push_kind_for_status("input_required"),
            Some(PushKind::ApprovalPending)
        );
        // 高频或无需打扰的状态明确不推。
        for quiet in [
            "running",
            "pending",
            "todo",
            "detached",
            "interrupted",
            "cancelled",
            "",
        ] {
            assert_eq!(push_kind_for_status(quiet), None, "{quiet} must not notify");
        }
    }

    #[test]
    fn decide_reads_only_task_id_and_status() {
        // 上游载荷即使带了业务字段,判决也不会把它们带进推送。
        let payload = json!({
            "task_id": "task-1",
            "status": "failed",
            "title": "Refactor the billing service",
            "projectName": "Aeroric",
            "cwd": "/Users/someone/secret",
            "failure_reason": "boom",
        });
        let decision = decide(&payload, 11).expect("failed must notify");
        assert_eq!(decision.kind, PushKind::TaskFailed);
        assert_eq!(decision.envelope.event_seq, 11);
        let serialized = serde_json::to_string(&decision.envelope).unwrap();
        for forbidden in ["Refactor", "Aeroric", "/Users/", "boom"] {
            assert!(
                !serialized.contains(forbidden),
                "envelope leaked {forbidden:?}: {serialized}"
            );
        }
        // 键集合固定。
        let value: Value = serde_json::from_str(&serialized).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["collapseKey", "eventSeq", "kind", "v"]);

        // 缺 task_id 或缺 status → 不推。
        assert!(decide(&json!({"status": "done"}), 1).is_none());
        assert!(decide(&json!({"task_id": "t", "status": "running"}), 1).is_none());
        assert!(decide(&json!({"task_id": "  ", "status": "done"}), 1).is_none());
    }

    #[test]
    fn collapse_key_is_stable_per_task_and_kind_without_the_task_id() {
        let a = envelope_for("task-secret-id", PushKind::TaskDone, 1);
        let b = envelope_for("task-secret-id", PushKind::TaskDone, 99);
        // 同一任务同一类别 → 同一折叠键(不同 seq 也让 APNs 合并)。
        assert_eq!(a.collapse_key, b.collapse_key);
        // 换类别 → 不同折叠键(完成与失败不该互相吞掉)。
        assert_ne!(
            a.collapse_key,
            envelope_for("task-secret-id", PushKind::TaskFailed, 1).collapse_key
        );
        // 换任务 → 不同折叠键。
        assert_ne!(
            a.collapse_key,
            envelope_for("other-task", PushKind::TaskDone, 1).collapse_key
        );
        // 折叠键不含任务 id 原文,且不超过 APNs 的 64 字节上限。
        assert!(!a.collapse_key.contains("task-secret-id"));
        assert!(a.collapse_key.len() <= 64);
        assert!(a.collapse_key.ends_with(":done"));
    }

    #[test]
    fn bindings_track_registration_invalidation_and_revocation() {
        let bindings = PushBindings::new();
        assert!(bindings.is_empty());
        bindings.register("dev-1", "ios", 100);
        bindings.register("dev-2", "ipados", 200);
        assert_eq!(bindings.active_device_ids().len(), 2);

        // 重复注册是幂等更新,不产生第二条记录。
        bindings.register("dev-1", "ios", 300);
        assert_eq!(bindings.snapshot().len(), 2);
        assert_eq!(
            bindings
                .snapshot()
                .iter()
                .find(|b| b.device_id == "dev-1")
                .unwrap()
                .registered_at_ms,
            300
        );

        // 失效打标只生效一次,且立刻退出目标集合。
        assert!(bindings.mark_invalidated("dev-1"));
        assert!(!bindings.mark_invalidated("dev-1"));
        assert!(!bindings.mark_invalidated("ghost"));
        assert_eq!(bindings.active_device_ids(), vec!["dev-2".to_string()]);

        // 撤销是删除,连失效记录一起清掉。
        assert!(bindings.unregister("dev-2"));
        assert!(!bindings.unregister("dev-2"));
        // dev-1 仍在表里(它是「失效」而不是「撤销」:记录留着让 UI 能解释原因)。
        assert_eq!(bindings.snapshot().len(), 1);
        assert!(bindings.snapshot()[0].invalidated);
        assert!(bindings.active_device_ids().is_empty());
        assert!(bindings.unregister("dev-1"));
        assert!(bindings.is_empty());
    }

    #[test]
    fn sender_url_must_be_https_except_loopback() {
        let config = |url: &str| PushSenderConfig {
            url: url.to_string(),
            user: "u".to_string(),
            secret: "s".to_string(),
        };
        assert!(PushSenderClient::new(config("")).is_err());
        assert!(PushSenderClient::new(config("http://push.example.com")).is_err());
        assert!(PushSenderClient::new(config("ws://push.example.com")).is_err());
        assert!(PushSenderClient::new(config("https://push.example.com/")).is_ok());
        assert_eq!(
            PushSenderClient::new(config("https://push.example.com/"))
                .unwrap()
                .config()
                .url,
            "https://push.example.com"
        );
        // loopback 的 http 允许(本地联调)。
        assert!(PushSenderClient::new(config("http://127.0.0.1:6792")).is_ok());
        assert!(PushSenderClient::new(config("http://localhost:6792")).is_ok());
    }

    #[test]
    fn bind_request_uses_empty_token_to_unbind_and_signs_the_semantics() {
        let client = PushSenderClient::new(PushSenderConfig {
            url: "https://push.example.com".to_string(),
            user: "u1".to_string(),
            secret: "s1".to_string(),
        })
        .unwrap();
        let bind = client.build_bind_request("d1", "ios", "tok", 5, "n1");
        assert_eq!(bind.token, "tok");
        assert_eq!(bind.user, "u1");
        assert_eq!(bind.v, PUSH_PROTOCOL_VERSION);
        assert!(!bind.signature.is_empty());

        // 签名必须覆盖 token:换 token 就得换签名(否则可以拿别人的签名改 body)。
        let other = client.build_bind_request("d1", "ios", "tok2", 5, "n1");
        assert_ne!(bind.signature, other.signature);

        let unbind = client.build_bind_request("d1", "ios", "", 5, "n2");
        assert!(unbind.token.is_empty());
    }

    #[test]
    fn push_request_signature_covers_devices_kind_and_collapse_key() {
        let client = PushSenderClient::new(PushSenderConfig {
            url: "https://push.example.com".to_string(),
            user: "u1".to_string(),
            secret: "s1".to_string(),
        })
        .unwrap();
        let envelope = envelope_for("t1", PushKind::TaskDone, 3);
        let devices = vec!["d1".to_string()];
        let base = client.build_push_request(&devices, &envelope, 9, "n");

        // 换设备、换 kind、换 seq、换 collapse 都要换签名。
        let mut other_envelope = envelope.clone();
        other_envelope.kind = PushKind::TaskFailed;
        let variants = [
            client.build_push_request(&["d2".to_string()], &envelope, 9, "n"),
            client.build_push_request(&devices, &other_envelope, 9, "n"),
            client.build_push_request(&devices, &envelope, 9, "other-nonce"),
        ];
        for variant in variants {
            assert_ne!(variant.signature, base.signature);
        }
        // 同样的入参 → 同样的签名(可复现,发送端才有得比)。
        assert_eq!(
            client
                .build_push_request(&devices, &envelope, 9, "n")
                .signature,
            base.signature
        );
    }

    #[test]
    fn push_signature_matches_what_the_sender_recomputes() {
        // 这条是**跨进程契约**:宿主算出的签名必须与发送端对同一入参算出的
        // 完全一致。发送端的实现就是 remote-protocol 里那一份,所以这里直接比对
        // 「宿主 API 产物」与「协议函数产物」。
        let client = PushSenderClient::new(PushSenderConfig {
            url: "https://push.example.com".to_string(),
            user: "owner".to_string(),
            secret: "s".to_string(),
        })
        .unwrap();
        let envelope = envelope_for("task-x", PushKind::ApprovalPending, 42);
        let devices = vec!["dev-a".to_string(), "dev-b".to_string()];
        let built = client.build_push_request(&devices, &envelope, 1_700_000_000_000, "nonce-z");

        // `push_signature_message` 返回的就是**完整**待签串(方法/路径/时间/nonce/
        // 规范体都在里面),所以宿主与发送端对同一入参算出同一个 HMAC。
        let canonical = push_signature_message(
            "owner",
            &devices,
            envelope.kind,
            envelope.event_seq,
            &envelope.collapse_key,
            1_700_000_000_000,
            "nonce-z",
        );
        assert!(canonical.starts_with("POST\n/v1/push\n"), "{canonical}");
        assert_eq!(built.signature, sign_with_secret("s", &canonical));

        // 固定向量:发送端 crate 的 `auth::sign("secret-a", <同一 canonical>)`
        // 必须得到同一个十六进制串(两条路径的 HMAC 口径已经被这一对常量钉死)。
        assert_eq!(
            sign_with_secret(
                "secret-a",
                &push_signature_message(
                    "owner",
                    &devices,
                    PushKind::ApprovalPending,
                    42,
                    "ab12cd34ef56:approval",
                    1_700_000_000_000,
                    "nonce-z",
                )
            ),
            "874b92a0641412b75469e2d767142e93fcbe9c2a2689462cdae3db1fd868e5b9"
        );
        assert_eq!(
            sign_with_secret(
                "secret-a",
                &remote_protocol::bind_signature_message(
                    "owner",
                    "dev-1",
                    "ios",
                    "aabbccdd",
                    1_700_000_000_000,
                    "bind-nonce",
                )
            ),
            "c5e0dd4278b08f9d65d49dddea4d4d85eeb8250d2662390033329622739c6e69"
        );
    }

    #[test]
    fn ack_parsing_reports_invalidations_and_backoff() {
        let outcome = interpret_response(
            200,
            &json!({
                "v": 1,
                "accepted": 1,
                "invalidatedDeviceIds": ["d2"],
                "retryAfterMs": 4000
            })
            .to_string(),
        )
        .expect("success ack");
        assert_eq!(outcome.accepted, 1);
        assert_eq!(outcome.invalidated_device_ids, vec!["d2".to_string()]);
        assert_eq!(outcome.retry_after_ms, Some(4000));

        let failure = interpret_response(
            401,
            &json!({"code":"unauthorized","message":"unknown user or bad signature","retryable":false})
                .to_string(),
        )
        .expect_err("must be an error");
        assert_eq!(failure.code, PushErrorCode::Unauthorized);
        assert!(!failure.retryable);

        let limited = interpret_response(
            429,
            &json!({"code":"rate_limited","message":"slow down","retryable":true}).to_string(),
        )
        .expect_err("must be an error");
        assert!(limited.retryable);

        // 反向代理返回 HTML:归类为 upstream,且**不**把原始体带出来。
        let html = interpret_response(502, "<html><body>/srv/proxy</body></html>")
            .expect_err("must be an error");
        assert_eq!(html.code, PushErrorCode::Upstream);
        assert!(!html.message.contains("/srv/proxy"), "{}", html.message);
        assert!(html.retryable);
    }

    #[test]
    fn backoff_is_clamped_into_a_sane_window() {
        assert_eq!(clamp_retry_after(None), None);
        assert_eq!(
            clamp_retry_after(Some(0)),
            Some(Duration::from_millis(1_000))
        );
        assert_eq!(
            clamp_retry_after(Some(4_000)),
            Some(Duration::from_millis(4_000))
        );
        assert_eq!(
            clamp_retry_after(Some(u64::MAX)),
            Some(Duration::from_millis(MAX_RETRY_AFTER_MS))
        );
    }

    #[test]
    fn relay_scheme_rules_match_the_documented_deployment() {
        assert!(relay_scheme_problem("wss://relay.example.com").is_none());
        assert!(relay_scheme_problem("ws://localhost:6791").is_none());
        assert!(relay_scheme_problem("ws://127.0.0.1:6791").is_none());
        assert!(relay_scheme_problem("ws://relay.example.com").is_some());
        assert!(relay_scheme_problem("http://relay.example.com").is_some());
        // 空值 = 「没有用 relay」,这不是方案错误:直连网络本来就是许可配置。
        assert!(relay_scheme_problem("").is_none());
        assert!(relay_scheme_problem("   ").is_none());
    }

    #[test]
    fn config_round_trips_and_defaults_to_disabled() {
        let dir = std::env::temp_dir().join(format!("aeroric-push-core-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 缺失 → 未启用。
        assert_eq!(load_config(&dir), PushConfigFile::default());
        assert!(!load_config(&dir).enabled);

        let config = PushConfigFile {
            enabled: true,
            sender_url: Some("https://push.example.com".to_string()),
            user: Some("u1".to_string()),
        };
        save_config(&dir, &config).unwrap();
        assert_eq!(load_config(&dir), config);

        // 私密权限。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(config_path(&dir))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        // 坏文件 → 回默认,不 panic。
        std::fs::write(config_path(&dir), "{not json").unwrap();
        assert_eq!(load_config(&dir), PushConfigFile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
