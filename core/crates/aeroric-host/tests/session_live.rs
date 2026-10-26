//! ADR-043 N3: `session.*` over the host dispatcher, against an in-process
//! fake `codex app-server`. Covers every method, the record write-back of
//! `session.configure`, reopening a closed session on prompt, the replay gap,
//! and that a host assembled without a structured executor offers none of it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::projects::ProjectService;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::sessions::live::claude::driver::{ClaudeLauncher, ClaudeStart};
use aeroric_core::sessions::live::codex::rpc::Transport;
use aeroric_core::sessions::live::codex::server::Launcher;
use aeroric_core::sessions::live::executor::{
    ClaudeKey, Route, Routing, ServerKey, StructuredExecutor,
};
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    executor::HostWindowFacts, AgentFamilies, Attachments, NewTask, ProtocolFamily, TaskService,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::dispatcher::{dispatch, CallContext, HostServices, HostWiring};
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use aeroric_host::live::TaskServiceSink;
use aeroric_protocol::envelope::{Frame, Request};
use serde_json::{json, Value};
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(5);

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

/// `claude` is the Claude family; every other agent is Codex.
struct Families;

fn family_of(agent: &str) -> ProtocolFamily {
    if agent == "claude" {
        ProtocolFamily::Claude
    } else {
        ProtocolFamily::Codex
    }
}

impl AgentFamilies for Families {
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        family_of(agent)
    }
    fn session_family(&self, agent: &str) -> ProtocolFamily {
        family_of(agent)
    }
    fn is_builtin(&self, _agent: &str) -> bool {
        false
    }
    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        family_of(agent)
    }
}

/// The fake app-server's end of one launched process.
struct Peer {
    from_client: mpsc::UnboundedReceiver<String>,
    to_client: mpsc::UnboundedSender<String>,
}

impl Peer {
    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(WAIT, self.from_client.recv())
            .await
            .expect("the client sent nothing")
            .expect("the client closed");
        serde_json::from_str(&line).expect("json")
    }
    async fn expect(&mut self, method: &str) -> Value {
        let message = self.next().await;
        assert_eq!(message["method"], method, "unexpected {message}");
        message
    }
    fn send(&self, message: Value) {
        self.to_client
            .send(message.to_string())
            .expect("client gone");
    }
    fn reply(&self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }
    fn notify(&self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }
}

struct TestRouting {
    launcher: Launcher,
    claude: ClaudeLauncher,
}

impl Routing for TestRouting {
    fn route(&self, family: ProtocolFamily, agent: &str, config: Option<&str>) -> Option<Route> {
        if config.is_some() {
            return None;
        }
        Some(match family {
            ProtocolFamily::Claude => Route::Claude(ClaudeKey {
                agent: agent.to_string(),
                config: None,
            }),
            _ => Route::Codex(ServerKey::Agent(agent.to_string())),
        })
    }
    fn codex_launcher(&self, _key: &ServerKey) -> Launcher {
        Arc::clone(&self.launcher)
    }
    fn claude_launcher(&self, _key: &ClaudeKey) -> ClaudeLauncher {
        Arc::clone(&self.claude)
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Host {
    call: Arc<CallContext>,
    service: Arc<TaskService>,
    capabilities: Vec<String>,
    peers: mpsc::UnboundedReceiver<Peer>,
    /// Each Claude process start, with the peer for its stdio.
    claude_peers: mpsc::UnboundedReceiver<(ClaudeStart, Peer)>,
    project: aeroric_core::storage::Record,
    structured: Arc<StructuredExecutor>,
    families: Arc<dyn AgentFamilies>,
    context: Arc<CoreContext>,
    _root: PathBuf,
}

/// Writes task records into the data root before the host is assembled, as a
/// previous host process would have left them.
type Seed = Box<dyn FnOnce(&Arc<CoreContext>)>;

impl Host {
    async fn start(with_live: bool) -> Host {
        Self::start_seeded(with_live, Box::new(|_| {})).await
    }

    async fn start_seeded(with_live: bool, seed: Seed) -> Host {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!("ah-slive-{}-{n}", std::process::id()));
        std::fs::create_dir_all(root.join("home/.aeroric")).expect("home");
        let paths = AppPaths::from_user_home(root.join("home"));
        let context = Arc::new(
            CoreContext::acquire(
                paths.clone(),
                Arc::new(aeroric_core::clock::SystemClock),
                &LeaseOwner {
                    application: "slive-test".into(),
                    version: "0-test".into(),
                },
                &LocalProbe,
            )
            .expect("writer context"),
        );
        let project_dir = root.join("workspace");
        std::fs::create_dir_all(&project_dir).expect("workspace");
        let (upserted, _) = ProjectService::new(Arc::clone(&context))
            .open_local(
                &project_dir.to_string_lossy(),
                "project-1",
                context.clock.now_ms() as i64,
            )
            .expect("open project");
        seed(&context);
        let (peers_tx, peers) = mpsc::unbounded_channel();
        let launcher: Launcher = Arc::new(move || {
            let (transport, from_client, to_client) = Transport::pair();
            let _ = peers_tx.send(Peer {
                from_client,
                to_client,
            });
            Ok((transport, None))
        });
        let (claude_tx, claude_peers) = mpsc::unbounded_channel();
        let claude: ClaudeLauncher = Arc::new(move |start: &ClaudeStart| {
            let (transport, from_client, to_client) = Transport::pair();
            let _ = claude_tx.send((
                start.clone(),
                Peer {
                    from_client,
                    to_client,
                },
            ));
            Ok((transport, None))
        });
        let registry = Arc::new(TaskRegistry::default());
        let terminal = Arc::new(TerminalService::new());
        let log = Arc::new(EventLog::default());
        let pty = CliPtyExecutor::new(
            Arc::clone(&context),
            Arc::clone(&registry),
            Arc::clone(&terminal),
            Arc::clone(&log),
        );
        let sink = TaskServiceSink::new(Arc::clone(&log));
        let structured = StructuredExecutor::new(
            pty.clone() as Arc<_>,
            Arc::new(TestRouting { launcher, claude }),
            Arc::clone(&sink) as Arc<_>,
            tokio::runtime::Handle::current(),
        );
        let families: Arc<dyn AgentFamilies> = Arc::new(Families);
        let service = Arc::new(
            TaskService::new(
                Arc::clone(&context),
                Arc::clone(&families),
                structured.clone() as Arc<_>,
                Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
            )
            .expect("task service"),
        );
        pty.attach(&service);
        sink.attach(&service);
        let token_path = paths.data_dir().join("host.token");
        let assembled = HostServices::with_tasks_and_wiring(
            Arc::new(aeroric_host::auth::HostToken::load_or_create(&token_path).expect("token")),
            log,
            Arc::clone(&context),
            Arc::clone(&service),
            Arc::clone(&families),
            pty,
            terminal,
            registry,
            HostWiring {
                live: with_live.then(|| Arc::clone(&structured)),
                ..Default::default()
            },
        );
        let capabilities = assembled
            .capabilities
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let call = Arc::new(CallContext {
            services: Arc::clone(&assembled.services),
            connection: Arc::new(tokio::sync::Mutex::new(
                aeroric_host::subscriptions::Connection::new(),
            )),
        });
        let token = std::fs::read_to_string(&token_path)
            .expect("token")
            .trim()
            .to_string();
        let handshake = dispatch(
            request("handshake", json!({"token": token, "clientId": "c"})),
            Arc::clone(&call),
        )
        .await;
        assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

        Host {
            call,
            service,
            capabilities,
            peers,
            claude_peers,
            project: upserted.project,
            structured,
            families,
            context,
            _root: root,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        match dispatch(request(method, params), Arc::clone(&self.call)).await {
            Frame::Success(success) => Ok(success.result),
            Frame::Failure(failure) => Err(serde_json::to_value(&failure.error).unwrap()),
            other => panic!("unexpected frame {other:?}"),
        }
    }

    fn start_task(&self, prompt: &str) {
        self.start_agent_task("codex", prompt);
    }

    fn start_agent_task(&self, agent: &str, prompt: &str) {
        self.service
            .create(
                &self.project,
                NewTask {
                    id: "task-1".into(),
                    project_id: "project-1".into(),
                    prompt: prompt.into(),
                    agent: agent.into(),
                    permission_mode: "ask".into(),
                    immediate: true,
                    selected_model: None,
                    reasoning_effort: None,
                    speed: None,
                    dsh_agent_preset: None,
                    agent_config_id: None,
                    images: vec![],
                    texts: vec![],
                },
            )
            .expect("create");
        self.service
            .start(
                &self.project,
                "task-1",
                "op-1",
                true,
                None,
                false,
                Attachments {
                    images: vec![],
                    texts: vec![],
                },
                HostWindowFacts::default(),
            )
            .expect("start");
    }

    fn record(&self) -> Value {
        let stamped = self.service.load("project-1").expect("load");
        serde_json::to_value(&stamped.records.first().expect("task").0).unwrap()
    }

    fn record_of(&self, task_id: &str) -> Value {
        let stamped = self.service.load("project-1").expect("load");
        let record = stamped
            .records
            .into_iter()
            .find(|r| r.get("id").and_then(Value::as_str) == Some(task_id))
            .expect("task");
        serde_json::to_value(&record.0).unwrap()
    }

    async fn until(&self, what: &str, check: impl Fn(&Host) -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !check(self) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}: {}",
                self.record()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn state(&self) -> Value {
        self.call("session.state", json!({"taskId": "task-1"}))
            .await
            .expect("state")
    }

    async fn until_state(&self, what: &str, check: impl Fn(&Value) -> bool) -> Value {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let state = self.state().await;
            if check(&state) {
                return state;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}: {state}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn request(method: &str, params: Value) -> Request {
    serde_json::from_value(
        json!({"v": 1, "id": format!("r-{method}"), "method": method, "params": params}),
    )
    .expect("request")
}

fn types(batch: &Value) -> Vec<String> {
    batch["updates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["type"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_session_method_round_trips_through_the_host() {
    let mut host = Host::start(true).await;
    assert!(host.capabilities.iter().any(|c| c == "session.live"));
    host.start_task("hi");

    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(
        &start,
        json!({"thread": {"id": "th-1", "path": "/tmp/th-1.jsonl"}}),
    );
    let turn = peer.expect("turn/start").await;
    assert_eq!(turn["params"]["input"][0]["text"], "hi");
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "item/agentMessage/delta",
        json!({"threadId": "th-1", "itemId": "m1", "delta": "好"}),
    );
    peer.send(
        json!({"jsonrpc": "2.0", "id": 7, "method": "item/commandExecution/requestApproval",
        "params": {"threadId": "th-1", "itemId": "c1", "command": "rm -rf build"}}),
    );

    let state = host
        .until_state("a pending approval", |s| {
            s["pending"].as_array().is_some_and(|p| p.len() == 1)
        })
        .await;
    assert_eq!(
        (
            state["live"].clone(),
            state["family"].clone(),
            state["sessionId"].clone()
        ),
        (json!(true), json!("codex"), json!("th-1"))
    );
    assert_eq!(state["pending"][0]["summary"], "rm -rf build");
    assert_eq!(state["structured"], true);
    host.until("input_required", |h| {
        h.record()["status"] == "input_required"
    })
    .await;

    let batch = host
        .call("session.updates", json!({"taskId": "task-1", "after": 0}))
        .await
        .unwrap();
    assert_eq!(
        types(&batch),
        ["turnStarted", "messageChunk", "permissionRequest"]
    );
    assert_eq!(batch["latestSeq"], 3);
    assert!(batch.get("gap").is_none());
    let tail = host
        .call(
            "session.updates",
            json!({"taskId": "task-1", "after": 2, "limit": 1}),
        )
        .await
        .unwrap();
    assert_eq!(types(&tail), ["permissionRequest"]);

    let unknown = host
        .call(
            "session.respond",
            json!({"taskId": "task-1", "requestId": "8", "decision": "allowOnce"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    host.call(
        "session.respond",
        json!({"taskId": "task-1", "requestId": "7", "decision": "deny", "reason": "别删"}),
    )
    .await
    .unwrap();
    assert_eq!(peer.next().await["result"], json!({"decision": "decline"}));
    let steer = peer.expect("turn/steer").await;
    assert!(steer["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .contains("别删"));
    peer.reply(&steer, json!({"turnId": "u1"}));

    host.call(
        "session.configure",
        json!({"taskId": "task-1", "model": "gpt-x", "permission": "full_access"}),
    )
    .await
    .unwrap();
    let record = host.record();
    assert_eq!(
        (
            record["selectedModel"].clone(),
            record["permissionMode"].clone()
        ),
        (json!("gpt-x"), json!("full_access"))
    );
    let bad = host
        .call(
            "session.configure",
            json!({"taskId": "task-1", "permission": "root"}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "INVALID_ARGUMENT");

    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    host.until("done", |h| h.record()["status"] == "done").await;

    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "next"}),
    )
    .await
    .unwrap();
    let turn = peer.expect("turn/start").await;
    assert_eq!(
        (
            turn["params"]["model"].clone(),
            turn["params"]["approvalPolicy"].clone()
        ),
        (json!("gpt-x"), json!("never"))
    );
    peer.reply(&turn, json!({"turn": {"id": "u2"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u2"}}),
    );
    host.until_state("the second turn", |s| s["turnActive"] == true)
        .await;
    host.call("session.interrupt", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let interrupt = peer.expect("turn/interrupt").await;
    assert_eq!(interrupt["params"]["turnId"], "u2");
    peer.reply(&interrupt, json!({}));
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u2", "status": "interrupted"}}),
    );
    host.until("interrupted", |h| h.record()["status"] == "interrupted")
        .await;
    let idle = host
        .call("session.interrupt", json!({"taskId": "task-1"}))
        .await
        .unwrap_err();
    assert_eq!(idle["code"], "INVALID_ARGUMENT");

    host.call(
        "session.command",
        json!({"taskId": "task-1", "command": "compact"}),
    )
    .await
    .unwrap();
    let compact = peer.expect("thread/compact/start").await;
    peer.reply(&compact, json!({}));

    host.call("session.close", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let unsubscribe = peer.expect("thread/unsubscribe").await;
    peer.reply(&unsubscribe, json!({"status": "unsubscribed"}));
    let closed = host.state().await;
    assert_eq!(
        (
            closed["live"].clone(),
            closed["sessionId"].clone(),
            closed["model"].clone()
        ),
        (json!(false), json!("th-1"), json!("gpt-x"))
    );
    let latest = closed["latestSeq"].as_u64().unwrap();
    assert!(latest > 3, "the log outlives the close");
    assert_eq!(
        closed["structured"], true,
        "a closed Codex task still opens the live page"
    );
    assert_eq!(
        host.call("session.close", json!({"taskId": "task-1"}))
            .await
            .unwrap_err()["code"],
        "NOT_FOUND"
    );

    // D3: a prompt to the closed session reopens the same thread.
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "again"}),
    )
    .await
    .unwrap();
    let resume = peer.expect("thread/resume").await;
    assert_eq!(resume["params"]["threadId"], "th-1");
    peer.reply(&resume, json!({"thread": {"id": "th-1"}}));
    let turn = peer.expect("turn/start").await;
    assert_eq!(turn["params"]["input"][0]["text"], "again");
    let more = host
        .call(
            "session.updates",
            json!({"taskId": "task-1", "after": latest}),
        )
        .await
        .unwrap();
    assert!(
        more["latestSeq"].as_u64().unwrap() >= latest,
        "seq continues across the reopen"
    );

    let gap = host
        .call(
            "session.updates",
            json!({"taskId": "task-1", "after": 999_999}),
        )
        .await
        .unwrap();
    assert_eq!(gap["gap"]["oldestAvailable"], 1);
    assert_eq!(gap["updates"], json!([]));
    let strict = host
        .call(
            "session.updates",
            json!({"taskId": "task-1", "after": 0, "extra": 1}),
        )
        .await
        .unwrap_err();
    assert_eq!(strict["code"], "INVALID_ARGUMENT");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_without_a_session_answers_not_found() {
    let host = Host::start(true).await;
    assert_eq!(
        host.call("session.prompt", json!({"taskId": "nope", "text": "x"}))
            .await
            .unwrap_err()["code"],
        "NOT_FOUND"
    );
    assert_eq!(
        host.call("session.state", json!({"taskId": "nope"}))
            .await
            .unwrap_err()["code"],
        "NOT_FOUND"
    );
    let empty = host
        .call("session.updates", json!({"taskId": "nope", "after": 0}))
        .await
        .unwrap();
    assert_eq!(empty, json!({"updates": [], "latestSeq": 0}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_without_a_structured_executor_offers_no_session_methods() {
    let host = Host::start(false).await;
    assert!(!host.capabilities.iter().any(|c| c == "session.live"));
    let refused = host
        .call("session.prompt", json!({"taskId": "task-1", "text": "x"}))
        .await
        .unwrap_err();
    assert_eq!(refused["code"], "UNSUPPORTED_CAPABILITY");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paginated_codex_session_is_not_blocked() {
    let host = Host::start(true).await;
    host.service
        .create(
            &host.project,
            NewTask {
                id: "task-1".into(),
                project_id: "project-1".into(),
                prompt: "old".into(),
                agent: "codex".into(),
                permission_mode: "ask".into(),
                immediate: false,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
                agent_config_id: None,
                images: vec![],
                texts: vec![],
            },
        )
        .expect("create");
    let rollout = host._root.join("rollout-paginated.jsonl");
    std::fs::write(&rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"th-old\",\"history_mode\":\"paginated\"}}\n").unwrap();
    host.service
        .attach_session(
            "project-1",
            "task-1",
            ProtocolFamily::Codex,
            "codex",
            Some("th-old"),
            Some(&rollout.to_string_lossy()),
        )
        .expect("attach");
    // Codex 0.163's app-server resumes paginated threads (N6a).
    let state = host.state().await;
    assert_eq!(state["live"], false);
    assert_eq!(state["structured"], true);
    assert!(state.get("resumeBlocked").is_none(), "{state}");
}

impl Peer {
    /// The next Claude control_request of `subtype`; returns its request_id.
    async fn expect_control(&mut self, subtype: &str) -> String {
        let message = self.next().await;
        assert_eq!(message["type"], "control_request", "{message}");
        assert_eq!(message["request"]["subtype"], subtype, "{message}");
        message["request_id"].as_str().unwrap().to_string()
    }
    fn control_ok(&self, request_id: &str, response: Value) {
        self.send(json!({"type": "control_response", "response":
            {"subtype": "success", "request_id": request_id, "response": response}}));
    }
    /// Claude's side of one turn: handshake, the prompt, init, a result.
    async fn claude_turn(&mut self, session_id: &str, prompt: &str) {
        let id = self.expect_control("initialize").await;
        self.control_ok(&id, json!({"commands": []}));
        let user = self.next().await;
        assert_eq!(user["message"]["content"], prompt, "{user}");
        self.send(json!({"type": "system", "subtype": "init", "session_id": session_id, "model": "sonnet"}));
        self.send(
            json!({"type": "result", "subtype": "success", "is_error": false,
            "session_id": session_id, "usage": {"input_tokens": 1, "output_tokens": 1}}),
        );
        let id = self.expect_control("get_context_usage").await;
        self.control_ok(&id, json!({"totalTokens": 10, "maxTokens": 200000}));
    }
}

/// N6a B4: a closed Claude session reopens on the next prompt with `--resume`
/// of the session recorded under the Claude-prefixed fields.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_closed_claude_session_reopens_with_its_recorded_session() {
    let mut host = Host::start(true).await;
    host.start_agent_task("claude", "hi");
    let (start, mut peer) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .expect("claude started")
        .expect("launcher");
    assert_eq!(start.resume, None);
    peer.claude_turn("cs-1", "hi").await;
    host.until("claude session recorded", |h| {
        h.record()["claudeSessionId"] == "cs-1"
    })
    .await;
    host.until_state("idle", |s| s["turnActive"] == false).await;
    host.call("session.close", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let closed = host.state().await;
    assert_eq!(closed["live"], false);
    assert_eq!(closed["sessionId"], "cs-1", "{closed}");

    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "again"}),
    )
    .await
    .unwrap();
    let (start, mut peer) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .expect("claude restarted")
        .expect("launcher");
    assert_eq!(start.resume.as_deref(), Some("cs-1"));
    peer.claude_turn("cs-1", "again").await;
}

/// A previous host process died with native sessions open: their rows still
/// say `running` / `input_required`, and no driver in this process will ever
/// settle them. The new host settles them when it assembles the session
/// services — a recorded failure becomes `failed`, anything else
/// `interrupted` (the PTY boot rule) — and leaves PTY and finished tasks alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restarted_host_settles_native_tasks_left_running() {
    let seed: Seed = Box::new(|context| {
        let task = |id: &str, agent: &str, status: &str, extra: Value| {
            let mut record = json!({
                "id": id, "projectId": "project-1", "prompt": "x", "agent": agent,
                "permissionMode": "ask", "status": status, "createdAt": 1, "generation": "2",
            });
            record
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value::<aeroric_core::storage::Record>(record).unwrap()
        };
        aeroric_core::storage::save_tasks(
            context,
            "project-1",
            &[
                task(
                    "failed-run",
                    "claude",
                    "running",
                    json!({"failureReason": "API Error: 401"}),
                ),
                task("quiet-run", "codex", "running", json!({})),
                task(
                    "waiting-run",
                    "codex",
                    "input_required",
                    json!({"attentionRequestedAt": 7}),
                ),
                // TestRouting keeps a pinned configuration on the PTY executor.
                task(
                    "pty-run",
                    "codex",
                    "running",
                    json!({"agentConfigId": "cfg"}),
                ),
                task("finished", "claude", "done", json!({"completedAt": 9})),
            ],
        )
        .expect("seed tasks");
    });
    let host = Host::start_seeded(true, seed).await;

    let failed = host.record_of("failed-run");
    assert_eq!(failed["status"], "failed", "{failed}");
    assert_eq!(failed["failureReason"], "API Error: 401");
    assert!(failed["completedAt"].is_u64(), "{failed}");

    let quiet = host.record_of("quiet-run");
    assert_eq!(quiet["status"], "interrupted", "{quiet}");
    assert!(quiet["attentionRequestedAt"].is_u64(), "{quiet}");
    assert!(quiet.get("completedAt").is_none(), "{quiet}");

    let waiting = host.record_of("waiting-run");
    assert_eq!(waiting["status"], "interrupted", "{waiting}");
    assert_eq!(
        waiting["attentionRequestedAt"], 7,
        "the first request stands"
    );

    assert_eq!(host.record_of("pty-run")["status"], "running");
    assert_eq!(host.record_of("finished")["status"], "done");
    assert_eq!(host.record_of("finished")["completedAt"], 9);

    // Settled is settled: a second pass changes nothing.
    let again = aeroric_host::session_live_services::settle_orphaned_sessions(
        Arc::clone(&host.structured),
        Arc::clone(&host.families),
        Arc::clone(&host.service),
        Arc::clone(&host.context),
    );
    assert!(again.is_empty(), "{again:?}");
}

/// A task whose driver is alive in this process is not an orphan.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settling_leaves_a_task_with_a_live_driver_alone() {
    let mut host = Host::start(true).await;
    host.start_task("hello");
    let _peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .expect("no app-server launched")
        .expect("launcher gone");
    let before = host.record()["status"].clone();
    assert!(
        matches!(before.as_str(), Some("pending" | "running")),
        "{before}"
    );
    let settled = aeroric_host::session_live_services::settle_orphaned_sessions(
        Arc::clone(&host.structured),
        Arc::clone(&host.families),
        Arc::clone(&host.service),
        Arc::clone(&host.context),
    );
    assert!(settled.is_empty(), "{settled:?}");
    assert_eq!(host.record()["status"], before);
}

/// A05: auto checks are chosen per project through the task; off until set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_checks_are_toggled_per_project_through_a_task() {
    let host = Host::start(true).await;
    host.start_task("hi");
    let get = || host.call("session.autoCheck.get", json!({"taskId": "task-1"}));
    assert_eq!(
        get().await.unwrap(),
        json!({"tests": false, "diagnostics": false})
    );
    let set = host
        .call(
            "session.autoCheck.set",
            json!({"taskId": "task-1", "tests": true, "diagnostics": false}),
        )
        .await
        .unwrap();
    assert_eq!(set, json!({"tests": true, "diagnostics": false}));
    assert_eq!(get().await.unwrap(), set);
    let saved = aeroric_core::storage::load_settings(&host.context)
        .unwrap()
        .found()
        .unwrap();
    assert_eq!(
        saved["auto_checks"]["project-1"],
        json!({"tests": true, "diagnostics": false})
    );
    assert_eq!(
        host.call("session.autoCheck.get", json!({"taskId": "nope"}))
            .await
            .unwrap_err()["code"],
        "NOT_FOUND"
    );
    // Nothing has run, so nothing failed.
    assert_eq!(
        host.call("session.autoCheck.failures", json!({}))
            .await
            .unwrap(),
        json!({"failures": []})
    );
}
