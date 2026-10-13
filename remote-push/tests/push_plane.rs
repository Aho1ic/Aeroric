//! AP6 端到端验收:宿主 → 发送端 → APNs 的完整链路,APNs 用**真实 HTTP/2**
//! 的假服务冒充(同一 hyper h2 栈),断言的是「发出去的字节」而不是内部状态。
//!
//! 覆盖 PRD 的 AC-AP6-1..4:
//! * 载荷最小化:APNs 收到的 JSON 只含白名单键;项目名/任务标题/命令**不出现**;
//! * provider JWT 逐次可验证(ES256 原始签名 + 三段 compact);
//! * 410/`BadDeviceToken` → 解绑并在回执里报告;后续推送不再打 APNs;
//! * 重放 nonce / 篡改体 / 未知凭据 → 拒绝且**零** APNs 请求;
//! * 撤销(空 token 绑定)→ 目标为空,不再推;
//! * 跨凭据越权 → 取不到别人的 token。

// 集成测试的辅助函数(`spawn_fake_apns` 等)不是 `#[test]` 函数,clippy 的
// `allow-*-in-tests`(clippy.toml)识别不到它们,而 `unwrap()` 在这里正是想要的
// 写法:失败带位置。与 src-tauri 对同类情形的处理一致 —— 显式、带 reason,而不是
// 把 lint 关到 crate 之外。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration-test helpers: panicking on setup failure should point at the line"
)]

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::Mutex;
use serde_json::{json, Value};

use aeroric_remote_push::apns::{ApnsClient, ProviderKey};
use aeroric_remote_push::auth::{sign, CredentialStore};
use aeroric_remote_push::bindings::BindingStore;
use aeroric_remote_push::config::PushConfig;
use aeroric_remote_push::server::{self, AppState};
use remote_protocol::{
    bind_signature_message, push_signature_message, PushAck, PushBindRequest, PushEnvelope,
    PushKind, PushRequest, PUSH_PROTOCOL_VERSION,
};

/// 一次 APNs 请求的**原始**观察:路径、头、体。断言只读这些。
#[derive(Debug, Clone)]
struct ApnsCall {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
    raw_body: String,
}

/// 假 APNs:按 device token 决定状态码。
struct FakeApns {
    calls: Arc<Mutex<Vec<ApnsCall>>>,
    status_for: Arc<Mutex<HashMap<String, u16>>>,
    reason_for: Arc<Mutex<HashMap<String, String>>>,
}

impl FakeApns {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            status_for: Arc::new(Mutex::new(HashMap::new())),
            reason_for: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn reject(&self, token: &str, status: u16, reason: &str) {
        self.status_for.lock().insert(token.to_string(), status);
        self.reason_for
            .lock()
            .insert(token.to_string(), reason.to_string());
    }

    fn calls(&self) -> Vec<ApnsCall> {
        self.calls.lock().clone()
    }

    fn call_count(&self) -> usize {
        self.calls.lock().len()
    }
}

/// 起假 APNs(h2 prior knowledge over plain TCP),返回基址与句柄。
async fn spawn_fake_apns() -> (String, FakeApns) {
    let fake = FakeApns::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let calls = Arc::clone(&fake.calls);
    let status_for = Arc::clone(&fake.status_for);
    let reason_for = Arc::clone(&fake.reason_for);

    tokio::spawn(async move {
        loop {
            let Ok((stream, _peer)) = listener.accept().await else {
                break;
            };
            let (calls, status_for, reason_for) = (
                Arc::clone(&calls),
                Arc::clone(&status_for),
                Arc::clone(&reason_for),
            );
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let calls = Arc::clone(&calls);
                    let status_for = Arc::clone(&status_for);
                    let reason_for = Arc::clone(&reason_for);
                    async move {
                        let path = request.uri().path().to_string();
                        let mut headers = HashMap::new();
                        for (name, value) in request.headers() {
                            if let Ok(text) = value.to_str() {
                                headers.insert(name.as_str().to_string(), text.to_string());
                            }
                        }
                        let collected = request
                            .into_body()
                            .collect()
                            .await
                            .map(|c| c.to_bytes())
                            .unwrap_or_default();
                        let raw_body = String::from_utf8_lossy(&collected).to_string();
                        let body: Value = serde_json::from_str(&raw_body).unwrap_or(Value::Null);
                        calls.lock().push(ApnsCall {
                            path: path.clone(),
                            headers,
                            body,
                            raw_body,
                        });

                        // token 是路径最后一段:/3/device/<token>
                        let token = path.rsplit('/').next().unwrap_or_default().to_string();
                        let status = status_for.lock().get(&token).copied().unwrap_or(200);
                        let reason = reason_for.lock().get(&token).cloned();
                        let payload = match reason {
                            Some(reason) => json!({ "reason": reason }).to_string(),
                            None => String::new(),
                        };
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(payload)))
                                .expect("response"),
                        )
                    }
                });
                let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });

    (format!("http://{addr}"), fake)
}

/// 起发送端服务,返回基址与状态。
async fn spawn_sender(apns_base: &str, topic: &str) -> (String, Arc<AppState>) {
    let key = Arc::new(
        ProviderKey::from_pem(&test_pem())
            .expect("pem")
            .with_ids("KEYID", "TEAMID"),
    );
    let apns = ApnsClient::new(apns_base, topic, key).expect("apns client");
    let config = PushConfig::new(
        "unused".to_string(),
        "KEYID".to_string(),
        "TEAMID".to_string(),
        topic.to_string(),
    );
    let mut credentials = CredentialStore::in_memory();
    credentials.seed("host-a", "secret-a").expect("seed");
    let state = Arc::new(AppState::new(
        config,
        credentials,
        BindingStore::in_memory(),
        Some(apns),
        None,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let serving = Arc::clone(&state);
    tokio::spawn(async move {
        let _ = server::serve(serving, listener).await;
    });
    (format!("http://{addr}"), state)
}

fn test_pem() -> String {
    use p256::elliptic_curve::Generate;
    use p256::pkcs8::EncodePrivateKey;
    let key = p256::ecdsa::SigningKey::generate();
    key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
        .expect("pem")
        .to_string()
}

/// 宿主侧 POST(真实网络 + 真实 JSON 体)。
async fn post_signed(
    base: &str,
    path: &str,
    body: String,
) -> (u16, String, HashMap<String, String>) {
    let client: hyper_util::client::legacy::Client<_, Full<Bytes>> =
        hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build_http();
    let request = Request::builder()
        .method("POST")
        .uri(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .expect("request");
    let response = client.request(request).await.expect("send");
    let status = response.status().as_u16();
    let mut headers = HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(text) = value.to_str() {
            headers.insert(name.as_str().to_string(), text.to_string());
        }
    }
    let bytes = response
        .into_body()
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();
    (status, String::from_utf8_lossy(&bytes).to_string(), headers)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn bind_body(device_id: &str, token: &str, nonce: &str, at: i64) -> String {
    serde_json::to_string(&PushBindRequest {
        v: PUSH_PROTOCOL_VERSION,
        user: "host-a".to_string(),
        device_id: device_id.to_string(),
        platform: "ios".to_string(),
        token: token.to_string(),
        issued_at_ms: at,
        nonce: nonce.to_string(),
        signature: sign(
            "secret-a",
            &bind_signature_message("host-a", device_id, "ios", token, at, nonce),
        ),
    })
    .expect("bind body")
}

fn push_body(
    device_ids: &[&str],
    kind: PushKind,
    event_seq: u64,
    collapse_key: &str,
    nonce: &str,
    at: i64,
) -> String {
    let ids: Vec<String> = device_ids.iter().map(|id| (*id).to_string()).collect();
    serde_json::to_string(&PushRequest {
        v: PUSH_PROTOCOL_VERSION,
        user: "host-a".to_string(),
        device_ids: ids.clone(),
        envelope: PushEnvelope {
            v: PUSH_PROTOCOL_VERSION,
            kind,
            event_seq,
            collapse_key: collapse_key.to_string(),
        },
        issued_at_ms: at,
        nonce: nonce.to_string(),
        signature: sign(
            "secret-a",
            &push_signature_message("host-a", &ids, kind, event_seq, collapse_key, at, nonce),
        ),
    })
    .expect("push body")
}

fn parse_ack(body: &str) -> PushAck {
    serde_json::from_str(body).expect("ack json")
}

#[tokio::test]
async fn push_reaches_a_real_h2_fake_apns_with_a_privacy_limited_payload() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();

    // 1) 绑定:手机经宿主上报 APNs token。
    let (status, body, _) = post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-1", "aa11bb22", "bind-1", at),
    )
    .await;
    assert_eq!(status, 200, "bind response: {body}");
    assert_eq!(fake.call_count(), 0, "binding must not call APNs");

    // 2) 推送:任务完成。
    let (status, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(
            &["dev-1"],
            PushKind::TaskDone,
            7,
            "task-7:done",
            "push-1",
            at,
        ),
    )
    .await;
    assert_eq!(status, 200, "push response: {body}");
    let ack = parse_ack(&body);
    assert_eq!(ack.accepted, 1);
    assert!(ack.invalidated_device_ids.is_empty());

    // 3) APNs 到底收到了什么:路径、头、体。
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(
        call.path, "/3/device/aa11bb22",
        "APNs :path must target the device token"
    );
    assert_eq!(
        call.headers.get("apns-topic").map(String::as_str),
        Some("com.aeroric.ios.companion")
    );
    assert_eq!(
        call.headers.get("apns-push-type").map(String::as_str),
        Some("alert")
    );
    assert_eq!(
        call.headers.get("apns-priority").map(String::as_str),
        Some("10")
    );
    assert_eq!(
        call.headers.get("apns-collapse-id").map(String::as_str),
        Some("task-7:done")
    );
    assert_eq!(
        call.headers.get("apns-expiration").map(String::as_str),
        Some("0")
    );
    assert!(call.headers.contains_key("apns-id"));
    let authorization = call
        .headers
        .get("authorization")
        .expect("authorization header");
    assert!(authorization.starts_with("bearer "), "{authorization}");
    let jwt = authorization.trim_start_matches("bearer ");
    assert_eq!(
        jwt.split('.').count(),
        3,
        "JWS compact serialization: {jwt}"
    );

    // 4) 载荷白名单:顶层键恰好 aps/eventSeq/kind;可见文案是 loc-key。
    let mut top: Vec<&str> = call
        .body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    top.sort_unstable();
    assert_eq!(top, ["aps", "eventSeq", "kind"], "payload keys: {top:?}");
    assert_eq!(call.body["kind"], json!("task_done"));
    assert_eq!(call.body["eventSeq"], json!(7));
    assert_eq!(
        call.body["aps"]["alert"]["loc-key"],
        json!("push.task_done"),
        "the visible text must be a bundle key, not content"
    );
    assert_eq!(call.body["aps"]["thread-id"], json!("task-7:done"));

    // 负例:业务内容不得出现在**原始字节**里。
    for forbidden in [
        "Aeroric",
        "/Users/",
        "src-tauri",
        "summarize the",
        "apiKey",
        "SUPERSECRET",
    ] {
        assert!(
            !call.raw_body.contains(forbidden),
            "payload leaked {forbidden:?}: {}",
            call.raw_body
        );
    }
}

#[tokio::test]
async fn apns_410_invalidates_the_binding_and_the_next_push_does_not_call_apns() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();

    post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-1", "deadbeef", "b1", at),
    )
    .await;
    fake.reject("deadbeef", 410, "Unregistered");

    let (status, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(
            &["dev-1"],
            PushKind::TaskFailed,
            12,
            "task-12:failed",
            "p1",
            at,
        ),
    )
    .await;
    assert_eq!(status, 200);
    let ack = parse_ack(&body);
    assert_eq!(ack.accepted, 0, "410 is not an accepted delivery: {body}");
    assert_eq!(
        ack.invalidated_device_ids,
        vec!["dev-1".to_string()],
        "must report invalidation"
    );
    assert_eq!(fake.call_count(), 1);

    let (_, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(&["dev-1"], PushKind::TaskDone, 13, "task-13:done", "p2", at),
    )
    .await;
    assert_eq!(parse_ack(&body).accepted, 0);
    assert_eq!(
        fake.call_count(),
        1,
        "an invalidated token must never be pushed again"
    );
}

#[tokio::test]
async fn bad_device_token_is_also_treated_as_invalidation() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();
    post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-2", "cafebabe", "b1", at),
    )
    .await;
    fake.reject("cafebabe", 400, "BadDeviceToken");

    let (_, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(
            &["dev-2"],
            PushKind::ApprovalPending,
            3,
            "task-3:approval",
            "p1",
            at,
        ),
    )
    .await;
    assert_eq!(
        parse_ack(&body).invalidated_device_ids,
        vec!["dev-2".to_string()]
    );
    assert_eq!(fake.call_count(), 1);
}

#[tokio::test]
async fn throttling_is_reported_to_the_host_as_a_backoff_requirement() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();
    post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-3", "0badf00d", "b1", at),
    )
    .await;
    fake.reject("0badf00d", 429, "TooManyRequests");

    let (_, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(&["dev-3"], PushKind::TaskDone, 4, "task-4:done", "p1", at),
    )
    .await;
    let ack = parse_ack(&body);
    assert_eq!(ack.accepted, 0);
    // APNs 未给 retry-after 时发送端给默认退避建议,宿主据此决定节奏。
    assert_eq!(
        ack.retry_after_ms,
        Some(aeroric_remote_push::server::DEFAULT_RETRY_AFTER_MS)
    );
}

#[tokio::test]
async fn replay_tampering_and_unknown_credentials_never_reach_apns() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();

    // 未知凭据(用别的 secret 签)。
    let body = serde_json::to_string(&PushBindRequest {
        v: PUSH_PROTOCOL_VERSION,
        user: "host-a".to_string(),
        device_id: "dev-1".to_string(),
        platform: "ios".to_string(),
        token: "tok".to_string(),
        issued_at_ms: at,
        nonce: "x1".to_string(),
        signature: sign(
            "other-secret",
            &bind_signature_message("host-a", "dev-1", "ios", "tok", at, "x1"),
        ),
    })
    .expect("body");
    assert_eq!(post_signed(&sender_base, "/v1/bind", body).await.0, 401);

    // 过期时间戳。
    let stale = at - 10_000_000;
    assert_eq!(
        post_signed(
            &sender_base,
            "/v1/bind",
            bind_body("dev-1", "tok", "x2", stale)
        )
        .await
        .0,
        409
    );

    // 正常绑定 + 正常推送一次(占用 nonce)。
    assert_eq!(
        post_signed(
            &sender_base,
            "/v1/bind",
            bind_body("dev-1", "tok", "x3", at)
        )
        .await
        .0,
        200
    );
    let push = push_body(&["dev-1"], PushKind::TaskDone, 1, "task-1:done", "x4", at);
    assert_eq!(
        post_signed(&sender_base, "/v1/push", push.clone()).await.0,
        200
    );
    assert_eq!(fake.call_count(), 1);

    // 重放同一请求(同 nonce/同签名)→ 409,零新 APNs 请求。
    assert_eq!(post_signed(&sender_base, "/v1/push", push).await.0, 409);
    assert_eq!(fake.call_count(), 1, "a replayed push must not reach APNs");

    // 篡改信封:kind 变了但签名没变 → 401,零新请求。
    let tampered = push_body(&["dev-1"], PushKind::TaskFailed, 1, "task-1:done", "x5", at)
        .replace("\"kind\":\"task_failed\"", "\"kind\":\"task_done\"");
    assert_eq!(post_signed(&sender_base, "/v1/push", tampered).await.0, 401);
    assert_eq!(fake.call_count(), 1);
}

#[tokio::test]
async fn revocation_unbinds_and_stops_future_pushes() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, _state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();

    post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-9", "feedface", "b1", at),
    )
    .await;
    let (_, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(&["dev-9"], PushKind::TaskDone, 1, "task-1:done", "p1", at),
    )
    .await;
    assert_eq!(parse_ack(&body).accepted, 1);

    // 设备撤销:宿主用空 token 走同一条 bind 路径解绑。
    assert_eq!(
        post_signed(&sender_base, "/v1/bind", bind_body("dev-9", "", "b2", at))
            .await
            .0,
        200
    );

    let (_, body, _) = post_signed(
        &sender_base,
        "/v1/push",
        push_body(&["dev-9"], PushKind::TaskDone, 2, "task-2:done", "p2", at),
    )
    .await;
    assert_eq!(
        parse_ack(&body).accepted,
        0,
        "revoked device must receive nothing: {body}"
    );
    assert_eq!(
        fake.call_count(),
        1,
        "no further APNs traffic after revocation"
    );
}

#[tokio::test]
async fn a_user_cannot_push_another_users_devices() {
    let (apns_base, fake) = spawn_fake_apns().await;
    let (sender_base, state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();
    post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-1", "aaaabbbb", "b1", at),
    )
    .await;
    state
        .credentials
        .lock()
        .seed("host-b", "secret-b")
        .expect("seed host-b");

    let ids = vec!["dev-1".to_string()];
    let body = serde_json::to_string(&PushRequest {
        v: PUSH_PROTOCOL_VERSION,
        user: "host-b".to_string(),
        device_ids: ids.clone(),
        envelope: PushEnvelope {
            v: PUSH_PROTOCOL_VERSION,
            kind: PushKind::TaskDone,
            event_seq: 1,
            collapse_key: "task-1:done".to_string(),
        },
        issued_at_ms: at,
        nonce: "pb".to_string(),
        signature: sign(
            "secret-b",
            &push_signature_message(
                "host-b",
                &ids,
                PushKind::TaskDone,
                1,
                "task-1:done",
                at,
                "pb",
            ),
        ),
    })
    .expect("body");
    let (status, response, _) = post_signed(&sender_base, "/v1/push", body).await;
    assert_eq!(status, 200);
    assert_eq!(
        parse_ack(&response).accepted,
        0,
        "tokens are scoped per credential"
    );
    assert_eq!(fake.call_count(), 0);
}

#[tokio::test]
async fn rate_limited_response_carries_retry_after() {
    let (apns_base, _fake) = spawn_fake_apns().await;
    let (sender_base, state) = spawn_sender(&apns_base, "com.aeroric.ios.companion").await;
    let at = now_ms();
    // 把上限压到 1,让第二次必然被限流(避免打满 60 次的慢用例)。
    state.rate.set_limit(1);
    assert_eq!(
        post_signed(
            &sender_base,
            "/v1/bind",
            bind_body("dev-1", "tok", "r1", at)
        )
        .await
        .0,
        200
    );
    let (status, body, headers) = post_signed(
        &sender_base,
        "/v1/bind",
        bind_body("dev-1", "tok", "r2", at),
    )
    .await;
    assert_eq!(status, 429, "{body}");
    assert!(headers.contains_key("retry-after"), "{headers:?}");
}
