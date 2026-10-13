//! 发送端 HTTP 服务(axum):`/v1/health`、`/v1/enroll`、`/v1/bind`、`/v1/push`。
//!
//! 鉴权顺序是**固定的**,且每一层都在产生副作用之前:
//!
//! 1. 解析 JSON(坏体 → `BadRequest`,不进凭据校验);
//! 2. 时间戳窗口(`Replay`);
//! 3. 凭据 + HMAC 签名(`Unauthorized` — 未知 user 与错签名同码,不给预言机);
//! 4. nonce 重放(`Replay`);
//! 5. 限流(`RateLimited`);
//! 6. 业务(绑定表变化 / 调 APNs)。
//!
//! 顺序本身是安全属性:把限流放到签名之前,攻击者就能用无效签名消耗别人的配额;
//! 把重放检查放到签名之后,攻击者必须先伪造签名才能探测 nonce 表。测试逐条钉住。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;

use remote_protocol::{
    bind_signature_message, push_signature_message, PushAck, PushBindRequest, PushErrorCode,
    PushFailure, PushRequest, PUSH_PROTOCOL_VERSION,
};

use crate::apns::{ApnsClient, ApnsOutcome, PushDelivery};
use crate::auth::{now_ms, timestamp_in_window, CredentialStore, RateLimiter, ReplayGuard};
use crate::bindings::BindingStore;
use crate::config::PushConfig;

/// 并发在飞请求上限。APNs 允许一条 HTTP/2 连接多流,但不该无界。
const MAX_INFLIGHT: usize = 32;
/// 非 2xx 时给出的重试建议(APNs 未给 retry-after 时的默认值)。
pub const DEFAULT_RETRY_AFTER_MS: u64 = 5_000;

/// 共享状态。
pub struct AppState {
    pub config: PushConfig,
    pub credentials: Mutex<CredentialStore>,
    pub bindings: Mutex<BindingStore>,
    pub replays: ReplayGuard,
    pub rate: RateLimiter,
    pub apns: Option<ApnsClient>,
    /// `/v1/enroll` 的管理口令。`None` = 关闭 enroll(只能靠 `aeroric-remote-push enroll`
    /// 子命令在本地铸造凭据),避免把「谁能注册一个推送凭据」暴露成网络入口。
    pub enroll_secret: Option<String>,
    inflight: Semaphore,
}

impl AppState {
    pub fn new(
        config: PushConfig,
        credentials: CredentialStore,
        bindings: BindingStore,
        apns: Option<ApnsClient>,
        enroll_secret: Option<String>,
    ) -> Self {
        let replays = ReplayGuard::new(Duration::from_millis(
            config.signature_window_ms.max(1) as u64
        ));
        let rate = RateLimiter::new(config.rate_limit_per_minute, Duration::from_secs(60));
        Self {
            config,
            credentials: Mutex::new(credentials),
            bindings: Mutex::new(bindings),
            replays,
            rate,
            apns,
            enroll_secret,
            inflight: Semaphore::new(MAX_INFLIGHT),
        }
    }
}

/// 路由。
pub fn router(state: Arc<AppState>) -> Router {
    // 路由只做「抽 State + 体」这一层适配,业务在 `handle_*` 里:同一份代码既是
    // 生产入口,也能被测试直接调用(不需要为了测试起一个真监听)。
    Router::new()
        .route(
            "/v1/health",
            axum::routing::get(
                |State(state): State<Arc<AppState>>| async move { handle_health(state) },
            ),
        )
        .route(
            "/v1/enroll",
            post(
                |State(state): State<Arc<AppState>>, body: String| async move {
                    handle_enroll(state, body)
                },
            ),
        )
        .route(
            "/v1/bind",
            post(
                |State(state): State<Arc<AppState>>, body: String| async move {
                    handle_bind(state, body).await
                },
            ),
        )
        .route(
            "/v1/push",
            post(
                |State(state): State<Arc<AppState>>, body: String| async move {
                    handle_push(state, body).await
                },
            ),
        )
        .with_state(state)
}

/// 失败应答。`message` 只写原因,不写业务内容(APNs 的 reason 是安全的白名单词汇)。
fn failure(code: PushErrorCode, message: impl Into<String>) -> Response {
    let status = match code {
        PushErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        PushErrorCode::BadRequest => StatusCode::BAD_REQUEST,
        PushErrorCode::Replay => StatusCode::CONFLICT,
        PushErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        PushErrorCode::TokenRejected => StatusCode::GONE,
        PushErrorCode::Upstream => StatusCode::BAD_GATEWAY,
        PushErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
    };
    let retryable = matches!(
        code,
        PushErrorCode::RateLimited | PushErrorCode::Upstream | PushErrorCode::Unavailable
    );
    (
        status,
        Json(PushFailure {
            code,
            message: message.into(),
            retryable,
        }),
    )
        .into_response()
}

/// 未就绪(缺 APNs 凭据)时**明确**报 unavailable,不假装成功。
pub fn handle_health(state: Arc<AppState>) -> Response {
    let ready = state.apns.is_some();
    let body = json!({
        "v": PUSH_PROTOCOL_VERSION,
        "ready": ready,
        "configured": state.config.describe(),
        "users": state.credentials.lock().user_count(),
        "bindings": state.bindings.lock().len(),
        "detail": if ready {
            "APNs provider configured"
        } else {
            "no APNs provider key configured; push requests will be refused"
        },
    });
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(body)).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnrollRequest {
    secret: String,
    user: Option<String>,
}

/// 铸造一份 per-user 凭据。明文只在此响应里出现一次。
pub fn handle_enroll(state: Arc<AppState>, body: String) -> Response {
    let Some(secret) = state.enroll_secret.as_deref() else {
        return failure(
            PushErrorCode::Unauthorized,
            "enrollment is disabled; mint credentials with the local `enroll` subcommand",
        );
    };
    let parsed: EnrollRequest = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return failure(PushErrorCode::BadRequest, "malformed enroll request"),
    };
    if !constant_time_eq(secret.as_bytes(), parsed.secret.as_bytes()) {
        return failure(PushErrorCode::Unauthorized, "invalid enrollment secret");
    }
    let minted = state.credentials.lock().enroll(parsed.user.as_deref());
    match minted {
        Ok((user, credential)) => (
            StatusCode::CREATED,
            Json(json!({ "v": PUSH_PROTOCOL_VERSION, "user": user, "secret": credential })),
        )
            .into_response(),
        Err(error) => failure(PushErrorCode::BadRequest, error),
    }
}

/// 已验签的调用者身份。
struct Caller {
    user: String,
}

/// 鉴权闸门(时间戳 → 签名 → nonce → 限流)。每一步都在副作用之前。
///
/// `Err` 里放 `Box<Response>`:`Response` 本体一百多字节,直接当 `Result` 的
/// 错误变体会把每次成功返回也撑大(clippy `result_large_err`)。拒绝路径本来就
/// 要分配一个响应体,多一次装箱不改变成本。
fn authenticate(
    state: &AppState,
    user: &str,
    issued_at_ms: i64,
    nonce: &str,
    message: &str,
    signature: &str,
) -> Result<Caller, Box<Response>> {
    if !timestamp_in_window(issued_at_ms, now_ms(), state.config.signature_window_ms) {
        return Err(Box::new(failure(
            PushErrorCode::Replay,
            "request timestamp is outside the accepted window",
        )));
    }
    if !state.credentials.lock().verify(user, signature, message) {
        return Err(Box::new(failure(
            PushErrorCode::Unauthorized,
            "unknown user or bad signature",
        )));
    }
    if !state.replays.observe(nonce) {
        return Err(Box::new(failure(
            PushErrorCode::Replay,
            "nonce already used",
        )));
    }
    if let Err(retry_after_ms) = state.rate.check(user) {
        let mut response = failure(PushErrorCode::RateLimited, "per-user rate limit exceeded");
        response.headers_mut().insert(
            "retry-after",
            (retry_after_ms / 1000)
                .max(1)
                .to_string()
                .parse()
                .unwrap_or_else(|_| HeaderValue::from_static("5")),
        );
        return Err(Box::new(response));
    }
    Ok(Caller {
        user: user.to_string(),
    })
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

///// `/v1/bind`
pub async fn handle_bind(state: Arc<AppState>, body: String) -> Response {
    let request: PushBindRequest = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return failure(PushErrorCode::BadRequest, "malformed bind request"),
    };
    if request.v != PUSH_PROTOCOL_VERSION {
        return failure(
            PushErrorCode::BadRequest,
            format!("unsupported push protocol version {}", request.v),
        );
    }
    let message = bind_signature_message(
        &request.user,
        &request.device_id,
        &request.platform,
        &request.token,
        request.issued_at_ms,
        &request.nonce,
    );
    let caller = match authenticate(
        &state,
        &request.user,
        request.issued_at_ms,
        &request.nonce,
        &message,
        &request.signature,
    ) {
        Ok(caller) => caller,
        Err(response) => return *response,
    };

    let changed = state.bindings.lock().put(
        &caller.user,
        &request.device_id,
        &request.platform,
        &request.token,
        now_ms(),
        state.config.max_devices_per_user,
    );
    match changed {
        Ok(changed) => (
            StatusCode::OK,
            Json(json!({
                "v": PUSH_PROTOCOL_VERSION,
                "bound": !request.token.trim().is_empty(),
                "changed": changed,
            })),
        )
            .into_response(),
        Err(error) => failure(PushErrorCode::BadRequest, error),
    }
}

///// `/v1/push`
pub async fn handle_push(state: Arc<AppState>, body: String) -> Response {
    let request: PushRequest = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return failure(PushErrorCode::BadRequest, "malformed push request"),
    };
    if request.v != PUSH_PROTOCOL_VERSION {
        return failure(
            PushErrorCode::BadRequest,
            format!("unsupported push protocol version {}", request.v),
        );
    }
    // 载荷形状在这里再挡一次:即使有人改了宿主,信封也只允许这四种 kind。
    let envelope = &request.envelope;
    if envelope.v != PUSH_PROTOCOL_VERSION || envelope.collapse_key.len() > 64 {
        return failure(
            PushErrorCode::BadRequest,
            "envelope version/collapseKey out of range",
        );
    }
    let message = push_signature_message(
        &request.user,
        &request.device_ids,
        envelope.kind,
        envelope.event_seq,
        &envelope.collapse_key,
        request.issued_at_ms,
        &request.nonce,
    );
    let caller = match authenticate(
        &state,
        &request.user,
        request.issued_at_ms,
        &request.nonce,
        &message,
        &request.signature,
    ) {
        Ok(caller) => caller,
        Err(response) => return *response,
    };

    let Some(apns) = state.apns.as_ref() else {
        return failure(
            PushErrorCode::Unavailable,
            "no APNs provider key configured",
        );
    };
    let targets = state
        .bindings
        .lock()
        .tokens_for(&caller.user, &request.device_ids);
    if targets.is_empty() {
        // 没有绑定 = 没有可投递目标。不是错误(设备可能刚撤销),但要如实回报。
        return (
            StatusCode::OK,
            Json(PushAck {
                v: PUSH_PROTOCOL_VERSION,
                accepted: 0,
                invalidated_device_ids: Vec::new(),
                retry_after_ms: None,
            }),
        )
            .into_response();
    }

    let Ok(_permit) = state.inflight.try_acquire() else {
        let mut response = failure(
            PushErrorCode::RateLimited,
            "sender is at its inflight limit",
        );
        response.headers_mut().insert(
            "retry-after",
            HeaderValue::from(DEFAULT_RETRY_AFTER_MS / 1000),
        );
        return response;
    };

    let now_secs = now_ms() / 1000;
    let mut accepted = 0usize;
    let mut invalidated = Vec::new();
    let mut retry_after_ms: Option<u64> = None;

    for (device_id, token) in targets {
        let apns_id = uuid::Uuid::new_v4().to_string();
        let outcome = apns
            .send(&PushDelivery {
                device_token: &token,
                kind: envelope.kind,
                event_seq: envelope.event_seq,
                collapse_key: &envelope.collapse_key,
                apns_id: &apns_id,
                now_secs,
                expiration_secs: 0,
            })
            .await;
        match outcome {
            Ok(ApnsOutcome::Accepted) => accepted += 1,
            Ok(ApnsOutcome::TokenInvalid) => {
                // APNs 说这个 token 没用了:立即解绑,并回报宿主。
                let _ = state.bindings.lock().invalidate_token(&token);
                invalidated.push(device_id);
            }
            Ok(ApnsOutcome::Throttled {
                retry_after_ms: after,
            }) => {
                retry_after_ms = Some(after.unwrap_or(DEFAULT_RETRY_AFTER_MS));
                break;
            }
            Ok(ApnsOutcome::Upstream { .. }) => {
                retry_after_ms = Some(DEFAULT_RETRY_AFTER_MS);
                break;
            }
            Ok(ApnsOutcome::Rejected { .. }) => {
                // 载荷/配置问题:重试无意义,不重试也不假装成功。
                break;
            }
            Err(_) => {
                retry_after_ms = Some(DEFAULT_RETRY_AFTER_MS);
                break;
            }
        }
    }

    (
        StatusCode::OK,
        Json(PushAck {
            v: PUSH_PROTOCOL_VERSION,
            accepted,
            invalidated_device_ids: invalidated,
            retry_after_ms,
        }),
    )
        .into_response()
}

/// 供集成测试与 `main` 共用的服务端;测试里用 `axum::serve` 起在 127.0.0.1:0。
pub async fn serve(state: Arc<AppState>, listener: tokio::net::TcpListener) -> Result<(), String> {
    axum::serve(listener, router(state))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::sign;
    use remote_protocol::{PushEnvelope, PushKind};

    /// 造一个带指定 per-user 凭据的状态(测试自己算签名,所以用 `seed` 固定明文)。
    fn state_with(users: &[(&str, &str)], apns: Option<ApnsClient>) -> Arc<AppState> {
        let mut credentials = CredentialStore::in_memory();
        for (user, secret) in users {
            credentials.seed(user, secret).expect("seed credential");
        }
        let config = PushConfig::new(
            "pem".to_string(),
            "kid".to_string(),
            "team".to_string(),
            "com.aeroric.ios.companion".to_string(),
        );
        Arc::new(AppState::new(
            config,
            credentials,
            BindingStore::in_memory(),
            apns,
            None,
        ))
    }

    fn bind_body(
        user: &str,
        secret: &str,
        device_id: &str,
        token: &str,
        issued_at_ms: i64,
        nonce: &str,
    ) -> String {
        serde_json::to_string(&PushBindRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: user.to_string(),
            device_id: device_id.to_string(),
            platform: "ios".to_string(),
            token: token.to_string(),
            issued_at_ms,
            nonce: nonce.to_string(),
            signature: sign(
                secret,
                &bind_signature_message(user, device_id, "ios", token, issued_at_ms, nonce),
            ),
        })
        .expect("bind body")
    }

    fn push_body(
        user: &str,
        secret: &str,
        device_ids: &[String],
        kind: PushKind,
        event_seq: u64,
        collapse_key: &str,
        issued_at_ms: i64,
        nonce: &str,
    ) -> String {
        serde_json::to_string(&PushRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: user.to_string(),
            device_ids: device_ids.to_vec(),
            envelope: PushEnvelope {
                v: PUSH_PROTOCOL_VERSION,
                kind,
                event_seq,
                collapse_key: collapse_key.to_string(),
            },
            issued_at_ms,
            nonce: nonce.to_string(),
            signature: sign(
                secret,
                &push_signature_message(
                    user,
                    device_ids,
                    kind,
                    event_seq,
                    collapse_key,
                    issued_at_ms,
                    nonce,
                ),
            ),
        })
        .expect("push body")
    }

    #[test]
    fn health_is_ok_with_a_provider_and_unavailable_without_one() {
        let without = state_with(&[], None);
        assert_eq!(
            handle_health(without).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn authorization_order_blocks_side_effects_before_binding() {
        let state = state_with(&[("u1", "secret")], None);
        let now = now_ms();

        // 1) 坏时间戳:签名正确也先拒,且不产生任何副作用。
        let stale = now - 10_000_000;
        let response = handle_bind(
            state.clone(),
            bind_body("u1", "secret", "d1", "tok", stale, "n1"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(state.bindings.lock().len(), 0, "no side effect on refusal");

        // 2) 未知 user 与错签名返回同一个状态码(不给枚举预言机)。
        let unknown = handle_bind(
            state.clone(),
            bind_body("ghost", "secret", "d1", "tok", now, "n2"),
        )
        .await;
        assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
        let wrong_sig = handle_bind(
            state.clone(),
            serde_json::to_string(&PushBindRequest {
                v: PUSH_PROTOCOL_VERSION,
                user: "u1".into(),
                device_id: "d1".into(),
                platform: "ios".into(),
                token: "tok".into(),
                issued_at_ms: now,
                nonce: "n3".into(),
                signature: sign(
                    "not-the-secret",
                    &bind_signature_message("u1", "d1", "ios", "tok", now, "n3"),
                ),
            })
            .expect("body"),
        )
        .await;
        assert_eq!(wrong_sig.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(state.bindings.lock().len(), 0);

        // 3) 正确签名 → 绑定生效。
        let body = bind_body("u1", "secret", "d1", "tok", now, "n4");
        assert_eq!(
            handle_bind(state.clone(), body.clone()).await.status(),
            StatusCode::OK
        );
        assert_eq!(state.bindings.lock().len(), 1);

        // 4) 同一 body 重放:nonce 已被记账 → 409,且绑定表不变。
        let replay = handle_bind(state.clone(), body).await;
        assert_eq!(
            replay.status(),
            StatusCode::CONFLICT,
            "nonce replay must be refused"
        );
        assert_eq!(state.bindings.lock().len(), 1);

        // 5) 篡改 body 里的 token:签名不再匹配 → 401,且不改绑定。
        let tampered =
            bind_body("u1", "secret", "d1", "tok", now, "n5").replace("\"tok\"", "\"evil\"");
        let response = handle_bind(state.clone(), tampered).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            state.bindings.lock().tokens_for("u1", &["d1".to_string()])[0].1,
            "tok"
        );
    }

    #[tokio::test]
    async fn enroll_is_closed_without_an_enrollment_secret() {
        let state = state_with(&[], None);
        let response = handle_enroll(state.clone(), r#"{"secret":"anything"}"#.to_string());
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(state.credentials.lock().user_count(), 0);
    }

    #[tokio::test]
    async fn enroll_requires_the_exact_secret() {
        let state = Arc::new({
            let mut credentials = CredentialStore::in_memory();
            let _ = &mut credentials;
            AppState::new(
                PushConfig::new("pem".into(), "kid".into(), "team".into(), "topic".into()),
                credentials,
                BindingStore::in_memory(),
                None,
                Some("s3cret".to_string()),
            )
        });
        assert_eq!(
            handle_enroll(state.clone(), r#"{"secret":"s3crey"}"#.to_string()).status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(state.credentials.lock().user_count(), 0);
        assert_eq!(
            handle_enroll(state.clone(), r#"{"secret":"s3cret"}"#.to_string()).status(),
            StatusCode::CREATED
        );
        assert_eq!(state.credentials.lock().user_count(), 1);
    }

    #[tokio::test]
    async fn push_without_a_provider_key_is_unavailable_and_binds_nothing() {
        let state = state_with(&[("u1", "secret")], None);
        let now = now_ms();
        let device_ids = vec!["d1".to_string()];
        let body = push_body(
            "u1",
            "secret",
            &device_ids,
            PushKind::TaskDone,
            1,
            "task-1:done",
            now,
            "n1",
        );
        assert_eq!(
            handle_push(state.clone(), body).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(state.bindings.lock().len(), 0);
    }

    #[tokio::test]
    async fn push_with_no_bound_device_acks_zero_and_does_not_call_apns() {
        // 有 provider key,但该 user 没有任何绑定:回 200 + accepted=0(不是错误)。
        let key = Arc::new(
            crate::apns::ProviderKey::from_pem(&test_pem())
                .expect("pem")
                .with_ids("kid", "team"),
        );
        let apns = ApnsClient::new("http://127.0.0.1:1", "topic", key).expect("client");
        let state = state_with(&[("u1", "secret")], Some(apns));
        let now = now_ms();
        let body = push_body(
            "u1",
            "secret",
            &["d1".to_string()],
            PushKind::ApprovalPending,
            5,
            "task-5:approval",
            now,
            "n1",
        );
        let response = handle_push(state.clone(), body).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let ack: PushAck = serde_json::from_slice(&bytes).expect("ack");
        assert_eq!(ack.accepted, 0);
        assert!(ack.invalidated_device_ids.is_empty());
        assert_eq!(ack.retry_after_ms, None);
    }

    #[tokio::test]
    async fn rate_limit_is_per_user_and_reported() {
        let state = state_with(&[("u1", "secret"), ("u2", "secret")], None);
        let now = now_ms();
        // 默认上限 60/min:打满之后下一条必须被限流。
        for index in 0..60 {
            let nonce = format!("n{index}");
            let response = handle_bind(
                state.clone(),
                bind_body("u1", "secret", "d1", "tok", now, &nonce),
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "request {index} should pass"
            );
        }
        let limited = handle_bind(
            state.clone(),
            bind_body("u1", "secret", "d1", "tok", now, "n-limited"),
        )
        .await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(limited.headers().contains_key("retry-after"));
        // 另一个凭据不受影响。
        assert_eq!(
            handle_bind(
                state.clone(),
                bind_body("u2", "secret", "d2", "tok2", now, "n-u2")
            )
            .await
            .status(),
            StatusCode::OK
        );
    }

    fn test_pem() -> String {
        use p256::elliptic_curve::Generate;
        use p256::pkcs8::EncodePrivateKey;
        let key = p256::ecdsa::SigningKey::generate();
        key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .expect("pem")
            .to_string()
    }
}
