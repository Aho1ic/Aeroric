use super::*;
use crate::sessions::live::codex::fake::{until, Sink};
use std::time::Duration;

/// omp's end of one launched process.
struct Peer {
    start: OmpStart,
    from_client: mpsc::UnboundedReceiver<String>,
    to_client: mpsc::UnboundedSender<String>,
}

impl Peer {
    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.from_client.recv())
            .await
            .expect("the driver sent nothing")
            .expect("closed");
        serde_json::from_str(&line).unwrap()
    }
    /// The next command of `kind`; returns its id.
    async fn expect(&mut self, kind: &str) -> (String, Value) {
        let frame = self.next().await;
        assert_eq!(frame["type"], kind, "{frame}");
        (frame["id"].as_str().unwrap_or_default().to_string(), frame)
    }
    fn send(&self, frame: Value) {
        self.to_client.send(frame.to_string()).unwrap();
    }
    fn ok(&self, id: &str, command: &str, data: Value) {
        self.send(json!({"id": id, "type": "response", "command": command, "success": true, "data": data}));
    }
    async fn answer(&mut self, kind: &str, data: Value) -> Value {
        let (id, frame) = self.expect(kind).await;
        self.ok(&id, kind, data);
        frame
    }
    async fn handshake(&mut self) {
        self.send(
            json!({"type": "ready", "protocolVersion": 1, "supportedProtocolVersions": [1, 2]}),
        );
        self.answer("negotiate_protocol", json!({"protocolVersion": 2}))
            .await;
        self.answer("set_event_filter", json!({})).await;
        self.answer("set_subagent_subscription", json!({})).await;
        self.state(json!([])).await;
    }
    async fn state(&mut self, todos: Value) {
        self.answer(
            "get_state",
            json!({"sessionId": "os1", "sessionFile": "/h/sessions/os1.jsonl",
            "model": {"id": "fake-1", "provider": "fake"}, "todoPhases": todos,
            "contextUsage": {"tokens": 1100, "contextWindow": 200000, "percent": 0.55}}),
        )
        .await;
    }
    /// Accept a prompt; returns its id for the `prompt_result`.
    async fn prompt(&mut self, text: &str) -> String {
        let (id, frame) = self.expect("prompt").await;
        assert_eq!(frame["message"], text);
        self.ok(&id, "prompt", json!({}));
        id
    }
    fn result(&self, id: &str, status: &str) {
        self.send(json!({"type": "prompt_result", "id": id, "agentInvoked": true, "status": status, "sessionSettled": true}));
    }
}

struct Harness {
    session: Arc<OmpSession>,
    sink: Arc<Sink>,
    peers: mpsc::UnboundedReceiver<Peer>,
}

fn start() -> Harness {
    let (peers_tx, peers) = mpsc::unbounded_channel();
    let launcher: OmpLauncher = Arc::new(move |start: &OmpStart| {
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
    let session = OmpSession::start(
        OmpSessionSpec {
            extra_dirs: Vec::new(),
            task_id: "t1".into(),
            project_id: "p".into(),
            generation: 1,
            agent: "omp".into(),
            cwd: "/w".into(),
            permission: "ask".into(),
            model: None,
            effort: Some("high".into()),
            session: None,
        },
        launcher,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        log,
        &tokio::runtime::Handle::current(),
    );
    Harness {
        session,
        sink,
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

/// First prompt: the process starts, handshakes and gets the message.
async fn running(h: &mut Harness) -> (Peer, String) {
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(h).await;
    assert_eq!(peer.start.resume, None);
    assert_eq!(peer.start.effort.as_deref(), Some("high"));
    peer.handshake().await;
    let id = peer.prompt("hi").await;
    wait_status(&h.sink, TaskStatus::Running).await;
    (peer, id)
}

#[tokio::test]
async fn a_prompt_streams_a_turn_settles_done_and_reads_the_plan() {
    let mut h = start();
    let (mut peer, id) = running(&mut h).await;
    assert_eq!(
        *h.sink.sessions.lock(),
        [("os1".to_string(), Some("/h/sessions/os1.jsonl".to_string()))]
    );
    peer.send(
        json!({"type": "message_update", "messageId": "msg-2", "message": {"role": "assistant"},
        "assistantMessageEvent": {"type": "text_delta", "delta": "好的"}}),
    );
    peer.send(json!({"type": "tool_execution_start", "toolCallId": "call_1", "toolName": "bash", "args": {"command": "ls"}}));
    peer.send(json!({"type": "tool_execution_end", "toolCallId": "call_1", "toolName": "bash", "result": {}, "isError": false}));
    peer.send(
        json!({"type": "message_end", "messageId": "msg-2", "message": {"role": "assistant",
        "content": [{"type": "text", "text": "好的"}], "usage": {"input": 10, "output": 2}}}),
    );
    peer.result(&id, "completed");
    peer.state(json!([{"name": "一", "tasks": [{"content": "读代码", "status": "in_progress"}]}]))
        .await;
    wait_status(&h.sink, TaskStatus::Done).await;
    wait_type(&h.sink, "plan").await;
    let types = h.sink.update_types();
    for kind in [
        "turnStarted",
        "messageChunk",
        "toolCall",
        "toolCallUpdate",
        "messageCompleted",
        "turnEnded",
        "plan",
    ] {
        assert!(
            types.iter().any(|t| t == kind),
            "missing {kind} in {types:?}"
        );
    }
    let snapshot = h.session.snapshot();
    assert!(snapshot.permission_locked);
    assert_eq!(snapshot.plan[0].status, "inProgress");
    assert_eq!(
        (snapshot.context_used, snapshot.context_limit),
        (Some(1100), Some(200000))
    );
    assert_eq!(snapshot.family, Some(ProtocolFamily::Omp));
}

#[tokio::test]
async fn an_allow_for_session_grant_is_reanswered_after_a_restart() {
    let mut h = start();
    let (mut peer, id) = running(&mut h).await;
    let ask = json!({"type": "extension_ui_request", "id": "u1", "method": "select",
        "title": "Allow tool: bash\nCommand: npm test", "options": ["Approve", "Approve bash for this session", "Deny"]});
    peer.send(json!({"type": "tool_execution_start", "toolCallId": "call_9", "toolName": "bash", "args": {"command": "npm test"}}));
    peer.send(ask.clone());
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let pending = h.session.snapshot().pending;
    assert_eq!(
        (pending[0].kind.as_str(), pending[0].summary.as_str()),
        ("bash", "Command: npm test")
    );
    assert_eq!(pending[0].tool_call_id.as_deref(), Some("call_9"));
    h.session.respond("u1", Decision::AllowForSession).unwrap();
    let answer = peer.next().await;
    assert_eq!(
        answer,
        json!({"type": "extension_ui_response", "id": "u1", "value": "Approve bash for this session"})
    );
    wait_status(&h.sink, TaskStatus::Running).await;
    peer.result(&id, "completed");
    peer.state(json!([])).await;
    wait_status(&h.sink, TaskStatus::Done).await;

    // Idle: the process goes; the next prompt resumes the same session file.
    h.session.suspend();
    let sink = Arc::clone(&h.sink);
    let session = Arc::clone(&h.session);
    until("suspended", move || {
        session.idle_since_ms().is_none() && sink.last_status() == Some(TaskStatus::Done)
    })
    .await;
    h.session.prompt("again".into()).unwrap();
    let mut second = launched(&mut h).await;
    assert_eq!(
        second.start.resume.as_deref(),
        Some("/h/sessions/os1.jsonl")
    );
    second.handshake().await;
    let _ = second.prompt("again").await;
    second.send(json!({"type": "extension_ui_request", "id": "u2", "method": "select",
        "title": "Allow tool: bash\nCommand: npm run lint", "options": ["Approve", "Approve bash for this session", "Deny"]}));
    let auto = second.next().await;
    assert_eq!(auto["value"], "Approve bash for this session");
    wait_type(&h.sink, "autoApproved").await;
    assert!(h.session.snapshot().pending.is_empty());

    h.session.revoke_grant("bash").unwrap();
    second.send(json!({"type": "extension_ui_request", "id": "u3", "method": "select",
        "title": "Allow tool: bash\nCommand: rm -rf x", "options": ["Approve", "Approve bash for this session", "Deny"]}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    h.session
        .respond(
            "u3",
            Decision::Deny {
                reason: Some("别删".into()),
            },
        )
        .unwrap();
    assert_eq!(second.next().await["value"], "Deny");
    let steer = second.next().await;
    assert_eq!(
        (steer["type"].as_str(), steer["message"].as_str()),
        (Some("steer"), Some("我拒绝了这次操作：别删"))
    );
}

#[tokio::test]
async fn questions_wait_for_an_answer_and_time_out_with_omp() {
    let mut h = start();
    let (mut peer, _id) = running(&mut h).await;
    peer.send(json!({"type": "extension_ui_request", "id": "q1", "method": "confirm", "title": "继续？", "message": "会覆盖文件", "timeout": 30000}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let question = h.session.snapshot().questions[0].clone();
    assert_eq!(
        (question.style.as_str(), question.timeout_ms),
        ("confirm", Some(30000))
    );
    h.session.answer("q1", Answer::Confirmed(true)).unwrap();
    assert_eq!(
        peer.next().await,
        json!({"type": "extension_ui_response", "id": "q1", "confirmed": true})
    );
    wait_type(&h.sink, "questionResolved").await;

    peer.send(json!({"type": "extension_ui_request", "id": "q2", "method": "select", "title": "用哪个？", "options": ["vitest", "jest"]}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    peer.send(
        json!({"type": "extension_ui_request", "id": "c1", "method": "cancel", "targetId": "q2"}),
    );
    let session = Arc::clone(&h.session);
    until("question gone", move || {
        session.snapshot().questions.is_empty()
    })
    .await;
    wait_status(&h.sink, TaskStatus::Running).await;
    assert!(h
        .session
        .answer("q2", Answer::Value("jest".into()))
        .is_err());
    peer.send(json!({"type": "extension_ui_request", "id": "w1", "method": "setWidget", "widgetKey": "autoresearch"}));
    peer.send(json!({"type": "extension_ui_request", "id": "n1", "method": "notify", "message": "已保存", "notifyType": "info"}));
    wait_type(&h.sink, "notice").await;
}

#[tokio::test]
async fn a_message_during_a_turn_steers_and_the_permission_is_locked() {
    let mut h = start();
    let (mut peer, id) = running(&mut h).await;
    h.session.prompt("顺便改 README".into()).unwrap();
    let (steer_id, steer) = peer.expect("steer").await;
    assert_eq!(steer["message"], "顺便改 README");
    peer.ok(&steer_id, "steer", json!({}));
    assert!(matches!(
        h.session.configure(ConfigChange { permission: Some("full_access".into()), ..Default::default() }),
        Err(DriverError::Unsupported(message)) if message == LOCKED_PERMISSION
    ));
    h.session
        .configure(ConfigChange {
            model: Some("fake/fake-2".into()),
            ..Default::default()
        })
        .unwrap();
    let (model_id, model) = peer.expect("set_model").await;
    assert_eq!(
        (model["provider"].as_str(), model["modelId"].as_str()),
        (Some("fake"), Some("fake-2"))
    );
    peer.ok(&model_id, "set_model", json!({}));
    h.session.interrupt().unwrap();
    let (abort_id, _) = peer.expect("abort").await;
    peer.ok(&abort_id, "abort", json!({}));
    peer.result(&id, "aborted");
    peer.state(json!([])).await;
    wait_status(&h.sink, TaskStatus::Interrupted).await;
}

#[tokio::test]
async fn the_process_exiting_mid_turn_fails_it() {
    let mut h = start();
    let (peer, _id) = running(&mut h).await;
    drop(peer);
    wait_status(&h.sink, TaskStatus::Failed).await;
    assert!(!h.session.snapshot().turn_active);
}

/// R1-2: the delivery receipts the driver pushed.
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

async fn receipt(sink: &Arc<Sink>, outcome: ReceiptOutcome) {
    let sink = Arc::clone(sink);
    until("receipt", move || {
        receipts(&sink).contains(&("dv-1".to_string(), outcome))
    })
    .await;
}

/// R1-2: omp's answer to the steer is the receipt.
#[tokio::test]
async fn a_delivered_message_is_receipted_by_omps_answer() {
    let mut h = start();
    let (mut peer, _) = running(&mut h).await;
    h.session
        .deliver("dv-1".into(), "委派的结果".into())
        .unwrap();
    let (id, frame) = peer.expect("steer").await;
    assert_eq!(frame["message"], "委派的结果");
    peer.ok(&id, "steer", json!({}));
    receipt(&h.sink, ReceiptOutcome::Delivered).await;
}

// 2026-11-07 (A14b): omp's async work keeps the session up, and a turn it
// starts on its own has a boundary.

#[tokio::test]
async fn an_unsettled_session_stays_up_and_its_own_turn_has_a_boundary() {
    let mut h = start();
    let (mut peer, id) = running(&mut h).await;
    peer.send(json!({"type": "prompt_result", "id": id, "agentInvoked": true, "status": "completed", "sessionSettled": false}));
    peer.state(json!([])).await;
    wait_status(&h.sink, TaskStatus::Done).await;
    assert_eq!(h.session.idle_since_ms(), None, "async work pending");
    let started = h
        .sink
        .update_types()
        .iter()
        .filter(|t| *t == "turnStarted")
        .count();

    peer.send(json!({"type": "agent_start"}));
    peer.send(
        json!({"type": "message_update", "messageId": "msg-9", "message": {"role": "assistant"},
        "assistantMessageEvent": {"type": "text_delta", "delta": "后台任务完成了"}}),
    );
    wait_status(&h.sink, TaskStatus::Running).await;
    assert_eq!(
        h.sink
            .update_types()
            .iter()
            .filter(|t| *t == "turnStarted")
            .count(),
        started + 1
    );
    peer.send(json!({"type": "agent_end"}));
    peer.state(json!([])).await;
    wait_status(&h.sink, TaskStatus::Done).await;
    assert_eq!(h.session.idle_since_ms(), None, "still unsettled");
    peer.send(json!({"type": "session_settled"}));
    let session = Arc::clone(&h.session);
    until("idle", move || session.idle_since_ms().is_some()).await;
}

#[tokio::test]
async fn an_exit_while_unsettled_says_the_async_work_ended() {
    let mut h = start();
    let (mut peer, id) = running(&mut h).await;
    peer.send(json!({"type": "prompt_result", "id": id, "agentInvoked": true, "status": "completed", "sessionSettled": false}));
    peer.state(json!([])).await;
    wait_status(&h.sink, TaskStatus::Done).await;
    drop(peer);
    let sink = Arc::clone(&h.sink);
    until("notice", move || {
        sink.updates.lock().iter().any(|u| matches!(&u.kind,
            SessionUpdateKind::Error { message, .. } if message == "omp 进程已退出，未结算的后台作业随之结束"))
    })
    .await;
}
