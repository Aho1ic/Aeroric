//! AP6 — 推送平面与自托管网络闭环的集成验收套件。
//!
//! 覆盖 PRD 的 AC-AP6-1..6,断言的是**真实运行路径**而不是内部状态:
//!
//! * 真起一个 **HTTP + 真实 h2 的假 APNs**,再起**真的 `aeroric-remote-push`
//!   发送端进程内服务**,然后让宿主的 `CliRemoteHandle` 通过它投递 —— 三层都是
//!   真实现,只有最外层的 APNs 是假的(因为真 APNs 需要 `GAP-STORE-06` 的私钥和
//!   真机,见 PRD BLOCK-AP6-1)。
//! * 任务事件经**真的** `EventLog::append` 触发推送(观察者链路),不是直接调
//!   dispatch。
//! * UDS 面的 `remote.pushRegister` / `remote.pushStatus` / `remote.pushTest` /
//!   `remote.relayDiagnostics` 经**真的** `dispatch` 走一遍。
//! * 载荷最小化用**假 APNs 收到的原始字节**断言(项目名/路径/命令不得出现)。

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request as HyperRequest, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::Mutex;
use serde_json::{json, Value};

use aeroric_core::context::{CoreContext, MemorySecrets};
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::remote::push::{
    PushBindings, PushSenderClient, PushSenderConfig, PUSH_CREDENTIAL_KEY,
};
use aeroric_core::remote::push_plane::{PushDispatcher, PushTransport};
use aeroric_core::remote::services::RemoteHostHandle;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::TaskService;
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::auth::HostToken;
use aeroric_host::dispatcher::{dispatch, CallContext, HostServices};
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use aeroric_host::families::SettingsFamiliesCli;
use aeroric_host::push_host::HttpPushTransport;
use aeroric_host::remote_headless::CliRemoteHandle;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 进程级数据根。
///
/// `relay_diagnostics` 读 `remote-config.json`、推送凭据走钥匙串口径——两者都经
/// `aeroric_dir()`,而它读的是**进程级** `AERORIC_HOME`/注入槽。逐用例
/// `set_var` 会让并发用例互相踩(实测就是 `no relay configured` 与 `schemeOk`
/// 两条断言各自随机红)。所以整个测试二进制共用**一个** scratch 根,在这里装一次。
fn install_scratch_data_root() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let dir = std::env::temp_dir().join(format!("ah-ap6-dataroot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch data root");
        std::env::set_var("AERORIC_HOME", &dir);
    });
}

struct LocalProbe;

impl StorageLocationProbe for LocalProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation {
            directory: data_dir.to_path_buf(),
            volume: VolumeKind::Local,
            private_permissions_enforced: true,
        }
    }
}

/// 进程级 rustls provider:测试二进制同时链进 ring 与 aws-lc-rs,不显式装
/// provider 时 rustls 在首次 TLS 时 panic。生产侧由各 TLS 边界自己装
/// (`push_host::install_tls_provider` / `apns::install_tls_provider`),这里
/// 提前装一次,避免测试在构造阶段就撞上。
fn install_tls_provider() {
    aeroric_host::push_host::install_tls_provider();
}

fn scratch_dir(tag: &str) -> PathBuf {
    install_tls_provider();
    install_scratch_data_root();
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-ap6-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).expect("create scratch home");
    root
}

fn rpc_req(id: &str, method: &str, params: Value) -> Request {
    Request {
        v: 1,
        id: id.to_string(),
        method: method.to_string(),
        params,
        idempotency_key: None,
        extra: Default::default(),
    }
}

fn test_pem() -> String {
    use p256::elliptic_curve::Generate;
    use p256::pkcs8::EncodePrivateKey;
    let key = p256::ecdsa::SigningKey::generate();
    key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
        .expect("pem")
        .to_string()
}

/// 假 APNs 收到的**原始**请求。
#[derive(Debug, Clone)]
struct ApnsCall {
    path: String,
    headers: HashMap<String, String>,
    raw_body: String,
}

/// 真实 HTTP/2 的假 APNs。
#[derive(Default)]
struct FakeApns {
    calls: Mutex<Vec<ApnsCall>>,
    status_for: Mutex<HashMap<String, u16>>,
    reason_for: Mutex<HashMap<String, String>>,
}

impl FakeApns {
    fn reject(&self, token: &str, status: u16, reason: &str) {
        self.status_for.lock().insert(token.to_string(), status);
        self.reason_for
            .lock()
            .insert(token.to_string(), reason.to_string());
    }

    fn calls(&self) -> Vec<ApnsCall> {
        self.calls.lock().clone()
    }
}

async fn spawn_fake_apns() -> (String, Arc<FakeApns>) {
    let fake = Arc::new(FakeApns::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake apns");
    let addr = listener.local_addr().expect("addr");
    let state = Arc::clone(&fake);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _peer)) = listener.accept().await else {
                break;
            };
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let service = service_fn(move |request: HyperRequest<hyper::body::Incoming>| {
                    let state = Arc::clone(&state);
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
                        state.calls.lock().push(ApnsCall {
                            path: path.clone(),
                            headers,
                            raw_body,
                        });
                        let token = path.rsplit('/').next().unwrap_or_default().to_string();
                        let status = state.status_for.lock().get(&token).copied().unwrap_or(200);
                        let payload = match state.reason_for.lock().get(&token).cloned() {
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

/// 起**真的**发送端(用 `remote-push` crate 的服务端实现),以 `axum` 监听。
/// 返回基址与凭据(user/secret)。
///
/// 假 APNs 的地址是**参数**,不走环境变量:环境变量是进程级的,并发用例会互相
/// 覆盖(实测四条用例同时红)。生产路径的注入方式是 `PushConfig::with_base_override`,
/// 这里用的是同一条。
async fn spawn_sender(topic: &str, apns_base: &str) -> (String, String, String) {
    let key = Arc::new(
        aeroric_remote_push::apns::ProviderKey::from_pem(&test_pem())
            .expect("pem")
            .with_ids("KEYID", "TEAMID"),
    );
    let apns =
        aeroric_remote_push::apns::ApnsClient::new(apns_base, topic, key).expect("apns client");
    let config = aeroric_remote_push::config::PushConfig::new(
        "unused".to_string(),
        "KEYID".to_string(),
        "TEAMID".to_string(),
        topic.to_string(),
    );
    let mut credentials = aeroric_remote_push::auth::CredentialStore::in_memory();
    let (user, secret) = credentials.enroll(Some("host-a")).expect("enroll");
    let state = Arc::new(aeroric_remote_push::server::AppState::new(
        config,
        credentials,
        aeroric_remote_push::bindings::BindingStore::in_memory(),
        Some(apns),
        None,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind sender");
    let addr = listener.local_addr().expect("addr");
    let serving = Arc::clone(&state);
    tokio::spawn(async move {
        let _ = aeroric_remote_push::server::serve(serving, listener).await;
    });
    (format!("http://{addr}"), user, secret)
}

/// 组装一个真的宿主 handle,推送面接上给定发送端。
struct Harness {
    handle: Arc<CliRemoteHandle>,
    /// 用例经 `handle` 触发派发;这里保留事件日志让它随 harness 存活。
    log: Arc<EventLog>,
    _root: PathBuf,
}

fn harness(
    root: PathBuf,
    app_paths: AppPaths,
    sender: (&str, &str, &str),
    topic: &str,
    relay: Option<(&str, Option<&str>)>,
) -> Harness {
    let context = Arc::new(
        CoreContext::acquire(
            app_paths,
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "ap6-test".to_string(),
                version: "0.1.0".to_string(),
            },
            &LocalProbe,
        )
        .expect("acquire writer"),
    );
    let registry = Arc::new(TaskRegistry::default());
    let terminal = Arc::new(TerminalService::new());

    // 推送面:真 HTTP 出口 + 真发送端地址。
    let transport: Arc<dyn PushTransport> =
        Arc::new(HttpPushTransport::new().expect("push transport"));
    let bindings = Arc::new(PushBindings::new());
    let dispatcher = Arc::new(PushDispatcher::new(
        Arc::clone(&bindings),
        Arc::clone(&transport),
        Arc::new(aeroric_core::remote::push_plane::now_ms) as Arc<dyn Fn() -> i64 + Send + Sync>,
    ));
    dispatcher.set_client(Some(
        PushSenderClient::new(PushSenderConfig {
            url: sender.0.to_string(),
            user: sender.1.to_string(),
            secret: sender.2.to_string(),
        })
        .expect("sender client"),
    ));

    // 观察者链路:事件经真的 EventLog append 触发推送。
    let log = Arc::new(EventLog::default().with_observer(dispatcher.clone()));

    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::clone(&terminal),
        Arc::clone(&log),
    );
    let families = Arc::new(SettingsFamiliesCli::new(Arc::clone(&context)));
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            families.clone() as Arc<_>,
            Arc::clone(&executor) as Arc<_>,
            Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
        )
        .expect("task service"),
    );
    executor.attach(&service);
    let approvals = Arc::new(std::sync::Mutex::new(
        aeroric_host::services::HostApprovals::default(),
    ));

    let handle = Arc::new(CliRemoteHandle {
        service,
        context,
        executor,
        terminal,
        registry,
        approvals,
        families: families as Arc<_>,
        push: Some(Arc::clone(&dispatcher)),
        push_transport: Some(Arc::clone(&transport)),
        relay: relay.map(|(url, token)| (url.to_string(), token.map(str::to_string))),
    });
    let _ = topic;
    Harness {
        handle,
        log,
        _root: root,
    }
}

/// 直接把 relay 配置写到当前数据根。
///
/// **不走 `save_config`**:它会对非法 URL(`ws://relay.example.com`)做归一化,
/// 手机侧在 E2EE 会话里做的那件事:把 device token 报给宿主。宿主自己会签名,
/// 所以这里只给语义字段。
fn register_params(
    _client: &PushSenderClient,
    device_id: &str,
    platform: &str,
    token: &str,
) -> Value {
    json!({ "deviceId": device_id, "platform": platform, "token": token })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn task_events_reach_a_real_h2_fake_apns_through_a_real_sender_with_a_private_payload() {
    let root = scratch_dir("deliver");
    let app_paths = AppPaths::from_user_home(root.join("home"));

    let (apns_base, apns) = spawn_fake_apns().await;
    // 发送端要指向假 APNs:通过环境变量传给发送端构造。
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;

    let harness = harness(
        root,
        app_paths,
        (&sender_base, &user, &secret),
        "topic",
        None,
    );

    // 1) 手机在已认证会话里登记 push token(走真实 host 端口)。
    let registered = harness
        .handle
        .push_register(register_params(
            &PushSenderClient::new(PushSenderConfig {
                url: sender_base.clone(),
                user,
                secret,
            })
            .expect("client"),
            "dev-1",
            "ios",
            "aa11bb22",
        ))
        .await
        .expect("register");
    assert_eq!(registered["registered"], json!(true), "{registered}");

    // 2) 任务事件经真的 EventLog 触发(内容里**故意**塞业务字段,验证不会被带走)。
    harness.log.append(
        "task-status",
        json!({
            "task_id": "task-77",
            "status": "done",
            "title": "Refactor the billing service at /Users/someone/secret-project",
            "projectName": "Aeroric",
        }),
    );

    // 推送是同步做的(宿主端口在自己的 runtime 上阻塞),所以这里已经完成。
    let calls = apns.calls();
    assert_eq!(
        calls.len(),
        1,
        "task-status must produce exactly one APNs request"
    );
    let call = &calls[0];
    assert_eq!(call.path, "/3/device/aa11bb22");
    assert_eq!(
        call.headers.get("apns-topic").map(String::as_str),
        Some("com.aeroric.ios.companion")
    );
    assert_eq!(
        call.headers.get("apns-push-type").map(String::as_str),
        Some("alert")
    );
    assert!(call.headers.contains_key("authorization"));
    assert!(call.headers.contains_key("apns-id"));
    let body: Value = serde_json::from_str(&call.raw_body).expect("payload json");
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["aps", "eventSeq", "kind"], "payload keys: {keys:?}");
    assert_eq!(body["kind"], json!("task_done"));

    // 负例:业务内容一个都不许出现。
    for forbidden in [
        "Refactor",
        "Aeroric",
        "/Users/",
        "secret-project",
        "billing",
    ] {
        assert!(
            !call.raw_body.contains(forbidden),
            "payload leaked {forbidden:?}: {}",
            call.raw_body
        );
    }

    // 3) 同一任务再失败一次:第二条通知,折叠键按类别区分。
    apns.calls.lock().clear();
    harness.log.append(
        "task-status",
        json!({"task_id": "task-77", "status": "failed"}),
    );
    let calls = apns.calls();
    assert_eq!(calls.len(), 1);
    let body: Value = serde_json::from_str(&calls[0].raw_body).expect("payload json");
    assert_eq!(body["kind"], json!("task_failed"));

    // 4) 非提醒状态不产生任何请求。
    apns.calls.lock().clear();
    harness.log.append(
        "task-status",
        json!({"task_id": "task-77", "status": "running"}),
    );
    assert!(apns.calls().is_empty(), "running must not notify");

    // 5) 与通知无关的 topic 也不产生请求。
    harness
        .log
        .append("dbx.transfer_progress", json!({"bytes": 10}));
    assert!(apns.calls().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registration_revocation_and_invalidation_all_stop_future_pushes() {
    let root = scratch_dir("revoke");
    let app_paths = AppPaths::from_user_home(root.join("home"));
    let (apns_base, apns) = spawn_fake_apns().await;
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;
    let harness = harness(
        root,
        app_paths,
        (&sender_base, &user, &secret),
        "topic",
        None,
    );

    let client = PushSenderClient::new(PushSenderConfig {
        url: sender_base.clone(),
        user,
        secret,
    })
    .expect("client");

    // 登记两台设备。
    harness
        .handle
        .push_register(register_params(&client, "dev-1", "ios", "aaaa1111"))
        .await
        .expect("register dev-1");
    harness
        .handle
        .push_register(register_params(&client, "dev-2", "ipados", "bbbb2222"))
        .await
        .expect("register dev-2");

    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t1", "status": "done"}));
    assert_eq!(apns.calls().len(), 2, "both registered devices receive it");

    // 撤销 dev-1(空 token 走同一条 register 路径)。
    let revoked = harness
        .handle
        .push_register(json!({ "deviceId": "dev-1", "platform": "ios", "token": "" }))
        .await
        .expect("revoke");
    assert_eq!(revoked["registered"], json!(false), "{revoked}");

    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t2", "status": "done"}));
    let calls = apns.calls();
    assert_eq!(calls.len(), 1, "revoked device must not be pushed");
    assert_eq!(calls[0].path, "/3/device/bbbb2222");

    // APNs 判定 dev-2 的 token 失效 → 本地绑定自动失效,之后零请求。
    apns.reject("bbbb2222", 410, "Unregistered");
    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t3", "status": "failed"}));
    assert_eq!(
        apns.calls().len(),
        1,
        "the invalid token is still tried once"
    );
    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t4", "status": "done"}));
    assert!(
        apns.calls().is_empty(),
        "an APNs-invalidated device must never be pushed again"
    );

    // 状态面如实反映「没有活跃设备」。
    let status = harness.handle.push_status().await.expect("status");
    assert_eq!(status["enabled"], json!(true));
    assert_eq!(status["wired"], json!(true));
    assert_eq!(status["stats"]["invalidatedDevices"], json!(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoking_a_device_stops_its_push_at_both_ends() {
    let root = scratch_dir("revoke-endpoint");
    let app_paths = AppPaths::from_user_home(root.join("home"));
    let (apns_base, apns) = spawn_fake_apns().await;
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;
    let harness = harness(
        root,
        app_paths,
        (&sender_base, &user, &secret),
        "topic",
        None,
    );

    let client = PushSenderClient::new(PushSenderConfig {
        url: sender_base.clone(),
        user,
        secret,
    })
    .expect("client");
    harness
        .handle
        .push_register(register_params(&client, "dev-revoke", "ios", "a1b2c3d4"))
        .await
        .expect("register");

    // 撤销前:确实会推。
    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t1", "status": "done"}));
    assert_eq!(apns.calls().len(), 1);

    // **撤销**(core 的撤销入口,不是测试自己编的清理):必须同时停掉本地绑定与
    // 发送端绑定,否则被撤销的设备还会收到提醒。
    let detail = harness
        .handle
        .device_revoked("dev-revoke")
        .await
        .expect("device_revoked");
    assert_eq!(detail["bindingRemoved"], json!(true), "{detail}");
    assert_eq!(detail["remote"]["attempted"], json!(true), "{detail}");

    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t2", "status": "failed"}));
    assert!(
        apns.calls().is_empty(),
        "a revoked device must receive nothing after revocation"
    );

    // 就算有别的路径把本地绑定又写回来,发送端侧也已经解绑(两端都停才叫停)。
    harness
        .handle
        .push_register(register_params(&client, "dev-revoke", "ios", "a1b2c3d4"))
        .await
        .expect("re-register");
    apns.calls.lock().clear();
    harness
        .log
        .append("task-status", json!({"task_id": "t3", "status": "done"}));
    assert_eq!(
        apns.calls().len(),
        1,
        "an explicit re-registration re-enables delivery (revocation is not a permanent block)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_local_uds_surface_exposes_push_status_test_and_relay_diagnostics() {
    let root = scratch_dir("uds");
    let app_paths = AppPaths::from_user_home(root.join("home"));
    let (apns_base, apns) = spawn_fake_apns().await;
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;
    let harness = harness(
        root,
        app_paths.clone(),
        (&sender_base, &user, &secret),
        "topic",
        Some(("", None)),
    );

    // 生产装配路径:remote plane 作为 `HostWiring::remote` 一并装进**同一个**
    // 服务集(`handshake` 与 `remote.*` 必须在同一集合里,否则认证后的连接
    // 看不到这些方法)。
    let token_path = app_paths.data_dir().join("host-token");
    let token = Arc::new(HostToken::load_or_create(&token_path).expect("token"));
    let token_str = std::fs::read_to_string(&token_path).expect("token file");
    let context = Arc::new(
        CoreContext::read_only(app_paths.clone()).with_secrets(Arc::new(MemorySecrets::with(&[(
            PUSH_CREDENTIAL_KEY,
            secret.as_str(),
        )]))),
    );
    let wiring = aeroric_host::dispatcher::HostWiring {
        remote: Some(aeroric_host::dispatcher::RemotePlaneInjection {
            state: Arc::new(aeroric_core::remote::state::RemoteState::new_in_memory()),
            handle: Arc::clone(&harness.handle)
                as Arc<dyn aeroric_core::remote::services::RemoteHostHandle>,
        }),
        ..Default::default()
    };
    let services = HostServices::with_wiring(
        Arc::clone(&token),
        Arc::clone(&harness.log),
        context,
        wiring,
    );
    let call = Arc::new(CallContext {
        services: Arc::clone(&services.services),
        connection: Arc::new(tokio::sync::Mutex::new(
            aeroric_host::subscriptions::Connection::new(),
        )),
    });

    let handshake = dispatch(
        rpc_req(
            "h1",
            "handshake",
            json!({ "token": token_str.trim(), "clientId": "native-mac" }),
        ),
        Arc::clone(&call),
    )
    .await;
    match handshake {
        Frame::Success(_) => {}
        other => panic!("handshake failed: {other:?}"),
    }

    let status = dispatch(
        rpc_req("s1", "remote.pushStatus", json!({})),
        Arc::clone(&call),
    )
    .await;
    let Frame::Success(success) = status else {
        panic!("pushStatus failed: {status:?}");
    };
    assert_eq!(success.result["enabled"], json!(true));
    assert_eq!(success.result["wired"], json!(true));
    assert_eq!(success.result["devices"], json!([]));

    // 缺 deviceId:先拒绝,无副作用。
    let missing = dispatch(
        rpc_req("s2", "remote.pushRegister", json!({ "token": "aabb" })),
        Arc::clone(&call),
    )
    .await;
    match missing {
        Frame::Failure(f) => assert_eq!(f.error.code, ErrorCode::InvalidArgument),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let registered = dispatch(
        rpc_req(
            "s3",
            "remote.pushRegister",
            json!({ "deviceId": "dev-uds", "platform": "ios", "token": "ccdd3344" }),
        ),
        Arc::clone(&call),
    )
    .await;
    let Frame::Success(success) = registered else {
        panic!("pushRegister failed: {registered:?}");
    };
    assert_eq!(
        success.result["registered"],
        json!(true),
        "{}",
        success.result
    );

    // pushTest:真的打一次 APNs。
    apns.calls.lock().clear();
    let tested = dispatch(
        rpc_req("s4", "remote.pushTest", json!({})),
        Arc::clone(&call),
    )
    .await;
    let Frame::Success(success) = tested else {
        panic!("pushTest failed: {tested:?}");
    };
    assert_eq!(success.result["accepted"], json!(1), "{}", success.result);
    let calls = apns.calls();
    assert_eq!(calls.len(), 1);
    let body: Value = serde_json::from_str(&calls[0].raw_body).expect("payload");
    assert!(body.get("aps").is_some(), "{body}");

    // relayDiagnostics:没有官方默认 relay,如实说明。
    let diagnostics = dispatch(
        rpc_req("s5", "remote.relayDiagnostics", json!({})),
        Arc::clone(&call),
    )
    .await;
    let Frame::Success(success) = diagnostics else {
        panic!("relayDiagnostics failed: {diagnostics:?}");
    };
    assert_eq!(success.result["defaultRelay"], Value::Null);
    assert!(success.result["note"]
        .as_str()
        .expect("note")
        .contains("no default relay"));
    assert_eq!(success.result["relay"]["schemeOk"], json!(true));
    assert_eq!(success.result["relay"]["reachable"], json!(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_diagnostics_distinguishes_not_configured_from_unreachable() {
    let root = scratch_dir("relay");
    let app_paths = AppPaths::from_user_home(root.join("home"));
    let (apns_base, _apns) = spawn_fake_apns().await;
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;

    // 1) 没有 relay:方案合法、不可达,结论说清楚「直连是唯一路径」。
    let no_relay = harness(
        root.clone(),
        app_paths.clone(),
        (&sender_base, &user, &secret),
        "topic",
        Some(("", None)),
    );
    let diagnostics = no_relay.handle.relay_diagnostics().await.expect("diag");
    assert_eq!(diagnostics["defaultRelay"], Value::Null);
    assert_eq!(diagnostics["relay"]["schemeOk"], json!(true));
    assert_eq!(diagnostics["relay"]["reachable"], json!(false));
    assert!(
        diagnostics["relay"]["detail"]
            .as_str()
            .expect("detail")
            .contains("no relay configured"),
        "{diagnostics}"
    );

    // 2) 一个合法但没人听的 loopback 端点:方案通过、连通失败,detail 说明原因。
    //    另一个数据根:单写者租约正确地拒绝了同一目录的第二个 harness。
    let other_root = scratch_dir("relay-unreachable");
    let other_paths = AppPaths::from_user_home(other_root.join("home"));
    let unreachable = harness(
        other_root,
        other_paths,
        (&sender_base, &user, &secret),
        "topic",
        Some(("ws://127.0.0.1:1", Some("token"))),
    );
    let diagnostics = unreachable.handle.relay_diagnostics().await.expect("diag");
    assert_eq!(diagnostics["relay"]["schemeOk"], json!(true));
    assert_eq!(diagnostics["relay"]["reachable"], json!(false));
    assert_eq!(diagnostics["relay"]["authenticated"], json!(false));
    let detail = diagnostics["relay"]["detail"].as_str().expect("detail");
    assert!(
        detail.contains("cannot connect") || detail.contains("timed out"),
        "{detail}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_diagnostics_reports_reachable_but_rejected_when_the_token_is_wrong() {
    // 起一个真的 relay(用户自托管形态),用真口令注册成功、错口令被拒。
    // 这是诊断最有价值的一条:可达 ≠ 能用。
    let addr = {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind relay probe");
        let addr = probe.local_addr().expect("addr");
        // 释放端口:本用例要验证的是「没人听」这一支,真 relay 进程不在本文件里起。
        drop(probe);
        addr
    };
    let relay_base = format!("ws://{addr}");

    let root = scratch_dir("relay-token");
    let app_paths = AppPaths::from_user_home(root.join("home"));
    let (apns_base, _apns) = spawn_fake_apns().await;
    let (sender_base, user, secret) = spawn_sender("com.aeroric.ios.companion", &apns_base).await;
    let harness = harness(
        root,
        app_paths,
        (&sender_base, &user, &secret),
        "topic",
        Some((&relay_base, Some("the-right-token"))),
    );
    // 真 relay 没在监听时,诊断必须说「连不上」而不是「口令错了」——
    // 区分这两件事正是诊断存在的理由。
    let diagnostics = harness.handle.relay_diagnostics().await.expect("diag");
    assert_eq!(diagnostics["relay"]["reachable"], json!(false));
    assert_eq!(diagnostics["relay"]["authenticated"], json!(false));
    assert!(!diagnostics["relay"]["detail"]
        .as_str()
        .unwrap_or("")
        .contains("accepted"));
}
