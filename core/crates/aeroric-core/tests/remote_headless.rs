//! P8-03 刀三b 第一片：自宿主 `remote/auth_stress_tests.rs` 逐字迁移（AuthStore 已上收 core）。
//! 后续片：lan_roundtrip 的 ws/auth/rpc 用例随 core server 上收迁入本文件（▶ 步 4 锚点）。

//! `AuthStore` 的爆破 / 并发压力测试(计划 4 的第一块)。
//!
//! **为什么不测「猜中 token」**:invite 和 device token 都是 32 字节 CSPRNG
//! → base64url(`generate_token`),256 位熵。写个循环去猜是在演戏,不是测试。
//! 真正会出错的是**猜不中时的那条路径**:
//!
//! | 风险 | 本文件对应的用例 |
//! |---|---|
//! | 限流闸门有 TOCTOU,并发能突破 | `concurrent_bad_invites_*` |
//! | 单次使用的 invite 在竞态下被用两次 | `single_use_invite_survives_a_concurrent_stampede` |
//! | 一个 IP 被封会牵连别的 IP | `throttle_is_per_peer_*` |
//! | 攻击者用大量 IP 把限流表撑爆内存 | `throttle_map_stays_bounded_*` |
//! | 报错文案区分「不认识」和「差一点」,给出枚举预言机 | `failure_messages_do_not_distinguish_*` |
//!
//! 全部只在进程内跑,不开端口、不碰真实文件(`AuthStore::in_memory`)。

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Barrier};

use parking_lot::Mutex;

use aeroric_core::remote::auth::{AuthOutcome, AuthStore, THROTTLE_FREE_FAILURES};

fn peer(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(10, 0, 0, last))
}

/// 和生产一致的持有方式:`RemoteState` 就是把 `AuthStore` 放在 parking_lot Mutex 里。
fn shared_store() -> Arc<Mutex<AuthStore>> {
    Arc::new(Mutex::new(AuthStore::in_memory()))
}

/// 取出错误文案。
///
/// **不用 `Result::expect_err`,因为它要求 `AuthOutcome: Debug`** ——
/// 而 `AuthOutcome::Paired` 里装着明文 device token,给它派生 Debug 就等于
/// 给「某天某处顺手写个 `{:?}` 把长期凭据打进日志」开了门。为一个测试便利
/// 去放宽生产类型的可打印性不值得,所以这里自己接管这一步。
fn err_of(result: Result<AuthOutcome, String>, context: &str) -> String {
    match result {
        Ok(_) => panic!("{context}"),
        Err(err) => err,
    }
}

/// 让 `threads` 个线程在同一瞬间(Barrier 对齐)各跑一次 `op`,收集返回值。
fn stampede<T, F>(threads: usize, op: F) -> Vec<T>
where
    T: Send + 'static,
    F: Fn(usize) -> T + Send + Sync + 'static,
{
    let barrier = Arc::new(Barrier::new(threads));
    let op = Arc::new(op);
    let handles: Vec<_> = (0..threads)
        .map(|i| {
            let barrier = Arc::clone(&barrier);
            let op = Arc::clone(&op);
            std::thread::spawn(move || {
                // 对齐:尽量让所有线程同时撞上临界区,而不是排队进去。
                barrier.wait();
                op(i)
            })
        })
        .collect();
    handles
        .into_iter()
        .map(|h| h.join().expect("worker thread panicked"))
        .collect()
}

// ── 限流闸门的并发正确性 ─────────────────────────────────────────────────────

#[test]
fn concurrent_bad_invites_end_with_the_peer_blocked() {
    // 这一条钉的是「闸门没有 TOCTOU」。`authenticate` 内部是
    // 先 `throttle_wait` 再 `record_failure` 两步,如果调用方在两步之间放锁,
    // N 个并发请求就能全部通过闸门、把免费次数当成 N 而不是 3。
    // 生产的持有方式是整个 `authenticate` 在一次 lock 内完成(remote/mod.rs:187),
    // 所以这里也整体加锁 —— 断言的是这个组合的最终状态。
    let store = shared_store();
    let attempts = 64;

    let results = {
        let store = Arc::clone(&store);
        stampede(attempts, move |i| {
            let name = format!("attacker-{i}");
            store
                .lock()
                .authenticate(peer(7), Some("wrong-invite"), None, Some(&name))
                .is_ok()
        })
    };

    // 没有一次能成功 —— invite 是错的,任何并发交错都不该放行。
    assert_eq!(results.iter().filter(|ok| **ok).count(), 0);

    // 收工后该 IP 必须处于封禁态。用「再来一次」间接观察:
    // 封禁态下的报错是限流文案,不是凭据文案。
    let err = err_of(
        store
            .lock()
            .authenticate(peer(7), Some("wrong-invite"), None, None),
        "blocked peer must not be allowed through",
    );
    assert!(
        err.contains("Too many failed attempts"),
        "并发爆破之后应当处于封禁态,实际报错: {err}"
    );
}

#[test]
fn throttle_blocks_within_the_free_allowance_even_under_concurrency() {
    // 上一条只断言「最后被封」,封在第几次没管。这一条把次数钉住:
    // 免费次数是 THROTTLE_FREE_FAILURES,所以能拿到「凭据错误」文案的次数
    // 不应超过它 —— 之后全部应该是限流文案。
    let store = shared_store();
    let attempts = 32;

    let messages = {
        let store = Arc::clone(&store);
        stampede(attempts, move |_| {
            err_of(
                store
                    .lock()
                    .authenticate(peer(8), Some("wrong-invite"), None, None),
                "bad invite must fail",
            )
        })
    };

    let credential_errors = messages
        .iter()
        .filter(|m| m.contains("Invalid or expired invite"))
        .count();
    let throttle_errors = messages
        .iter()
        .filter(|m| m.contains("Too many failed attempts"))
        .count();

    assert_eq!(
        credential_errors + throttle_errors,
        attempts,
        "所有失败都应落在这两类文案里,实际: {messages:?}"
    );
    assert!(
        credential_errors <= THROTTLE_FREE_FAILURES as usize + 1,
        "凭据错误只该出现 <= {} 次(免费额度),实际 {credential_errors} 次 —— \
         说明限流闸门在并发下被绕过了",
        THROTTLE_FREE_FAILURES as usize + 1
    );
    assert!(
        throttle_errors > 0,
        "32 次并发爆破里应当出现限流文案,实际一次都没有"
    );
}

// ── invite 的一次性语义 ──────────────────────────────────────────────────────

#[test]
fn single_use_invite_survives_a_concurrent_stampede() {
    // invite 是一次性的。N 个线程拿**同一个正确的** invite 同时冲进来,
    // 只能有一个配对成功;其余必须失败且不产生第二个设备。
    // 这是本文件里唯一一条「正确凭据」的并发用例,也是最容易出竞态的地方 ——
    // `authenticate_inner` 里「查 invite → 落盘设备 → 删 invite」是三步。
    let store = shared_store();
    let invite = store.lock().create_invite().expect("invite");
    let threads = 32;

    let outcomes = {
        let store = Arc::clone(&store);
        let invite = invite.clone();
        stampede(threads, move |i| {
            let name = format!("phone-{i}");
            match store
                .lock()
                .authenticate(peer(9), Some(&invite), None, Some(&name))
            {
                Ok(AuthOutcome::Paired { device_id, .. }) => Some(device_id),
                Ok(_) => panic!("invite 路径只应产生 Paired"),
                Err(_) => None,
            }
        })
    };

    let paired: Vec<_> = outcomes.into_iter().flatten().collect();
    assert_eq!(
        paired.len(),
        1,
        "同一个 invite 在 {threads} 路并发下只能成功一次,实际成功 {} 次",
        paired.len()
    );

    // 设备表里也只能多出这一台 —— 断言成功次数还不够,
    // 万一失败路径也悄悄 push 了一条,上面那条断言照样通过。
    let store = store.lock();
    assert_eq!(store.devices().len(), 1, "设备表不应出现第二台设备");
    assert!(store.contains_device(&paired[0]));
}

#[test]
fn a_consumed_invite_cannot_be_replayed() {
    // 上一条是并发视角,这一条是时间视角:成功配对之后,同一个 invite 再用必须失败。
    // 分开写是因为两者会被不同的 bug 打破 —— 竞态 vs. 忘了 remove。
    let store = shared_store();
    let invite = store.lock().create_invite().expect("invite");

    let first = store
        .lock()
        .authenticate(peer(10), Some(&invite), None, Some("Phone"));
    assert!(matches!(first, Ok(AuthOutcome::Paired { .. })));

    let replay = err_of(
        store
            .lock()
            .authenticate(peer(10), Some(&invite), None, Some("Phone")),
        "consumed invite must not authenticate again",
    );
    assert!(
        replay.contains("Invalid or expired invite"),
        "重放已消费的 invite 应当报凭据无效,实际: {replay}"
    );
    assert_eq!(store.lock().devices().len(), 1, "重放不应再建设备");
}

// ── 限流的隔离性与状态增长 ───────────────────────────────────────────────────

#[test]
fn throttle_is_per_peer_and_does_not_punish_bystanders() {
    // 限流按 IP 记账。把 A 打到封禁,B 必须仍能正常配对 ——
    // 否则局域网里一台设备输错几次就能把全家人都锁在门外(DoS)。
    let store = shared_store();
    let invite = store.lock().create_invite().expect("invite");

    for _ in 0..(THROTTLE_FREE_FAILURES + 4) {
        let _ = store
            .lock()
            .authenticate(peer(20), Some("wrong-invite"), None, None);
    }
    let blocked = err_of(
        store
            .lock()
            .authenticate(peer(20), Some("wrong-invite"), None, None),
        "peer 20 should be blocked",
    );
    assert!(blocked.contains("Too many failed attempts"));

    // 旁观者 IP 拿正确 invite 必须一次通过。
    let bystander = store
        .lock()
        .authenticate(peer(21), Some(&invite), None, Some("Innocent"));
    // 先取错误文案再判断:`AuthOutcome` 没有 Debug,而且 `matches!` 会移动 Result,
    // 移动之后再在断言消息里用 `.err()` 是借用检查错误。
    let failure = bystander.as_ref().err().cloned();
    assert!(
        matches!(bystander, Ok(AuthOutcome::Paired { .. })),
        "另一个 IP 不应被牵连,实际报错: {failure:?}"
    );
}

#[test]
fn throttle_map_stays_bounded_under_a_distributed_attack() {
    // 限流表按 IP 建条目。攻击者换 IP 就能新建一条,所以必须有上限 ——
    // 否则这张表本身就是内存放大器。`record_failure` 里的清理阈值是 1024。
    //
    // 每个 IP 只打 1 次(<= 免费额度),故意让条目**处于未封禁态** ——
    // 这正是清理逻辑会回收的那一类。只打一次也更贴近真实的分布式扫描。
    let store = shared_store();
    let peers = 4096;

    for i in 0..peers {
        let octets = ((i as u32) >> 8) as u8;
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, octets, (i % 256) as u8));
        let _ = store.lock().authenticate(ip, Some("wrong"), None, None);
    }

    let tracked = store.lock().throttle_len();
    assert!(
        tracked <= 1025,
        "{peers} 个不同 IP 各失败一次后,限流表应被清理到 <= 1025 条,实际 {tracked} 条"
    );
}

// ── 报错文案不做区分 ────────────────────────────────────────────────────────

#[test]
fn failure_messages_do_not_distinguish_near_misses() {
    // 报错文案不能透露「这个 token 存在但过期了」「前缀对了」之类的信息,
    // 否则攻击者可以拿文案当预言机做枚举。同一类凭据的所有失败必须同一句话。
    //
    // 注意每次都换 IP:同一个 IP 打几次就进限流了,文案会变成限流文案,
    // 那时候比较的就不是凭据路径了(这一步很容易写错成假绿)。
    let store = shared_store();
    let real = store.lock().create_invite().expect("invite");

    let mut probes = vec![
        String::from(""),
        String::from("x"),
        String::from("wrong-invite"),
        // 和真 invite 同长度、只差最后一个字符 —— 「差一点」的典型形态
        {
            let mut near = real.clone();
            near.pop();
            near.push(if real.ends_with('A') { 'B' } else { 'A' });
            near
        },
        // 前缀正确但被截断
        real[..real.len() / 2].to_string(),
    ];
    probes.push("A".repeat(real.len()));

    let mut messages = HashSet::new();
    for (i, probe) in probes.iter().enumerate() {
        let err = err_of(
            store
                .lock()
                .authenticate(peer(100 + i as u8), Some(probe), None, None),
            "none of these probes is the real invite",
        );
        messages.insert(err);
    }

    assert_eq!(
        messages.len(),
        1,
        "所有猜错 invite 的失败必须是同一句话,实际出现了 {} 种: {messages:?}",
        messages.len()
    );

    // device token 路径同理,单独跑一遍(它走的是另一段代码:线性扫描 + 常量时间比较)。
    let mut token_messages = HashSet::new();
    for (i, probe) in probes.iter().enumerate() {
        let err = err_of(
            store
                .lock()
                .authenticate(peer(150 + i as u8), None, Some(probe), None),
            "none of these probes is a real device token",
        );
        token_messages.insert(err);
    }
    assert_eq!(
        token_messages.len(),
        1,
        "所有猜错 device token 的失败必须是同一句话,实际: {token_messages:?}"
    );
}

#[test]
fn a_revoked_device_token_fails_like_an_unknown_one() {
    // 撤销之后,那个 token 的报错必须和「从没见过的 token」一模一样。
    // 如果撤销留下了可区分的文案,攻击者就能确认某个 token 曾经有效。
    let store = shared_store();
    let invite = store.lock().create_invite().expect("invite");

    let token = match store
        .lock()
        .authenticate(peer(200), Some(&invite), None, Some("Phone"))
    {
        Ok(AuthOutcome::Paired { device_token, .. }) => device_token,
        Ok(_) => panic!("invite 路径只应产生 Paired"),
        Err(err) => panic!("expected pairing, got error: {err}"),
    };
    let device_id = store.lock().devices()[0].id.clone();
    assert!(store.lock().revoke(&device_id).expect("revoke"));

    let revoked_err = err_of(
        store
            .lock()
            .authenticate(peer(201), None, Some(&token), None),
        "revoked token must not authenticate",
    );
    let unknown_err = err_of(
        store
            .lock()
            .authenticate(peer(202), None, Some("never-issued"), None),
        "unknown token must not authenticate",
    );

    assert_eq!(
        revoked_err, unknown_err,
        "已撤销的 token 和从未签发的 token 报错必须一致"
    );
}

// ── 刀四：headless 服务面 e2e ────────────────────────────────────────────
//
// 验收「关闭全部 GUI、只跑 `aeroric-cli serve` 时，fake 配对客户端可
// list/create/cancel task」的机器证据：core WS server + 无头 handle
// （CLI 生产装配在 aeroric-host 复用同一 trait，方法面与本 fake 一致）。

use aeroric_core::remote::crypto;
use aeroric_core::remote::server;
use aeroric_core::remote::services::{RemoteHostHandle, RemoteTerminalStreams};
use aeroric_core::remote::state::{ListenerScope, OutboundSender, RemoteState};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;

/// 无头 handle：存储/任务面全部脚本化应答，shape 与 CLI 生产装配一致。
struct HeadlessHandle;

#[async_trait]
impl RemoteHostHandle for HeadlessHandle {
    fn desktop_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    fn emit(&self, _event: &str, _payload: Value) {}
    fn audit_log(&self, _event: &str, _payload: Value) {}

    async fn load_projects(&self) -> Result<Value, String> {
        Ok(json!([{ "id": "p1", "name": "headless", "path": "/tmp/p1" }]))
    }

    async fn set_project_pinned(&self, _project_id: &str, _pinned: bool) -> Result<(), String> {
        Ok(())
    }

    async fn load_project_tasks(&self, project_id: &str) -> Result<Value, String> {
        if project_id == "p1" {
            Ok(json!([{ "id": "t1", "projectId": "p1", "status": "todo" }]))
        } else {
            Ok(json!([]))
        }
    }

    async fn task_input(&self, task_id: &str, _text: &str) -> Result<Value, String> {
        Err(format!("Task is not running: {task_id}"))
    }

    async fn task_respond(&self, _task_id: &str, _params: Value) -> Result<Value, String> {
        Err("Task is not running".to_string())
    }

    async fn task_cancel(&self, _project_id: &str, task_id: &str) -> Result<Value, String> {
        Ok(json!({ "ok": true, "taskId": task_id, "status": "cancelled" }))
    }

    async fn task_complete(&self, _project_id: &str, task_id: &str) -> Result<Value, String> {
        Ok(json!({ "ok": true, "taskId": task_id, "status": "done" }))
    }

    async fn task_request(&self, payload: Value) -> Result<Value, String> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("create");
        if kind == "create" {
            let prompt = payload
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(json!({
                "accepted": true,
                "taskId": format!("headless-{prompt_len}", prompt_len = prompt.len()),
                "task": Value::Null,
            }))
        } else {
            Err("Task not found: none".to_string())
        }
    }

    async fn session_messages(&self, _params: Value) -> Result<Value, String> {
        Ok(json!({ "available": false }))
    }

    async fn project_files(&self, _params: Value) -> Result<Value, String> {
        Ok(json!([]))
    }

    async fn project_read_file(&self, _params: Value) -> Result<Value, String> {
        Err("not found".to_string())
    }

    async fn project_write_file(&self, _params: Value) -> Result<Value, String> {
        Ok(json!({ "ok": true }))
    }

    async fn git_changes(&self, _params: Value) -> Result<Value, String> {
        Ok(json!([]))
    }

    async fn git_diff(&self, _params: Value) -> Result<Value, String> {
        Ok(json!(""))
    }

    async fn agent_config_list(&self) -> Result<Value, String> {
        Ok(json!([]))
    }

    async fn agent_config_save(&self, _params: Value) -> Result<Value, String> {
        Ok(json!({ "ok": true }))
    }

    async fn agent_config_detect_models(&self, _params: Value) -> Result<Value, String> {
        Ok(json!({ "models": [] }))
    }

    async fn agent_config_create(&self, _params: Value) -> Result<Value, String> {
        Ok(json!({ "ok": true }))
    }

    async fn agents_list(&self) -> Result<Value, String> {
        Ok(json!([{ "value": "claude", "family": "claude" }]))
    }

    async fn agents_models(&self, _agent: &str) -> Result<Value, String> {
        Ok(json!({ "models": [] }))
    }

    async fn stats_summary(&self) -> Result<Value, String> {
        Ok(json!({ "totalAgentsSpawned": 0 }))
    }

    fn terminal_streams(&self, _outbound: OutboundSender) -> Box<dyn RemoteTerminalStreams> {
        struct NoStreams;
        impl RemoteTerminalStreams for NoStreams {
            fn handle_frame(&mut self, _bytes: &[u8]) {}
            fn abort_all(&mut self) {}
        }
        Box::new(NoStreams)
    }
}

const HEADLESS_WAIT: Duration = Duration::from_secs(5);

/// 已配对的 headless 测试客户端（hello → E2EE 握手 → auth）。
type ClientWs = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct HeadlessClient {
    ws: ClientWs,
    session: crypto::SessionCrypto,
}

impl HeadlessClient {
    async fn send_ctrl(&mut self, value: Value) {
        let frame = self
            .session
            .encrypt_frame(crypto::KIND_CTRL, value.to_string().as_bytes())
            .expect("encrypt ctrl");
        self.ws.send(Message::Binary(frame)).await.expect("send");
    }

    async fn next_ctrl(&mut self) -> Value {
        loop {
            let msg = tokio::time::timeout(HEADLESS_WAIT, self.ws.next())
                .await
                .expect("timed out waiting for frame")
                .expect("stream ended")
                .expect("frame error");
            match msg {
                Message::Binary(bytes) => {
                    let (kind, plain) = self.session.decrypt_frame(&bytes).expect("decrypt");
                    assert_eq!(kind, crypto::KIND_CTRL);
                    return serde_json::from_str(&String::from_utf8(plain).expect("utf8"))
                        .expect("json");
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                other => panic!("unexpected frame: {other:?}"),
            }
        }
    }
}

async fn headless_connect(port: u16, server_pub: &str, invite: &str) -> HeadlessClient {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let handshake = crypto::initiate_handshake(server_pub).expect("initiate handshake");
    ws.send(Message::Text(
        json!({"v":2,"type":"hello","pub": handshake.eph_pub_b64}).to_string(),
    ))
    .await
    .expect("send hello");
    let ack = loop {
        let msg = tokio::time::timeout(HEADLESS_WAIT, ws.next())
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
    let mut client = HeadlessClient { ws, session };
    client
        .send_ctrl(json!({"v":2,"id":0,"method":"auth","params":{"invite":invite,"deviceName":"Headless"}}))
        .await;
    let auth = client.next_ctrl().await;
    assert_eq!(auth["ok"], json!(true), "auth: {auth}");
    client
}

async fn headless_rpc(client: &mut HeadlessClient, id: u64, method: &str, params: Value) -> Value {
    client
        .send_ctrl(json!({"v":2,"id":id,"method":method,"params":params}))
        .await;
    client.next_ctrl().await
}

/// e2e：headless serve 上 fake 配对客户端完成 auth → tasks.list →
/// task.create → task.cancel 全流程（验收第 2 条的机器证据）。
#[tokio::test]
async fn headless_serve_serves_list_create_cancel_over_paired_ws() {
    let state = Arc::new(RemoteState::new_in_memory());
    let handle: Arc<dyn RemoteHostHandle> = Arc::new(HeadlessHandle);

    let invite = state.auth.lock().create_invite().expect("invite");
    let server_pub = state.keys.public_b64();
    let server = server::start_with_scope(
        Arc::clone(&state),
        Arc::clone(&handle),
        0,
        ListenerScope::Loopback,
    )
    .await
    .expect("headless server start");
    let port = server.port;

    let mut phone = headless_connect(port, &server_pub, &invite).await;

    let list = headless_rpc(&mut phone, 1, "tasks.list", json!({"projectId":"p1"})).await;
    assert_eq!(list["ok"], json!(true), "list: {list}");
    assert_eq!(list["result"][0]["id"], json!("t1"));

    let created = headless_rpc(&mut phone, 2, "task.create", json!({
        "projectId": "p1", "prompt": "do it headless", "agent": "claude", "permissionMode": "ask"
    })).await;
    assert_eq!(created["ok"], json!(true), "create: {created}");
    assert_eq!(created["result"]["accepted"], json!(true));
    assert!(created["result"]["taskId"].as_str().is_some());

    let cancelled = headless_rpc(
        &mut phone,
        3,
        "task.cancel",
        json!({
            "projectId": "p1", "taskId": "t1"
        }),
    )
    .await;
    assert_eq!(cancelled["ok"], json!(true), "cancel: {cancelled}");
    assert_eq!(cancelled["result"]["status"], json!("cancelled"));

    server.shutdown(&state);
}
