use super::*;
use crate::sessions::live::codex::fake::{launched, launcher, until, Peer, Sink};
use crate::sessions::live::update::{Replay, SessionUpdate, ToolKind};

struct Harness {
    session: Arc<CodexSession>,
    sink: Arc<Sink>,
    log: SharedLog,
    peers: mpsc::UnboundedReceiver<Peer>,
}

fn spec() -> CodexSessionSpec {
    CodexSessionSpec {
        extra_dirs: Vec::new(),
        task_id: "t1".into(),
        project_id: "p1".into(),
        generation: 3,
        agent: "codex".into(),
        cwd: "/w".into(),
        permission: "auto_edit".into(),
        model: Some("gpt-5".into()),
        effort: None,
        thread_id: None,
    }
}

fn start(spec: CodexSessionSpec) -> Harness {
    let (launcher, peers) = launcher();
    let sink = Arc::new(Sink::default());
    let log = SharedLog::new(&spec.task_id);
    let session = CodexSession::start(
        spec,
        CodexServer::new(launcher),
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

/// Launch → thread/start → turn/start → turn/started, as a first prompt does.
async fn running(h: &mut Harness) -> Peer {
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    assert_eq!(
        start["params"],
        json!({"cwd": "/w", "approvalPolicy": "on-request", "sandbox": "workspace-write", "model": "gpt-5"})
    );
    peer.reply(
        &start,
        json!({"thread": {"id": "th1", "path": "/s/th1.jsonl"}}),
    );
    let turn = peer.expect("turn/start").await;
    assert_eq!(turn["params"]["threadId"], "th1");
    assert_eq!(
        turn["params"]["input"],
        json!([{"type": "text", "text": "hi", "text_elements": []}])
    );
    assert_eq!(
        turn["params"]["sandboxPolicy"],
        json!({"type": "workspaceWrite"})
    );
    assert_eq!(turn["params"]["model"], "gpt-5");
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th1", "turn": {"id": "u1"}}),
    );
    let sink = Arc::clone(&h.sink);
    until("running", move || {
        sink.last_status() == Some(TaskStatus::Running)
    })
    .await;
    peer
}

fn complete(peer: &Peer, status: &str) {
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": status}}),
    );
}

#[tokio::test]
async fn a_prompt_runs_one_turn_and_settles_done() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    peer.notify(
        "item/agentMessage/delta",
        json!({"threadId": "th1", "itemId": "m1", "delta": "你好"}),
    );
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;

    assert_eq!(
        h.sink.statuses(),
        vec![TaskStatus::Running, TaskStatus::Done]
    );
    assert!(h
        .sink
        .statuses
        .lock()
        .iter()
        .all(|(_, _, generation)| *generation == 3));
    assert_eq!(
        *h.sink.sessions.lock(),
        vec![("th1".to_string(), Some("/s/th1.jsonl".to_string()))]
    );
    assert_eq!(
        h.sink.update_types(),
        vec!["turnStarted", "messageChunk", "turnEnded"]
    );
    let Replay::Updates(updates) = h.log.since(0) else {
        panic!("no gap expected")
    };
    assert_eq!(
        updates.iter().map(|u| u.seq).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let snapshot = h.session.snapshot();
    assert_eq!(snapshot.session_id.as_deref(), Some("th1"));
    assert!(!snapshot.turn_active);
    assert!(h.session.idle_since_ms().is_some());
}

#[tokio::test]
async fn a_repeated_warning_is_shown_once_per_session() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    for message in [
        "Model metadata not found",
        "Model metadata not found",
        "Other",
    ] {
        peer.notify("warning", json!({"threadId": "th1", "message": message}));
    }
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;
    let Replay::Updates(updates) = h.log.since(0) else {
        panic!("no gap expected")
    };
    let notices: Vec<String> = updates
        .iter()
        .filter_map(|u| match &u.kind {
            SessionUpdateKind::Error { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(notices, ["Model metadata not found", "Other"]);
}

#[tokio::test]
async fn an_approval_waits_for_the_user_and_a_reasoned_denial_steers() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.request(
        7,
        "item/commandExecution/requestApproval",
        json!({"threadId": "th1", "itemId": "c1", "command": "rm -rf x"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    let pending = h.session.snapshot().pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (pending[0].id.as_str(), pending[0].summary.as_str()),
        ("7", "rm -rf x")
    );
    assert_eq!(
        h.session.respond("8", Decision::AllowOnce),
        Err(DriverError::UnknownRequest("8".into()))
    );

    h.session
        .respond(
            "7",
            Decision::Deny {
                reason: Some("别删".into()),
            },
        )
        .unwrap();
    assert_eq!(
        peer.next().await,
        json!({"jsonrpc": "2.0", "id": 7, "result": {"decision": "decline"}})
    );
    let steer = peer.expect("turn/steer").await;
    assert_eq!(steer["params"]["expectedTurnId"], "u1");
    assert!(steer["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .contains("别删"));
    peer.reply(&steer, json!({"turnId": "u1"}));
    let sink = Arc::clone(&h.sink);
    until("running again", move || {
        sink.last_status() == Some(TaskStatus::Running)
    })
    .await;
    assert!(h.session.snapshot().pending.is_empty());
    assert!(h
        .sink
        .update_types()
        .contains(&"permissionResolved".to_string()));
}

#[tokio::test]
async fn an_allowance_is_answered_in_codex_terms_without_a_steer() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.request(
        8,
        "item/fileChange/requestApproval",
        json!({"threadId": "th1", "itemId": "f1"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    h.session.respond("8", Decision::AllowForSession).unwrap();
    assert_eq!(
        peer.next().await["result"],
        json!({"decision": "acceptForSession"})
    );
    peer.silent().await;
}

#[tokio::test]
async fn interrupting_settles_interrupted_and_configuration_applies_to_the_next_turn() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    h.session
        .configure(ConfigChange {
            model: Some("gpt-x".into()),
            permission: Some("full_access".into()),
            effort: Some("high".into()),
        })
        .unwrap();
    h.session.interrupt().unwrap();
    let interrupt = peer.expect("turn/interrupt").await;
    assert_eq!(
        interrupt["params"],
        json!({"threadId": "th1", "turnId": "u1"})
    );
    peer.reply(&interrupt, json!({}));
    complete(&peer, "interrupted");
    let sink = Arc::clone(&h.sink);
    until("interrupted", move || {
        sink.last_status() == Some(TaskStatus::Interrupted)
    })
    .await;
    assert!(h.session.interrupt().is_err(), "nothing left to interrupt");

    h.session.prompt("next".into()).unwrap();
    let turn = peer.expect("turn/start").await;
    assert_eq!(turn["params"]["model"], "gpt-x");
    assert_eq!(turn["params"]["effort"], "high");
    assert_eq!(turn["params"]["approvalPolicy"], "never");
    assert_eq!(
        turn["params"]["sandboxPolicy"],
        json!({"type": "dangerFullAccess"})
    );
    assert_eq!(h.session.snapshot().model.as_deref(), Some("gpt-x"));
}

#[tokio::test]
async fn a_failed_turn_carries_codex_s_reason() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": "failed", "error": {"message": "quota"}}}),
    );
    let sink = Arc::clone(&h.sink);
    until("failed", move || {
        sink.last_status() == Some(TaskStatus::Failed)
    })
    .await;
    assert_eq!(
        h.sink.statuses.lock().last().unwrap().1.as_deref(),
        Some("quota")
    );
}

#[tokio::test]
async fn a_crash_fails_the_turn_and_the_next_prompt_resumes_on_a_new_process() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.request(
        9,
        "item/commandExecution/requestApproval",
        json!({"threadId": "th1", "command": "ls"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    peer.close();
    let sink = Arc::clone(&h.sink);
    until("failed", move || {
        sink.last_status() == Some(TaskStatus::Failed)
    })
    .await;
    assert_eq!(
        h.sink.statuses.lock().last().unwrap().1.as_deref(),
        Some("Codex 进程已退出")
    );
    assert!(h.session.snapshot().pending.is_empty());

    h.session.prompt("again".into()).unwrap();
    let mut second = launched(&mut h.peers).await;
    second.handshake().await;
    let resume = second.expect("thread/resume").await;
    assert_eq!(resume["params"], json!({"threadId": "th1"}));
    second.reply(&resume, json!({"thread": {"id": "th1"}}));
    let turn = second.expect("turn/start").await;
    assert_eq!(turn["params"]["input"][0]["text"], "again");
}

#[tokio::test]
async fn suspending_an_idle_session_unsubscribes_and_a_prompt_resumes_it() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;

    h.session.suspend();
    let unsubscribe = peer.expect("thread/unsubscribe").await;
    assert_eq!(unsubscribe["params"], json!({"threadId": "th1"}));
    peer.reply(&unsubscribe, json!({"status": "unsubscribed"}));
    let session = Arc::clone(&h.session);
    until("released", move || session.idle_since_ms().is_none()).await;

    h.session.prompt("back".into()).unwrap();
    let resume = peer.expect("thread/resume").await;
    peer.reply(&resume, json!({"thread": {"id": "th1"}}));
    peer.expect("turn/start").await;
    assert!(
        h.peers.try_recv().is_err(),
        "the same process serves the resume"
    );
}

#[tokio::test]
async fn a_request_the_driver_does_not_handle_is_refused_and_kept_as_unknown() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.request(
        11,
        "item/tool/requestUserInput",
        json!({"threadId": "th1", "questions": []}),
    );
    let refused = peer.next().await;
    assert_eq!(
        (refused["id"].clone(), refused["error"]["code"].clone()),
        (json!(11), json!(-32601))
    );
    let sink = Arc::clone(&h.sink);
    until("unknown kept", move || {
        sink.update_types().contains(&"unknown".to_string())
    })
    .await;
}

#[tokio::test]
async fn a_launcher_refusal_fails_the_task_with_its_reason() {
    let sink = Arc::new(Sink::default());
    let launcher: super::super::server::Launcher =
        Arc::new(|| Err("Codex 0.100.0 太旧，请升级 Codex".to_string()));
    let session = CodexSession::start(
        spec(),
        CodexServer::new(launcher),
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        SharedLog::new("t1"),
        &tokio::runtime::Handle::current(),
    );
    session.prompt("hi".into()).unwrap();
    let waiting = Arc::clone(&sink);
    until("failed", move || {
        waiting.last_status() == Some(TaskStatus::Failed)
    })
    .await;
    assert!(sink.statuses.lock()[0]
        .1
        .as_deref()
        .unwrap()
        .contains("请升级"));
}

#[tokio::test]
async fn a_resumed_session_opens_its_thread_and_settles_done() {
    let mut h = start(CodexSessionSpec {
        extra_dirs: Vec::new(),
        thread_id: Some("th1".into()),
        ..spec()
    });
    h.session.attach().unwrap();
    let mut peer = launched(&mut h.peers).await;
    peer.handshake().await;
    let resume = peer.expect("thread/resume").await;
    peer.reply(
        &resume,
        json!({"thread": {"id": "th1", "path": "/s/th1.jsonl"}}),
    );
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;
    assert_eq!(h.sink.sessions.lock()[0].0, "th1");
    h.session.rebind(4);
    h.session.close().unwrap();
    peer.expect("thread/unsubscribe").await;
    assert!(h.session.prompt("x".into()).is_err(), "closed");
}

/// CU 2026-11-03: closing mid-turn (the toolbar's stop) ends the turn as
/// interrupted; unsubscribed, Codex's own end of it never arrives.
#[tokio::test]
async fn closing_mid_turn_ends_the_turn_as_interrupted() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    h.session.close().unwrap();
    let interrupt = peer.expect("turn/interrupt").await;
    peer.reply(&interrupt, json!({}));
    let unsubscribe = peer.expect("thread/unsubscribe").await;
    peer.reply(&unsubscribe, json!({}));
    let sink = Arc::clone(&h.sink);
    until("interrupted", move || {
        sink.updates.lock().iter().any(|u| {
            matches!(
                u.kind,
                SessionUpdateKind::TurnEnded {
                    reason: TurnEndReason::Interrupted,
                    ..
                }
            )
        })
    })
    .await;
    assert!(!h.session.snapshot().turn_active);
}

/// B4′: the handoff brief goes out as `developerInstructions` on thread/start.
#[tokio::test]
async fn the_brief_is_sent_as_developer_instructions() {
    let mut h = start(spec());
    assert!(h.session.set_instructions("# 交接清单".into()));
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    assert_eq!(start["params"]["developerInstructions"], "# 交接清单");
}

/// CU F1 (2026-10-31): the context server is required, so Codex waits for it
/// instead of leaving the Aeroric tools out of a new session's first turn.
#[tokio::test]
async fn the_context_server_is_required() {
    let mut h = start(spec());
    assert!(h
        .session
        .set_context_server(crate::sessions::live::context_server::ContextServer {
            command: "aeroric-cli".into(),
            args: vec!["mcp-context".into()],
            env: vec![("AERORIC_CONTEXT_TOKEN".into(), "t".into())],
        }));
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(&mut h.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    let server = &start["params"]["config"]["mcp_servers"]["aeroric_context"];
    assert_eq!(server["required"], true, "{start}");
    assert_eq!(server["default_tools_approval_mode"], "approve");
    assert_eq!(server["command"], "aeroric-cli");
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

/// Notes whether the session counted its turn as running when the receipt
/// arrived.
#[derive(Default)]
struct Probe {
    session: std::sync::OnceLock<std::sync::Weak<CodexSession>>,
    busy_at_receipt: parking_lot::Mutex<Option<bool>>,
}

impl LiveStatusSink for Probe {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}

    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}

    fn update(&self, update: &SessionUpdate) {
        if matches!(update.kind, SessionUpdateKind::DeliveryReceipt { .. }) {
            let busy = self
                .session
                .get()
                .and_then(std::sync::Weak::upgrade)
                .map(|s| s.snapshot().turn_active);
            *self.busy_at_receipt.lock() = busy;
        }
    }
}

/// CU 2026-11-03 (C13): a message that starts a turn is receipted only once
/// that turn counts as running, so the queue holds the next one back.
#[tokio::test]
async fn a_receipt_comes_after_the_turn_it_started() {
    let (launcher, mut peers) = launcher();
    let probe = Arc::new(Probe::default());
    let session = CodexSession::start(
        spec(),
        CodexServer::new(launcher),
        Arc::clone(&probe) as Arc<dyn LiveStatusSink>,
        SharedLog::new("t1"),
        &tokio::runtime::Handle::current(),
    );
    probe.session.set(Arc::downgrade(&session)).ok();
    session.deliver("dv-1".into(), "委派的结果".into()).unwrap();
    let mut peer = launched(&mut peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    peer.reply(
        &start,
        json!({"thread": {"id": "th1", "path": "/s/th1.jsonl"}}),
    );
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    let seen = Arc::clone(&probe);
    until("receipt", move || seen.busy_at_receipt.lock().is_some()).await;
    assert_eq!(*probe.busy_at_receipt.lock(), Some(true));
}

/// R1-2: Codex's answer to the steer is the receipt.
#[tokio::test]
async fn a_delivered_message_is_receipted_by_codexs_answer() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    assert!(h.session.receipts());
    h.session
        .deliver("dv-1".into(), "委派的结果".into())
        .unwrap();
    let steer = peer.expect("turn/steer").await;
    assert_eq!(steer["params"]["input"][0]["text"], "委派的结果");
    peer.reply(&steer, json!({}));
    receipt(&h.sink, ReceiptOutcome::Delivered).await;
    h.session.prompt("plain".into()).unwrap();
    let plain = peer.expect("turn/steer").await;
    peer.reply(&plain, json!({}));
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(receipts(&h.sink).len(), 1, "a plain prompt has no receipt");
}

// 2026-11-07 (A14b): a command still running after its turn is a background
// terminal; the session stays up until it completes.

fn background_command(peer: &Peer) {
    peer.notify(
        "item/started",
        json!({"threadId": "th1", "turnId": "u1", "item": {"type": "commandExecution", "id": "c1",
            "command": "npm run dev", "cwd": "/w", "status": "inProgress", "processId": "p1",
            "commandActions": []}}),
    );
}

#[tokio::test]
async fn a_background_command_keeps_the_session_until_it_completes() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    background_command(&peer);
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;
    assert_eq!(h.session.idle_since_ms(), None, "c1 still runs");
    peer.notify(
        "item/completed",
        json!({"threadId": "th1", "turnId": "u1", "item": {"type": "commandExecution", "id": "c1",
            "command": "npm run dev", "cwd": "/w", "status": "completed", "exitCode": 0,
            "commandActions": []}}),
    );
    let session = Arc::clone(&h.session);
    until("idle", move || session.idle_since_ms().is_some()).await;
}

#[tokio::test]
async fn a_server_exit_between_turns_says_its_background_commands_ended() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    background_command(&peer);
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;
    peer.close();
    let sink = Arc::clone(&h.sink);
    until("notice", move || {
        sink.updates.lock().iter().any(|u| matches!(&u.kind,
            SessionUpdateKind::Error { message, .. } if message == "Codex 进程已退出，1 个后台命令随之结束"))
    })
    .await;
    assert_eq!(h.sink.last_status(), Some(TaskStatus::Done));
}

// 2026-11-07: Codex's own subagents. Their threads are routed here; their
// approvals are asked here and answered to them; their progress is a line on
// the collab card that spawned them.

fn spawn_worker(peer: &Peer) {
    peer.notify(
        "item/started",
        json!({"threadId": "th1", "turnId": "u1", "item": {"type": "collabAgentToolCall", "id": "col1",
            "tool": "spawnAgent", "status": "inProgress", "senderThreadId": "th1",
            "receiverThreadIds": ["th-kid"], "prompt": "跑测试", "agentsStates": {}}}),
    );
    peer.notify(
        "thread/started",
        json!({"thread": {"id": "th-kid", "parentThreadId": "th1", "agentNickname": "worker"}}),
    );
    peer.notify(
        "turn/started",
        json!({"threadId": "th-kid", "turn": {"id": "ku1"}}),
    );
}

fn card_lines(sink: &Sink) -> Vec<(ToolStatus, String)> {
    lines_on(sink, "col1")
}

fn lines_on(sink: &Sink, card: &str) -> Vec<(ToolStatus, String)> {
    sink.updates
        .lock()
        .iter()
        .filter_map(|u| match &u.kind {
            SessionUpdateKind::ToolCallUpdate {
                id,
                status,
                output_delta: Some(line),
                ..
            } if id == card => Some((*status, line.clone())),
            _ => None,
        })
        .collect()
}

/// Multi-agent v2 as Codex 0.163 / 0.166 send it (2026-11-08 wire capture):
/// the spawn is a `subAgentActivity`, the child gets no `thread/started`.
fn spawn_v2_probe(peer: &Peer) {
    let activity = json!({"type": "subAgentActivity", "id": "call1", "kind": "started",
        "agentPath": "/root/probe", "agentThreadId": "th-v2"});
    for method in ["item/started", "item/completed"] {
        peer.notify(
            method,
            json!({"threadId": "th1", "turnId": "u1", "item": activity}),
        );
    }
    peer.notify(
        "turn/started",
        json!({"threadId": "th-v2", "turn": {"id": "ku2"}}),
    );
}

#[tokio::test]
async fn a_v2_subagent_is_named_by_its_activity_and_a_refused_reason_goes_to_the_session() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    spawn_v2_probe(&peer);
    let sink = Arc::clone(&h.sink);
    until("started line", move || !lines_on(&sink, "call1").is_empty()).await;
    assert_eq!(
        lines_on(&h.sink, "call1")[0],
        (ToolStatus::Running, "子代理 probe 开始\n".to_string())
    );
    assert!(h.sink.updates.lock().iter().any(|u| matches!(&u.kind,
        SessionUpdateKind::ToolCall { id, kind: ToolKind::Subagent, title, .. }
            if id == "call1" && title == "子代理 probe")));

    peer.request(
        9,
        "item/commandExecution/requestApproval",
        json!({"threadId": "th-v2", "turnId": "ku2", "itemId": "kc1", "command": "touch probe.txt"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    assert_eq!(
        h.session.snapshot().pending[0].subagent.as_deref(),
        Some("probe")
    );
    h.session
        .respond(
            "9",
            Decision::Deny {
                reason: Some("别建文件".into()),
            },
        )
        .unwrap();
    assert_eq!(
        peer.next().await,
        json!({"jsonrpc": "2.0", "id": 9, "result": {"decision": "decline"}})
    );
    let to_child = peer.expect("turn/steer").await;
    assert_eq!(to_child["params"]["threadId"], "th-v2");
    peer.reply_error(
        &to_child,
        -32600,
        "direct app-server input is not allowed for multi-agent v2 sub-agents",
    );
    let to_session = peer.expect("turn/steer").await;
    assert_eq!(
        (
            to_session["params"]["threadId"].clone(),
            to_session["params"]["expectedTurnId"].clone(),
            to_session["params"]["input"][0]["text"].clone()
        ),
        (
            json!("th1"),
            json!("u1"),
            json!("用户拒绝了子代理 probe 的这一步：别建文件")
        )
    );
    peer.reply(&to_session, json!({"turnId": "u1"}));

    let finished = json!({"type": "subAgentActivity", "id": "subagent-completed-ku2",
        "kind": "completed", "agentPath": "/root/probe", "agentThreadId": "th-v2"});
    for method in ["item/started", "item/completed"] {
        peer.notify(
            method,
            json!({"threadId": "th1", "turnId": "u1", "item": finished}),
        );
    }
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-v2", "turn": {"id": "ku2", "status": "completed"}}),
    );
    let sink = Arc::clone(&h.sink);
    until("finished line", move || lines_on(&sink, "call1").len() == 2).await;
    assert_eq!(
        lines_on(&h.sink, "call1")[1],
        (ToolStatus::Completed, "子代理 probe 完成\n".to_string())
    );
    assert!(
        !h.sink.updates.lock().iter().any(|u| matches!(&u.kind,
            SessionUpdateKind::ToolCall { id, .. } | SessionUpdateKind::ToolCallUpdate { id, .. }
                if id == "subagent-completed-ku2")),
        "the child's end is a line on its card, not a card of its own"
    );
}

#[tokio::test]
async fn a_subagent_s_approval_is_asked_here_and_its_denial_reason_goes_to_it() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    spawn_worker(&peer);
    peer.request(
        9,
        "item/commandExecution/requestApproval",
        json!({"threadId": "th-kid", "turnId": "ku1", "itemId": "kc1", "command": "npm test"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    let pending = h.session.snapshot().pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].subagent.as_deref(), Some("worker"));
    assert!(h.sink.updates.lock().iter().any(|u| matches!(&u.kind,
        SessionUpdateKind::PermissionRequest { subagent: Some(label), .. } if label == "worker")));

    h.session
        .respond(
            "9",
            Decision::Deny {
                reason: Some("先别跑全量".into()),
            },
        )
        .unwrap();
    assert_eq!(
        peer.next().await,
        json!({"jsonrpc": "2.0", "id": 9, "result": {"decision": "decline"}})
    );
    let steer = peer.expect("turn/steer").await;
    assert_eq!(
        (
            steer["params"]["threadId"].clone(),
            steer["params"]["expectedTurnId"].clone()
        ),
        (json!("th-kid"), json!("ku1")),
        "the reason goes to the subagent's turn"
    );
    peer.reply(&steer, json!({"turnId": "ku1"}));
    let sink = Arc::clone(&h.sink);
    until("running again", move || {
        sink.last_status() == Some(TaskStatus::Running)
    })
    .await;
}

#[tokio::test]
async fn a_subagent_s_turn_is_a_line_on_its_card_and_leaves_the_session_s_status() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    spawn_worker(&peer);
    let sink = Arc::clone(&h.sink);
    until("started line", move || !card_lines(&sink).is_empty()).await;
    assert_eq!(
        card_lines(&h.sink)[0],
        (ToolStatus::Running, "子代理 worker 开始\n".to_string())
    );
    peer.notify(
        "item/completed",
        json!({"threadId": "th-kid", "turnId": "ku1", "item": {"type": "fileChange", "id": "kf1",
            "status": "completed", "changes": [{"path": "src/a.rs", "kind": {"type": "update"}}]}}),
    );
    // v1's wait names the same child; its lines stay on the spawn's card.
    peer.notify(
        "item/started",
        json!({"threadId": "th1", "turnId": "u1", "item": {"type": "collabAgentToolCall", "id": "col2",
            "tool": "wait", "status": "inProgress", "senderThreadId": "th1",
            "receiverThreadIds": ["th-kid"], "agentsStates": {}}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-kid", "turn": {"id": "ku1", "status": "completed"}}),
    );
    let sink = Arc::clone(&h.sink);
    until("finished line", move || card_lines(&sink).len() == 2).await;
    assert_eq!(
        card_lines(&h.sink)[1],
        (
            ToolStatus::Completed,
            "子代理 worker 完成 · 改了 src/a.rs\n".to_string()
        )
    );
    assert_eq!(
        h.sink.last_status(),
        Some(TaskStatus::Running),
        "the session's own turn still runs"
    );
    assert!(!h.sink.update_types().contains(&"turnEnded".to_string()));
}

#[tokio::test]
async fn a_running_subagent_keeps_the_session_up_and_its_late_approval_returns_to_done() {
    let mut h = start(spec());
    let peer = running(&mut h).await;
    spawn_worker(&peer);
    complete(&peer, "completed");
    let sink = Arc::clone(&h.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;
    assert_eq!(h.session.idle_since_ms(), None, "the subagent still runs");

    peer.request(
        10,
        "item/fileChange/requestApproval",
        json!({"threadId": "th-kid", "turnId": "ku1", "itemId": "kf2"}),
    );
    let sink = Arc::clone(&h.sink);
    until("input required", move || {
        sink.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    h.session.respond("10", Decision::AllowOnce).unwrap();
    let sink = Arc::clone(&h.sink);
    until("done again", move || {
        sink.last_status() == Some(TaskStatus::Done)
    })
    .await;

    peer.notify(
        "turn/completed",
        json!({"threadId": "th-kid", "turn": {"id": "ku1", "status": "completed"}}),
    );
    let session = Arc::clone(&h.session);
    until("idle", move || session.idle_since_ms().is_some()).await;
}

#[tokio::test]
async fn a_thread_nobody_spawned_is_still_refused() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.request(
        11,
        "item/commandExecution/requestApproval",
        json!({"threadId": "th-stranger", "turnId": "x", "itemId": "c", "command": "rm -rf /"}),
    );
    let answer = peer.next().await;
    assert_eq!(answer["error"]["code"], -32601);
    assert!(h.session.snapshot().pending.is_empty());
}
