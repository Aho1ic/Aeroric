use super::*;
use crate::sessions::live::codex::fake::{launched, launcher, until, Peer, Sink};
use crate::sessions::live::update::Replay;

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
