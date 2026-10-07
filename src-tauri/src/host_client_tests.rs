//! P8-05 切片三验收：外部宿主的客户端与监督层。
//!
//! 两半，各自的判据不同：
//!
//! - **客户端**（`HostClient`）：起一个**真实 UDS 服务端**（脚本化的，只答握手与一两个
//!   方法）跑真连接——token 读文件、握手一帧、请求/应答、失败帧、断连。线的形状不能靠
//!   读代码证明（服务端一侧的 `decode_line` 才说了算）。
//! - **监督层**（`HostSupervisor`）：四个端口全注入，于是"先连后起"（**一次都不 spawn**）、
//!   "活宿主不兼容不杀不抢"、"认证失败不被装扮成版本偏斜"、"helper 只在两处找"、
//!   "明确关掉 spawn 时绝不自立"这些判据可以逐条钉住。
//!
//! 真进程那一半（两个 `serve` 抢租约、败者回收）在
//! `scripts/native/tauri-host-smoke.mjs`：那里的判据是 PID。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aeroric_protocol::envelope::{decode_line, encode_line, Failure, Frame, Request, Success};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::host_client::{host_socket_path, host_token_path, HostClient, HostClientError};
use crate::host_supervisor::{
    generated_helper_path, helper_file_name, HelperOrigin, HelperSpawner, HostSupervisor,
    LivenessProbe, ReadinessWaiter, SupervisorError, SupervisorOutcome,
};

// ── 脚本化的真 UDS 服务端 ───────────────────────────────────────────────────

/// 服务端脚本：每次收到请求时怎么答。返回 `None` 表示"不答、直接断开"。
type Script = Arc<dyn Fn(&str, &serde_json::Value) -> Option<Frame> + Send + Sync>;

struct ScriptedHost {
    socket: PathBuf,
    data_dir: PathBuf,
    seen: Arc<Mutex<Vec<String>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<()>,
}

impl ScriptedHost {
    async fn start(tag: &str, script: Script, token: Option<&str>) -> Self {
        // `/tmp` 短根：macOS 的 `$TMPDIR` 逼近 `sun_path` 上限（276 片案底），
        // 组合出来的 socket 路径会直接 `SUN_LEN` 失败。与 host harness 同约定。
        let root = PathBuf::from("/tmp").join(format!(
            "ahc-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("root");
        if let Some(token) = token {
            std::fs::write(root.join("host.token"), token).expect("token");
        }
        let socket = host_socket_path(&root);
        let listener = UnixListener::bind(&socket).expect("bind");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_task = Arc::clone(&seen);
        let (shutdown, mut stop) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut stop => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((stream, _)) = accepted else { break };
                let script = Arc::clone(&script);
                let seen = Arc::clone(&seen_for_task);
                tokio::spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut reader = BufReader::new(read);
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                        let Ok(Frame::Request(request)) = decode_line(line.trim_end().as_bytes())
                        else {
                            return;
                        };
                        seen.lock().unwrap().push(request.method.clone());
                        let Some(reply) = script(&request.method, &request.params) else {
                            return; // 脚本说"断开"
                        };
                        let reply = match reply {
                            Frame::Success(mut success) => {
                                success.id = request.id.clone();
                                Frame::Success(success)
                            }
                            Frame::Failure(mut failure) => {
                                failure.id = request.id.clone();
                                Frame::Failure(failure)
                            }
                            other => other,
                        };
                        let Ok(encoded) = encode_line(&reply) else {
                            return;
                        };
                        if write.write_all(encoded.as_bytes()).await.is_err() {
                            return;
                        }
                        let _ = write.flush().await;
                    }
                });
            }
        });
        Self {
            socket,
            data_dir: root,
            seen,
            shutdown: Some(shutdown),
            handle,
        }
    }

    fn methods(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for ScriptedHost {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.handle.abort();
        std::fs::remove_dir_all(&self.data_dir).ok();
    }
}

fn success(result: serde_json::Value) -> Frame {
    Frame::Success(Success {
        v: aeroric_protocol::PROTOCOL_VERSION,
        id: String::new(),
        ok: true,
        result,
        extra: Default::default(),
    })
}

fn failure(code: aeroric_protocol::error::ErrorCode, message: &str) -> Frame {
    Frame::Failure(Failure {
        v: aeroric_protocol::PROTOCOL_VERSION,
        id: String::new(),
        ok: false,
        error: aeroric_protocol::error::RpcError {
            code,
            message: message.to_string(),
            retryable: false,
            detail: None,
        },
        extra: Default::default(),
    })
}

/// 只接受正确 token 的脚本（握手以外的请求都断开）。
fn handshake_script(expected: &'static str) -> Script {
    Arc::new(move |method, params| {
        if method != "handshake" {
            return None;
        }
        let presented = params
            .get("token")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if presented != expected {
            return Some(failure(
                aeroric_protocol::error::ErrorCode::Unauthenticated,
                "presented token does not match this host's data directory",
            ));
        }
        Some(success(serde_json::json!({
            "clientId": params.get("clientId").cloned().unwrap_or_default(),
            "capabilities": [],
        })))
    })
}

// ── 客户端 ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_client_authenticates_with_the_token_file() {
    let host = ScriptedHost::start("auth", handshake_script("s3cret"), Some("s3cret")).await;
    let client = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect("connect");
    assert_eq!(client.socket(), host.socket);
    assert_eq!(client.socket(), host_socket_path(&host.data_dir));
    assert_eq!(host.methods(), vec!["handshake".to_string()]);
}

#[tokio::test]
async fn a_wrong_token_is_a_rejection_not_a_transport_error() {
    let host = ScriptedHost::start("badtoken", handshake_script("s3cret"), Some("other")).await;
    let error = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect_err("must be rejected");
    match error {
        HostClientError::Rejected { code, message } => {
            assert!(code.contains("Unauthenticated"), "{code}");
            assert!(message.contains("token"), "{message}");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_token_file_says_which_path_was_expected() {
    let host = ScriptedHost::start("notoken", handshake_script("s3cret"), None).await;
    let error = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect_err("must refuse before connecting");
    match error {
        HostClientError::NoToken { path } => assert_eq!(path, host_token_path(&host.data_dir)),
        other => panic!("expected NoToken, got {other:?}"),
    }
    // 没有 token 就**不连接**：`methods` 为空说明服务端一个请求都没收到。
    assert!(host.methods().is_empty(), "{:?}", host.methods());
}

#[tokio::test]
async fn a_missing_socket_is_unreachable_not_a_panic() {
    let root = PathBuf::from("/tmp").join(format!("ahc-gone-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(host_token_path(&root), "s3cret").expect("token");
    let error = HostClient::connect(&root, "tauri")
        .await
        .expect_err("no host");
    assert!(
        matches!(error, HostClientError::Unreachable { .. }),
        "{error:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn requests_and_failures_round_trip_over_a_real_socket() {
    let script: Script = Arc::new(|method, params| match method {
        "handshake" => Some(success(
            serde_json::json!({ "clientId": "tauri", "capabilities": [] }),
        )),
        "task.list" => {
            // 参数要真的到达服务端——这是"客户端把 params 发出去了"的唯一证据。
            let project = params
                .get("projectId")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            Some(success(serde_json::json!({ "tasks": [], "echo": project })))
        }
        "task.get" => Some(failure(
            aeroric_protocol::error::ErrorCode::NotFound,
            "no such task",
        )),
        _ => None,
    });
    let host = ScriptedHost::start("roundtrip", script, Some("t")).await;
    let mut client = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect("connect");

    let result = client
        .request("task.list", serde_json::json!({ "projectId": "p1" }))
        .await
        .expect("list");
    assert_eq!(result["echo"], serde_json::json!("p1"));

    let error = client
        .request("task.get", serde_json::json!({ "taskId": "ghost" }))
        .await
        .expect_err("not found");
    match error {
        HostClientError::Rpc { code, message } => {
            assert!(code.contains("NotFound"), "{code}");
            assert_eq!(message, "no such task");
        }
        other => panic!("expected Rpc, got {other:?}"),
    }
    assert_eq!(
        host.methods(),
        vec![
            "handshake".to_string(),
            "task.list".to_string(),
            "task.get".to_string()
        ]
    );
}

#[tokio::test]
async fn a_server_that_hangs_up_mid_request_is_a_transport_error() {
    let script: Script = Arc::new(|method, _| {
        if method == "handshake" {
            Some(success(
                serde_json::json!({ "clientId": "tauri", "capabilities": [] }),
            ))
        } else {
            None // 断开
        }
    });
    let host = ScriptedHost::start("hangup", script, Some("t")).await;
    let mut client = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect("connect");
    let error = client
        .request("usage.read_usage_statistics", serde_json::json!({}))
        .await
        .expect_err("the host hung up");
    assert!(matches!(error, HostClientError::Transport(_)), "{error:?}");
}

// ── 监督层（端口全注入）────────────────────────────────────────────────────

#[derive(Default)]
struct FakeProbe {
    live: bool,
}

impl LivenessProbe for FakeProbe {
    fn is_live(&self, _socket: &Path) -> bool {
        self.live
    }
}

#[derive(Default)]
struct CountingSpawner {
    calls: Mutex<Vec<PathBuf>>,
    fail: Mutex<Option<String>>,
}

impl HelperSpawner for CountingSpawner {
    fn spawn(&self, helper: &Path, _data_dir: &Path) -> Result<(), SupervisorError> {
        if let Some(reason) = self.fail.lock().unwrap().clone() {
            return Err(SupervisorError::SpawnFailed {
                path: helper.to_path_buf(),
                reason,
            });
        }
        self.calls.lock().unwrap().push(helper.to_path_buf());
        Ok(())
    }
}

#[derive(Default)]
struct FixedWaiter {
    ready: bool,
}

impl ReadinessWaiter for FixedWaiter {
    fn wait(&self, _socket: &Path, _timeout_ms: u64) -> bool {
        self.ready
    }
}

struct Rig {
    root: PathBuf,
    exe_dir: PathBuf,
}

impl Rig {
    fn new(tag: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "ahs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let exe_dir = root.join("Contents/MacOS");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        Self { root, exe_dir }
    }

    fn data_dir(&self) -> PathBuf {
        let dir = self.root.join("data");
        std::fs::create_dir_all(&dir).expect("data dir");
        dir
    }

    fn supervisor(
        &self,
        generated: Option<PathBuf>,
        live: bool,
        spawner: Arc<CountingSpawner>,
        ready: bool,
    ) -> HostSupervisor {
        HostSupervisor::new(
            self.exe_dir.clone(),
            generated,
            Arc::new(FakeProbe { live }),
            spawner as Arc<dyn HelperSpawner>,
            Arc::new(FixedWaiter { ready }),
        )
    }

    fn write_helper(&self) -> PathBuf {
        let path = self.exe_dir.join(helper_file_name());
        std::fs::write(&path, b"#!/bin/sh\n").expect("helper");
        path
    }

    fn cleanup(&self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

#[tokio::test]
async fn without_spawning_a_live_host_is_never_replaced() {
    // 活宿主存在时连不上（token 不对）⇒ Authenticate；而且**一次 spawn 都没有**。
    let rig = Rig::new("attach");
    let data_dir = rig.data_dir();
    let script_host = ScriptedHost::start("live", handshake_script("real"), Some("real")).await;
    std::fs::write(host_token_path(&script_host.data_dir), "wrong").expect("token");
    let spawner = Arc::new(CountingSpawner::default());
    let supervisor = rig.supervisor(None, true, Arc::clone(&spawner), true);

    let error = supervisor
        .attach_or_spawn(&script_host.data_dir, "tauri")
        .await
        .expect_err("wrong token must fail");
    assert!(
        matches!(error, SupervisorError::Authenticate { .. }),
        "{error:?}"
    );
    assert!(
        spawner.calls.lock().unwrap().is_empty(),
        "有活宿主时一次都不许 spawn（哪怕连不上）"
    );
    let _ = data_dir;
    rig.cleanup();
}

#[tokio::test]
async fn a_live_compatible_host_is_attached_directly() {
    let rig = Rig::new("attach-ok");
    let host = ScriptedHost::start("live-ok", handshake_script("t"), Some("t")).await;
    let spawner = Arc::new(CountingSpawner::default());
    let supervisor = rig.supervisor(None, true, Arc::clone(&spawner), true);

    let outcome = supervisor
        .attach_or_spawn(&host.data_dir, "tauri")
        .await
        .expect("attach");
    match outcome {
        SupervisorOutcome::Attached(client) => assert_eq!(client.socket(), host.socket),
        other => panic!("expected Attached, got {:?}", other.is_attached()),
    }
    assert!(spawner.calls.lock().unwrap().is_empty());
    assert_eq!(host.methods(), vec!["handshake".to_string()]);
    rig.cleanup();
}

#[tokio::test]
async fn version_skew_on_a_live_host_is_reported_not_replaced() {
    // 服务端对一个"新方法"回未知方法 ⇒ 客户端看到 Rpc，监督层判为版本偏斜。
    let script: Script = Arc::new(|method, _params| match method {
        "handshake" => Some(success(
            serde_json::json!({ "clientId": "tauri", "capabilities": [] }),
        )),
        _ => Some(failure(
            aeroric_protocol::error::ErrorCode::InvalidArgument,
            "unknown method: maintenance.get_notifications",
        )),
    });
    // 版本偏斜的判据在连不上时的错误文案里；这里让 socket "活"但连不上是不可能的，
    // 所以直接走"活⇒连"，再用一个只会回 not-found 的服务端验证分类函数的行为。
    let host = ScriptedHost::start("skew", script, Some("t")).await;
    let mut client = HostClient::connect(&host.data_dir, "tauri")
        .await
        .expect("connect");
    let error = client
        .request("maintenance.get_notifications", serde_json::json!({}))
        .await
        .expect_err("unknown method");
    assert!(
        error.to_string().to_lowercase().contains("unknown method"),
        "版本偏斜的文案要认得出来：{error}"
    );

    // 判决层：socket 判活、连接被拒（无 token）⇒ Authenticate（认证，不是版本）。
    let rig = Rig::new("skew-decision");
    let spawner = Arc::new(CountingSpawner::default());
    let supervisor = rig.supervisor(None, true, Arc::clone(&spawner), true);
    let blind = rig.data_dir();
    std::fs::write(host_token_path(&blind), "t").expect("token");
    let error = supervisor
        .attach_or_spawn(&blind, "tauri")
        .await
        .expect_err("no live socket at this socket path");
    // 注意：`FakeProbe{live:true}` 说"活"，但没有真 socket ⇒ 连接失败 ⇒ 不是版本偏斜。
    assert!(
        matches!(error, SupervisorError::Authenticate { .. }),
        "{error:?}"
    );
    assert!(
        spawner.calls.lock().unwrap().is_empty(),
        "不兼容/连不上都不许杀旧起新"
    );
    rig.cleanup();
}

#[tokio::test]
async fn the_helper_is_only_looked_for_in_the_bundle_or_the_generated_path() {
    let rig = Rig::new("resolve");
    let supervisor = rig.supervisor(None, false, Arc::new(CountingSpawner::default()), true);
    let error = supervisor.resolve_helper().expect_err("no helper anywhere");
    assert!(
        matches!(error, SupervisorError::HelperMissing { .. }),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("prepare-tauri-host"),
        "报错要点出补 helper 的那条命令：{error}"
    );

    let generated = generated_helper_path(&rig.root.join("binaries"), "aarch64-apple-darwin");
    std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
    std::fs::write(&generated, b"#!/bin/sh\n").unwrap();
    let supervisor = rig.supervisor(
        Some(generated.clone()),
        false,
        Arc::new(CountingSpawner::default()),
        true,
    );
    assert_eq!(
        supervisor.resolve_helper().expect("generated"),
        HelperOrigin::Generated(generated)
    );

    let bundled = rig.write_helper();
    assert_eq!(
        supervisor.resolve_helper().expect("bundled"),
        HelperOrigin::Bundle(bundled)
    );
    rig.cleanup();
}

#[tokio::test]
async fn without_spawning_a_missing_host_is_an_explicit_refusal() {
    let rig = Rig::new("nospawn");
    rig.write_helper();
    let spawner = Arc::new(CountingSpawner::default());
    let supervisor = rig
        .supervisor(None, false, Arc::clone(&spawner), true)
        .without_spawning();

    let error = supervisor
        .attach_or_spawn(&rig.data_dir(), "tauri")
        .await
        .expect_err("must not self-appoint");
    assert!(
        matches!(error, SupervisorError::HelperMissing { .. }),
        "{error:?}"
    );
    assert!(
        spawner.calls.lock().unwrap().is_empty(),
        "明确关掉 spawn 时绝不自立"
    );
    rig.cleanup();
}

#[tokio::test]
async fn spawning_is_used_only_when_the_ports_allow_it() {
    let rig = Rig::new("spawn");
    let helper = rig.write_helper();
    let spawner = Arc::new(CountingSpawner::default());
    let host = ScriptedHost::start("spawned", handshake_script("t"), Some("t")).await;
    // 用真 socket 冒充"helper 起来之后有活儿了"：`FakeProbe{live:false}` 让判决走
    // spawn 分支，服务端已经在那儿等着我们连。
    let supervisor = rig.supervisor(None, false, Arc::clone(&spawner), true);
    let outcome = supervisor
        .attach_or_spawn(&host.data_dir, "tauri")
        .await
        .expect("spawn-and-attach");
    assert!(
        matches!(outcome, SupervisorOutcome::SpawnedAndAttached(_)),
        "expected spawn path"
    );
    assert_eq!(spawner.calls.lock().unwrap().as_slice(), &[helper]);

    // 就绪超时：spawn 成功但 socket 永远不活 ⇒ NotReady（不是"假装连上了"）。
    let rig2 = Rig::new("notready");
    rig2.write_helper();
    let spawner2 = Arc::new(CountingSpawner::default());
    let supervisor2 = rig2.supervisor(None, false, Arc::clone(&spawner2), false);
    let error = supervisor2
        .attach_or_spawn(&rig2.data_dir(), "tauri")
        .await
        .expect_err("must not claim ready");
    match error {
        SupervisorError::NotReady { waited_ms, .. } => assert!(waited_ms > 0),
        other => panic!("expected NotReady, got {other:?}"),
    }

    // spawn 失败保留原文。
    let rig3 = Rig::new("spawnfail");
    rig3.write_helper();
    let failing = Arc::new(CountingSpawner {
        calls: Mutex::new(Vec::new()),
        fail: Mutex::new(Some("permission denied (os error 13)".to_string())),
    });
    let supervisor3 = rig3.supervisor(None, false, failing, true);
    let error = supervisor3
        .attach_or_spawn(&rig3.data_dir(), "tauri")
        .await
        .expect_err("must fail");
    assert!(
        error
            .to_string()
            .contains("permission denied (os error 13)"),
        "{error}"
    );

    rig.cleanup();
    rig2.cleanup();
    rig3.cleanup();
}

/// 端口计数：`is_live` 被问了几次（"判决真的问了探测"这件事的唯一证据）。
#[tokio::test]
async fn the_decision_actually_consults_the_probe() {
    struct CountingProbe(Arc<AtomicUsize>);
    impl LivenessProbe for CountingProbe {
        fn is_live(&self, _socket: &Path) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst);
            false
        }
    }
    let rig = Rig::new("probe");
    rig.write_helper();
    let asks = Arc::new(AtomicUsize::new(0));
    let supervisor = HostSupervisor::new(
        rig.exe_dir.clone(),
        None,
        Arc::new(CountingProbe(Arc::clone(&asks))),
        Arc::new(CountingSpawner::default()) as Arc<dyn HelperSpawner>,
        Arc::new(FixedWaiter { ready: false }),
    );
    let _ = supervisor.attach_or_spawn(&rig.data_dir(), "tauri").await;
    assert!(asks.load(Ordering::SeqCst) >= 1, "判决必须先问探测");
    rig.cleanup();
}

/// `ScriptedHost` 自己要能证明"服务端真的解出了请求"——否则上面所有断言都可能是空跑。
#[tokio::test]
async fn the_scripted_host_really_decodes_what_the_client_writes() {
    let host = ScriptedHost::start("sanity", handshake_script("t"), Some("t")).await;
    let mut stream = UnixStream::connect(&host.socket).await.expect("dial");
    let frame = Frame::Request(Request {
        v: aeroric_protocol::PROTOCOL_VERSION,
        id: "probe-1".to_string(),
        method: "handshake".to_string(),
        params: serde_json::json!({ "token": "t", "clientId": "sanity" }),
        idempotency_key: None,
        extra: Default::default(),
    });
    let encoded = encode_line(&frame).expect("encode");
    stream.write_all(encoded.as_bytes()).await.expect("write");
    let (read, _) = stream.split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read");
    let reply = decode_line(line.trim_end().as_bytes()).expect("decode");
    assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
    assert_eq!(host.methods(), vec!["handshake".to_string()]);
}
