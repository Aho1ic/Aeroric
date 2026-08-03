//! 局域网 WS 链路集成测试:mock runtime + 真实 TCP socket。
//!
//! 覆盖 M0-M4 验收面:E2EE 握手、invite 配对握手、device token 复连、
//! 白名单 RPC、事件桥推送、坏 token 拒绝、撤销即断连、优雅停机、
//! 终端流快照-水位-输入-resize、任务操作 RPC、旧协议拒连。
//! 测试客户端以 crypto.rs 的 client 侧实现模拟手机端:先明文 hello 握手,
//! 之后全部消息封在加密二进制帧内(kind=1 控制 JSON,kind=2 终端帧)。

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{Emitter, Listener, Manager};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::terminal_frames::{decode_terminal_frame, TerminalFrame, TerminalOpcode};
use super::{crypto, server, RemoteState};

type ClientWs = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const WAIT: Duration = Duration::from_secs(5);

fn empty_task_manager() -> crate::TaskManager {
    use parking_lot::Mutex;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    crate::TaskManager {
        pty_masters: Mutex::new(HashMap::new()),
        pty_writers: Mutex::new(HashMap::new()),
        child_handles: Mutex::new(HashMap::new()),
        pending_pty_sizes: Mutex::new(HashMap::new()),
        cancelled_tasks: Mutex::new(HashSet::new()),
        manually_completed_tasks: Mutex::new(HashSet::new()),
        codex_sessions: Mutex::new(HashMap::new()),
        claude_sessions: Mutex::new(HashMap::new()),
        claimed_session_paths: Mutex::new(HashSet::new()),
        initial_input_signals: Arc::new(Mutex::new(HashMap::new())),
        wsl_active_ids: Mutex::new(HashSet::new()),
        codex_rpc: Arc::new(Mutex::new(None)),
    }
}

/// 已完成 E2EE 握手的测试客户端(模拟手机端 e2ee.ts)。
struct TestClient {
    ws: ClientWs,
    session: crypto::SessionCrypto,
}

async fn raw_connect(port: u16) -> ClientWs {
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("client connect");
    ws
}

/// 连接并完成 E2EE 握手(hello → hello_ack → confirm 验证)。
async fn connect(port: u16, server_pub: &str) -> TestClient {
    let mut ws = raw_connect(port).await;
    let handshake = crypto::initiate_handshake(server_pub).expect("initiate handshake");
    ws.send(Message::Text(
        json!({"v":2,"type":"hello","pub":handshake.eph_pub_b64}).to_string(),
    ))
    .await
    .expect("send hello");
    let ack = loop {
        let msg = timeout(WAIT, ws.next())
            .await
            .expect("timed out waiting for hello_ack")
            .expect("stream ended")
            .expect("frame error");
        match msg {
            Message::Text(text) => break serde_json::from_str::<Value>(&text).expect("ack json"),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame during handshake: {other:?}"),
        }
    };
    assert_eq!(ack["type"], json!("hello_ack"), "handshake ack: {ack}");
    let session = handshake
        .finish(
            ack["pub"].as_str().unwrap(),
            ack["confirm"].as_str().unwrap(),
        )
        .expect("finish handshake");
    TestClient { ws, session }
}

impl TestClient {
    async fn send_json(&mut self, value: Value) {
        let frame = self
            .session
            .encrypt_frame(crypto::KIND_CTRL, value.to_string().as_bytes())
            .expect("encrypt ctrl frame");
        self.ws
            .send(Message::Binary(frame))
            .await
            .expect("send frame");
    }

    /// 读到下一条加密帧并解密(跳过 Ping/Pong 控制帧)。
    async fn next_frame(&mut self) -> (u8, Vec<u8>) {
        loop {
            let msg = timeout(WAIT, self.ws.next())
                .await
                .expect("timed out waiting for frame")
                .expect("stream ended")
                .expect("frame error");
            match msg {
                Message::Binary(bytes) => {
                    return self.session.decrypt_frame(&bytes).expect("decrypt frame")
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                other => panic!("unexpected frame: {other:?}"),
            }
        }
    }

    async fn next_json(&mut self) -> Value {
        let (kind, plain) = self.next_frame().await;
        assert_eq!(kind, crypto::KIND_CTRL, "expected ctrl frame");
        serde_json::from_str(std::str::from_utf8(&plain).expect("utf8 ctrl frame"))
            .expect("json frame")
    }

    /// 读到流终结(Close 或连接断开)。
    async fn expect_closed(&mut self) {
        loop {
            match timeout(WAIT, self.ws.next())
                .await
                .expect("timed out waiting for close")
            {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => continue,
            }
        }
    }

    /// 读到下一条终端二进制帧(跳过控制面帧)。
    async fn next_terminal_frame(&mut self) -> TerminalFrame {
        loop {
            let (kind, plain) = self.next_frame().await;
            if kind != crypto::KIND_TERMINAL {
                continue;
            }
            if let Some(frame) = decode_terminal_frame(&plain) {
                return frame;
            }
        }
    }

    async fn send_terminal_frame(&mut self, frame: TerminalFrame) {
        let sealed = self
            .session
            .encrypt_frame(crypto::KIND_TERMINAL, &frame.encode())
            .expect("encrypt terminal frame");
        self.ws
            .send(Message::Binary(sealed))
            .await
            .expect("send terminal frame");
    }
}

#[test]
fn lan_pairing_rpc_push_and_revoke_roundtrip() {
    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    let handle = app.handle().clone();

    tauri::async_runtime::block_on(async move {
        let state = handle.state::<RemoteState>();
        let server_pub = state.keys.public_b64();
        let invite = state.auth.lock().create_invite().expect("invite");

        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        // ── invite 配对 ──
        let mut phone = connect(port, &server_pub).await;
        phone
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"invite":invite,"deviceName":"Test Phone"}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(true), "pairing reply: {reply}");
        let device_token = reply["result"]["deviceToken"]
            .as_str()
            .expect("deviceToken issued")
            .to_string();
        let device_id = reply["result"]["deviceId"].as_str().unwrap().to_string();

        // ── 白名单 RPC ──
        phone.send_json(json!({"v":2,"id":2,"method":"ping"})).await;
        assert_eq!(phone.next_json().await["result"], json!("pong"));

        phone
            .send_json(json!({"v":2,"id":3,"method":"projects.list"}))
            .await;
        let projects = phone.next_json().await;
        assert_eq!(projects["ok"], json!(true));
        assert!(projects["result"].is_array());

        // 未注册方法必须拒绝(窄面 API 保证)。
        phone
            .send_json(json!({"v":2,"id":4,"method":"run_task"}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(false));

        // 已认证连接不允许再次 auth。
        phone
            .send_json(json!({"v":2,"id":5,"method":"auth","params":{}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(false));

        // ── 事件桥推送 ──
        handle
            .emit("task-status", json!({"task_id":"t1","status":"running"}))
            .expect("emit");
        let push = phone.next_json().await;
        assert_eq!(push["push"], json!("task-status"));
        assert_eq!(push["data"]["task_id"], json!("t1"));

        // ── device token 复连 ──
        let mut second = connect(port, &server_pub).await;
        second
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"deviceToken":device_token}}))
            .await;
        let reauth = second.next_json().await;
        assert_eq!(reauth["ok"], json!(true));
        assert_eq!(reauth["result"]["deviceId"], json!(device_id.clone()));

        // ── 坏 token 拒绝并断开 ──
        let mut intruder = connect(port, &server_pub).await;
        intruder
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"deviceToken":"forged"}}))
            .await;
        assert_eq!(intruder.next_json().await["ok"], json!(false));
        intruder.expect_closed().await;

        // ── 撤销设备:立即断开其在线连接 ──
        let existed = state.auth.lock().revoke(&device_id).expect("revoke");
        assert!(existed);
        state.clients.disconnect_device(&device_id);
        phone.expect_closed().await;
        second.expect_closed().await;

        // ── 停机后端口不再接受新连接 ──
        server.shutdown(&handle);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .is_err());
    });
}

#[test]
fn unauthenticated_first_message_is_rejected() {
    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    let handle = app.handle().clone();

    tauri::async_runtime::block_on(async move {
        let server_pub = handle.state::<RemoteState>().keys.public_b64();
        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        // 第一条控制消息不是 auth → 拒绝并断开。
        let mut ws = connect(port, &server_pub).await;
        ws.send_json(json!({"v":2,"id":1,"method":"projects.list"}))
            .await;
        let reply = ws.next_json().await;
        assert_eq!(reply["ok"], json!(false));
        ws.expect_closed().await;

        // 协议版本不匹配 → 拒绝。
        let mut ws2 = connect(port, &server_pub).await;
        ws2.send_json(json!({"v":99,"id":1,"method":"auth","params":{}}))
            .await;
        assert_eq!(ws2.next_json().await["ok"], json!(false));
        ws2.expect_closed().await;

        server.shutdown(&handle);
    });
}

/// E2EE 边界:旧版明文客户端拒连;错误静态公钥的握手被 confirm 拦截。
#[test]
fn e2ee_rejects_legacy_and_wrong_key_clients() {
    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    let handle = app.handle().clone();

    tauri::async_runtime::block_on(async move {
        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        // 旧版 v1 客户端(直接发明文 auth)→ hello_error + 断连,绝不降级。
        let mut legacy = raw_connect(port).await;
        legacy
            .send(Message::Text(
                json!({"v":1,"id":1,"method":"auth","params":{"deviceToken":"x"}}).to_string(),
            ))
            .await
            .expect("send legacy auth");
        let msg = timeout(WAIT, legacy.next())
            .await
            .expect("reply timeout")
            .expect("stream ended")
            .expect("frame error");
        let Message::Text(text) = msg else {
            panic!("expected hello_error text, got {msg:?}")
        };
        let reply: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(reply["type"], json!("hello_error"));
        assert!(reply["error"]
            .as_str()
            .unwrap()
            .contains("Unsupported protocol"));

        // pin 了错误公钥的客户端:握手 confirm 验证失败(客户端侧防中间人)。
        let wrong_keys = crypto::StaticKeys::ephemeral().unwrap();
        let mut mitm = raw_connect(port).await;
        let handshake = crypto::initiate_handshake(&wrong_keys.public_b64()).unwrap();
        mitm.send(Message::Text(
            json!({"v":2,"type":"hello","pub":handshake.eph_pub_b64}).to_string(),
        ))
        .await
        .expect("send hello");
        let msg = timeout(WAIT, mitm.next())
            .await
            .expect("ack timeout")
            .expect("stream ended")
            .expect("frame error");
        let Message::Text(text) = msg else {
            panic!("expected hello_ack, got {msg:?}")
        };
        let ack: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(ack["type"], json!("hello_ack"));
        let err = handshake
            .finish(
                ack["pub"].as_str().unwrap(),
                ack["confirm"].as_str().unwrap(),
            )
            .unwrap_err();
        assert!(err.contains("identity verification failed"));

        server.shutdown(&handle);
    });
}

/// 终端流全链路:快照(历史尾部)→ 水位精确衔接 live 输出 → 输入回写 PTY → resize 回显。
/// 历史文件走真实 ~/.aeroric 路径,任务 id 唯一化并在测试尾部清理。
#[test]
fn terminal_stream_snapshot_live_input_and_resize() {
    struct SharedWriter(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    app.manage(empty_task_manager());
    let handle = app.handle().clone();

    let task_id = format!("remote-m2-test-{}", uuid::Uuid::new_v4());
    let written = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));

    // 假 PTY writer:验证 Input 帧最终落到 pty_writers
    {
        let tm = handle.state::<crate::TaskManager>();
        tm.pending_pty_sizes
            .lock()
            .insert(task_id.clone(), (120, 40));
        tm.pty_writers.lock().insert(
            task_id.clone(),
            std::sync::Arc::new(parking_lot::Mutex::new(
                Box::new(SharedWriter(written.clone())) as Box<dyn std::io::Write + Send>,
            )),
        );
    }
    // 种下快照历史(mirror reader 线程行为:append 在 publish 之前)
    crate::storage::append_task_terminal_history(&task_id, "SNAPSHOT-你好").expect("seed history");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let handle = handle.clone();
        let task_id = task_id.clone();
        let written = written.clone();
        tauri::async_runtime::block_on(async move {
            let state = handle.state::<RemoteState>();
            let server_pub = state.keys.public_b64();
            let invite = state.auth.lock().create_invite().expect("invite");
            let server = server::start(handle.clone(), 0)
                .await
                .expect("server start");
            let port = server.port;

            let mut phone = connect(port, &server_pub).await;
            phone
                .send_json(json!({"v":2,"id":1,"method":"auth","params":{"invite":invite,"deviceName":"Term Phone"}}))
                .await;
            assert_eq!(phone.next_json().await["ok"], json!(true));

            // ── 订阅:快照三段 ──
            const STREAM: u32 = 7;
            phone
                .send_terminal_frame(TerminalFrame::new(
                    TerminalOpcode::Subscribe,
                    STREAM,
                    0,
                    serde_json::to_vec(&json!({"taskId": task_id})).unwrap(),
                ))
                .await;
            let start = phone.next_terminal_frame().await;
            assert_eq!(start.opcode, TerminalOpcode::SnapshotStart);
            assert_eq!(start.stream_id, STREAM);
            let meta: Value = serde_json::from_slice(&start.payload).unwrap();
            assert_eq!(meta["live"], json!(true));
            let chunk = phone.next_terminal_frame().await;
            assert_eq!(chunk.opcode, TerminalOpcode::SnapshotChunk);
            assert_eq!(
                std::str::from_utf8(&chunk.payload).unwrap(),
                "SNAPSHOT-你好"
            );
            assert_eq!(
                phone.next_terminal_frame().await.opcode,
                TerminalOpcode::SnapshotEnd
            );

            // ── live 输出:append + publish(水位之上才透传) ──
            crate::storage::append_task_terminal_history(&task_id, "LIVE-输出")
                .expect("append live");
            super::terminal_hub::hub().publish(&task_id, "LIVE-输出");
            let output = phone.next_terminal_frame().await;
            assert_eq!(output.opcode, TerminalOpcode::Output);
            assert_eq!(std::str::from_utf8(&output.payload).unwrap(), "LIVE-输出");
            assert_eq!(output.seq, 1);

            // ── 输入回写 ──
            phone
                .send_terminal_frame(TerminalFrame::new(
                    TerminalOpcode::Input,
                    STREAM,
                    0,
                    b"y\r".to_vec(),
                ))
                .await;
            // 输入是异步消费的,轮询等待落盘
            let mut got_input = false;
            for _ in 0..50 {
                if written.lock().as_slice() == b"y\r" {
                    got_input = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(got_input, "input frame should reach the PTY writer");

            // ── resize:无 master → 记入 pending,并回显 Resized ──
            phone
                .send_terminal_frame(TerminalFrame::new(
                    TerminalOpcode::Resize,
                    STREAM,
                    0,
                    serde_json::to_vec(&json!({"cols": 92, "rows": 38})).unwrap(),
                ))
                .await;
            let resized = phone.next_terminal_frame().await;
            assert_eq!(resized.opcode, TerminalOpcode::Resized);
            let size: Value = serde_json::from_slice(&resized.payload).unwrap();
            assert_eq!(size["cols"], json!(92));
            {
                let tm = handle.state::<crate::TaskManager>();
                assert_eq!(tm.pending_pty_sizes.lock().get(&task_id), Some(&(92, 38)));
            }

            // ── unsubscribe:最后一个远程订阅释放后恢复订阅前桌面尺寸 ──
            phone
                .send_terminal_frame(TerminalFrame::new(
                    TerminalOpcode::Unsubscribe,
                    STREAM,
                    0,
                    Vec::new(),
                ))
                .await;
            let mut restored = false;
            for _ in 0..50 {
                let size = {
                    let tm = handle.state::<crate::TaskManager>();
                    let current = tm.pending_pty_sizes.lock().get(&task_id).copied();
                    current
                };
                if size == Some((120, 40)) {
                    restored = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(restored, "unsubscribe should restore the desktop PTY size");

            // ── 每连接最多 8 个终端流,第 9 个明确拒绝 ──
            for stream_id in 20..28 {
                phone
                    .send_terminal_frame(TerminalFrame::new(
                        TerminalOpcode::Subscribe,
                        stream_id,
                        0,
                        serde_json::to_vec(&json!({"taskId": task_id})).unwrap(),
                    ))
                    .await;
                assert_eq!(
                    phone.next_terminal_frame().await.opcode,
                    TerminalOpcode::SnapshotStart
                );
                assert_eq!(
                    phone.next_terminal_frame().await.opcode,
                    TerminalOpcode::SnapshotChunk
                );
                assert_eq!(
                    phone.next_terminal_frame().await.opcode,
                    TerminalOpcode::SnapshotEnd
                );
            }
            phone
                .send_terminal_frame(TerminalFrame::new(
                    TerminalOpcode::Subscribe,
                    28,
                    0,
                    serde_json::to_vec(&json!({"taskId": task_id})).unwrap(),
                ))
                .await;
            let limit_error = phone.next_terminal_frame().await;
            assert_eq!(limit_error.opcode, TerminalOpcode::Error);
            assert!(std::str::from_utf8(&limit_error.payload)
                .unwrap()
                .contains("Too many terminal streams"));

            server.shutdown(&handle);
        });
    }));

    // 清理真实目录中的测试历史文件,无论断言成败
    let _ = crate::storage::delete_task_terminal_histories(vec![task_id]);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// M3 任务操作 RPC:prompt 注入、审批按键映射、create/resume 事件桥、参数校验。
/// 存储路径只做「不存在的 id → 报错」的只读断言,不写真实 ~/.aeroric。
#[test]
fn m3_task_rpcs_input_respond_and_event_bridge() {
    struct SharedWriter(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    app.manage(empty_task_manager());
    let handle = app.handle().clone();

    let task_id = format!("remote-m3-test-{}", uuid::Uuid::new_v4());
    let written = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));

    // 桌面前端视角:remote-task-request 事件桥(listen_any 与 webview 同源)
    let (req_tx, req_rx) = std::sync::mpsc::channel::<Value>();
    let request_handle = handle.clone();
    handle.listen_any("remote-task-request", move |event| {
        if let Ok(payload) = serde_json::from_str::<Value>(event.payload()) {
            let _ = req_tx.send(payload.clone());
            if let Some(request_id) = payload.get("requestId").and_then(Value::as_str) {
                let result = if payload["kind"] == json!("create") {
                    Ok(json!({ "accepted": true, "taskId": "desktop-created-task" }))
                } else {
                    Err("Desktop could not restore the requested session".to_string())
                };
                let _ = request_handle
                    .state::<RemoteState>()
                    .task_requests
                    .resolve(request_id, result);
            }
        }
    });

    tauri::async_runtime::block_on(async move {
        let state = handle.state::<RemoteState>();
        let server_pub = state.keys.public_b64();
        let invite = state.auth.lock().create_invite().expect("invite");
        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        let mut phone = connect(port, &server_pub).await;
        phone
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"invite":invite,"deviceName":"M3 Phone"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(true));

        // ── task.input:任务不在运行 → 明确报错 ──
        phone
            .send_json(
                json!({"v":2,"id":2,"method":"task.input","params":{"taskId":task_id,"text":"hi"}}),
            )
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(false));
        assert!(reply["error"].as_str().unwrap().contains("not running"));

        // 注入假 PTY writer 后:bracketed paste 包裹 + 回车提交
        {
            let tm = handle.state::<crate::TaskManager>();
            tm.pty_writers.lock().insert(
                task_id.clone(),
                std::sync::Arc::new(parking_lot::Mutex::new(
                    Box::new(SharedWriter(written.clone())) as Box<dyn std::io::Write + Send>,
                )),
            );
        }
        phone
            .send_json(json!({"v":2,"id":3,"method":"task.input","params":{"taskId":task_id,"text":"fix the bug\nthen run tests\n"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(true));
        assert_eq!(
            String::from_utf8(written.lock().clone()).unwrap(),
            "\x1b[200~fix the bug\nthen run tests\x1b[201~\r"
        );

        // ── task.respond:没有当前审批 requestId 时拒绝,也不再开放原始 keys 注入 ──
        written.lock().clear();
        phone
            .send_json(json!({"v":2,"id":4,"method":"task.respond","params":{"taskId":task_id,"requestId":"stale","action":"approve"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(false));
        assert!(written.lock().is_empty());

        phone
            .send_json(json!({"v":2,"id":5,"method":"task.respond","params":{"taskId":task_id,"requestId":"current","action":"keys","keys":"2\r"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(false));
        assert!(written.lock().is_empty());

        // ── task.create:参数校验 + 事件桥 ──
        phone
            .send_json(json!({"v":2,"id":6,"method":"task.create","params":{"projectId":"p1","prompt":"do it","agent":"claude","permissionMode":"yolo"}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(
            reply["ok"],
            json!(false),
            "bad permissionMode must be rejected"
        );

        phone
            .send_json(json!({"v":2,"id":7,"method":"task.create","params":{"projectId":"p1","prompt":"do it","agent":"claude","permissionMode":"ask"}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["result"]["taskId"], json!("desktop-created-task"));
        let request = req_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("remote-task-request emitted");
        assert_eq!(request["kind"], json!("create"));
        assert_eq!(request["projectId"], json!("p1"));
        assert_eq!(request["prompt"], json!("do it"));
        assert_eq!(request["permissionMode"], json!("ask"));

        // ── task.resume:桌面拒绝会回传真实失败,不能提前返回 accepted ──
        phone
            .send_json(json!({"v":2,"id":8,"method":"task.resume","params":{"projectId":"p1","taskId":task_id}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(false));
        assert!(reply["error"]
            .as_str()
            .unwrap()
            .contains("could not restore"));
        let request = req_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("resume request emitted");
        assert_eq!(request["kind"], json!("resume"));
        assert_eq!(request["taskId"], json!(task_id.clone()));

        // ── 存储只读校验:不存在的项目 → 明确报错(不写任何文件) ──
        let ghost = format!("ghost-{}", uuid::Uuid::new_v4().simple());
        for (id, method) in [
            (9u64, "session.messages"),
            (10, "task.cancel"),
            (11, "task.complete"),
        ] {
            phone
                .send_json(json!({"v":2,"id":id,"method":method,"params":{"projectId":ghost,"taskId":"t1"}}))
                .await;
            let reply = phone.next_json().await;
            assert_eq!(
                reply["ok"],
                json!(false),
                "{method} should fail for ghost project"
            );
            assert!(reply["error"].as_str().unwrap().contains("not found"));
        }

        server.shutdown(&handle);
    });
}

/// M3 会话增量推送:watcher 钩子解析新行并广播 session.appended;
/// 解析不出消息的批次不推送。
#[test]
fn m3_session_appended_push_reaches_clients() {
    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    let handle = app.handle().clone();

    tauri::async_runtime::block_on(async move {
        let state = handle.state::<RemoteState>();
        let server_pub = state.keys.public_b64();
        let invite = state.auth.lock().create_invite().expect("invite");
        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        let mut phone = connect(port, &server_pub).await;
        phone
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"invite":invite,"deviceName":"Sess Phone"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(true));

        // 无消息批(纯噪音行)不推送;随后的有效批必须是客户端收到的第一条推送
        super::publish_session_appended(
            &handle,
            "task-sess",
            &[
                "not json at all".to_string(),
                json!({"type":"summary"}).to_string(),
            ],
            false,
        );
        let assistant_line = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "让我看看这个文件"},
                {"type": "tool_use", "id": "tu1", "name": "Read", "input": {"path": "a.rs"}}
            ]}
        })
        .to_string();
        super::publish_session_appended(&handle, "task-sess", &[assistant_line], false);

        let push = phone.next_json().await;
        assert_eq!(push["push"], json!("session.appended"));
        assert_eq!(push["data"]["task_id"], json!("task-sess"));
        let messages = push["data"]["messages"].as_array().expect("messages array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], json!("assistant"));
        assert_eq!(messages[0]["content"][0]["type"], json!("text"));
        assert_eq!(messages[0]["content"][1]["type"], json!("tool_use"));
        assert_eq!(messages[0]["content"][1]["name"], json!("Read"));

        server.shutdown(&handle);
    });
}

/// M5:推送 watermark 补发(events.since)与只读 files/git RPC 的窄面校验。
/// 存储相关只做「不存在的项目 → 报错」「路径穿越 → 拒绝」的只读断言。
#[test]
fn m5_event_watermark_and_readonly_rpc_guards() {
    let app = tauri::test::mock_app();
    app.manage(RemoteState::new_in_memory());
    let handle = app.handle().clone();

    tauri::async_runtime::block_on(async move {
        let state = handle.state::<RemoteState>();
        let server_pub = state.keys.public_b64();
        let invite = state.auth.lock().create_invite().expect("invite");
        let server = server::start(handle.clone(), 0)
            .await
            .expect("server start");
        let port = server.port;

        let mut phone = connect(port, &server_pub).await;
        phone
            .send_json(json!({"v":2,"id":1,"method":"auth","params":{"invite":invite,"deviceName":"M5 Phone"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(true));

        // ── 推送携带 seq(进事件日志) ──
        handle
            .emit("task-status", json!({"task_id":"t1","status":"running"}))
            .expect("emit running");
        let first = phone.next_json().await;
        assert_eq!(first["push"], json!("task-status"));
        assert_eq!(first["seq"], json!(1));
        handle
            .emit(
                "task-status",
                json!({"task_id":"t1","status":"input_required"}),
            )
            .expect("emit input_required");
        let second = phone.next_json().await;
        assert_eq!(second["seq"], json!(2));

        // ── watermark 补发:after=1 只补 seq 2 ──
        phone
            .send_json(json!({"v":2,"id":2,"method":"events.since","params":{"after":1}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(true), "events.since reply: {reply}");
        let result = &reply["result"];
        assert_eq!(result["latestSeq"], json!(2));
        assert_eq!(result["reset"], json!(false));
        let events = result["events"].as_array().expect("events array");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["seq"], json!(2));
        assert_eq!(events[0]["event"], json!("task-status"));
        assert_eq!(events[0]["data"]["status"], json!("input_required"));

        // 无 watermark(after=0)→ 不补发
        phone
            .send_json(json!({"v":2,"id":3,"method":"events.since","params":{"after":0}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["result"]["events"].as_array().unwrap().len(), 0);
        assert_eq!(reply["result"]["latestSeq"], json!(2));

        // ── 只读 RPC 守卫 ──
        let ghost = format!("ghost-{}", uuid::Uuid::new_v4().simple());
        for (id, method) in [(4u64, "project.files"), (5, "git.changes")] {
            phone
                .send_json(json!({"v":2,"id":id,"method":method,"params":{"projectId":ghost}}))
                .await;
            let reply = phone.next_json().await;
            assert_eq!(
                reply["ok"],
                json!(false),
                "{method} must fail for ghost project"
            );
            assert!(reply["error"].as_str().unwrap().contains("not found"));
        }
        // 路径穿越在项目解析之前就被拒绝
        phone
            .send_json(json!({"v":2,"id":6,"method":"git.diff","params":{"projectId":ghost,"path":"../../etc/passwd"}}))
            .await;
        let reply = phone.next_json().await;
        assert_eq!(reply["ok"], json!(false));
        assert!(reply["error"]
            .as_str()
            .unwrap()
            .contains("Invalid file path"));
        phone
            .send_json(json!({"v":2,"id":7,"method":"project.readFile","params":{"projectId":ghost,"path":"/etc/passwd"}}))
            .await;
        assert_eq!(phone.next_json().await["ok"], json!(false));

        server.shutdown(&handle);
    });
}
