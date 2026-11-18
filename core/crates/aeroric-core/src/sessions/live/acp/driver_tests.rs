use super::*;
use crate::sessions::live::acp::dialect::DSH;
use crate::sessions::live::codex::fake::{until, Sink};
use crate::sessions::live::update::ToolStatus;

/// The agent's end of one launched process.
struct Peer {
    start: AcpStart,
    from_client: mpsc::UnboundedReceiver<String>,
    to_client: mpsc::UnboundedSender<String>,
}

/// `configOptions` shaped like `dsh acp` 0.2.2-rc.1's.
fn dsh_options(model: &str, effort: &str) -> Value {
    json!([
        {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": model,
         "options": [
            {"group": "deepseek-official", "name": "DeepSeek", "options": [
                {"value": "[\"deepseek-official\",\"deepseek-flash\"]", "name": "DeepSeek-V4.1-Flash"},
                {"value": "[\"deepseek-official\",\"deepseek-v4-pro\"]", "name": "DeepSeek-V4-Pro"}]},
            {"group": "deepseek-account", "name": "DeepSeek Account", "options": [
                {"value": "[\"deepseek-account\",\"deepseek-flash\"]", "name": "DeepSeek-V4.1-Flash"},
                {"value": "[\"deepseek-account\",\"deepseek-v4-pro\"]", "name": "DeepSeek-V4-Pro"}]}]},
        {"id": "reasoning_effort", "name": "Reasoning effort", "category": "thought_level", "type": "select",
         "currentValue": effort, "options": [
            {"value": "off", "name": "Off"}, {"value": "low", "name": "Low"},
            {"value": "high", "name": "High"}, {"value": "max", "name": "Max"}]}
    ])
}

impl Peer {
    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.from_client.recv())
            .await
            .expect("the driver sent nothing")
            .expect("closed");
        serde_json::from_str(&line).unwrap()
    }
    async fn expect(&mut self, method: &str) -> Value {
        let frame = self.next().await;
        assert_eq!(frame["method"], method, "{frame}");
        frame
    }
    fn send(&self, frame: Value) {
        self.to_client.send(frame.to_string()).unwrap();
    }
    fn reply(&self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }
    fn update(&self, update: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": "s1", "update": update}}));
    }
    async fn initialize(&mut self, capabilities: Value) {
        let init = self.expect("initialize").await;
        assert_eq!(init["params"]["protocolVersion"], 1);
        assert_eq!(init["params"]["clientCapabilities"]["terminal"], false);
        self.reply(
            &init,
            json!({"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []}),
        );
    }
    /// Handshake and a new session `s1`; returns the `session/new` request.
    async fn open(&mut self) -> Value {
        self.initialize(json!({"sessionCapabilities": {"close": {}, "resume": {}}}))
            .await;
        let new = self.expect("session/new").await;
        self.reply(
            &new,
            json!({"sessionId": "s1", "configOptions": dsh_options("[\"deepseek-official\",\"deepseek-flash\"]", "high")}),
        );
        new
    }
    async fn prompt(&mut self, text: &str) -> Value {
        let prompt = self.expect("session/prompt").await;
        assert_eq!(prompt["params"]["sessionId"], "s1");
        assert_eq!(prompt["params"]["prompt"][0]["text"], text);
        prompt
    }
}

struct Harness {
    session: Arc<AcpSession>,
    sink: Arc<Sink>,
    log: SharedLog,
    peers: mpsc::UnboundedReceiver<Peer>,
}

fn start_with(permission: &str, model: Option<&str>, session: Option<&str>) -> Harness {
    let (peers_tx, peers) = mpsc::unbounded_channel();
    let launcher: AcpLauncher = Arc::new(move |start: &AcpStart| {
        let (transport, from_client, to_client) = Transport::pair();
        let _ = peers_tx.send(Peer {
            start: start.clone(),
            from_client,
            to_client,
        });
        Ok((transport, None))
    });
    let sink = Arc::new(Sink::default());
    let log = SharedLog::new("t1");
    let session = AcpSession::start(
        AcpSessionSpec {
            task_id: "t1".into(),
            project_id: "p".into(),
            generation: 1,
            agent: "dsh".into(),
            cwd: "/w".into(),
            permission: permission.into(),
            model: model.map(str::to_string),
            effort: None,
            session: session.map(str::to_string),
            dialect: &DSH,
            transcript: Some(Arc::new(|id: &str| {
                Some(format!("/h/sessions/{id}/session.v4.jsonl"))
            })),
        },
        launcher,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        log.clone(),
        &tokio::runtime::Handle::current(),
    );
    Harness {
        session,
        sink,
        log,
        peers,
    }
}

async fn launched(h: &mut Harness) -> Peer {
    tokio::time::timeout(Duration::from_secs(5), h.peers.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn wait_status(sink: &Arc<Sink>, status: TaskStatus) {
    let sink = Arc::clone(sink);
    until("status", move || sink.last_status() == Some(status)).await;
}

async fn wait_type(sink: &Arc<Sink>, kind: &'static str) {
    let sink = Arc::clone(sink);
    until(kind, move || sink.update_types().iter().any(|t| t == kind)).await;
}

fn receipts(sink: &Sink) -> Vec<(String, ReceiptOutcome)> {
    sink.updates
        .lock()
        .iter()
        .filter_map(|u| match &u.kind {
            SessionUpdateKind::DeliveryReceipt { id, outcome, .. } => Some((id.clone(), *outcome)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_turn_streams_asks_and_ends() {
    let mut h = start_with("ask", Some("deepseek-v4-pro"), None);
    assert!(h.session.set_context_server(ContextServer {
        command: "/bin/aeroric-cli".into(),
        args: vec!["mcp-context".into()],
        env: vec![("AERORIC_CONTEXT_TOKEN".into(), "tok".into())],
    }));
    h.session.deliver("d1".into(), "写两个文件".into()).unwrap();
    let mut peer = launched(&mut h).await;
    // The tier is pinned in the environment, over anything inherited.
    assert_eq!(
        peer.start.env,
        vec![("DSH_PERMISSION_MODE".to_string(), "read-only".to_string())]
    );
    let new = peer.open().await;
    assert_eq!(new["params"]["cwd"], "/w");
    let server = &new["params"]["mcpServers"][0];
    assert_eq!(server["name"], "aeroric_context");
    assert_eq!(
        server["env"][0],
        json!({"name": "AERORIC_CONTEXT_TOKEN", "value": "tok"})
    );
    // The chosen model, from the provider DSH already uses.
    let set = peer.expect("session/set_config_option").await;
    assert_eq!(set["params"]["configId"], "model");
    assert_eq!(
        set["params"]["value"],
        "[\"deepseek-official\",\"deepseek-v4-pro\"]"
    );
    peer.reply(
        &set,
        json!({"configOptions": dsh_options("[\"deepseek-official\",\"deepseek-v4-pro\"]", "high")}),
    );
    let prompt = peer.prompt("写两个文件").await;
    wait_status(&h.sink, TaskStatus::Running).await;
    assert_eq!(
        h.sink.sessions.lock().clone(),
        vec![("s1".to_string(), None)]
    );
    assert_eq!(
        h.session.snapshot().model.as_deref(),
        Some("deepseek-v4-pro")
    );
    assert_eq!(h.session.snapshot().effort.as_deref(), Some("high"));

    peer.update(json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "write", "kind": "other",
        "status": "in_progress", "rawInput": {"file_path": "/w/a.txt", "justification": "需要写入"}}));
    peer.send(
        json!({"jsonrpc": "2.0", "id": 0, "method": "session/request_permission",
        "params": {"sessionId": "s1", "toolCall": {"toolCallId": "c1"}, "options": [
            {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}]}}),
    );
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    assert_eq!(
        receipts(&h.sink),
        vec![("d1".to_string(), ReceiptOutcome::Delivered)]
    );
    let pending = h.session.snapshot().pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, "fileChange");
    assert_eq!(pending[0].summary, "write /w/a.txt（需要写入）");
    assert_eq!(pending[0].options, ["allowOnce", "deny"]);
    h.session
        .respond(&pending[0].id, Decision::AllowOnce)
        .unwrap();
    let answer = peer.next().await;
    assert_eq!(answer["id"], 0);
    assert_eq!(
        answer["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "allow-once"}})
    );
    wait_status(&h.sink, TaskStatus::Running).await;

    peer.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "c2", "title": "write", "kind": "other",
        "status": "in_progress", "rawInput": {"file_path": "/w/b.txt"}}),
    );
    peer.send(json!({"jsonrpc": "2.0", "id": 1, "method": "session/request_permission",
        "params": {"sessionId": "s1", "toolCall": {"toolCallId": "c2"}, "options": [
            {"optionId": "allow-once", "kind": "allow_once"}, {"optionId": "reject-once", "kind": "reject_once"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let id = h.session.snapshot().pending[0].id.clone();
    h.session
        .respond(&id, Decision::Deny { reason: None })
        .unwrap();
    assert_eq!(
        peer.next().await["result"]["outcome"]["optionId"],
        "reject-once"
    );
    peer.update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "c2", "status": "failed",
        "content": [{"type": "content", "content": {"type": "text", "text": "Error: the user rejected"}}]}));
    // The answer right behind the last chunk must not overtake it.
    peer.update(
        json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1",
        "content": {"type": "text", "text": "a.txt 好了，"}}),
    );
    peer.update(
        json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1",
        "content": {"type": "text", "text": "b.txt 被拒绝"}}),
    );
    peer.update(json!({"sessionUpdate": "usage_update", "used": 12809, "size": 1000000}));
    peer.reply(&prompt, json!({"stopReason": "end_turn"}));
    wait_status(&h.sink, TaskStatus::Done).await;

    let updates = h.sink.updates.lock().clone();
    assert!(updates.iter().any(|u| matches!(&u.kind,
        SessionUpdateKind::ToolCallUpdate { id, status: ToolStatus::Declined, .. } if id == "c2")));
    assert!(updates.iter().any(|u| matches!(&u.kind,
        SessionUpdateKind::MessageCompleted { text: Some(text), .. } if text == "a.txt 好了，b.txt 被拒绝")));
    let ended = updates
        .iter()
        .position(|u| {
            matches!(
                &u.kind,
                SessionUpdateKind::TurnEnded {
                    reason: TurnEndReason::Completed,
                    ..
                }
            )
        })
        .expect("the turn ended");
    assert!(
        updates[..ended].iter().any(|u| matches!(
            &u.kind,
            SessionUpdateKind::Usage {
                context_used: Some(12809),
                ..
            }
        )),
        "everything the agent sent comes before the turn's end"
    );
    assert_eq!(receipts(&h.sink).len(), 1, "delivered once");
    // The transcript is recorded once the turn wrote it.
    assert_eq!(
        h.sink.sessions.lock().last().cloned(),
        Some((
            "s1".to_string(),
            Some("/h/sessions/s1/session.v4.jsonl".to_string())
        ))
    );
    assert!(h.log.latest_seq() > 0);
}

#[tokio::test]
async fn a_message_during_a_turn_is_the_next_turn() {
    let mut h = start_with("auto_edit", None, None);
    h.session.prompt("第一条".into()).unwrap();
    let mut peer = launched(&mut h).await;
    assert_eq!(peer.start.env[0].1, "workspace-write");
    peer.open().await;
    let first = peer.prompt("第一条").await;
    wait_status(&h.sink, TaskStatus::Running).await;
    h.session.deliver("d2".into(), "第二条".into()).unwrap();
    wait_type(&h.sink, "notice").await;
    assert_eq!(
        receipts(&h.sink),
        vec![("d2".to_string(), ReceiptOutcome::Accepted)]
    );
    peer.reply(&first, json!({"stopReason": "end_turn"}));
    let second = peer.prompt("第二条").await;
    peer.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "收到"}}));
    peer.reply(&second, json!({"stopReason": "end_turn"}));
    let sink = Arc::clone(&h.sink);
    until("second delivered", move || {
        receipts(&sink).contains(&("d2".to_string(), ReceiptOutcome::Delivered))
    })
    .await;
    wait_status(&h.sink, TaskStatus::Done).await;
}

#[tokio::test]
async fn a_refusal_reason_goes_to_the_agent_as_the_next_message() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("写文件".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    let prompt = peer.prompt("写文件").await;
    peer.send(json!({"jsonrpc": "2.0", "id": 3, "method": "session/request_permission",
        "params": {"sessionId": "s1", "toolCall": {"toolCallId": "w", "title": "write", "rawInput": {"file_path": "/etc/x"}},
            "options": [{"optionId": "a", "kind": "allow_once"}, {"optionId": "r", "kind": "reject_once"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let id = h.session.snapshot().pending[0].id.clone();
    h.session
        .respond(
            &id,
            Decision::Deny {
                reason: Some("别写 /etc".into()),
            },
        )
        .unwrap();
    assert_eq!(peer.next().await["result"]["outcome"]["optionId"], "r");
    peer.reply(&prompt, json!({"stopReason": "end_turn"}));
    peer.prompt("我拒绝了这次操作：别写 /etc").await;
}

#[tokio::test]
async fn interrupting_cancels_open_approvals_and_the_turn() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("跑测试".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    let prompt = peer.prompt("跑测试").await;
    peer.send(json!({"jsonrpc": "2.0", "id": 7, "method": "session/request_permission",
        "params": {"sessionId": "s1", "toolCall": {"toolCallId": "x", "title": "bash", "rawInput": {"command": "cargo test"}},
            "options": [{"optionId": "a", "kind": "allow_once"}, {"optionId": "r", "kind": "reject_once"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    assert_eq!(h.session.snapshot().pending[0].summary, "bash cargo test");
    assert_eq!(h.session.snapshot().pending[0].kind, "command");
    h.session.interrupt().unwrap();
    let cancelled = peer.next().await;
    assert_eq!(cancelled["id"], 7);
    assert_eq!(cancelled["result"]["outcome"]["outcome"], "cancelled");
    let cancel = peer.expect("session/cancel").await;
    assert_eq!(cancel["params"]["sessionId"], "s1");
    assert!(cancel.get("id").is_none(), "a notification");
    peer.reply(&prompt, json!({"stopReason": "cancelled"}));
    wait_status(&h.sink, TaskStatus::Interrupted).await;
    assert!(h.session.snapshot().pending.is_empty());
}

#[tokio::test]
async fn a_stop_the_agent_ignores_ends_the_turn_and_the_process() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    peer.prompt("hi").await;
    wait_status(&h.sink, TaskStatus::Running).await;
    h.session.interrupt().unwrap();
    peer.expect("session/cancel").await;
    // The agent never answers; the grace runs out.
    wait_status(&h.sink, TaskStatus::Interrupted).await;
    assert!(!h.session.snapshot().turn_active);
    assert!(h.sink.update_types().iter().any(|t| t == "notice"));
    // The next message starts a new process that resumes the session.
    h.session.prompt("再来".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.initialize(json!({"sessionCapabilities": {"resume": {}}}))
        .await;
    assert_eq!(
        peer.expect("session/resume").await["params"]["sessionId"],
        "s1"
    );
}

#[tokio::test]
async fn a_known_session_resumes_and_never_becomes_a_new_one() {
    let mut h = start_with("ask", None, Some("s1"));
    h.session.attach().unwrap();
    wait_status(&h.sink, TaskStatus::Done).await;
    h.session.prompt("接着来".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.initialize(json!({"sessionCapabilities": {"resume": {}}}))
        .await;
    let resume = peer.expect("session/resume").await;
    assert_eq!(resume["params"]["sessionId"], "s1");
    peer.reply(
        &resume,
        json!({"configOptions": dsh_options("[\"deepseek-account\",\"deepseek-flash\"]", "low")}),
    );
    peer.prompt("接着来").await;
    assert_eq!(
        h.session.snapshot().model.as_deref(),
        Some("deepseek-flash")
    );
    assert_eq!(h.session.snapshot().effort.as_deref(), Some("low"));
    // The session id did not change, so nothing new is attached.
    assert!(h.sink.sessions.lock().is_empty());

    // An agent that cannot resume fails the message instead of starting over.
    let mut h = start_with("ask", None, Some("s9"));
    h.session.deliver("d9".into(), "接着来".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.initialize(json!({"loadSession": true})).await;
    wait_status(&h.sink, TaskStatus::Failed).await;
    let (_, reason, _) = h.sink.statuses.lock().last().cloned().unwrap();
    assert!(reason.unwrap().contains("不支持恢复会话"));
    assert_eq!(
        receipts(&h.sink),
        vec![("d9".to_string(), ReceiptOutcome::Rejected)]
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), peer.from_client.recv())
            .await
            .map_or(true, |line| line.is_none()),
        "no session/new"
    );
}

#[tokio::test]
async fn a_tier_the_agent_cannot_hold_is_refused_before_launch() {
    let mut h = start_with("bypass", None, None);
    h.session.deliver("d1".into(), "hi".into()).unwrap();
    wait_status(&h.sink, TaskStatus::Failed).await;
    assert!(h.peers.try_recv().is_err(), "nothing launched");
    assert_eq!(
        receipts(&h.sink),
        vec![("d1".to_string(), ReceiptOutcome::Rejected)]
    );
    assert!(matches!(
        h.session.configure(ConfigChange {
            permission: Some("full_access".into()),
            ..Default::default()
        }),
        Err(DriverError::Unsupported(_))
    ));
    assert!(h.session.command(SlashCommand::Compact).is_err());
}

#[tokio::test]
async fn an_agent_that_exits_mid_turn_fails_the_turn() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    peer.prompt("hi").await;
    wait_status(&h.sink, TaskStatus::Running).await;
    drop(peer);
    wait_status(&h.sink, TaskStatus::Failed).await;
    let (_, reason, _) = h.sink.statuses.lock().last().cloned().unwrap();
    assert_eq!(reason.as_deref(), Some("DSH 进程已退出"));
    // The late failed `session/prompt` does not end a second turn.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let ended = h
        .sink
        .update_types()
        .iter()
        .filter(|t| *t == "turnEnded")
        .count();
    assert_eq!(ended, 1);
    // The next message starts the agent again and resumes the same session.
    h.session.prompt("再来".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.initialize(json!({"sessionCapabilities": {"resume": {}}}))
        .await;
    assert_eq!(
        peer.expect("session/resume").await["params"]["sessionId"],
        "s1"
    );
}

#[test]
fn models_and_efforts_are_picked_from_what_the_agent_offers() {
    let options = dsh_options("[\"deepseek-account\",\"deepseek-flash\"]", "high");
    let model = &options[0];
    // Several providers have it: the one in use wins.
    assert_eq!(
        pick_model(model, "deepseek-v4-pro").as_deref(),
        Some("[\"deepseek-account\",\"deepseek-v4-pro\"]")
    );
    assert_eq!(
        pick_model(model, "[\"deepseek-official\",\"deepseek-flash\"]").as_deref(),
        Some("[\"deepseek-official\",\"deepseek-flash\"]")
    );
    assert_eq!(
        pick_model(model, "DeepSeek-V4-Pro").as_deref(),
        Some("[\"deepseek-official\",\"deepseek-v4-pro\"]")
    );
    assert_eq!(pick_model(model, "gpt-5"), None);
    let effort = &options[1];
    assert_eq!(pick_effort(effort, "low").as_deref(), Some("low"));
    assert_eq!(pick_effort(effort, "medium").as_deref(), Some("high"));
    assert_eq!(pick_effort(effort, "xhigh").as_deref(), Some("max"));
    assert_eq!(pick_effort(effort, "minimal").as_deref(), Some("low"));
    assert_eq!(pick_effort(effort, "turbo"), None);
    assert_eq!(shown_value("[\"aeroric\",\"glm-5\"]"), "glm-5");
    assert_eq!(shown_value("plain"), "plain");
}

/// 2026-11-17: a handshake that went through is one line with each step's
/// time; one that stopped says where, how long it waited, how long the
/// process had run, and the process's last stderr lines.
#[test]
fn a_handshake_is_logged_step_by_step() {
    let mut handshake = Handshake::default();
    handshake.record("启动", Duration::from_millis(12));
    handshake.record("initialize", Duration::from_millis(3980));
    handshake.record("session/new", Duration::from_millis(218));
    assert_eq!(
        handshake.summary("DSH", "t1", Some(4242)),
        "[acp] DSH task=t1 pid=4242 握手 4210ms：启动 12ms · initialize 3980ms · session/new 218ms"
    );

    let mut started = Handshake::default();
    started.record("启动", Duration::from_millis(9));
    let stuck = started.failure(
        "DSH",
        "t2",
        Some(7),
        "initialize",
        Duration::from_secs(30),
        Some(Duration::from_millis(30_012)),
        &[
            "+20ms loading config".into(),
            "+29990ms still waiting".into(),
        ],
        "DSH 握手超时（30 秒没有回答）",
    );
    let lines: Vec<&str> = stuck.lines().collect();
    assert_eq!(
        lines[0],
        "[acp] DSH task=t2 pid=7 握手停在 initialize：等了 30000ms，进程已运行 30012ms；之前：启动 9ms；报错：DSH 握手超时（30 秒没有回答）"
    );
    assert_eq!(lines[1], "[acp]   stderr 最后 2 行：");
    assert_eq!(lines[3], "[acp]   +29990ms still waiting");

    let silent = Handshake::default().failure(
        "DSH",
        "t3",
        None,
        "启动",
        Duration::from_millis(1),
        None,
        &[],
        "没装",
    );
    assert_eq!(
        silent,
        "[acp] DSH task=t3 pid=- 握手停在 启动：等了 1ms；报错：没装\n[acp]   stderr 没有输出"
    );
}

/// A3: only `end_turn` completes a turn; the spec's limits fail it without
/// a raw state; a stop reason outside the spec, or none, fails it and keeps
/// what was sent; a stop the person asked for stays an interruption.
#[test]
fn only_end_turn_completes_a_turn() {
    use TurnEndReason::{Completed, Failed, Interrupted};
    let ok = |stop: Value| -> Result<Value, RpcError> {
        Ok(if stop.is_null() {
            json!({})
        } else {
            json!({ "stopReason": stop })
        })
    };
    let cases: Vec<(
        Result<Value, RpcError>,
        bool,
        TurnEndReason,
        Option<&str>,
        Option<&str>,
    )> = vec![
        (ok(json!("end_turn")), false, Completed, None, None),
        (ok(json!("cancelled")), false, Interrupted, None, None),
        (ok(json!("end_turn")), true, Interrupted, None, None),
        (ok(json!("paused")), true, Interrupted, None, None),
        (
            ok(json!("refusal")),
            false,
            Failed,
            Some("DSH 拒绝了这次请求"),
            None,
        ),
        (
            ok(json!("max_tokens")),
            false,
            Failed,
            Some("DSH 到了单轮上限（max_tokens），这一轮没做完"),
            None,
        ),
        (
            ok(json!("max_turn_requests")),
            false,
            Failed,
            Some("DSH 到了单轮上限（max_turn_requests），这一轮没做完"),
            None,
        ),
        (
            ok(Value::Null),
            false,
            Failed,
            Some("DSH 没有说这一轮是怎么结束的"),
            Some(""),
        ),
        (
            ok(json!("")),
            false,
            Failed,
            Some("DSH 没有说这一轮是怎么结束的"),
            Some(""),
        ),
        (
            ok(json!("paused")),
            false,
            Failed,
            Some("DSH 返回了没见过的结束状态：paused"),
            Some("paused"),
        ),
        (
            ok(json!(3)),
            false,
            Failed,
            Some("DSH 返回了没见过的结束状态：3"),
            Some("3"),
        ),
        (
            Err(RpcError::Remote {
                code: -32603,
                message: "boom".into(),
            }),
            false,
            Failed,
            Some("DSH 这一轮失败：boom (-32603)"),
            None,
        ),
    ];
    for (result, interrupted, reason, error, raw) in cases {
        let what = format!("{result:?} interrupted={interrupted}");
        assert_eq!(
            turn_outcome("DSH", &result, interrupted),
            (reason, error.map(str::to_string), raw.map(str::to_string)),
            "{what}"
        );
    }
}

/// A3: a turn the agent ended on a stop reason outside the spec is a
/// failure that carries it, never a completed turn.
#[tokio::test]
async fn an_unknown_stop_reason_fails_the_turn_and_keeps_it() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    let prompt = peer.prompt("hi").await;
    peer.reply(&prompt, json!({"stopReason": "paused"}));
    wait_status(&h.sink, TaskStatus::Failed).await;
    let updates = h.sink.updates.lock().clone();
    let ended: Vec<_> = updates
        .iter()
        .filter_map(|u| match &u.kind {
            SessionUpdateKind::TurnEnded {
                reason,
                error,
                raw_status,
                ..
            } => Some((*reason, error.clone(), raw_status.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0, TurnEndReason::Failed);
    assert_eq!(ended[0].2.as_deref(), Some("paused"));
    assert!(
        ended[0]
            .1
            .as_deref()
            .is_some_and(|e| e.ends_with("返回了没见过的结束状态：paused")),
        "{:?}",
        ended[0].1
    );
}

/// B1-3 (A2): an answer that cannot be written to DSH does not pass for
/// delivered — the request stays open, says why, and can be answered again;
/// a second answer while one is on its way is refused.
#[tokio::test]
async fn an_approval_that_is_not_written_stays_open() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("写文件".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.open().await;
    peer.prompt("写文件").await;
    peer.send(json!({"jsonrpc": "2.0", "id": 3, "method": "session/request_permission",
        "params": {"sessionId": "s1", "toolCall": {"toolCallId": "w", "title": "write"},
            "options": [{"optionId": "a", "kind": "allow_once"}, {"optionId": "r", "kind": "reject_once"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let id = h.session.snapshot().pending[0].id.clone();
    // The agent's input closes: nothing written gets through.
    let Peer {
        to_client,
        from_client,
        ..
    } = peer;
    drop(from_client);
    h.session.respond(&id, Decision::AllowOnce).unwrap();
    wait_type(&h.sink, "permissionSendFailed").await;
    let types = h.sink.update_types();
    assert!(
        !types.iter().any(|t| t == "permissionResolved"),
        "{types:?}"
    );
    assert_eq!(h.session.snapshot().pending.len(), 1, "still open");
    assert_eq!(h.sink.last_status(), Some(TaskStatus::InputRequired));
    // Free again: another answer is taken (and fails the same way).
    assert!(h.session.respond(&id, Decision::AllowOnce).is_ok());
    drop(to_client);
}

/// B1-4: a DSH that answers `initialize` and then never `session/new` does
/// not hold the session hostage: Stop (before the turn has begun) and Close
/// cut the handshake short at once instead of waiting out [`HANDSHAKE`].
#[tokio::test]
async fn stop_cuts_a_hanging_handshake_short() {
    let mut h = start_with("ask", None, None);
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h).await;
    peer.initialize(json!({"sessionCapabilities": {}})).await;
    let _new = peer.expect("session/new").await;
    let asked = std::time::Instant::now();
    // No turn has begun yet, so Stop says so — and still cuts the handshake.
    let _ = h.session.interrupt();
    let sink = Arc::clone(&h.sink);
    tokio::time::timeout(
        Duration::from_secs(2),
        until("the turn ends", move || {
            matches!(
                sink.last_status(),
                Some(TaskStatus::Failed | TaskStatus::Interrupted)
            )
        }),
    )
    .await
    .expect("the handshake must stop within two seconds, not thirty");
    assert!(asked.elapsed() < Duration::from_secs(2));
    drop(peer);
}
