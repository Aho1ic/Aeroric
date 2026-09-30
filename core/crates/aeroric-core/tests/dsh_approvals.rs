//! dsh 启动编排的审批/投递端到端（P3-05 Action 5：fake server，不调真实付费
//! 模型；补记二百六十九）。断言面＝`run_dsh_task` 经 [`MockPorts`] 与
//! [`FakeDshServer`] 走完整编排：wire 契约、`/permission` 失败结算、斜杠命令
//! 早退、空 prompt 交互态、图片拒绝。审批绑定规则本体（take-and-consume、
//! 代隔离、重放拒绝）在 `dsh_host.rs` 的 registry 测试已钉；本文件钉的是
//! **执行器如何消费这些判定**——失败必须同时 `retire_runs` + `failed(reason)`，
//! 交互态必须 `input_required` 且不进 `pending`。

mod common;

use common::dsh_fake::{catalog, session_created, FakeDshServer, MockPorts};

use aeroric_core::agents::dsh_host::executor::{run_dsh_task_core, DshLaunch};

fn mk_launch(task_id: &str, prompt: &str) -> DshLaunch {
    DshLaunch {
        generation: 7,
        task_id: task_id.to_string(),
        agent: "dsh".to_string(),
        project_path: "/proj".to_string(),
        prompt: prompt.to_string(),
        session_id: None,
        workspace_id: None,
        agent_preset: None,
        prompt_mode: None,
        selected_model: None,
        reasoning_effort: None,
        permission_mode: None,
        images: None,
        client_time_zone: None,
    }
}

async fn wired() -> (FakeDshServer, MockPorts) {
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond("session/modelCatalog", FakeDshServer::ok(catalog()));
    // 默认 prompt 应答＝纯 ack（无 command 回答，turn 由流侧结算）。
    server.respond(
        "session/prompt",
        FakeDshServer::ok(serde_json::json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    (server, ports)
}

#[tokio::test]
async fn launch_delivers_prompt_with_wire_contract() {
    let (server, ports) = wired().await;
    run_dsh_task_core(&ports, mk_launch("t1", "hi"), ports.sink())
        .await
        .expect("launch must settle ok");

    // create_session 的 wire：request.cwd 是本地工作区，恢复字段缺席。
    let created = server.payload_of("session/create", 0);
    assert_eq!(created["args"]["request"]["cwd"], "/proj");
    assert!(created["args"]["request"].get("sessionId").is_none());

    // prompt 的 wire：client-minted requestId、默认 queue、text content。
    let prompt = server.payload_of("session/prompt", 0);
    let request = &prompt["args"]["request"];
    assert_eq!(request["sessionId"], "s1");
    assert_eq!(request["mode"], "queue");
    assert!(request["requestId"]
        .as_str()
        .is_some_and(|id| !id.trim().is_empty()));
    assert_eq!(request["content"][0]["type"], "text");
    assert_eq!(request["content"][0]["text"], "hi");

    // 端口事件序：清账→登记→流→投递（truncate 在 fresh 分支必须出现）。
    let events = ports.events();
    assert!(events.contains(&"clear:t1".to_string()));
    assert!(events.contains(&"truncate:t1".to_string()));
    assert!(events.contains(&"run:t1:7".to_string()));
    assert!(events.contains(&"stream:t1".to_string()));
    assert!(events
        .iter()
        .any(|e| e.starts_with("session:t1:7:/fake/sessions/")));
    assert_eq!(
        *ports.statuses.lock(),
        vec![("t1".to_string(), "pending".to_string(), None)]
    );
}

#[tokio::test]
async fn permission_command_unavailable_settles_failed_and_retires() {
    let (server, ports) = wired().await;
    // 该 preset 的 agent 不支持 `/permission`：value 为 null 即不可用。
    server.respond(
        "commands/execute",
        FakeDshServer::ok(serde_json::Value::Null),
    );
    let mut launch = mk_launch("t1", "hi");
    launch.permission_mode = Some("full_access".to_string());

    let error = run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect_err("null permission result must fail the launch");
    assert_eq!(
        error,
        "DSH permission command is unavailable in this agent preset"
    );

    // 抑制登记先于命令、失败后撤销；prompt 绝不投递。
    let events = ports.events();
    let expect_at = events
        .iter()
        .position(|e| e == "expect:t1:permission")
        .expect("expect_internal_command must run first");
    let clear_at = events
        .iter()
        .position(|e| e == "clear-ic:t1")
        .expect("the suppression must be cleared");
    assert!(expect_at < clear_at);
    assert!(events.contains(&"retire:t1".to_string()));
    assert_eq!(server.count("session/prompt"), 0);
    assert_eq!(
        *ports.statuses.lock(),
        vec![(
            "t1".to_string(),
            "failed".to_string(),
            Some("DSH permission command is unavailable in this agent preset".to_string())
        )]
    );
}

#[tokio::test]
async fn slash_command_answer_settles_input_required_after_pending() {
    let (server, ports) = wired().await;
    server.respond(
        "session/prompt",
        FakeDshServer::ok(
            serde_json::json!({ "accepted": true, "command": { "text": "/help did stuff" } }),
        ),
    );
    run_dsh_task_core(&ports, mk_launch("t1", "/help"), ports.sink())
        .await
        .expect("inline command answer is a success path");

    // 命令回答 echo 进终端（\r\n 包边与宿主逐字一致），并结算为等待用户。
    assert_eq!(
        *ports.echoed.lock(),
        vec!["\r\n/help did stuff\r\n".to_string()],
        "echo 行必须带 CRLF 包边"
    );
    let statuses = ports.statuses.lock();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].1, "pending");
    assert_eq!(statuses[1].1, "input_required");
    assert_eq!(statuses[1].2, None);
}

#[tokio::test]
async fn empty_prompt_starts_interactive_without_pending() {
    let (server, ports) = wired().await;
    run_dsh_task_core(&ports, mk_launch("t1", "   "), ports.sink())
        .await
        .expect("terminal-only start is ok");

    assert_eq!(server.count("session/prompt"), 0, "空白 prompt 不投递");
    assert_eq!(
        *ports.statuses.lock(),
        vec![("t1".to_string(), "input_required".to_string(), None)],
        "交互态直接 input_required，不经过 pending"
    );
}

#[tokio::test]
async fn image_rejection_fails_before_wire_and_retires() {
    let (server, ports) = wired().await;
    let mut launch = mk_launch("t1", "look");
    // 白名单外的 data url：parse_prompt_image 在构造 content 时就拒绝。
    launch.images = Some(vec!["data:application/pdf;base64,AAA".to_string()]);

    let error = run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect_err("unsupported image type must fail the launch");
    assert!(!error.is_empty());
    assert_eq!(server.count("session/prompt"), 0, "拒绝发生在请求之前");
    assert!(ports.events().contains(&"retire:t1".to_string()));
    let statuses = ports.statuses.lock();
    // pending 在投递前已报（与宿主序一致），失败是其后的第二条。
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].1, "pending");
    assert_eq!(statuses[1].1, "failed");
    assert!(statuses[1].2.is_some(), "失败原因必须随上报携带");
}

#[tokio::test]
async fn broken_webui_startup_short_circuits_before_any_rpc() {
    let server = FakeDshServer::start().await;
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("t1", "hi");
    launch.agent = "broken".to_string();

    let error = run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect_err("ensure_webui failure must propagate verbatim");
    assert_eq!(error, "DSH Web URL is unavailable");
    assert_eq!(server.count("session/create"), 0);
    // ensure 之前的三步（clear/truncate）已发生，之后的登记/流/结算都没发生。
    let events = ports.events();
    assert!(events.contains(&"clear:t1".to_string()));
    assert!(!events.iter().any(|e| e.starts_with("run:")));
    assert!(!events.contains(&"retire:t1".to_string()));
    assert!(
        ports.statuses.lock().is_empty(),
        "ensure 失败走 `?` 早退，不经失败结算块"
    );
}

// ── 验收行 2：无 GUI 经 typed 方法回答审批（补记二百七十二） ──────────────
//
// 这三条全部是**同步纯逻辑**用例——没有 AppHandle、没有窗口、没有 emit，
// 证明「回答审批」这件事在 core 就能完成；「无回答时保持等待」由结构证明：
// registry 唯一退场口是 take/宿主撤回，坏请求在 consume 之前就被拒绝，
// 等待态不会被错误输入吃掉。

use aeroric_core::agents::dsh_host::approvals::{
    take_waterfall_answer, PendingWaterfall, WaterfallKind, WaterfallRegistry,
};
use serde_json::json;

fn registry_with_pending() -> WaterfallRegistry {
    let mut registry = WaterfallRegistry::default();
    registry.set_client_id("gen-1".to_string());
    registry.insert(
        "ev-1".to_string(),
        PendingWaterfall {
            kind: WaterfallKind::Approval,
            session_id: Some("s1".to_string()),
        },
    );
    registry
}

#[test]
fn answers_a_pending_approval_without_any_window() {
    let mut registry = registry_with_pending();
    let answer = take_waterfall_answer(
        &mut registry,
        "gen-1",
        "ev-1",
        &json!({ "kind": "allow", "value": true }),
    )
    .expect("typed answer must work headless");
    assert_eq!(answer.session_hint.as_deref(), Some("s1"));
    assert_eq!(answer.args["eventId"], "ev-1");
    assert_eq!(answer.args["clientId"], "gen-1");
    assert_eq!(answer.args["outcome"]["kind"], "allow");
    assert!(registry.all_pending().is_empty(), "回答即消费");
}

#[test]
fn duplicate_answer_never_resurrects_the_pending_entry() {
    let mut registry = registry_with_pending();
    take_waterfall_answer(&mut registry, "gen-1", "ev-1", &json!({ "kind": "allow" }))
        .expect("first answer");
    // 重放：consume-once 后拿不到 entry，只给幂等路由提示；本地账不回滚，
    // 网关侧的再次批准拒绝由 wire 语义承担（registry 测试 251 钉过）。
    let replay = take_waterfall_answer(&mut registry, "gen-1", "ev-1", &json!({ "kind": "allow" }))
        .expect("replay stays routable");
    assert!(replay.session_hint.is_none());
    assert!(registry.all_pending().is_empty());
}

#[test]
fn missing_kind_is_refused_before_any_consumption() {
    let mut registry = registry_with_pending();
    let error = take_waterfall_answer(&mut registry, "gen-1", "ev-1", &json!({}))
        .expect_err("outcome without kind must be refused");
    assert_eq!(error, "DSH event outcome must carry a kind");
    // 等待态完好：坏输入没吃掉 pending——「无回答时保持等待」的可观测面。
    assert_eq!(registry.all_pending().len(), 1);
}
