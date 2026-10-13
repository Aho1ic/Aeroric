//! APNs 客户端:provider JWT(ES256)+ HTTP/2 POST `/3/device/<token>`。
//!
//! 事实来源是 Apple《Sending notification requests to APNs》与
//! 《Establishing a token-based connection to APNs》:
//!
//! * 传输必须 HTTP/2 + TLS1.2+,主机 `api.sandbox.push.apple.com` /
//!   `api.push.apple.com`,端口 443(或 2197)。
//! * 头:`:method POST`、`:path /3/device/<device_token>`、`authorization:
//!   bearer <provider_token>`、`apns-push-type`、`apns-topic`、`apns-id`、
//!   `apns-expiration`、`apns-priority`、`apns-collapse-id`。
//! * provider token 是 **ES256 JWT**,claims 为 `iss`(Team ID)与 `iat`;
//!   Apple 要求**不超过 1 小时**刷新一次,同一 token 复用(频繁重签会被拒)。
//! * 载荷 ≤4KB,不得压缩;`apns-push-type` 必须与载荷内容相符。
//!
//! 本模块**不**把 APNs 私钥暴露给调用方:密钥在 [`ProviderKey`] 内部,
//! 只有 [`ApnsClient::provider_token`] 会用到,而它只输出 JWT。

use std::sync::Arc;
use std::time::{Duration, Instant};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use parking_lot::Mutex;
use serde_json::json;

use remote_protocol::PushKind;

/// APNs provider token 的最大复用时长。Apple:不超过 1 小时;这里取 50 分钟,
/// 留出时钟偏移与请求在飞时间。
const TOKEN_REUSE: Duration = Duration::from_secs(50 * 60);
/// 单条通知的 APNs 请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// APNs 私钥。**不派生 `Debug`/`Clone` 之外的任何吐出接口**。
pub struct ProviderKey {
    signing: SigningKey,
    key_id: String,
    team_id: String,
}

impl ProviderKey {
    /// 解析 `.p8`(PKCS#8 PEM)。失败即拒绝启动 —— 不是「先跑起来再说」。
    pub fn from_pem(pem: &str) -> Result<Self, String> {
        use p256::pkcs8::DecodePrivateKey;
        let signing = SigningKey::from_pkcs8_pem(pem.trim())
            .map_err(|e| format!("APNs key is not a valid PKCS#8 EC private key: {e}"))?;
        Ok(Self {
            signing,
            key_id: String::new(),
            team_id: String::new(),
        })
    }

    pub fn with_ids(mut self, key_id: impl Into<String>, team_id: impl Into<String>) -> Self {
        self.key_id = key_id.into();
        self.team_id = team_id.into();
        self
    }

    /// 给测试用:验证 JWT 签名所需的公钥(生产路径不使用)。
    pub fn verifying_key(&self) -> p256::ecdsa::VerifyingKey {
        *self.signing.verifying_key()
    }

    /// 铸造 ES256 JWT(header `{alg:ES256,kid}` / claims `{iss,iat}`)。
    ///
    /// 用 JWS 的 **ES256 原始签名**(r||s,64 字节),不是 DER ——APNs 只接受前者,
    /// 这是 provider JWT 最常见的失败原因。
    pub fn jwt(&self, issued_at_secs: i64) -> Result<String, String> {
        let header = json!({ "alg": "ES256", "kid": self.key_id });
        let claims = json!({ "iss": self.team_id, "iat": issued_at_secs });
        let header_b64 = b64url(&serde_json::to_vec(&header).map_err(|e| e.to_string())?);
        let claims_b64 = b64url(&serde_json::to_vec(&claims).map_err(|e| e.to_string())?);
        let signing_input = format!("{header_b64}.{claims_b64}");
        let signature: Signature = self.signing.sign(signing_input.as_bytes());
        Ok(format!("{signing_input}.{}", b64url(&signature.to_bytes())))
    }
}

/// 装配进程级 rustls provider,且只装一次。
///
/// **必须显式装**:本服务同时依赖 `hyper-rustls`(带 `ring`)与可能存在的
/// `aws-lc-rs`;rustls 0.23 在两者都可用时不会自己选,而是在**首次使用 TLS 时
/// panic**。发送端是部署方要跑起来的长期服务,一次启动即 panic 会表现为
/// 「服务起来了但一发就崩」——所以在这里、在构造连接器之前装好。
pub fn install_tls_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // 已被其它部件装过说明环境可用,`Err` 可以忽略。
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn b64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 一次投递的全部入参。
///
/// 收成一个结构体有两点好处:一是调用点的字段名自带说明(`collapse_key` /
/// `apns_id` / `expiration_secs` 都是同类型的位置参数,顺序写错不会报错);
/// 二是**它只提供 `.to_payload()` 这一条构造载荷的路径**,业务字段没有入口。
pub struct PushDelivery<'a> {
    pub device_token: &'a str,
    pub kind: PushKind,
    pub event_seq: u64,
    /// APNs 折叠键(≤64 字节)。只含任务 id 与类别,**不含**任务标题。
    pub collapse_key: &'a str,
    /// 本次请求的 `apns-id`(UUID);APNs 报错时会回传它,便于对账。
    pub apns_id: &'a str,
    pub now_secs: i64,
    /// `0` = 只尝试一次不存储(Apple:通知是 best-effort,不复述为可靠性承诺)。
    pub expiration_secs: u64,
}

/// 缓存中的 provider token。
struct CachedToken {
    token: String,
    minted: Instant,
}

/// APNs 响应分类。**只有这几种**,把 APNs 的字符串原因翻译成宿主能决策的语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApnsOutcome {
    /// 202/200:APNs 已接受(不保证投递)。
    Accepted,
    /// 410 Unregistered / 400 BadDeviceToken:token 失效,应解绑。
    TokenInvalid,
    /// 429 TooManyRequests 或 503:要求退避。
    Throttled { retry_after_ms: Option<u64> },
    /// 其它 4xx(如 BadTopic、PayloadTooLarge):配置或载荷问题,**不重试**。
    Rejected { status: u16, reason: Option<String> },
    /// 连接层失败/超时/5xx:可重试。
    Upstream { status: Option<u16> },
}

impl ApnsOutcome {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Throttled { .. } | Self::Upstream { .. })
    }
}

/// APNs HTTP/2 客户端。
pub struct ApnsClient {
    base: String,
    topic: String,
    key: Arc<ProviderKey>,
    http: Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>,
    token: Mutex<Option<CachedToken>>,
}

impl ApnsClient {
    /// `base` 形如 `https://api.push.apple.com`;**`http://` 基址会自动走 h2c**,
    /// 供本地联调与测试里的假 APNs 使用(生产基址由 `PushConfig::apns_base` 给出)。
    pub fn new(base: &str, topic: &str, key: Arc<ProviderKey>) -> Result<Self, String> {
        install_tls_provider();
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http2()
            .build();
        // APNs 只接受 HTTP/2。`http2_only(true)` 对 `https://` 走 ALPN 协商到 h2,
        // 对 `http://`(测试的假 APNs)走 h2c prior knowledge —— 两者都是「只许 h2」,
        // 不允许回落到 HTTP/1.1(回落会让真实的 APNs 请求静默失败)。
        let http = Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build(https);
        if !base.starts_with("https://") && !base.starts_with("http://") {
            return Err(format!("APNs base must be http(s)://, got {base:?}"));
        }
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            topic: topic.to_string(),
            key,
            http,
            token: Mutex::new(None),
        })
    }

    /// 取得(必要时铸造)provider token。复用窗口内不重签:Apple 明确要求
    /// 复用,频繁刷新会被拒。
    pub fn provider_token(&self, now_secs: i64) -> Result<String, String> {
        let mut cached = self.token.lock();
        if let Some(current) = cached.as_ref() {
            if current.minted.elapsed() < TOKEN_REUSE {
                return Ok(current.token.clone());
            }
        }
        let token = self.key.jwt(now_secs)?;
        *cached = Some(CachedToken {
            token: token.clone(),
            minted: Instant::now(),
        });
        Ok(token)
    }

    /// 供测试断言缓存行为(重签次数由 token 字符串是否变化间接观察)。
    pub fn cached_token(&self) -> Option<String> {
        self.token.lock().as_ref().map(|c| c.token.clone())
    }

    /// 发一条通知。
    ///
    /// payload 由本函数按 `delivery.kind` 组装 —— 调用方**没有**传正文的入口,
    /// 这是载荷最小化的机器形状。
    pub async fn send(&self, delivery: &PushDelivery<'_>) -> Result<ApnsOutcome, String> {
        let PushDelivery {
            device_token,
            kind,
            event_seq,
            collapse_key,
            apns_id,
            now_secs,
            expiration_secs,
        } = *delivery;
        let token = self.provider_token(now_secs)?;
        let url = format!("{}/3/device/{device_token}", self.base);
        let payload = serde_json::to_vec(&json!({
            "aps": {
                "alert": { "loc-key": kind.loc_key() },
                "sound": "default",
                // 通知只在 >1 时有意义(提醒「有变化」);具体状态必须回 E2EE 取,
                // 所以这里只放一个「有更新」的计数语义位,不放业务内容。
                "badge": 1,
                "thread-id": collapse_key,
            },
            // 客户端据 eventSeq 决定是否需要重拉快照。**不是**授权凭据。
            "eventSeq": event_seq,
            "kind": match kind {
                PushKind::TaskDone => "task_done",
                PushKind::TaskFailed => "task_failed",
                PushKind::ApprovalPending => "approval_pending",
            },
        }))
        .map_err(|e| e.to_string())?;

        let request = Request::builder()
            .method("POST")
            .uri(&url)
            .header("authorization", format!("bearer {token}"))
            .header("apns-topic", &self.topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header("apns-id", apns_id)
            .header("apns-expiration", expiration_secs.to_string())
            .header("apns-collapse-id", collapse_key)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(payload)))
            .map_err(|e| e.to_string())?;

        let response = match tokio::time::timeout(REQUEST_TIMEOUT, self.http.request(request)).await
        {
            Ok(Ok(response)) => response,
            Ok(Err(_error)) => {
                return Ok(ApnsOutcome::Upstream { status: None });
            }
            Err(_) => {
                // 超时:APNs 可能仍在处理,但重试是安全的(push 不是幂等副作用)。
                return Ok(ApnsOutcome::Upstream { status: None });
            }
        };

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(|secs| secs * 1000);
        let body = response
            .into_body()
            .collect()
            .await
            .map(|collected| collected.to_bytes())
            .unwrap_or_default();
        let reason = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .map(str::to_string)
            });

        Ok(classify(status, reason, retry_after))
    }
}

/// APNs 状态码 → 语义。依据 Apple《Handling notification responses from APNs》。
fn classify(
    status: StatusCode,
    reason: Option<String>,
    retry_after_ms: Option<u64>,
) -> ApnsOutcome {
    let code = status.as_u16();
    if status.is_success() {
        return ApnsOutcome::Accepted;
    }
    match code {
        // 410:token 已不再对应任何设备;400 BadDeviceToken:token 形状/环境不对。
        410 => ApnsOutcome::TokenInvalid,
        400 if reason.as_deref() == Some("BadDeviceToken") => ApnsOutcome::TokenInvalid,
        // 429 TooManyRequests / 503 ServiceUnavailable:必须退避。
        429 => ApnsOutcome::Throttled { retry_after_ms },
        503 => ApnsOutcome::Throttled { retry_after_ms },
        // 5xx 其余:上游故障,可重试。
        500..=599 => ApnsOutcome::Upstream { status: Some(code) },
        // 其余 4xx:请求本身有问题(BadTopic/PayloadTooLarge/MissingTopic…),
        // 重试只会重复失败。
        _ => ApnsOutcome::Rejected {
            status: code,
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份测试用 .p8(EC P-256 PKCS#8)。它只用于本模块的签名/解析测试,
    /// 不是任何真实 Apple 账号的密钥。
    fn test_pem() -> String {
        use p256::elliptic_curve::Generate;
        use p256::pkcs8::EncodePrivateKey;
        let key = SigningKey::generate();
        key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .expect("pem")
            .to_string()
    }

    #[test]
    fn provider_jwt_is_es256_with_raw_signature_and_verifies() {
        let pem = test_pem();
        let key = ProviderKey::from_pem(&pem).unwrap().with_ids("KID", "TEAM");
        let jwt = key.jwt(1_700_000_000).unwrap();

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWS compact serialization: {jwt}");
        use base64::Engine;
        let header: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[0])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KID");
        let claims: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["iss"], "TEAM");
        assert_eq!(claims["iat"], 1_700_000_000);

        // 签名必须是 64 字节原始 (r||s),不是 DER —— APNs 只接受前者。
        let signature_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[2])
            .unwrap();
        assert_eq!(signature_bytes.len(), 64, "raw ES256 signature");
        let signature = Signature::from_slice(&signature_bytes).unwrap();
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        use p256::ecdsa::signature::Verifier;
        key.verifying_key()
            .verify(signing_input.as_bytes(), &signature)
            .expect("JWT signature verifies with the provider public key");
    }

    #[test]
    fn from_pem_rejects_non_ec_and_garbage() {
        assert!(ProviderKey::from_pem("").is_err());
        assert!(ProviderKey::from_pem(
            "-----BEGIN PRIVATE KEY-----\nnope\n-----END PRIVATE KEY-----"
        )
        .is_err());
    }

    #[test]
    fn classify_maps_apns_status_codes_to_actions() {
        assert_eq!(classify(StatusCode::OK, None, None), ApnsOutcome::Accepted);
        assert_eq!(
            classify(StatusCode::from_u16(410).unwrap(), None, None),
            ApnsOutcome::TokenInvalid
        );
        assert_eq!(
            classify(StatusCode::BAD_REQUEST, Some("BadDeviceToken".into()), None),
            ApnsOutcome::TokenInvalid
        );
        assert_eq!(
            classify(StatusCode::TOO_MANY_REQUESTS, None, Some(4_000)),
            ApnsOutcome::Throttled {
                retry_after_ms: Some(4_000)
            }
        );
        assert_eq!(
            classify(StatusCode::SERVICE_UNAVAILABLE, None, None),
            ApnsOutcome::Throttled {
                retry_after_ms: None
            }
        );
        assert_eq!(
            classify(StatusCode::INTERNAL_SERVER_ERROR, None, None),
            ApnsOutcome::Upstream { status: Some(500) }
        );
        // BadTopic / PayloadTooLarge 这类不重试。
        let rejected = classify(StatusCode::BAD_REQUEST, Some("BadTopic".into()), None);
        assert_eq!(
            rejected,
            ApnsOutcome::Rejected {
                status: 400,
                reason: Some("BadTopic".to_string())
            }
        );
        assert!(!rejected.retryable());
        assert!(ApnsOutcome::Upstream { status: None }.retryable());
        assert!(ApnsOutcome::Throttled {
            retry_after_ms: None
        }
        .retryable());
        assert!(!ApnsOutcome::TokenInvalid.retryable());
    }

    #[test]
    fn client_rejects_a_base_without_scheme() {
        let key = Arc::new(
            ProviderKey::from_pem(&test_pem())
                .unwrap()
                .with_ids("k", "t"),
        );
        assert!(ApnsClient::new("api.push.apple.com", "topic", key.clone()).is_err());
        assert!(ApnsClient::new("https://api.push.apple.com", "topic", key).is_ok());
    }
}
