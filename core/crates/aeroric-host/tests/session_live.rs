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

/// `claude` is the Claude family and `omp` is omp (A4: the read-only check
/// reads the launch family; no test here starts an omp task); every other
/// agent is Codex.
struct Families;

fn family_of(agent: &str) -> ProtocolFamily {
    match agent {
        "claude" => ProtocolFamily::Claude,
        "omp" => ProtocolFamily::Omp,
        _ => ProtocolFamily::Codex,
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

#[path = "session_live_faults/mod.rs"]
mod faults;

#[path = "session_live_turns/mod.rs"]
mod turns;

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

/// R1-2b: the user's message goes through the queue. During a turn, "now"
/// steers it at once; "after this turn" waits and gets a turn of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_users_message_steers_now_or_waits_for_the_turn() {
    let mut host = Host::start(true).await;
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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    host.until("running", |h| h.record()["status"] == "running")
        .await;

    let later = host
        .call(
            "session.prompt",
            json!({"taskId": "task-1", "text": "稍后", "mode": "afterTurn"}),
        )
        .await
        .unwrap();
    assert!(
        later["deliveryId"].as_str().unwrap().starts_with("dv-"),
        "{later}"
    );
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "插话"}),
    )
    .await
    .unwrap();
    let steer = peer.expect("turn/steer").await;
    assert_eq!(steer["params"]["input"][0]["text"], "插话");
    peer.reply(&steer, json!({"turnId": "u1"}));

    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    let next = peer.expect("turn/start").await;
    assert_eq!(next["params"]["input"][0]["text"], "稍后");
    let queue = host
        .call("delivery.list", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let kinds: Vec<_> = queue["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(kinds, ["user", "user"], "{queue}");
}

/// R1-3a: a turn the agent refused for its login marks that login; the
/// next completed turn clears it. The capability table answers per family.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_login_is_marked_until_a_turn_completes() {
    let mut host = Host::start(true).await;
    let table = host
        .call("session.capabilities", json!({"family": "claude"}))
        .await
        .unwrap();
    let review = table["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["feature"] == "review")
        .unwrap()
        .clone();
    assert_eq!(review["state"], "no", "{table}");

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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "failed",
            "error": {"message": "unexpected status 401 Unauthorized"}}}),
    );
    host.until("failed", |h| h.record()["status"] == "failed")
        .await;
    let expired = host
        .call("login.state", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(expired["expired"][0]["label"], "Codex（本机）", "{expired}");
    assert_eq!(expired["expired"][0]["command"], "codex login");

    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "again"}),
    )
    .await
    .unwrap();
    let again = peer.expect("turn/start").await;
    peer.reply(&again, json!({"turn": {"id": "u2"}}));
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u2", "status": "completed"}}),
    );
    host.until("done", |h| h.record()["status"] == "done").await;
    let cleared = host.call("login.state", json!({})).await.unwrap();
    assert_eq!(cleared["expired"], json!([]), "{cleared}");
}

/// A3: Codex ending a turn on a state its schema does not list fails the
/// task instead of settling it done; the state is kept on the record, and
/// the history grades that turn "没跑成 · 结束状态不明" even though Codex's
/// own rollout says it completed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_end_state_fails_the_turn_and_its_history_says_not_run() {
    let mut host = Host::start(true).await;
    let rollout = host._root.join("th-1.jsonl");
    std::fs::write(
        &rollout,
        [
            json!({"timestamp": "2026-01-01T00:00:00.000Z", "type": "event_msg",
                "payload": {"type": "task_started"}}),
            json!({"timestamp": "2026-01-01T00:00:03.000Z", "type": "event_msg",
                "payload": {"type": "token_count", "info": {
                    "total_token_usage": {"input_tokens": 10, "output_tokens": 2},
                    "last_token_usage": {"input_tokens": 10, "total_tokens": 12},
                    "model_context_window": 1000}}}),
            json!({"timestamp": "2026-01-01T00:00:05.000Z", "type": "event_msg",
                "payload": {"type": "task_complete"}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n"),
    )
    .unwrap();
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
        json!({"thread": {"id": "th-1", "path": rollout.to_string_lossy()}}),
    );
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "paused"}}),
    );
    host.until("failed and kept", |h| {
        h.record()["status"] == "failed" && h.record()["unknownTurnEnds"].is_array()
    })
    .await;
    let record = host.record();
    assert_eq!(
        record["failureReason"], "Codex 返回了没见过的结束状态：paused",
        "{record}"
    );
    let ends = record["unknownTurnEnds"].as_array().unwrap();
    assert_eq!(ends.len(), 1, "{record}");
    assert_eq!(ends[0]["status"], "paused");

    let history = host
        .call("session.history", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let turn = &history["stats"]["turns"][0];
    assert_eq!(turn["notRun"]["reason"], "unknownEnd", "{history}");
    assert_eq!(
        turn["notRun"]["message"],
        "Codex 返回了没见过的结束状态：paused"
    );
    assert_eq!(turn["failed"], true);
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

    // 2026-11-14 宿主重启后续跑: the restart says it cut them, and when;
    // only those can be continued, and auto-continue is off until set.
    assert_eq!(quiet["interruptedBy"], "hostRestart", "{quiet}");
    assert_eq!(quiet["hostRestarts"].as_array().map(Vec::len), Some(1));
    assert!(
        failed.get("interruptedBy").is_none(),
        "a failure is not a cut"
    );
    let not_cut = host
        .call(
            "session.continueAfterRestart",
            json!({"taskId": "finished"}),
        )
        .await
        .unwrap_err();
    assert_eq!(not_cut["code"], "INVALID_ARGUMENT", "{not_cut}");
    let unknown = host
        .call("session.continueAfterRestart", json!({"taskId": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    // 2026-11-14 无人值守自动重试: nothing waiting, nothing to cancel.
    let idle = host
        .call("session.autoRetry.cancel", json!({"taskId": "finished"}))
        .await
        .unwrap_err();
    assert_eq!(idle["code"], "INVALID_ARGUMENT", "{idle}");
    let unknown = host
        .call("session.autoRetry.cancel", json!({"taskId": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    // C6: 额度恢复后自动继续 is offered only for a turn the quota stopped.
    let offer = host
        .call("session.autoRetry.offer", json!({"taskId": "finished"}))
        .await
        .unwrap();
    assert_eq!(offer["available"], false, "{offer}");
    assert_eq!(offer["reason"], "上一轮不是因为额度用完没跑成");
    let not_quota = host
        .call("session.autoRetry.arm", json!({"taskId": "finished"}))
        .await
        .unwrap_err();
    assert_eq!(not_quota["code"], "INVALID_ARGUMENT", "{not_quota}");
    let unknown = host
        .call("session.autoRetry.arm", json!({"taskId": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    assert!(
        host.record_of("finished").get("autoRetry").is_none(),
        "nothing was armed"
    );
    let auto = host
        .call("session.resumeAfterRestart.get", json!({}))
        .await
        .unwrap();
    assert_eq!(auto, json!({"enabled": false}));
    host.call("session.resumeAfterRestart.set", json!({"enabled": true}))
        .await
        .unwrap();
    let auto = host
        .call("session.resumeAfterRestart.get", json!({}))
        .await
        .unwrap();
    assert_eq!(auto, json!({"enabled": true}));

    // Settled is settled: a second pass changes nothing.
    let again = aeroric_host::session_live_services::settle_orphaned_sessions(
        Arc::clone(&host.structured),
        Arc::clone(&host.families),
        Arc::clone(&host.service),
        Arc::clone(&host.context),
    );
    assert!(again.is_empty(), "{again:?}");
}

/// E5 (2026-11-19): an unattended session whose cut turn had already run
/// `git push` (its transcript says so) is not continued on its own, even
/// with auto-continue on; its record says what it did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cut_turn_that_already_pushed_is_not_continued_on_its_own() {
    let seed: Seed = Box::new(|context| {
        let transcript = context.paths.data_dir().join("pushed.jsonl");
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let lines = [
            json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "发版"}}),
            json!({"timestamp": t("01.000"), "type": "assistant", "message": {"id": "m1", "content": [
                {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "git push origin main"}}]}}),
            json!({"timestamp": t("02.000"), "type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
        ];
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::create_dir_all(context.paths.data_dir()).unwrap();
        std::fs::write(&transcript, text).unwrap();
        let record = json!({
            "id": "pushed-run", "projectId": "project-1", "prompt": "发版", "agent": "claude",
            "permissionMode": "ask", "status": "running", "createdAt": 1, "generation": "2",
            "delegateOf": "parent", "sessionFamily": "claude",
            "claudeSessionId": "cs-9", "claudeSessionPath": transcript.to_string_lossy(),
        });
        // E5 third step: continuing it already stalled; nothing goes out.
        let stalled = json!({
            "id": "stalled-run", "projectId": "project-1", "prompt": "修", "agent": "claude",
            "permissionMode": "ask", "status": "running", "createdAt": 1, "generation": "2",
            "delegateOf": "parent",
            "continueGuard": {"stalled": {"reason": "连续 2 次自动继续都没有成功的工具调用", "atMs": 1}},
        });
        aeroric_core::storage::save_tasks(
            context,
            "project-1",
            // Settled in this order: the stalled one is past when the
            // push is recorded.
            &[
                serde_json::from_value(stalled).unwrap(),
                serde_json::from_value(record).unwrap(),
            ],
        )
        .expect("seed tasks");
    });
    let host = Host::start_seeded(true, seed).await;
    host.call("session.resumeAfterRestart.set", json!({"enabled": true}))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let record = host.record_of("pushed-run");
        if record.get("outwardBeforeCut").is_some() {
            assert_eq!(record["outwardBeforeCut"], json!(["git push"]), "{record}");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never recorded: {record}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let record = host.record_of("pushed-run");
    assert_eq!(record["status"], "interrupted", "{record}");
    assert!(
        record.get("autoResumedAt").is_none(),
        "not continued: {record}"
    );
    // E5 third step: the same pass skipped the one whose continuing stalled.
    let stalled = host.record_of("stalled-run");
    assert_eq!(stalled["status"], "interrupted", "{stalled}");
    assert!(
        stalled.get("autoResumedAt").is_none(),
        "the guard held: {stalled}"
    );
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

/// A14b (2026-11-07): the 设置 → 通用 → Agent 进程 switch; absent means on.
#[tokio::test]
async fn idle_release_is_on_until_switched_off() {
    let host = Host::start(true).await;
    let get = || host.call("session.idleRelease.get", json!({}));
    assert_eq!(
        get().await.unwrap(),
        json!({"enabled": true, "idleMinutes": 60})
    );
    let set = host
        .call("session.idleRelease.set", json!({"enabled": false}))
        .await
        .unwrap();
    assert_eq!(set, json!({"enabled": false, "idleMinutes": 60}));
    assert_eq!(get().await.unwrap(), set);
    let saved = aeroric_core::storage::load_settings(&host.context)
        .unwrap()
        .found()
        .unwrap();
    assert_eq!(saved["release_idle_agents"], json!(false));
}

/// B4′: the checkpoint follows the session — the goal from the first prompt,
/// an approval recorded the moment it is asked — and is kept on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_checkpoint_records_the_goal_and_a_pending_approval() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.send(
        json!({"jsonrpc": "2.0", "id": 7, "method": "item/commandExecution/requestApproval",
        "params": {"threadId": "th-1", "itemId": "c1", "command": "git push"}}),
    );
    host.until_state("a pending approval", |s| {
        s["pending"].as_array().is_some_and(|p| p.len() == 1)
    })
    .await;

    let got = host
        .call("checkpoint.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let cp = &got["checkpoint"];
    assert_eq!(cp["goal"]["text"], "修复超时");
    assert_eq!(cp["goal"]["status"], "verified");
    assert_eq!(cp["intents"][0]["summary"], "git push");
    assert_eq!(cp["intents"][0]["state"], "pending");
    let file = host
        .context
        .paths
        .data_dir()
        .join("checkpoints")
        .join("task-1.json");
    assert!(file.is_file(), "saved when the approval was asked");

    host.call(
        "checkpoint.addConstraint",
        json!({"taskId": "task-1", "text": "不要推送"}),
    )
    .await
    .unwrap();
    let got = host
        .call("checkpoint.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(got["checkpoint"]["constraints"][0]["text"], "不要推送");
    assert_eq!(
        host.call(
            "checkpoint.setObligation",
            json!({"taskId": "task-1", "id": "nope", "state": "passed"})
        )
        .await
        .unwrap_err()["code"],
        "NOT_FOUND"
    );
    assert_eq!(
        host.call("checkpoint.get", json!({"taskId": "nope"}))
            .await
            .unwrap_err()["code"],
        "NOT_FOUND"
    );
}

/// A2: two ends answer the same approval while Codex is busy: the first is
/// held, the second is refused `BUSY`. Codex exits before the answer is
/// written: the request ends unsent, never as answered, and the checkpoint
/// records no consent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_codex_never_got_is_not_settled_and_the_second_answer_is_busy() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.send(
        json!({"jsonrpc": "2.0", "id": 7, "method": "item/commandExecution/requestApproval",
        "params": {"threadId": "th-1", "itemId": "c1", "command": "git push"}}),
    );
    host.until_state("a pending approval", |s| {
        s["pending"].as_array().is_some_and(|p| p.len() == 1)
    })
    .await;

    host.call("session.interrupt", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let _ = peer.expect("turn/interrupt").await;
    host.call(
        "session.respond",
        json!({"taskId": "task-1", "requestId": "7", "decision": "allowOnce"}),
    )
    .await
    .unwrap();
    let second = host
        .call(
            "session.respond",
            json!({"taskId": "task-1", "requestId": "7", "decision": "deny"}),
        )
        .await
        .unwrap_err();
    assert_eq!(second["code"], "BUSY");

    drop(peer);
    host.until_state("the request ended", |s| {
        s["pending"].as_array().is_some_and(|p| p.is_empty())
    })
    .await;
    let batch = host
        .call("session.updates", json!({"taskId": "task-1", "after": 0}))
        .await
        .unwrap();
    let updates = batch["updates"].as_array().unwrap();
    assert!(
        updates.iter().any(|u| u["type"] == "permissionSendFailed"
            && u["id"] == "7"
            && u["state"] == "ended"),
        "{batch}"
    );
    assert!(
        !updates.iter().any(|u| u["type"] == "permissionResolved"),
        "{batch}"
    );
    let got = host
        .call("checkpoint.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(got["checkpoint"]["intents"][0]["state"], "outcomeUnknown");
}

/// B4′: accepting the clarification card turns the ticked criteria into the
/// task's obligations and starts the work with them in the message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_a_clarification_starts_the_work_with_its_criteria() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    host.until_state("the turn ended", |s| s["turnActive"] == false)
        .await;

    let accepted = host
        .call(
            "clarify.accept",
            json!({"taskId": "task-1",
                "acceptance": [{"text": "测试通过", "check": "cargo test"}, {"text": "文档更新"}],
                "constraints": ["不要推送"]}),
        )
        .await
        .unwrap();
    assert_eq!(accepted, json!({"accepted": true}));
    let next = peer.expect("turn/start").await;
    let text = next["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("1. 测试通过（核对：`cargo test`）"), "{text}");
    assert!(text.contains("- 不要推送"), "{text}");

    let got = host
        .call("checkpoint.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let obligations = &got["checkpoint"]["obligations"];
    assert_eq!(obligations[0]["id"], "a1");
    assert_eq!(obligations[0]["check"], "cargo test");
    assert_eq!(obligations[1]["source"], "acceptance");
    assert_eq!(got["checkpoint"]["constraints"][0]["text"], "不要推送");
}

/// C 批 (2026-11-08): a cut message says so and reads on to the whole text;
/// tool calls are lines of their own; a transcript too big says why. It
/// used to cut at 4000 chars silently, skip tools, and read as empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_session_reads_on_shows_tools_and_says_why_it_cannot() {
    let mut host = Host::start(true).await;
    let rollout = std::env::temp_dir().join(format!(
        "ah-rollout-{}-{}.jsonl",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let long = "长".repeat(5000);
    let lines = [
        json!({"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "查一下"}]}}),
        json!({"type": "response_item", "payload": {"type": "function_call", "name": "exec_command",
            "arguments": "{\"cmd\":\"cargo test\"}", "call_id": "c1"}}),
        json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1",
            "output": "test result: ok"}}),
        json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
            "content": [{"type": "output_text", "text": long}]}}),
    ];
    std::fs::write(
        &rollout,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
    .unwrap();
    host.start_task("查一下");
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
        json!({"thread": {"id": "th-1", "path": rollout.to_string_lossy()}}),
    );
    host.until("the rollout is recorded", |h| {
        h.record().to_string().contains(&*rollout.to_string_lossy())
    })
    .await;
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let read = host
        .call("context.readSession", json!({"token": token}))
        .await
        .unwrap();
    let messages = read["messages"].as_array().unwrap();
    assert_eq!(read["total"], 3, "{read}");
    assert_eq!(
        messages[1]["text"],
        "[工具] exec_command：cargo test → test result: ok"
    );
    let last = &messages[2];
    assert_eq!(last["chars"], 5000);
    assert_eq!(last["truncated"], true);
    assert_eq!(last["nextOffset"], 4000);
    let rest = host
        .call(
            "context.readSession",
            json!({"token": token, "from": 2, "offset": 4000, "limit": 1}),
        )
        .await
        .unwrap();
    let piece = &rest["messages"][0];
    assert_eq!(piece["truncated"], false);
    assert!(piece.get("nextOffset").is_none());
    assert_eq!(
        format!(
            "{}{}",
            last["text"].as_str().unwrap(),
            piece["text"].as_str().unwrap()
        ),
        long,
        "两段拼回原文"
    );

    std::fs::OpenOptions::new()
        .write(true)
        .open(&rollout)
        .unwrap()
        .set_len(33 * 1024 * 1024)
        .unwrap();
    let too_big = host
        .call("context.readSession", json!({"token": token}))
        .await
        .unwrap_err();
    let _ = std::fs::remove_file(&rollout);
    assert_eq!(too_big["code"], "IO_ERROR");
    assert!(
        too_big["message"].as_str().unwrap().contains("超过 32 MiB"),
        "{too_big}"
    );
}

/// B4′: the context server's host methods answer only a live token, and only
/// for the token's own project.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_methods_need_the_session_token_and_stay_in_the_project() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");

    let bad = host
        .call("context.readHandoff", json!({"token": "made-up"}))
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "UNAUTHENTICATED");

    let handoff = host
        .call("context.readHandoff", json!({"token": token}))
        .await
        .unwrap();
    assert!(handoff["brief"].as_str().unwrap().starts_with("# 交接清单"));
    assert_eq!(handoff["checkpoint"]["goal"]["text"], "修复超时");

    let cross = host
        .call(
            "context.searchSessions",
            json!({"token": token, "query": "超时", "allProjects": true}),
        )
        .await
        .unwrap_err();
    assert_eq!(cross["code"], "INVALID_ARGUMENT");
    let search = host
        .call(
            "context.searchSessions",
            json!({"token": token, "query": "超时"}),
        )
        .await
        .unwrap();
    assert!(search["hits"].is_array());

    host.structured.context_tokens().revoke("task-1");
    assert_eq!(
        host.call("context.readHandoff", json!({"token": token}))
            .await
            .unwrap_err()["code"],
        "UNAUTHENTICATED"
    );
}

/// B4′: a side branch is a marked fork; only a branch can bring a result back,
/// and only once it has an answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_side_branch_is_a_marked_fork_of_the_session() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
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
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    host.until("the turn settled", |h| h.record()["status"] == "done")
        .await;
    let started = host
        .call("branch.start", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let branch = started["taskId"].as_str().unwrap().to_string();
    let record = host.record_of(&branch);
    assert_eq!(record["branchOf"], "task-1");
    assert!(record["name"].as_str().unwrap().ends_with("（侧支）"));

    let not_branch = host
        .call("branch.return", json!({"taskId": "task-1"}))
        .await
        .unwrap_err();
    assert_eq!(not_branch["message"], "这个会话不是侧支");
    let empty = host
        .call("branch.return", json!({"taskId": branch}))
        .await
        .unwrap_err();
    assert_eq!(empty["message"], "侧支还没有回答可以带回");
}

/// HANDOFF-001 (2026-10-30): a source move that falls back to a handoff (here:
/// no transcript to carry over) clears the old session, so the next message
/// starts a new thread with the brief instead of resuming the old one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_handoff_move_starts_a_new_session_instead_of_resuming() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-old"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-old", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-old", "turn": {"id": "u1", "status": "completed"}}),
    );
    host.until("the turn settled", |h| h.record()["status"] == "done")
        .await;
    assert_eq!(host.record()["codexSessionId"], "th-old");

    let moved = host
        .call(
            "session.moveSource",
            json!({"taskId": "task-1", "model": "fixture-new-model"}),
        )
        .await
        .unwrap();
    assert_eq!(moved["mode"], "handoff");
    let record = host.record();
    assert!(record.get("codexSessionId").is_none(), "{record}");
    assert!(record["pendingHandoff"].is_string());

    let unsubscribe = peer.expect("thread/unsubscribe").await;
    peer.reply(&unsubscribe, json!({"status": "unsubscribed"}));
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "继续"}),
    )
    .await
    .unwrap();
    let fresh = peer.next().await;
    assert_eq!(fresh["method"], "thread/start", "{fresh}");
    // H3: the opening first, then the brief.
    let brief = fresh["params"]["developerInstructions"].as_str().unwrap();
    assert!(brief.starts_with("# Aeroric 能做什么"), "{brief}");
    assert!(brief.contains("\n\n---\n\n# 交接清单"), "{brief}");
}

/// B4-1 (2026-10-30): with the review loop on, a Codex turn that changed a
/// file is reviewed by a read-only Claude task marked `reviewOf`, which sees
/// the diff and not the conversation; its blocker goes back to the Codex
/// session, and rejecting it settles the loop into the attention queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_turn_is_reviewed_by_another_family_and_answered() {
    let mut host = Host::start(true).await;
    let workspace = host._root.join("workspace");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    host.start_task("修复超时");
    let set = host
        .call("review.set", json!({"taskId": "task-1", "enabled": true}))
        .await
        .unwrap();
    assert_eq!(set["settings"]["reviewer"], "auto");
    let reviewers = host
        .call("review.reviewers", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(reviewers["auto"]["agent"], "claude", "{reviewers}");
    // E4: the rules in force, built-ins and the project's own.
    std::fs::create_dir_all(workspace.join(".aeroric")).unwrap();
    std::fs::write(
        workspace.join(".aeroric/review-rules.json"),
        r#"{"disable": ["shell"], "rules": [{"id": "money", "paths": ["*.rs"], "text": "金额用整数分"}]}"#,
    )
    .unwrap();
    let rules = host
        .call("review.rules", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(rules["rules"]["state"], "loaded", "{rules}");
    let listed = rules["rules"]["rules"].as_array().unwrap();
    assert!(listed
        .iter()
        .any(|r| r["id"] == "money" && r["source"] == "project" && r["severity"] == "major"));
    assert!(listed
        .iter()
        .any(|r| r["id"] == "shell" && r["disabled"] == true));
    std::fs::remove_dir_all(workspace.join(".aeroric")).unwrap();

    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-1"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "item/agentMessage/delta",
        json!({"threadId": "th-1", "itemId": "m1", "delta": "我的私下推理"}),
    );
    // Give the turn-start snapshot time before the file changes.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    std::fs::write(workspace.join("client.rs"), "loop { retry() }\n").unwrap();
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );

    let (claude, mut reviewer) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .expect("a reviewer started")
        .expect("launcher");
    assert_eq!(claude.permission, "plan");
    let id = reviewer.expect_control("initialize").await;
    reviewer.control_ok(&id, json!({"commands": []}));
    let asked = reviewer.next().await;
    let prompt = asked["message"]["content"].as_str().unwrap().to_string();
    assert!(prompt.starts_with("[Aeroric 审查]"), "{prompt}");
    assert!(
        prompt.contains("client.rs") && prompt.contains("retry()"),
        "{prompt}"
    );
    assert!(!prompt.contains("私下推理"));
    reviewer.send(
        json!({"type": "system", "subtype": "init", "session_id": "rv-s", "model": "sonnet"}),
    );
    reviewer.send(json!({"type": "assistant", "session_id": "rv-s", "message": {"id": "a1", "content": [
        {"type": "text", "text": "```json\n{\"verdict\":\"changes\",\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"client.rs\",\"line\":1,\"text\":\"重试没有上限\"}]}\n```"}]}}));
    reviewer.send(
        json!({"type": "result", "subtype": "success", "is_error": false,
        "session_id": "rv-s", "usage": {"input_tokens": 1, "output_tokens": 1}}),
    );

    let answer = peer.expect("turn/start").await;
    let text = answer["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.starts_with("[Aeroric 审查意见]"), "{text}");
    assert!(text.contains("重试没有上限"));
    let reviewer_task = host
        .call("review.get", json!({"taskId": "task-1"}))
        .await
        .unwrap()["card"]["reviewerTaskId"]
        .as_str()
        .unwrap()
        .to_string();
    let record = host.record_of(&reviewer_task);
    assert_eq!(record["reviewOf"], "task-1");
    assert_eq!(record["permissionMode"], "plan");
    assert!(record["name"].as_str().unwrap().ends_with("（审查）"));
    // A reviewer is never reviewed itself.
    let not = host
        .call(
            "review.set",
            json!({"taskId": reviewer_task, "enabled": true}),
        )
        .await
        .unwrap_err();
    assert_eq!(not["message"], "这个会话不能被审查");

    peer.reply(&answer, json!({"turn": {"id": "u2"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u2"}}),
    );
    peer.notify(
        "item/agentMessage/delta",
        json!({"threadId": "th-1", "itemId": "m2", "delta":
            "```json\n{\"responses\":[{\"id\":\"r1\",\"decision\":\"reject\",\"reason\":\"调用方已限流\"}]}\n```"}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u2", "status": "completed"}}),
    );
    let deadline = std::time::Instant::now() + WAIT;
    let card = loop {
        let got = host
            .call("review.get", json!({"taskId": "task-1"}))
            .await
            .unwrap();
        if got["card"]["state"] == "settled" {
            break got["card"].clone();
        }
        assert!(std::time::Instant::now() < deadline, "{got}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(card["findings"][0]["decision"], "reject");
    assert_eq!(card["findings"][0]["reason"], "调用方已限流");
    let attention = host.call("review.attention", json!({})).await.unwrap();
    assert_eq!(attention["items"][0]["taskId"], "task-1");
    let updates = host
        .call("session.updates", json!({"taskId": "task-1", "after": 0}))
        .await
        .unwrap();
    assert!(types(&updates).iter().any(|t| t == "review"));
}

/// B4-2 (2026-10-31): the user delegates from a Codex session to a read-only
/// reviewer of the other family. The Claude child gets the contract, not the
/// conversation; its answer becomes a result card; a child cannot delegate;
/// sending the result gives the Codex parent a turn; every step is audited.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_delegation_runs_in_a_child_and_its_result_comes_back() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-1"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "item/agentMessage/delta",
        json!({"threadId": "th-1", "itemId": "m1", "delta": "我的私下推理"}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );

    let above = host
        .call(
            "delegation.start",
            json!({"taskId": "task-1", "contract": {"instruction": "改", "permission": "full_access", "workspace": "shared"}}),
        )
        .await
        .unwrap_err();
    assert!(
        above["message"].as_str().unwrap().contains("不能高于"),
        "{above}"
    );
    let started = host
        .call(
            "delegation.start",
            json!({"taskId": "task-1", "contract": {"instruction": "查 reqwest 的连接超时", "role": "reviewer",
                "acceptance": [{"text": "给出示例"}]}}),
        )
        .await
        .unwrap();
    let id = started["delegation"]["delegationId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(started["delegation"]["contract"]["permission"], "plan");

    let (claude, mut child) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .expect("a child started")
        .expect("launcher");
    assert_eq!(claude.permission, "plan");
    let init = child.expect_control("initialize").await;
    child.control_ok(&init, json!({"commands": []}));
    let asked = child.next().await;
    let prompt = asked["message"]["content"].as_str().unwrap().to_string();
    assert!(prompt.starts_with("[Aeroric 委派]"), "{prompt}");
    assert!(prompt.contains("查 reqwest 的连接超时") && prompt.contains("审查员"));
    assert!(prompt.contains("给出示例"));
    assert!(!prompt.contains("私下推理"));
    child.send(
        json!({"type": "system", "subtype": "init", "session_id": "dg-s", "model": "sonnet"}),
    );
    child.send(
        json!({"type": "assistant", "session_id": "dg-s", "message": {"id": "a1", "content": [
        {"type": "text", "text": "用 ClientBuilder::connect_timeout"}]}}),
    );
    child.send(
        json!({"type": "result", "subtype": "success", "is_error": false,
        "session_id": "dg-s", "usage": {"input_tokens": 1, "output_tokens": 1}}),
    );

    let deadline = std::time::Instant::now() + WAIT;
    let card = loop {
        let got = host
            .call("delegation.list", json!({"taskId": "task-1"}))
            .await
            .unwrap();
        if got["delegations"][0]["state"] == "reported" {
            break got["delegations"][0].clone();
        }
        assert!(std::time::Instant::now() < deadline, "{got}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(card["result"], "用 ClientBuilder::connect_timeout");
    assert_eq!(card["agent"], "claude");
    assert_eq!(card["origin"], "user");
    assert_eq!(card["sent"], false);
    let child_id = card["childTaskId"].as_str().unwrap().to_string();
    // The child is renamed just after it is created; wait for that.
    host.until("the child is renamed", |h| {
        h.record_of(&child_id)["name"]
            .as_str()
            .is_some_and(|n| n.ends_with("（委派·审查员）"))
    })
    .await;
    let record = host.record_of(&child_id);
    assert_eq!(record["delegateOf"], "task-1");
    assert_eq!(record["permissionMode"], "plan");
    assert!(
        record["name"]
            .as_str()
            .unwrap()
            .ends_with("（委派·审查员）"),
        "{record}"
    );
    let listed = host
        .call("delegation.list", json!({"taskId": child_id}))
        .await
        .unwrap();
    assert_eq!(listed["delegateOf"], "task-1");
    let nested = host
        .call(
            "delegation.start",
            json!({"taskId": child_id, "contract": {"instruction": "再拆"}}),
        )
        .await
        .unwrap_err();
    assert!(
        nested["message"].as_str().unwrap().contains("深度"),
        "{nested}"
    );

    host.call(
        "delegation.send",
        json!({"taskId": "task-1", "delegationId": id}),
    )
    .await
    .unwrap();
    let back = peer.expect("turn/start").await;
    let text = back["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.starts_with("[Aeroric 委派结果]"), "{text}");
    assert!(text.contains("connect_timeout"));
    // R1-2: it went through the delivery queue; Codex's answer is the proof.
    peer.reply(&back, json!({"turn": {"id": "u2"}}));
    let deadline = std::time::Instant::now() + WAIT;
    let delivered = loop {
        let got = host
            .call("delivery.list", json!({"taskId": "task-1"}))
            .await
            .unwrap();
        if got["deliveries"][0]["state"] == "delivered" {
            break got["deliveries"][0].clone();
        }
        assert!(std::time::Instant::now() < deadline, "{got}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(delivered["kind"], "delegation");
    assert_eq!(delivered["evidence"], "protocol");
    assert!(
        delivered["label"]
            .as_str()
            .unwrap()
            .starts_with("委派结果 · 查 reqwest"),
        "{delivered}"
    );
    // A1: the card follows that delivery, and names it as its source.
    assert_eq!(
        delivered["sourceRef"],
        format!("delegation:task-1/{id}"),
        "{delivered}"
    );
    let deadline = std::time::Instant::now() + WAIT;
    let card = loop {
        let got = host
            .call("delegation.list", json!({"taskId": "task-1"}))
            .await
            .unwrap();
        if got["delegations"][0]["returnDelivery"]["state"] == "delivered" {
            break got["delegations"][0].clone();
        }
        assert!(std::time::Instant::now() < deadline, "{got}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(card["returnDelivery"]["id"], delivered["id"]);
    assert_eq!(card["returnDelivery"]["evidence"], "protocol");
    assert_eq!(card["sent"], true);
    let gone = host
        .call("delivery.resend", json!({"id": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(gone["code"], "NOT_FOUND", "{gone}");

    let audit = host
        .call("delegation.audit", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    let actions: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["refuse", "start", "report", "send"], "{audit}");
    let roles = host.call("delegation.roles", json!({})).await.unwrap();
    // Six built-ins since the expert (2026-11-13, Smart Friend).
    assert_eq!(roles["roles"].as_array().unwrap().len(), 6);
    let stats = host.call("delegation.stats", json!({})).await.unwrap();
    assert_eq!(stats["rows"][0]["reported"], 1);
    let updates = host
        .call("session.updates", json!({"taskId": "task-1", "after": 0}))
        .await
        .unwrap();
    assert!(types(&updates).iter().any(|t| t == "delegation"));
}

/// B4-3 (2026-10-31): the user writes the task's board; a decision waits in
/// the attention list until it is accepted; a hypothesis takes only its own
/// states.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_board_decision_waits_for_the_user() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let empty = host
        .call("board.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(empty["board"]["revision"], 0);
    assert_eq!(empty["shared"], false);
    host.call(
        "board.write",
        json!({"taskId": "task-1", "entry": {"kind": "goal", "text": "修复超时"}}),
    )
    .await
    .unwrap();
    let decision = host
        .call(
            "board.write",
            json!({"taskId": "task-1", "entry": {"kind": "decision", "text": "用 connect_timeout"}}),
        )
        .await
        .unwrap();
    assert_eq!(decision["entry"]["state"], "proposed");
    let id = decision["entry"]["id"].as_str().unwrap().to_string();
    let attention = host.call("board.attention", json!({})).await.unwrap();
    assert_eq!(attention["items"][0]["reason"], "decision");
    assert_eq!(attention["items"][0]["ownerTaskId"], "task-1");
    let bad = host
        .call(
            "board.write",
            json!({"taskId": "task-1", "entry": {"kind": "hypothesis", "text": "x", "state": "accepted"}}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "INVALID_ARGUMENT", "{bad}");
    host.call(
        "board.decide",
        json!({"taskId": "task-1", "entryId": id, "accept": true}),
    )
    .await
    .unwrap();
    let board = host
        .call("board.get", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(board["board"]["entries"][1]["state"], "accepted");
    assert_eq!(board["board"]["revision"], 3);
    let attention = host.call("board.attention", json!({})).await.unwrap();
    assert!(attention["items"].as_array().unwrap().is_empty());
    let invalid = host
        .call("context.boardRead", json!({"token": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(invalid["code"], "UNAUTHENTICATED", "{invalid}");
}

/// B5-1 (2026-10-31): what the user said to keep goes to a starting
/// session's system layer; an agent's proposal waits for the user, recall
/// finds it once confirmed, and a forgotten item cannot be proposed again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remembered_words_reach_a_new_session_and_proposals_wait() {
    let mut host = Host::start(true).await;
    let kept = host
        .call(
            "memory.remember",
            json!({"draft": {"projectId": "project-1", "kind": "profile", "category": "language",
                             "text": "回复一律用简体中文"}, "source": "/记住 回复一律用简体中文"}),
        )
        .await
        .unwrap();
    assert_eq!(kept["item"]["assertion"], "userExplicit");
    // C2: archiving says what it touched; restoring brings it back.
    let kept_id = kept["item"]["id"].clone();
    let archived = host
        .call("memory.archive", json!({"id": kept_id, "archived": true}))
        .await
        .unwrap();
    assert_eq!(archived["item"]["state"], "archived", "{archived}");
    assert_eq!(archived["withdrawal"]["items"], 1, "{archived}");
    let back = host
        .call("memory.archive", json!({"id": kept_id, "archived": false}))
        .await
        .unwrap();
    assert_eq!(back["item"]["state"], "active", "{back}");
    assert_eq!(back["withdrawal"]["items"], 0, "{back}");
    // C1: what the user said with /记住 has no session place to look up.
    let item_id = kept["item"]["id"].as_str().unwrap().to_string();
    let place = host
        .call(
            "memory.evidenceLocate",
            json!({"itemId": item_id, "index": 0}),
        )
        .await
        .unwrap();
    assert_eq!(place["status"], "unrecorded", "{place}");
    assert_eq!(place["excerpt"], "/记住 回复一律用简体中文");
    for (params, what) in [
        (json!({"itemId": item_id, "index": 9}), "no such evidence"),
        (json!({"itemId": "mem-nope", "index": 0}), "no such item"),
    ] {
        let missing = host
            .call("memory.evidenceLocate", params)
            .await
            .unwrap_err();
        assert_eq!(missing["code"], "NOT_FOUND", "{what}: {missing}");
    }
    let secret = host
        .call(
            "memory.remember",
            json!({"draft": {"kind": "fact", "text": "key sk-ant-abcdefgh123456"}}),
        )
        .await
        .unwrap_err();
    assert_eq!(secret["code"], "INVALID_ARGUMENT", "{secret}");

    host.start_task("修复超时");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    // H3: the profile is the opening's second part, after the purpose.
    let instructions = start["params"]["developerInstructions"].as_str().unwrap();
    assert!(
        instructions.starts_with("# Aeroric 能做什么"),
        "{instructions}"
    );
    assert!(
        instructions.contains("\n\n# Aeroric 记忆"),
        "{instructions}"
    );
    assert!(instructions.contains("[语言] 回复一律用简体中文"));

    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let enabled = host
        .call("context.memoryEnabled", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(enabled["enabled"], true);
    let proposed = host
        .call(
            "context.memoryPropose",
            json!({"token": token, "text": "发布前先跑 native:check sessions"}),
        )
        .await
        .unwrap();
    assert_eq!(proposed["item"]["state"], "pending");
    let id = proposed["item"]["id"].as_str().unwrap().to_string();
    let attention = host.call("memory.attention", json!({})).await.unwrap();
    assert_eq!(attention["items"][0]["id"], id.as_str());
    let nothing = host
        .call(
            "context.memoryRecall",
            json!({"token": token, "query": "发布"}),
        )
        .await
        .unwrap();
    assert_eq!(nothing["text"], "没有找到相关记忆。");
    host.call("memory.confirm", json!({"id": id, "accept": true}))
        .await
        .unwrap();
    let found = host
        .call(
            "context.memoryRecall",
            json!({"token": token, "query": "发布"}),
        )
        .await
        .unwrap();
    assert_eq!(found["items"][0]["assertion"], "agentConfirmed", "{found}");

    // C2: `removed` stays the count; `withdrawal` says what else it touched.
    let forgot = host.call("memory.forget", json!({"id": id})).await.unwrap();
    assert_eq!(forgot["removed"], 1, "{forgot}");
    assert_eq!(forgot["withdrawal"]["items"], 1, "{forgot}");
    assert_eq!(forgot["withdrawal"]["dreams"], 0, "{forgot}");
    let again = host
        .call(
            "context.memoryPropose",
            json!({"token": token, "text": "发布前先跑 native:check sessions"}),
        )
        .await
        .unwrap_err();
    assert_eq!(again["code"], "INVALID_ARGUMENT", "{again}");
    let list = host
        .call("memory.list", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["enabled"], true);
    host.call(
        "memory.project",
        json!({"projectId": "project-1", "enabled": false}),
    )
    .await
    .unwrap();
    let off = host
        .call(
            "context.memoryRecall",
            json!({"token": token, "query": "中文"}),
        )
        .await
        .unwrap_err();
    assert_eq!(off["code"], "INVALID_ARGUMENT");
}

/// B5-2 (2026-10-31): the 整理设置 and its guards; a session that cannot be
/// read, or whose project does not use memory, is skipped and says why —
/// nothing reaches a model.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extraction_is_off_until_a_model_is_chosen_and_says_why_it_skips() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let models = host.call("memory.models", json!({})).await.unwrap();
    assert!(models["models"].as_array().unwrap().is_empty());
    let refused = host
        .call("memory.settings", json!({"settings": {"auto": true}}))
        .await
        .unwrap_err();
    assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");
    let none = host
        .call("memory.extract", json!({"taskId": "task-1"}))
        .await
        .unwrap_err();
    assert!(
        none["message"]
            .as_str()
            .unwrap()
            .contains("还没有选分析模型"),
        "{none}"
    );
    let saved = host
        .call(
            "memory.settings",
            json!({"settings": {"auto": true, "connectionId": "c1", "model": "flash", "dailyLimit": 0}}),
        )
        .await
        .unwrap();
    assert_eq!(saved["settings"]["dailyLimit"], 1);
    assert!(saved["settings"]["enabledMs"].as_u64().unwrap() > 0);
    let off = host
        .call("memory.extract", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(off["run"]["state"], "skipped");
    assert_eq!(off["run"]["reason"], "项目没有开启记忆");
    host.call(
        "memory.project",
        json!({"projectId": "project-1", "enabled": true}),
    )
    .await
    .unwrap();
    let unread = host
        .call("memory.extract", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(unread["run"]["reason"], "读不到这个会话的对话");
    let runs = host.call("memory.runs", json!({})).await.unwrap();
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2);
    assert_eq!(runs["usedToday"], 0, "skips are free");
    let swept = host.call("memory.sweep", json!({})).await.unwrap();
    assert_eq!(swept["extracted"], 0, "task-1 is already handled");
    let outside = host
        .call("memory.importNative", json!({"path": "/etc/hosts"}))
        .await
        .unwrap_err();
    assert_eq!(outside["code"], "INVALID_ARGUMENT");
}

/// B6 (2026-11-13): 现在整理 needs a model; a scope with nothing alike is
/// recorded as skipped without a call, and the proposal and undo methods
/// refuse what does not exist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dream_needs_a_model_and_a_scope_with_nothing_alike_costs_nothing() {
    let host = Host::start(true).await;
    let none = host
        .call("memory.dream", json!({"projectId": "project-1"}))
        .await
        .unwrap_err();
    assert!(
        none["message"]
            .as_str()
            .unwrap()
            .contains("还没有选分析模型"),
        "{none}"
    );
    host.call(
        "memory.settings",
        json!({"settings": {"auto": false, "connectionId": "c1", "model": "flash"}}),
    )
    .await
    .unwrap();
    host.call(
        "memory.remember",
        json!({"draft": {"projectId": "project-1", "kind": "fact", "text": "数据集放在 /data/coco"}}),
    )
    .await
    .unwrap();
    let skipped = host
        .call("memory.dream", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(skipped["run"]["kind"], "dream");
    assert_eq!(skipped["run"]["state"], "skipped");
    assert_eq!(
        skipped["run"]["reason"],
        "没有字面相近的记忆，画像也没超出，不用整理"
    );
    // Second batch: nothing waits for 需要我处理, and only an accepted
    // proposal can be taken back.
    let pending = host.call("memory.dreamPending", json!({})).await.unwrap();
    assert!(pending["proposals"].as_array().unwrap().is_empty());
    let unknown = host
        .call("memory.dreamRevert", json!({"id": "dream-x"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    let proposals = host
        .call("memory.dreamProposals", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert!(proposals["proposals"].as_array().unwrap().is_empty());
    let unknown = host
        .call(
            "memory.dreamDecide",
            json!({"id": "dream-x", "accept": true}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    let skipped_id = skipped["run"]["id"].as_str().unwrap();
    let not_undoable = host
        .call("memory.dreamUndo", json!({"runId": skipped_id}))
        .await;
    assert!(not_undoable.is_ok(), "a skipped dream undoes nothing");
    let again = host
        .call("memory.dreamUndo", json!({"runId": skipped_id}))
        .await
        .unwrap_err();
    assert_eq!(again["code"], "INVALID_ARGUMENT", "once");
}

/// B6 (2026-11-14): 按意思找相近的记忆 is saved with the settings; before a
/// model has answered nothing is embedded, and recall still works by words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_semantic_switch_is_saved_and_its_status_starts_empty() {
    let host = Host::start(true).await;
    let saved = host
        .call("memory.settings", json!({"settings": {"semantic": true}}))
        .await
        .unwrap();
    assert_eq!(saved["settings"]["semantic"], true);
    let status = host.call("memory.semanticStatus", json!({})).await.unwrap();
    assert_eq!(status["status"]["embedded"], 0);
    assert_eq!(status["status"]["model"], "");
    assert!(status["model"].is_string());
}

/// A12 (2026-11-14): 定时任务 — a rule is saved, listed with its next time,
/// and 立即运行 with a failing pre-check wakes no model; bad rules and
/// unknown ids are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_schedule_with_a_failing_check_opens_no_session() {
    let host = Host::start(true).await;
    let rule = json!({
        "id": "", "name": "早上看新提交", "projectId": "project-1", "agent": "codex",
        "prompt": "看看有什么新提交", "repeat": {"kind": "daily", "minute": 540},
        "check": "echo 没有新提交; exit 1", "enabled": true
    });
    let saved = host
        .call("schedule.save", json!({ "rule": rule }))
        .await
        .unwrap();
    let id = saved["rule"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("sched-"));
    let listed = host.call("schedule.list", json!({})).await.unwrap();
    assert_eq!(listed["rules"][0]["id"], id.as_str());
    assert!(listed["rules"][0]["nextMs"].as_u64().is_some());

    // 2026-11-15 目检: the App's once and every rules (camelCase fields),
    // with the second batch's options, are saved and read back.
    let mut every = rule.clone();
    every["repeat"] = json!({"kind": "every", "hours": 2, "fromMs": 1_800_000_000_000u64});
    every["whenBusy"] = json!("after");
    every["activeHours"] = json!({"startMinute": 540, "endMinute": 1080});
    let saved_every = host
        .call("schedule.save", json!({ "rule": every }))
        .await
        .unwrap();
    assert_eq!(
        saved_every["rule"]["repeat"]["fromMs"],
        1_800_000_000_000u64
    );
    assert_eq!(saved_every["rule"]["whenBusy"], "after");
    assert_eq!(saved_every["rule"]["activeHours"]["endMinute"], 1080);
    let mut once = rule.clone();
    once["repeat"] = json!({"kind": "once", "atMs": 4_000_000_000_000u64});
    let saved_once = host
        .call("schedule.save", json!({ "rule": once }))
        .await
        .unwrap();
    assert_eq!(saved_once["rule"]["repeat"]["atMs"], 4_000_000_000_000u64);
    for extra in [&saved_every, &saved_once] {
        host.call("schedule.delete", json!({ "id": extra["rule"]["id"] }))
            .await
            .unwrap();
    }

    let run = host
        .call("schedule.runNow", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(run["run"]["outcome"], "checkFailed", "{run}");
    assert_eq!(run["run"]["detail"], "退出码 1：没有新提交");
    assert!(run["run"]["taskId"].is_null());
    let runs = host
        .call("schedule.runs", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(runs["runs"].as_array().unwrap().len(), 1);

    let mut nameless = rule.clone();
    nameless["name"] = json!(" ");
    let refused = host
        .call("schedule.save", json!({ "rule": nameless }))
        .await
        .unwrap_err();
    assert_eq!(refused["code"], "INVALID_ARGUMENT");
    let mut elsewhere = rule.clone();
    elsewhere["projectId"] = json!("project-none");
    let refused = host
        .call("schedule.save", json!({ "rule": elsewhere }))
        .await
        .unwrap_err();
    assert_eq!(refused["code"], "INVALID_ARGUMENT");

    host.call("schedule.delete", json!({ "id": id }))
        .await
        .unwrap();
    let gone = host
        .call("schedule.delete", json!({ "id": id }))
        .await
        .unwrap_err();
    assert_eq!(gone["code"], "NOT_FOUND");
}

/// 2026-11-15 工作安排第一批: a todo is written, handed out (a session opens
/// with the todo as its prompt), accepted only once handed out, and deleted;
/// bad todos and unknown ids are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_todo_is_handed_out_and_accepted_by_the_user() {
    let host = Host::start(true).await;
    let saved = host
        .call(
            "todo.save",
            json!({"todo": {"id": "", "projectId": "project-1", "title": "  给数据加载加单元测试 ", "notes": "先看 loader.py"}}),
        )
        .await
        .unwrap();
    let id = saved["todo"]["id"].as_str().unwrap().to_string();
    assert_eq!(saved["todo"]["title"], "给数据加载加单元测试");
    assert_eq!(saved["todo"]["state"], "open");
    let early = host
        .call("todo.decide", json!({"id": id, "decision": "accept"}))
        .await
        .unwrap_err();
    assert_eq!(early["code"], "INVALID_ARGUMENT", "not handed out yet");

    let assigned = host
        .call("todo.assign", json!({"id": id, "agent": "codex"}))
        .await
        .unwrap();
    let task = assigned["taskId"].as_str().unwrap().to_string();
    assert_eq!(assigned["todo"]["state"], "assigned");
    assert_eq!(assigned["todo"]["assignments"][0]["taskId"], task.as_str());
    let record = host
        .service
        .load("project-1")
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.get("id").and_then(Value::as_str) == Some(task.as_str()))
        .expect("the session is a task of the project");
    assert!(record
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap()
        .starts_with("给数据加载加单元测试\n\n先看 loader.py"));
    let listed = host.call("todo.list", json!({})).await.unwrap();
    assert_eq!(listed["todos"][0]["id"], id.as_str());
    assert!(listed["todos"][0]["progress"].is_string(), "{listed}");

    let done = host
        .call("todo.decide", json!({"id": id, "decision": "accept"}))
        .await
        .unwrap();
    assert_eq!(done["todo"]["state"], "done");
    let elsewhere = host
        .call(
            "todo.save",
            json!({"todo": {"id": "", "projectId": "project-none", "title": "x"}}),
        )
        .await
        .unwrap_err();
    assert_eq!(elsewhere["code"], "INVALID_ARGUMENT");
    host.call("todo.delete", json!({"id": id})).await.unwrap();
    let gone = host
        .call("todo.delete", json!({"id": id}))
        .await
        .unwrap_err();
    assert_eq!(gone["code"], "NOT_FOUND");
    let unknown = host
        .call("todo.assign", json!({"id": "todo-none", "agent": "codex"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
}

/// 2026-11-17 交办选配置: a configuration that cannot run is refused before
/// any session opens; one that can pins the session (`task.run`'s
/// `agentConfigId` lands on the task).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_todo_is_handed_out_only_with_a_configuration_that_can_run() {
    let host = Host::start(true).await;
    let saved = host
        .call(
            "todo.save",
            json!({"todo": {"id": "", "projectId": "project-1", "title": "换个模型做"}}),
        )
        .await
        .unwrap();
    let id = saved["todo"]["id"].as_str().unwrap().to_string();
    let tasks = |host: &Host| host.service.load("project-1").unwrap().records.len();
    let before = tasks(&host);
    for (configuration, words) in [("agent-config:gone", "配置不存在"), ("omp", "现在不能用")]
    {
        let refused = host
            .call(
                "todo.assign",
                json!({"id": id, "configurationId": configuration}),
            )
            .await
            .unwrap_err();
        assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");
        assert!(
            refused["message"].as_str().unwrap().contains(words),
            "{refused}"
        );
    }
    assert_eq!(tasks(&host), before, "nothing opened");
    let listed = host.call("todo.list", json!({})).await.unwrap();
    assert_eq!(listed["todos"][0]["state"], "open");

    // The pin reaches the task whatever the launch makes of it.
    let path = host
        .project
        .get("path")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let _ = host
        .call(
            "task.run",
            json!({"projectPath": path, "agent": "codex", "prompt": "x",
                   "taskId": "task-pinned", "agentConfigId": "deepseek"}),
        )
        .await;
    let record = host
        .service
        .load("project-1")
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.get("id").and_then(Value::as_str) == Some("task-pinned"))
        .expect("task.run made the task");
    assert_eq!(
        record.get("agentConfigId").and_then(Value::as_str),
        Some("deepseek")
    );
}

/// 2026-11-15 待办第二批: an Agent's proposal waits until the user takes it;
/// a 验收检查 is kept with the todo and runs only after the session's turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_proposes_a_todo_and_the_check_waits_for_the_turn() {
    let host = Host::start(true).await;
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let proposed = host
        .call(
            "context.todoPropose",
            json!({"token": token, "title": " 补 NMS 阈值的单测 ", "notes": "见 utils/nms.py"}),
        )
        .await
        .unwrap();
    assert!(
        proposed["text"].as_str().unwrap().starts_with("已提议"),
        "{proposed}"
    );
    let todo = &proposed["todo"];
    assert_eq!(
        (&todo["state"], &todo["proposedBy"], &todo["projectId"]),
        (&json!("proposed"), &json!("task-1"), &json!("project-1"))
    );
    let id = todo["id"].as_str().unwrap().to_string();
    let twin = host
        .call(
            "context.todoPropose",
            json!({"token": token, "title": "补 nms 阈值的单测"}),
        )
        .await
        .unwrap_err();
    assert_eq!(twin["code"], "INVALID_ARGUMENT");
    assert!(twin["message"].as_str().unwrap().contains(&id), "{twin}");
    let stranger = host
        .call(
            "context.todoPropose",
            json!({"token": "made-up", "title": "x"}),
        )
        .await
        .unwrap_err();
    assert_eq!(stranger["code"], "UNAUTHENTICATED");

    let early = host
        .call("todo.assign", json!({"id": id, "agent": "codex"}))
        .await
        .unwrap_err();
    assert_eq!(early["code"], "INVALID_ARGUMENT", "take it first");
    let taken = host
        .call("todo.decide", json!({"id": id, "decision": "take"}))
        .await
        .unwrap();
    assert_eq!(taken["todo"]["state"], "open");

    let saved = host
        .call(
            "todo.save",
            json!({"todo": {"id": id, "projectId": "project-1", "title": "补 NMS 阈值的单测",
                "check": " cargo test -p nms ", "fixOnFail": true}}),
        )
        .await
        .unwrap();
    assert_eq!(
        (&saved["todo"]["check"], &saved["todo"]["fixOnFail"]),
        (&json!("cargo test -p nms"), &json!(true))
    );
    let unassigned = host
        .call("todo.check", json!({"id": id}))
        .await
        .unwrap_err();
    assert_eq!(unassigned["code"], "INVALID_ARGUMENT");
    let assigned = host
        .call("todo.assign", json!({"id": id, "agent": "codex"}))
        .await
        .unwrap();
    let task = assigned["taskId"].as_str().unwrap().to_string();
    let record = host
        .service
        .load("project-1")
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.get("id").and_then(Value::as_str) == Some(task.as_str()))
        .unwrap();
    let prompt = record.get("prompt").and_then(Value::as_str).unwrap();
    assert!(prompt.contains("验收检查：`cargo test -p nms`"), "{prompt}");
    let running = host
        .call("todo.check", json!({"id": id}))
        .await
        .unwrap_err();
    assert_eq!(
        running["code"], "INVALID_ARGUMENT",
        "the turn has not ended"
    );
    let unknown = host
        .call("todo.check", json!({"id": "todo-none"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
}

/// B6 (2026-11-13): 代码版本链 hangs under a memory from a session; one the
/// user typed has none, and an unknown item is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_memory_without_a_session_has_no_code_chain() {
    let host = Host::start(true).await;
    let kept = host
        .call(
            "memory.remember",
            json!({"draft": {"projectId": "project-1", "kind": "fact", "text": "发布前跑 native:check"}}),
        )
        .await
        .unwrap();
    let id = kept["item"]["id"].as_str().unwrap();
    let chain = host.call("memory.chain", json!({"id": id})).await.unwrap();
    assert!(chain["changes"].as_array().unwrap().is_empty());
    let unknown = host
        .call("memory.chain", json!({"id": "mem-none"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
}

/// Q4 (2026-11-02): 整理历史会话 plans from the import index alone and
/// refuses a session it does not know; nothing reaches a model.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_history_plan_reads_only_the_index() {
    let host = Host::start(true).await;
    let plan = host
        .call(
            "memory.historyPlan",
            json!({"projectId": "project-1", "sinceMs": 0}),
        )
        .await
        .unwrap();
    assert_eq!(plan["sessions"].as_array().unwrap().len(), 0);
    assert_eq!(plan["pending"], 0);
    assert_eq!(plan["enabled"], false);
    assert!(plan.get("model").is_none(), "{plan}");
    assert!(plan["remainingToday"].as_u64().unwrap() > 0);
    let unknown = host
        .call("memory.extractImported", json!({"id": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "INVALID_ARGUMENT", "{unknown}");
}

/// B5-3a (2026-10-31): recall counts and shows how an item fared; the app
/// and an agent vote; an agent cannot vote on another project's item.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_counts_and_stays_in_the_project() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let kept = host
        .call(
            "memory.remember",
            json!({"draft": {"projectId": "project-1", "kind": "fact", "text": "发布前先跑 native:check"}}),
        )
        .await
        .unwrap();
    let id = kept["item"]["id"].as_str().unwrap().to_string();
    let elsewhere = host
        .call(
            "memory.remember",
            json!({"draft": {"projectId": "project-2", "kind": "fact", "text": "别的项目的约定"}}),
        )
        .await
        .unwrap();
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let found = host
        .call(
            "context.memoryRecall",
            json!({"token": token, "query": "发布"}),
        )
        .await
        .unwrap();
    assert!(
        found["text"].as_str().unwrap().contains("memory_feedback"),
        "{found}"
    );
    let voted = host
        .call(
            "context.memoryFeedback",
            json!({"token": token, "id": id, "helpful": true}),
        )
        .await
        .unwrap();
    assert_eq!(voted["item"]["stats"]["helpful"], 1);
    assert_eq!(voted["item"]["stats"]["retrieved"], 1);
    // C3: the same session recalling and voting again counts nothing more.
    host.call(
        "context.memoryRecall",
        json!({"token": token, "query": "发布"}),
    )
    .await
    .unwrap();
    let twice = host
        .call(
            "context.memoryFeedback",
            json!({"token": token, "id": id, "helpful": true}),
        )
        .await
        .unwrap();
    assert!(
        twice["text"].as_str().unwrap().contains("不重复计数"),
        "{twice}"
    );
    assert_eq!(twice["item"]["stats"]["helpful"], 1);
    assert_eq!(twice["item"]["stats"]["retrieved"], 1);
    let other = host
        .call(
            "context.memoryFeedback",
            json!({"token": token, "id": elsewhere["item"]["id"], "helpful": false}),
        )
        .await
        .unwrap_err();
    assert_eq!(other["code"], "NOT_FOUND", "{other}");
    let user = host
        .call("memory.feedback", json!({"id": id, "helpful": false}))
        .await
        .unwrap();
    assert_eq!(user["item"]["stats"]["wrong"], 1);
}

/// B5-3b (2026-10-31): three 有用 on an experience propose a skill; it waits
/// in attention, is edited, accepted (a version), reverted and disabled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trusted_experience_becomes_a_versioned_skill() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let experience = host
        .structured
        .memory()
        .add_experience(
            "project-1",
            "CSV 带 BOM：读取改用 utf-8-sig",
            &aeroric_protocol::memory::ExperienceDetail {
                problem: "首列名多出 BOM".into(),
                solution: "encoding=utf-8-sig".into(),
                ..Default::default()
            },
            aeroric_protocol::memory::Confidence::Low,
            aeroric_core::memory::evidence(Some("task-1"), None, None, "x", 1),
            1,
        )
        .unwrap();
    for _ in 0..3 {
        host.call(
            "memory.feedback",
            json!({"id": experience.id, "helpful": true}),
        )
        .await
        .unwrap();
    }
    let attention = host.call("memory.attention", json!({})).await.unwrap();
    let proposal = &attention["proposals"][0];
    assert_eq!(proposal["state"], "pending", "{attention}");
    let id = proposal["id"].as_str().unwrap().to_string();
    let edited = host
        .call(
            "memory.proposalEdit",
            json!({"id": id, "edit": {"description": "读带 BOM 的 CSV"}}),
        )
        .await
        .unwrap();
    assert_eq!(edited["proposal"]["description"], "读带 BOM 的 CSV");
    let accepted = host
        .call("memory.proposalDecide", json!({"id": id, "accept": true}))
        .await
        .unwrap();
    assert_eq!(accepted["proposal"]["activeVersion"], 1);
    let v2 = host
        .call(
            "memory.proposalEdit",
            json!({"id": id, "edit": {"body": "# v2"}}),
        )
        .await
        .unwrap();
    assert_eq!(v2["proposal"]["activeVersion"], 2);
    let back = host
        .call("memory.skillVersion", json!({"id": id, "version": 1}))
        .await
        .unwrap();
    assert_eq!(back["proposal"]["activeVersion"], 1);
    let off = host
        .call("memory.skillDisable", json!({"id": id, "disabled": true}))
        .await
        .unwrap();
    assert_eq!(off["proposal"]["state"], "disabled");
    let list = host.call("memory.proposals", json!({})).await.unwrap();
    assert_eq!(list["proposals"].as_array().unwrap().len(), 1);
    let none = host.call("memory.attention", json!({})).await.unwrap();
    assert!(none["proposals"].as_array().unwrap().is_empty());
}

/// C2 (2026-11-20): once an experience is forgotten, a word only it had is
/// found by no read the app or an agent has — memory list and search,
/// recall, 需要我处理, skills, the Wiki's pages, search, history and issues,
/// and the agent's Wiki tools — without compiling the Wiki again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forgotten_experience_is_read_back_from_nowhere() {
    const WORD: &str = "zebra-42";
    let host = Host::start(true).await;
    host.start_task("修复超时");
    host.call(
        "memory.project",
        json!({"projectId": "project-1", "enabled": true}),
    )
    .await
    .unwrap();
    let experience = host
        .structured
        .memory()
        .add_experience(
            "project-1",
            "部署前清空 zebra-42 缓存目录",
            &aeroric_protocol::memory::ExperienceDetail {
                problem: "旧缓存让部署失败".into(),
                solution: "rm -rf zebra-42".into(),
                ..Default::default()
            },
            aeroric_protocol::memory::Confidence::High,
            aeroric_core::memory::evidence(Some("task-1"), None, None, "x", 1),
            1,
        )
        .unwrap();
    host.call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    host.call(
        "wiki.agentAccess",
        json!({"projectId": "project-1", "enabled": true}),
    )
    .await
    .unwrap();
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let reads = [
        (
            "memory.list",
            json!({"projectId": "project-1", "query": "zebra"}),
        ),
        ("memory.list", json!({"projectId": "project-1"})),
        (
            "context.memoryRecall",
            json!({"token": token, "query": "zebra 缓存"}),
        ),
        ("memory.attention", json!({})),
        ("memory.proposals", json!({})),
        (
            "wiki.search",
            json!({"projectId": "project-1", "query": "zebra"}),
        ),
        (
            "wiki.page",
            json!({"projectId": "project-1", "slug": "runbook"}),
        ),
        (
            "wiki.revisions",
            json!({"projectId": "project-1", "slug": "runbook"}),
        ),
        ("wiki.issues", json!({"projectId": "project-1"})),
        (
            "context.wikiSearch",
            json!({"token": token, "query": "zebra"}),
        ),
        (
            "context.wikiRead",
            json!({"token": token, "slug": "runbook"}),
        ),
    ];
    let mut before = Vec::new();
    for (method, params) in &reads {
        let reply = host.call(method, params.clone()).await.unwrap();
        if reply.to_string().contains(WORD) {
            before.push(*method);
        }
    }
    for must in [
        "memory.list",
        "context.memoryRecall",
        "wiki.search",
        "wiki.page",
        "context.wikiRead",
    ] {
        assert!(
            before.contains(&must),
            "{must} should find it first: {before:?}"
        );
    }

    let forgot = host
        .call("memory.forget", json!({"id": experience.id}))
        .await
        .unwrap();
    assert_eq!(forgot["withdrawal"]["wikiPages"], 1, "{forgot}");
    assert!(forgot["withdrawal"].get("failed").is_none(), "{forgot}");
    for (method, params) in &reads {
        let reply = host.call(method, params.clone()).await.unwrap();
        assert!(
            !reply.to_string().contains(WORD),
            "{method} still reads it: {reply}"
        );
    }
}

/// C2 (2026-11-20): a Wiki that cannot be changed now is named in
/// `failed`, waits in the backlog (ids only) and is tried again when
/// 需要我处理 is read; once it goes through, its file and history are clean.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wiki_that_cannot_be_changed_is_tried_again() {
    const WORD: &str = "zebra-43";
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let experience = host
        .structured
        .memory()
        .add_experience(
            "project-1",
            "部署前清空 zebra-43 缓存目录",
            &aeroric_protocol::memory::ExperienceDetail {
                solution: "rm -rf zebra-43".into(),
                ..Default::default()
            },
            aeroric_protocol::memory::Confidence::High,
            aeroric_core::memory::evidence(Some("task-1"), None, None, "x", 1),
            1,
        )
        .unwrap();
    host.call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    // The page's Markdown file cannot be written: a folder stands there.
    let file = host
        .context
        .paths
        .data_dir()
        .join("knowledge/project-1/pages/runbook.md");
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();

    let forgot = host
        .call("memory.forget", json!({"id": experience.id}))
        .await
        .unwrap();
    let failed = forgot["withdrawal"]["failed"][0]
        .as_str()
        .unwrap_or_default();
    assert!(failed.contains("的 Wiki 没能更新"), "{forgot}");
    let waiting = host.call("memory.attention", json!({})).await.unwrap();
    assert_eq!(
        waiting["wikiBacklog"].as_array().unwrap().len(),
        1,
        "{waiting}"
    );
    assert!(
        !waiting.to_string().contains(WORD),
        "the backlog keeps ids, not words"
    );

    std::fs::remove_dir(&file).unwrap();
    let cleared = host.call("memory.attention", json!({})).await.unwrap();
    assert!(
        cleared["wikiBacklog"].as_array().unwrap().is_empty(),
        "{cleared}"
    );
    assert!(!std::fs::read_to_string(&file).unwrap().contains(WORD));
    for (method, params) in [
        (
            "wiki.page",
            json!({"projectId": "project-1", "slug": "runbook"}),
        ),
        (
            "wiki.revisions",
            json!({"projectId": "project-1", "slug": "runbook"}),
        ),
        (
            "wiki.search",
            json!({"projectId": "project-1", "query": "zebra"}),
        ),
    ] {
        let reply = host.call(method, params).await.unwrap();
        assert!(!reply.to_string().contains(WORD), "{method}: {reply}");
    }
}

/// B5.5-1 (2026-10-31): a project's notes compile into a Wiki the app reads
/// and edits and an agent reads, searches and flags.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_projects_notes_become_its_wiki() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let root = host._root.join("workspace");
    let notes = root.join(".agents/notes/implemented/feature");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("2026-10-31-wiki.md"),
        "# Agent Note: 项目 Wiki\n\nStatus: implemented\n\n## Problem\n\n知识散落。\n\n## Decision\n\n编译成页面。\n",
    )
    .unwrap();
    let report = host
        .call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(report["report"]["pages"], 2, "{report}");
    let page = host
        .call(
            "wiki.page",
            json!({"projectId": "project-1", "slug": "decision-wiki"}),
        )
        .await
        .unwrap();
    assert_eq!(page["page"]["status"], "implemented");
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    // R4-3: agents get the Wiki only once the user turns it on.
    let available = host
        .call("context.wikiAvailable", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(available["available"], false);
    let access = host
        .call(
            "wiki.agentAccess",
            json!({"projectId": "project-1", "enabled": true}),
        )
        .await
        .unwrap();
    assert_eq!(access["enabled"], true);
    let available = host
        .call("context.wikiAvailable", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(available["available"], true);
    let found = host
        .call(
            "context.wikiSearch",
            json!({"token": token, "query": "编译"}),
        )
        .await
        .unwrap();
    assert!(
        found["text"].as_str().unwrap().contains("decision-wiki"),
        "{found}"
    );
    let index = host
        .call("context.wikiRead", json!({"token": token}))
        .await
        .unwrap();
    assert!(index["text"].as_str().unwrap().starts_with("# 索引"));
    host.call(
        "context.wikiFlag",
        json!({"token": token, "slug": "decision-wiki", "text": "缺少后果一节"}),
    )
    .await
    .unwrap();
    let issues = host
        .call("wiki.issues", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(issues["issues"][0]["kind"], "agent");
    let bad = host
        .call(
            "wiki.exportPlan",
            json!({"projectId": "project-1", "dir": "../out"}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "INVALID_ARGUMENT");
}

/// CU 2026-10-31: 停止 on a native session mid-turn settles the task as
/// cancelled (closing it sends no turn end), and so does a cancel after the
/// session already went away.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_native_session_settles_the_task() {
    let mut host = Host::start(true).await;
    host.start_task("修复超时");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-c"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-c", "turn": {"id": "u1"}}),
    );
    host.until("running", |h| h.record()["status"] == "running")
        .await;
    host.service.cancel(&host.project, "task-1").unwrap();
    host.until("cancelled", |h| h.record()["status"] == "cancelled")
        .await;
}

/// R1-1: another session writing in the real directory refuses a writing
/// delegation from a different parent and is mentioned on a plain message.
#[tokio::test]
async fn a_second_session_in_the_directory_is_refused_or_told() {
    let mut host = Host::start(true).await;
    host.start_task("重构会话列表");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-1"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    host.until("the first turn is running", |h| {
        h.structured
            .registry()
            .get("task-1")
            .is_some_and(|d| d.snapshot().turn_active)
    })
    .await;

    host.service
        .create(
            &host.project,
            NewTask {
                id: "task-2".into(),
                project_id: "project-1".into(),
                prompt: "写文档".into(),
                agent: "codex".into(),
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
    host.service
        .start(
            &host.project,
            "task-2",
            "op-2",
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
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th-2"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u2"}}));

    let refused = host
        .call(
            "delegation.start",
            json!({"taskId": "task-2", "contract": {"instruction": "改",
                "workspace": "shared", "permission": "ask"}}),
        )
        .await
        .unwrap_err();
    assert!(
        refused["message"].as_str().unwrap().contains("正被"),
        "{refused}"
    );
    let reader = host
        .call(
            "delegation.start",
            json!({"taskId": "task-2", "contract": {"instruction": "查"}}),
        )
        .await;
    assert!(reader.is_ok(), "{reader:?}");
    let sent = host
        .call(
            "session.prompt",
            json!({"taskId": "task-2", "text": "接着写"}),
        )
        .await
        .unwrap();
    assert_eq!(sent["sharedWriters"][0]["taskId"], "task-1", "{sent}");
    assert_eq!(sent["accepted"], true);
}

/// R1-1: a project folder that went away keeps its pages and says so.
#[tokio::test]
async fn a_missing_project_folder_keeps_the_wiki_and_says_so() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let root = host._root.join("workspace");
    let notes = root.join(".agents/notes/implemented/feature");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("2026-10-31-wiki.md"),
        "# Agent Note: 项目 Wiki\n\nStatus: implemented\n\n## Problem\n\n知识散落。\n",
    )
    .unwrap();
    host.call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    let away = host._root.join("workspace-away");
    std::fs::rename(&root, &away).unwrap();
    let report = host
        .call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(report["report"]["removed"], 0, "{report}");
    let pages = host
        .call("wiki.pages", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(pages["sourcesUnavailable"]["all"], true, "{pages}");
    assert!(host
        .call(
            "wiki.page",
            json!({"projectId": "project-1", "slug": "decision-wiki"}),
        )
        .await
        .is_ok());
    std::fs::rename(&away, &root).unwrap();
    host.call("wiki.compile", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    let pages = host
        .call("wiki.pages", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(pages["sourcesUnavailable"]["all"], false, "{pages}");
}

/// Q-R6 (2026-11-13): a folder in the reference library is indexed in the
/// background and an agent searches and reads it through its context token;
/// a path that leaves the folder is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_reference_library_is_searched_and_read_by_an_agent() {
    let host = Host::start(true).await;
    let folder = std::env::temp_dir().join(format!("aeroric-ref-host-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(folder.join("nn")).unwrap();
    std::fs::write(
        folder.join("nn/head.py"),
        "class DetectHead:\n    def forward(self, x):\n        return x\n",
    )
    .unwrap();
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let none = host
        .call("context.referencesAvailable", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(none["available"], false);
    let added = host
        .call(
            "references.add",
            json!({"path": folder.to_str().unwrap(), "name": "yolo", "purpose": "检测头"}),
        )
        .await
        .unwrap();
    let id = added["reference"]["id"].as_str().unwrap().to_string();
    assert_eq!(added["reference"]["name"], "yolo");
    let mut files = 0;
    for _ in 0..100 {
        let list = host.call("references.list", json!({})).await.unwrap();
        let row = &list["references"][0];
        if row["indexing"] == false && row["files"].as_u64() == Some(1) {
            files = 1;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(files, 1, "indexed in the background");
    let available = host
        .call("context.referencesAvailable", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(available["available"], true);
    let found = host
        .call(
            "context.referenceSearch",
            json!({"token": token, "query": "DetectHead"}),
        )
        .await
        .unwrap();
    let text = found["text"].as_str().unwrap();
    assert!(text.contains("yolo · nn/head.py:1（定义 class）"), "{text}");
    let read = host
        .call(
            "context.referenceRead",
            json!({"token": token, "repo": "yolo", "path": "nn/head.py", "start": 2}),
        )
        .await
        .unwrap();
    assert!(read["text"]
        .as_str()
        .unwrap()
        .contains("2:     def forward"));
    let outside = host
        .call(
            "context.referenceRead",
            json!({"token": token, "repo": "yolo", "path": "../x"}),
        )
        .await
        .unwrap_err();
    assert_eq!(outside["code"], "INVALID_ARGUMENT");
    let stranger = host
        .call(
            "context.referenceSearch",
            json!({"token": "nope", "query": "DetectHead"}),
        )
        .await
        .unwrap_err();
    assert_eq!(stranger["code"], "UNAUTHENTICATED");
    host.call("references.remove", json!({"id": id}))
        .await
        .unwrap();
    let empty = host.call("references.list", json!({})).await.unwrap();
    assert!(empty["references"].as_array().unwrap().is_empty());
    assert!(
        folder.join("nn/head.py").exists(),
        "the folder is left alone"
    );
}

/// 2026-11-13 Smart Friend: the expert is set from the app's choices; until
/// then `ask_expert` is not offered and asking fails with where to set it.
#[tokio::test]
async fn the_expert_is_set_from_the_choices_before_an_agent_can_ask() {
    let host = Host::start(true).await;
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let none = host
        .call("context.expertAvailable", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(none["available"], false);
    let asked = host
        .call(
            "context.askExpert",
            json!({"token": token, "question": "为什么慢"}),
        )
        .await;
    assert!(
        format!("{asked:?}").contains("设置 → Agents → 求助专家"),
        "{asked:?}"
    );
    let shown = host.call("delegation.expert", json!({})).await.unwrap();
    assert!(shown["expert"].is_null());
    assert!(shown["choices"].is_array());
    let refused = host
        .call(
            "delegation.expert",
            json!({"set": {"configurationId": "agent-config:missing"}}),
        )
        .await;
    assert!(
        format!("{refused:?}").contains("不能用作专家"),
        "{refused:?}"
    );
    let bad = host
        .call("context.expertAvailable", json!({"token": "nope"}))
        .await;
    assert!(bad.is_err());
}

/// A4 (2026-11-17): a discussion's members only read, so a harness that
/// cannot hold itself read-only (omp: its `plan` is "ask every time") is
/// refused before anything is made; the capability table says the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_discussion_takes_only_members_that_hold_themselves_read_only() {
    let host = Host::start(true).await;
    let strict = |table: &Value| {
        table["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["feature"] == "strictReadOnly")
            .unwrap()["state"]
            .clone()
    };
    let omp = host
        .call("session.capabilities", json!({"family": "omp"}))
        .await
        .unwrap();
    assert_eq!(strict(&omp), "no", "{omp}");
    let codex = host
        .call("session.capabilities", json!({"family": "codex"}))
        .await
        .unwrap();
    assert_eq!(strict(&codex), "yes", "{codex}");

    let tasks = |host: &Host| host.service.load("project-1").unwrap().records.len();
    let before = tasks(&host);
    let refused = host
        .call(
            "discussion.start",
            json!({"projectId": "project-1", "topic": "Tabs or spaces?",
                   "members": [{"configurationId": "codex"}, {"configurationId": "omp"}]}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("不能保证只读") && message.contains("每次都问"),
        "{refused}"
    );
    assert_eq!(tasks(&host), before, "nothing made");
}

/// A6 (2026-11-17): a line of the transcript that is not JSON used to vanish
/// without a word; the first history page now says how many and where.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transcript_line_that_does_not_read_is_reported() {
    let mut host = Host::start(true).await;
    let rollout = host._root.join("th-1.jsonl");
    let good = |text: &str| {
        json!({"timestamp": "2026-01-01T00:00:00.000Z", "type": "response_item",
            "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": text}]}})
        .to_string()
    };
    std::fs::write(
        &rollout,
        format!(
            "{}\n{{\"type\": \"respon\n{}\n",
            good("first"),
            good("second")
        ),
    )
    .unwrap();
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
        json!({"thread": {"id": "th-1", "path": rollout.to_string_lossy()}}),
    );
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    host.until("done", |h| h.record()["status"] == "done").await;

    let history = host
        .call("session.history", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(
        history["unreadable"],
        json!({"count": 1, "firstLine": 2}),
        "{history}"
    );
    let texts: Vec<_> = history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["blocks"][0]["text"].as_str())
        .collect();
    assert_eq!(texts, ["first", "second"], "{history}");
    // Said once: an older page carries no repeat.
    let older = host
        .call(
            "session.history",
            json!({"taskId": "task-1", "cursor": "1"}),
        )
        .await
        .unwrap();
    assert!(older.get("unreadable").is_none(), "{older}");
}

/// B6 step 2: an agent reads deliverables through its context token — its
/// own task's, byte for byte as the app shows them; a task it has no tie
/// to is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_artifacts_read_the_tokens_task_and_refuse_strangers() {
    use aeroric_protocol::session_live::TurnFileChange;
    use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

    let host = Host::start(true).await;
    host.start_task("写报告");
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");

    let bad = host
        .call("context.artifactList", json!({"token": "made-up"}))
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "UNAUTHENTICATED");

    // A turn of task-1 wrote out/report.md in a repository.
    let repo = std::env::temp_dir().join(format!("aeroric-b6-host-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(repo.join("out")).unwrap();
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    let before = git(&["write-tree"]);
    let body = "# 报告\n第二行\n".repeat(50);
    std::fs::write(repo.join("out/report.md"), &body).unwrap();
    git(&["add", "-A"]);
    let after = git(&["write-tree"]);
    let record = TurnRecord {
        turn_id: "u1".into(),
        started_ms: 1,
        ended_ms: 2,
        top: repo.canonicalize().unwrap().to_string_lossy().into(),
        before,
        after,
        head_before: None,
        head_after: None,
        files: vec![TurnFileChange {
            path: "out/report.md".into(),
            additions: Some(100),
            deletions: Some(0),
        }],
        additions: 100,
        deletions: 0,
        state: TurnState::Pending,
        undone_ms: None,
        undo_group: None,
        state_before_undo: None,
        check: None,
    };
    host.structured.artifacts().register("task-1", &record);

    let list = host
        .call("context.artifactList", json!({"token": token}))
        .await
        .unwrap();
    assert_eq!(list["taskId"], "task-1");
    assert_eq!(list["readable"][0]["relation"], "当前任务");
    let artifact = &list["artifacts"][0];
    assert_eq!(artifact["path"], "out/report.md");
    let id = artifact["id"].as_str().unwrap().to_string();

    // Read in slices by `id@version`; together they are the file, and the
    // app's own method returns the same bytes.
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let read = host
            .call(
                "context.artifactRead",
                json!({"token": token, "artifact": format!("{id}@1"), "offset": offset, "limit": 100}),
            )
            .await
            .unwrap();
        let content = &read["content"];
        assert_eq!(content["encoding"], "utf8");
        text.push_str(content["content"].as_str().unwrap());
        match content["nextOffset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(text, body);
    let app = host
        .call(
            "artifacts.read",
            json!({"taskId": "task-1", "artifactId": id, "limit": 262144}),
        )
        .await
        .unwrap();
    assert_eq!(app["content"]["content"].as_str().unwrap(), body);

    let stranger = host
        .call(
            "context.artifactList",
            json!({"token": token, "taskId": "task-elsewhere"}),
        )
        .await
        .unwrap_err();
    assert_eq!(stranger["code"], "NOT_FOUND");
    assert!(stranger["message"].as_str().unwrap().contains("委派或侧支"));
    let unknown = host
        .call(
            "context.artifactRead",
            json!({"token": token, "artifact": "art-nope@1"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], "NOT_FOUND");
    std::fs::remove_dir_all(&repo).ok();
}

/// C4: the gateway's requests can be read at all — the totals say how many
/// had no reported usage, each request lists its tries.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gateway_requests_answer_with_totals_and_tries() {
    let host = Host::start(true).await;
    let read = host
        .call("gateway.requests", json!({"limit": 5}))
        .await
        .unwrap();
    let summary = &read["summary"];
    for field in [
        "totalRequests",
        "unknownUsageRequests",
        "cacheWrite5mTokens",
        "cacheWrite1hTokens",
    ] {
        assert!(summary[field].is_u64(), "{field}: {summary}");
    }
    let requests = read["requests"].as_array().expect("a list");
    assert!(requests.len() <= 5);
    for request in requests {
        assert!(request["attempts"].is_array(), "{request}");
        assert!(request["usageSource"].is_string(), "{request}");
    }
    let bad = host
        .call("gateway.requests", json!({"limit": 5, "extra": 1}))
        .await
        .unwrap_err();
    assert_eq!(bad["code"], "INVALID_ARGUMENT");
}

/// C3 (2026-11-20): a queued 整理 shows in `memory.runs` and
/// `memory.runCancel` cancels it at once; an ended or unknown run cannot be
/// canceled.
#[tokio::test(flavor = "multi_thread")]
async fn a_queued_run_is_listed_and_canceled() {
    let host = Host::start(true).await;
    let memory = host.structured.memory();
    let id = memory
        .queue_run("task-1", "project-1", "修复超时", 1)
        .unwrap();
    let runs = host.call("memory.runs", json!({})).await.unwrap();
    assert_eq!(runs["runs"][0]["id"], json!(id));
    assert_eq!(runs["runs"][0]["state"], json!("queued"));
    let canceled = host
        .call("memory.runCancel", json!({"runId": id}))
        .await
        .unwrap();
    assert_eq!(canceled["run"]["state"], json!("canceled"));
    let runs = host.call("memory.runs", json!({})).await.unwrap();
    assert_eq!(runs["runs"][0]["state"], json!("canceled"));
    assert!(host
        .call("memory.runCancel", json!({"runId": id}))
        .await
        .is_err());
    assert!(host
        .call("memory.runCancel", json!({"runId": "run-nope"}))
        .await
        .is_err());
}

/// C3 second step: 重建索引 is a row in 整理记录 and `memory.runs` says
/// whether the index is whole; a run with nothing to retry says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_index_is_rebuilt_as_a_run() {
    let host = Host::start(true).await;
    let runs = host.call("memory.runs", json!({})).await.unwrap();
    assert_eq!(runs["indexHealthy"], json!(true));
    let rebuilt = host.call("memory.reindex", json!({})).await.unwrap();
    assert_eq!(rebuilt["run"]["kind"], json!("reindex"));
    assert_eq!(rebuilt["run"]["state"], json!("done"));
    let runs = host.call("memory.runs", json!({})).await.unwrap();
    assert_eq!(runs["runs"][0]["id"], rebuilt["run"]["id"]);
    assert_eq!(runs["usedToday"], json!(0));
    assert_eq!(runs["monthCount"], json!(0));
    assert!(host
        .call("memory.runRetry", json!({"runId": rebuilt["run"]["id"]}))
        .await
        .is_err());
    assert!(host
        .call("memory.runRetry", json!({"runId": "run-nope"}))
        .await
        .is_err());
}

/// C7 (2026-11-20): `skills.catalog` lists the App's built-in skills as
/// living in the App; `skills.mounted` answers for any task (empty before
/// a launch is kept) and refuses a malformed request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_skill_catalog_and_mounts_answer() {
    let host = Host::start(true).await;
    let catalog = host.call("skills.catalog", json!({})).await.unwrap();
    let skills = catalog["skills"].as_array().unwrap();
    let builtin: Vec<_> = skills.iter().filter(|s| s["source"] == "builtin").collect();
    assert_eq!(
        builtin.len(),
        aeroric_core::extensions::builtin_skills::catalog().len()
    );
    assert!(builtin
        .iter()
        .all(|s| s["embedded"] == true && s["version"] == "随应用内置"));
    let scoped = host
        .call("skills.catalog", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert!(scoped["skills"].as_array().unwrap().len() >= builtin.len());
    let none = host
        .call("skills.mounted", json!({"taskId": "task-none"}))
        .await
        .unwrap();
    assert_eq!(none["launches"], json!([]));
    assert!(host.call("skills.mounted", json!({})).await.is_err());
}

/// C8 (2026-11-20): `session.capabilities` with a task carries the tools its
/// latest launch kept; without one kept, or without a task, no `tools`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capabilities_carry_a_launchs_tools() {
    use aeroric_core::sessions::live::tool_catalog::{catalog, Injection, Offered};
    let host = Host::start(true).await;
    let plain = host
        .call("session.capabilities", json!({"family": "codex"}))
        .await
        .unwrap();
    assert!(plain.get("tools").is_none());
    let none = host
        .call(
            "session.capabilities",
            json!({"family": "codex", "taskId": "task-none"}),
        )
        .await
        .unwrap();
    assert!(none.get("tools").is_none());
    host.structured.tool_log().note(
        "task-1",
        catalog("codex", Injection::Injected, Offered::default(), 5),
    );
    let kept = host
        .call(
            "session.capabilities",
            json!({"family": "codex", "taskId": "task-1"}),
        )
        .await
        .unwrap();
    assert_eq!(kept["tools"]["injected"], json!(true));
    assert_eq!(kept["tools"]["harness"], json!("codex"));
    let delegate = kept["tools"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "delegate")
        .unwrap();
    assert_eq!(delegate["owner"], json!("aeroric"));
    assert_eq!(delegate["scope"], json!("writes"));
    assert_eq!(delegate["ready"], json!(true));
}

/// C8 (2026-11-20): a context write with an idempotency key runs once per
/// task and key — the repeat gets the first result, marked `replayed` —
/// and the key is remembered on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_key_gets_the_first_result() {
    let host = Host::start(true).await;
    host.start_task("修复超时");
    let token = host
        .structured
        .context_tokens()
        .issue("task-1", "project-1");
    let keyed = |key: Option<&str>| {
        let mut request = json!({"v": 1, "id": "r-board", "method": "context.boardWrite",
            "params": {"token": token, "kind": "goal", "text": "修好超时"}});
        if let Some(key) = key {
            request["idempotencyKey"] = json!(key);
        }
        serde_json::from_value::<Request>(request).unwrap()
    };
    let call = |request: Request| {
        let context = Arc::clone(&host.call);
        async move {
            match dispatch(request, context).await {
                Frame::Success(success) => success.result,
                other => panic!("unexpected frame {other:?}"),
            }
        }
    };
    let first = call(keyed(Some("k-1"))).await;
    let again = call(keyed(Some("k-1"))).await;
    assert_eq!(first["entry"]["id"], again["entry"]["id"]);
    assert!(first.get("replayed").is_none());
    assert_eq!(again["replayed"], json!(true));
    let other = call(keyed(Some("k-2"))).await;
    assert_ne!(other["entry"]["id"], first["entry"]["id"]);
    let plain = call(keyed(None)).await;
    assert_ne!(plain["entry"]["id"], other["entry"]["id"]);
    assert_eq!(
        host.structured.boards().get("task-1").entries.len(),
        3,
        "three writes, not four"
    );
    let kept =
        std::fs::read_to_string(host.context.paths.data_dir().join("context-once.json")).unwrap();
    assert!(kept.contains("k-1") && kept.contains("k-2"));
}

/// H8: the automatic behaviours' tiers over RPC — recall listed at 候选,
/// moved up, 退回, and `recall.mode` reading the same tier; a bad tier and a
/// learned skill turned off here are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rollout_tiers_move_and_go_back_over_rpc() {
    let host = Host::start(true).await;
    let listed = host.call("rollout.list", json!({})).await.unwrap();
    let recall = &listed["features"][0];
    assert_eq!(recall["id"], "recall");
    assert_eq!(recall["tier"], "candidate");

    let up = host
        .call("rollout.set", json!({"id": "recall", "tier": "described"}))
        .await
        .unwrap();
    assert_eq!(up["feature"]["tier"], "described");
    assert_eq!(up["feature"]["history"][0]["tier"], "candidate");
    let mode = host.call("recall.mode", json!({})).await.unwrap();
    assert_eq!(mode["mode"], "described");

    let back = host
        .call("rollout.revert", json!({"id": "recall"}))
        .await
        .unwrap();
    assert_eq!(back["feature"]["tier"], "candidate");
    host.call("recall.mode", json!({"mode": "on"}))
        .await
        .unwrap();
    let listed = host.call("rollout.list", json!({})).await.unwrap();
    assert_eq!(listed["features"][0]["tier"], "default");

    let bad = host
        .call("rollout.set", json!({"id": "recall", "tier": "loud"}))
        .await
        .unwrap_err();
    assert!(bad["message"].as_str().unwrap().contains("tier"), "{bad}");
    let skill_off = host
        .call("rollout.set", json!({"id": "skill:x", "tier": "off"}))
        .await
        .unwrap_err();
    assert!(
        skill_off["message"].as_str().unwrap().contains("停用"),
        "{skill_off}"
    );
    let no_id = host.call("rollout.revert", json!({})).await.unwrap_err();
    assert!(no_id["message"].as_str().unwrap().contains("id"), "{no_id}");
}

/// D1: a PR link that is not one is refused before anything is fetched;
/// a project with no snapshots lists none; an unknown snapshot is not found.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn github_snapshots_check_their_arguments() {
    let host = Host::start(true).await;
    let bad = host
        .call(
            "github.pullSnapshot",
            json!({"projectId": "project-1", "url": "https://github.com/acme/widget/issues/7"}),
        )
        .await
        .unwrap_err();
    assert!(bad["message"].as_str().unwrap().contains("看不懂"), "{bad}");
    let no_project = host
        .call("github.pullSnapshot", json!({"url": "acme/widget#7"}))
        .await
        .unwrap_err();
    assert!(no_project["message"]
        .as_str()
        .unwrap()
        .contains("projectId"));
    let listed = host
        .call("github.snapshots", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(listed["snapshots"], json!([]));
    let missing = host
        .call(
            "github.snapshot",
            json!({"projectId": "project-1", "id": "42-7-aaaa"}),
        )
        .await
        .unwrap_err();
    assert!(missing["message"]
        .as_str()
        .unwrap()
        .contains("没有这份快照"));
}

/// D1 second step: reviewing a pinned PR fetches its head (here from a
/// local repository standing in for GitHub through `insteadOf`), checks it
/// out detached and starts a read-only Claude session there; the review is
/// kept running. A missing snapshot and a project that is not a repository
/// are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_pr_is_reviewed_read_only_in_its_own_checkout() {
    use aeroric_core::github::{snapshot_id, store};
    use aeroric_protocol::github::{PullFile, PullSnapshot, RepoRef};
    let mut host = Host::start(true).await;
    let git = |cwd: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let project = ProjectService::new(Arc::clone(&host.context))
        .list()
        .unwrap()
        .into_iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some("project-1"))
        .unwrap();
    let work = PathBuf::from(project.get("path").and_then(Value::as_str).unwrap());
    let data_dir = host.context.paths.data_dir().to_path_buf();

    let missing = host
        .call(
            "github.reviewStart",
            json!({"projectId": "project-1", "snapshotId": "nope"}),
        )
        .await
        .unwrap_err();
    assert!(missing["message"]
        .as_str()
        .unwrap()
        .contains("没有这份快照"));

    // GitHub's side: a repository whose PR #7 is one commit on main.
    let remote = work.parent().unwrap().join("gh-widget");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q"]);
    std::fs::write(remote.join("a.rs"), "fn a() {}\n").unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-qm", "base"]);
    let base = git(&remote, &["rev-parse", "HEAD"]);
    std::fs::write(remote.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    git(&remote, &["commit", "-qam", "pr"]);
    let head = git(&remote, &["rev-parse", "HEAD"]);
    git(&remote, &["update-ref", "refs/pull/7/head", &head]);
    git(&remote, &["reset", "-q", "--hard", &base]);

    let snapshot = PullSnapshot {
        id: snapshot_id(42, 7, &head),
        project_id: "project-1".into(),
        repo: RepoRef {
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widget".into(),
            repo_id: 42,
        },
        number: 7,
        title: "加 b".into(),
        head_ref: "feature".into(),
        base_ref: "main".into(),
        head_sha: head.clone(),
        base_sha: base.clone(),
        fetched_ms: 1,
        files: vec![PullFile {
            path: "a.rs".into(),
            status: "modified".into(),
            additions: 1,
            ..PullFile::default()
        }],
        ..PullSnapshot::default()
    };
    store::save(&data_dir, &snapshot).unwrap();
    let not_repo = host
        .call(
            "github.reviewStart",
            json!({"projectId": "project-1", "snapshotId": snapshot.id}),
        )
        .await
        .unwrap_err();
    assert!(
        not_repo["message"].as_str().unwrap().contains("git"),
        "{not_repo}"
    );

    // The user's clone; GitHub's URL leads to the local repository.
    git(&work, &["init", "-q"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "mine"]);
    let url = format!("url.{}.insteadOf", remote.display());
    git(
        &work,
        &["config", &url, "https://github.com/acme/widget.git"],
    );
    let started = host
        .call(
            "github.reviewStart",
            json!({"projectId": "project-1", "snapshotId": snapshot.id}),
        )
        .await
        .unwrap();
    let review = &started["review"];
    assert_eq!(review["state"], "running");
    assert_eq!(review["headSha"], head);
    let (start, _peer) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .expect("a reviewer started")
        .expect("launcher");
    assert!(
        start
            .cwd
            .ends_with(&format!(".aeroric/pr-reviews/{}", snapshot.id)),
        "{}",
        start.cwd
    );
    assert_eq!(start.permission, "plan");
    assert_eq!(git(Path::new(&start.cwd), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&work, &["log", "-1", "--format=%s"]),
        "mine",
        "the user's HEAD is theirs"
    );
    let listed = host
        .call("github.reviews", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(listed["reviews"][0]["id"], review["id"]);
    assert_eq!(listed["reviews"][0]["taskId"], review["taskId"]);
}

/// D2 second step: a finished review's publishing draft over RPC — one per
/// review, edited, nothing ticked refused before anything is sent, a draft
/// left sending reads as unknown; a review without findings yet has none.
/// None of it is among the tools agents get.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reviews_publishing_draft_is_the_users_alone() {
    use aeroric_core::github::{publish, review, snapshot_id, store};
    use aeroric_protocol::github::{
        PullFinding, PullReview, PullReviewState, PullSnapshot, RepoRef, WriteIntentState,
        WriteItemState,
    };
    let host = Host::start(true).await;
    let data_dir = host.context.paths.data_dir().to_path_buf();
    let head = "a".repeat(40);
    let snapshot = PullSnapshot {
        id: snapshot_id(42, 7, &head),
        project_id: "project-1".into(),
        repo: RepoRef {
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widget".into(),
            repo_id: 42,
        },
        number: 7,
        head_sha: head.clone(),
        fetched_ms: 1,
        ..PullSnapshot::default()
    };
    store::save(&data_dir, &snapshot).unwrap();
    let mut done = PullReview {
        id: "prrev-1".into(),
        project_id: "project-1".into(),
        snapshot_id: snapshot.id.clone(),
        head_sha: head.clone(),
        task_id: "task-x".into(),
        state: PullReviewState::Running,
        findings: vec![PullFinding {
            path: "a.rs".into(),
            line: Some(2),
            severity: "high".into(),
            body: "b 没有测试".into(),
            basis: String::new(),
        }],
        created_ms: 2,
        ..PullReview::default()
    };
    review::save(&data_dir, &done).unwrap();
    let early = host
        .call(
            "github.writeDraft",
            json!({"projectId": "project-1", "reviewId": "prrev-1"}),
        )
        .await
        .unwrap_err();
    assert!(
        early["message"].as_str().unwrap().contains("还没有结论"),
        "{early}"
    );

    done.state = PullReviewState::Done;
    review::save(&data_dir, &done).unwrap();
    let made = host
        .call(
            "github.writeDraft",
            json!({"projectId": "project-1", "reviewId": "prrev-1"}),
        )
        .await
        .unwrap();
    let id = made["write"]["id"].as_str().unwrap().to_string();
    assert_eq!(made["write"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(made["write"]["state"], "draft");
    let again = host
        .call(
            "github.writeDraft",
            json!({"projectId": "project-1", "reviewId": "prrev-1"}),
        )
        .await
        .unwrap();
    assert_eq!(again["write"]["id"], id.as_str(), "one draft per review");

    for item in ["summary", "c1"] {
        host.call(
            "github.writeEdit",
            json!({"projectId": "project-1", "id": id, "itemId": item, "included": false}),
        )
        .await
        .unwrap();
    }
    let empty = host
        .call(
            "github.writeEdit",
            json!({"projectId": "project-1", "id": id, "itemId": "c1", "body": "  "}),
        )
        .await
        .unwrap_err();
    assert!(empty["message"].as_str().unwrap().contains("不能为空"));
    let nothing = host
        .call(
            "github.publish",
            json!({"projectId": "project-1", "id": id}),
        )
        .await
        .unwrap_err();
    assert!(
        nothing["message"].as_str().unwrap().contains("没有勾选"),
        "{nothing}"
    );

    // A host that stopped mid-send left this on disk.
    let mut cut = publish::load(&data_dir, "project-1", &id).unwrap();
    cut.state = WriteIntentState::Sending;
    cut.items[1].state = WriteItemState::Sending;
    cut.items[1].included = true;
    publish::save(&data_dir, &cut).unwrap();
    let listed = host
        .call("github.writes", json!({"projectId": "project-1"}))
        .await
        .unwrap();
    assert_eq!(listed["writes"][0]["state"], "unknown");
    assert_eq!(listed["writes"][0]["items"][1]["state"], "unknown");

    let tools: Vec<&str> = aeroric_protocol::tools::AERORIC_TOOLS
        .iter()
        .map(|(n, _, _)| *n)
        .collect();
    assert!(
        tools
            .iter()
            .all(|t| !t.contains("publish") && !t.contains("github")),
        "{tools:?}"
    );
}
