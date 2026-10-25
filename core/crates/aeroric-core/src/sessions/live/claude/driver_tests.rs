use super::*;
use crate::sessions::live::codex::fake::{until, Sink};
use crate::sessions::live::update::Replay;
use std::time::Duration;

/// Claude's end of one launched process.
struct Peer {
    start: ClaudeStart,
    from_client: mpsc::UnboundedReceiver<String>,
    to_client: Option<mpsc::UnboundedSender<String>>,
}

impl Peer {
    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.from_client.recv())
            .await
            .expect("the driver sent nothing")
            .expect("closed");
        serde_json::from_str(&line).unwrap()
    }
    /// The next control_request of `subtype`; returns its request_id.
    async fn expect_control(&mut self, subtype: &str) -> (String, Value) {
        let message = self.next().await;
        assert_eq!(message["type"], "control_request", "{message}");
        assert_eq!(message["request"]["subtype"], subtype, "{message}");
        (
            message["request_id"].as_str().unwrap().to_string(),
            message["request"].clone(),
        )
    }
    async fn expect_user(&mut self) -> String {
        let message = self.next().await;
        assert_eq!(message["type"], "user", "{message}");
        message["message"]["content"].as_str().unwrap().to_string()
    }
    fn send(&self, message: Value) {
        self.to_client
            .as_ref()
            .expect("closed")
            .send(message.to_string())
            .unwrap();
    }
    fn ok(&self, request_id: &str, response: Value) {
        self.send(json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}}));
    }
    async fn handshake(&mut self) {
        let (id, _) = self.expect_control("initialize").await;
        self.ok(&id, json!({"commands": []}));
    }
    fn result(&self, subtype: &str) {
        self.send(json!({"type": "result", "subtype": subtype, "is_error": subtype != "success", "session_id": "cs1",
            "duration_ms": 1, "duration_api_ms": 1, "num_turns": 1, "usage": {"input_tokens": 100, "output_tokens": 20}}));
    }
    /// Answers the get_context_usage that follows every result.
    async fn context_usage(&mut self) {
        let (id, _) = self.expect_control("get_context_usage").await;
        self.ok(
            &id,
            json!({"totalTokens": 5000, "maxTokens": 200000, "categories": []}),
        );
    }
}

struct Harness {
    session: Arc<ClaudeSession>,
    sink: Arc<Sink>,
    log: SharedLog,
    peers: mpsc::UnboundedReceiver<Peer>,
}

fn spec() -> ClaudeSessionSpec {
    ClaudeSessionSpec {
        task_id: "t1".into(),
        project_id: "p".into(),
        generation: 1,
        agent: "claude".into(),
        cwd: "/w".into(),
        permission: "ask".into(),
        model: Some("sonnet".into()),
        effort: None,
        session_id: None,
    }
}

fn start(spec: ClaudeSessionSpec) -> Harness {
    let (peers_tx, peers) = mpsc::unbounded_channel();
    let launcher: ClaudeLauncher = Arc::new(move |start: &ClaudeStart| {
        let (transport, from_client, to_client) = Transport::pair();
        let _ = peers_tx.send(Peer {
            start: start.clone(),
            from_client,
            to_client: Some(to_client),
        });
        Ok((transport, None))
    });
    let sink = Arc::new(Sink::default());
    let log = SharedLog::new(&spec.task_id);
    let session = ClaudeSession::start(
        spec,
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

/// First prompt: the process starts, handshakes, gets the message, says init.
async fn running(h: &mut Harness) -> Peer {
    h.session.prompt("hi".into()).unwrap();
    let mut peer = launched(h).await;
    assert_eq!(peer.start.resume, None);
    assert_eq!(peer.start.model.as_deref(), Some("sonnet"));
    peer.handshake().await;
    assert_eq!(peer.expect_user().await, "hi");
    peer.send(
        json!({"type": "system", "subtype": "init", "session_id": "cs1", "model": "claude-sonnet"}),
    );
    wait_status(&h.sink, TaskStatus::Running).await;
    peer
}

#[tokio::test]
async fn a_prompt_streams_a_turn_and_settles_done_with_context_usage() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.send(json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m1"}}}));
    peer.send(json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "你好"}}}));
    peer.send(json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "你好"},
        {"type": "tool_use", "id": "tu1", "name": "Bash", "input": {"command": "ls"}}]}}));
    peer.send(json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "tu1", "content": "a.txt"}]}}));
    peer.result("success");
    peer.context_usage().await;
    wait_status(&h.sink, TaskStatus::Done).await;
    let sink = Arc::clone(&h.sink);
    until("context usage", move || {
        sink.update_types().last().map(String::as_str) == Some("usage")
    })
    .await;
    assert_eq!(h.sink.statuses(), [TaskStatus::Running, TaskStatus::Done]);
    assert_eq!(*h.sink.sessions.lock(), [("cs1".to_string(), None)]);
    assert_eq!(
        h.sink.update_types(),
        [
            "turnStarted",
            "messageChunk",
            "messageCompleted",
            "toolCall",
            "toolCallUpdate",
            "usage",
            "turnEnded",
            "usage"
        ]
    );
    let snapshot = h.session.snapshot();
    assert_eq!(
        (
            snapshot.session_id.as_deref(),
            snapshot.context_used,
            snapshot.context_limit
        ),
        (Some("cs1"), Some(5000), Some(200000))
    );
    assert_eq!(snapshot.family, Some(ProtocolFamily::Claude));
    let Replay::Updates(updates) = h.log.since(0) else {
        panic!()
    };
    assert_eq!(updates.len(), 8);
}

#[tokio::test]
async fn approvals_wait_for_the_user_and_deny_carries_the_reason() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.send(json!({"type": "control_request", "request_id": "perm1", "request": {"subtype": "can_use_tool", "tool_name": "Bash",
        "input": {"command": "rm -rf build"}, "tool_use_id": "tu9",
        "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "rm:*"}], "behavior": "allow", "destination": "localSettings"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    let pending = h.session.snapshot().pending;
    assert_eq!(pending[0].summary, "rm -rf build");
    assert_eq!(pending[0].options, ["allowOnce", "allowForSession", "deny"]);
    h.session
        .respond(
            "perm1",
            Decision::Deny {
                reason: Some("别删".into()),
            },
        )
        .unwrap();
    let answer = peer.next().await;
    assert_eq!(
        answer["response"],
        json!({"subtype": "success", "request_id": "perm1", "response": {"behavior": "deny", "message": "别删"}})
    );
    wait_status(&h.sink, TaskStatus::Running).await;

    peer.send(json!({"type": "control_request", "request_id": "perm2", "request": {"subtype": "can_use_tool", "tool_name": "Bash",
        "input": {"command": "ls"}, "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash"}], "behavior": "allow", "destination": "localSettings"}]}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    h.session
        .respond("perm2", Decision::AllowForSession)
        .unwrap();
    let answer = peer.next().await;
    assert_eq!(
        answer["response"]["response"]["updatedPermissions"][0]["destination"],
        "session"
    );
    assert_eq!(
        answer["response"]["response"]["updatedInput"],
        json!({"command": "ls"})
    );

    peer.send(json!({"type": "control_request", "request_id": "perm3", "request": {"subtype": "can_use_tool", "tool_name": "Write", "input": {"file_path": "/w/a"}}}));
    wait_status(&h.sink, TaskStatus::InputRequired).await;
    assert_eq!(
        h.session.snapshot().pending[0].options,
        ["allowOnce", "deny"]
    );
    peer.send(json!({"type": "control_cancel_request", "request_id": "perm3"}));
    wait_status(&h.sink, TaskStatus::Running).await;
    assert!(h.session.snapshot().pending.is_empty());
}

#[tokio::test]
async fn interrupt_settles_interrupted_and_configure_reaches_the_live_process() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    h.session
        .configure(ConfigChange {
            model: Some("opus".into()),
            permission: Some("full_access".into()),
            effort: None,
        })
        .unwrap();
    let (id, request) = peer.expect_control("set_model").await;
    assert_eq!(request["model"], "opus");
    peer.ok(&id, json!({}));
    let (id, request) = peer.expect_control("set_permission_mode").await;
    assert_eq!(request["mode"], "bypassPermissions");
    peer.ok(&id, json!({}));
    h.session.interrupt().unwrap();
    let (id, _) = peer.expect_control("interrupt").await;
    peer.ok(&id, json!({}));
    peer.result("error_during_execution");
    peer.context_usage().await;
    wait_status(&h.sink, TaskStatus::Interrupted).await;
    assert!(h.session.interrupt().is_err());
}

#[tokio::test]
async fn a_message_during_a_turn_is_claude_s_next_turn() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    h.session.prompt("also this".into()).unwrap();
    assert_eq!(peer.expect_user().await, "also this");
    peer.result("success");
    peer.context_usage().await;
    let sink = Arc::clone(&h.sink);
    until("a second turn", move || {
        sink.update_types()
            .iter()
            .filter(|t| *t == "turnStarted")
            .count()
            == 2
    })
    .await;
    peer.result("success");
    peer.context_usage().await;
    wait_status(&h.sink, TaskStatus::Done).await;
    assert!(!h.session.snapshot().turn_active);
}

#[tokio::test]
async fn a_crash_fails_the_turn_and_the_next_prompt_resumes_the_session() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.to_client = None;
    wait_status(&h.sink, TaskStatus::Failed).await;
    assert_eq!(
        h.sink.statuses.lock().last().unwrap().1.as_deref(),
        Some("Claude 进程已退出")
    );
    h.session.prompt("again".into()).unwrap();
    let mut second = launched(&mut h).await;
    assert_eq!(
        second.start.resume.as_deref(),
        Some("cs1"),
        "--resume the same session"
    );
    second.handshake().await;
    assert_eq!(second.expect_user().await, "again");
}

#[tokio::test]
async fn idle_suspend_ends_the_process_and_a_prompt_resumes_it() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.result("success");
    peer.context_usage().await;
    wait_status(&h.sink, TaskStatus::Done).await;
    assert!(h.session.idle_since_ms().is_some());
    h.session.suspend();
    let session = Arc::clone(&h.session);
    until("released", move || session.idle_since_ms().is_none()).await;
    h.session.prompt("back".into()).unwrap();
    let mut next = launched(&mut h).await;
    assert_eq!(next.start.resume.as_deref(), Some("cs1"));
    next.handshake().await;
    assert_eq!(next.expect_user().await, "back");
    assert_eq!(
        h.sink.statuses().last(),
        Some(&TaskStatus::Running),
        "no failure from the deliberate end"
    );
}

#[tokio::test]
async fn unhandled_requests_are_refused_and_compact_is_a_message_review_is_not() {
    let mut h = start(spec());
    let mut peer = running(&mut h).await;
    peer.send(json!({"type": "control_request", "request_id": "h1", "request": {"subtype": "hook_callback", "callback_id": "x"}}));
    let answer = peer.next().await;
    assert_eq!(answer["response"]["subtype"], "error");
    assert!(h.session.command(SlashCommand::Review).is_err());
    h.session.command(SlashCommand::Compact).unwrap();
    assert_eq!(peer.expect_user().await, "/compact");
}

#[tokio::test]
async fn a_launcher_refusal_fails_the_task() {
    let sink = Arc::new(Sink::default());
    let launcher: ClaudeLauncher =
        Arc::new(|_: &ClaudeStart| Err("Claude Code 2.0.0 太旧，请升级 Claude Code".to_string()));
    let session = ClaudeSession::start(
        spec(),
        launcher,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        SharedLog::new("t1"),
        &tokio::runtime::Handle::current(),
    );
    session.prompt("hi".into()).unwrap();
    wait_status(&sink, TaskStatus::Failed).await;
    assert!(sink.statuses.lock()[0]
        .1
        .as_deref()
        .unwrap()
        .contains("请升级"));
}

#[tokio::test]
async fn resuming_without_a_prompt_settles_done_and_the_first_prompt_resumes() {
    let mut h = start(ClaudeSessionSpec {
        session_id: Some("old".into()),
        ..spec()
    });
    h.session.attach().unwrap();
    wait_status(&h.sink, TaskStatus::Done).await;
    assert!(
        h.peers.try_recv().is_err(),
        "no process until there is something to say"
    );
    h.session.prompt("continue".into()).unwrap();
    let peer = launched(&mut h).await;
    assert_eq!(peer.start.resume.as_deref(), Some("old"));
}
