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
use aeroric_core::sessions::live::codex::rpc::Transport;
use aeroric_core::sessions::live::codex::server::Launcher;
use aeroric_core::sessions::live::executor::{Routing, StructuredExecutor};
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

struct CodexFamilies;

impl AgentFamilies for CodexFamilies {
    fn launch_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
    }
    fn session_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
    }
    fn is_builtin(&self, _agent: &str) -> bool {
        false
    }
    fn native_resume_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
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
}

impl Routing for TestRouting {
    fn structured(&self, family: ProtocolFamily, _agent: &str, config: Option<&str>) -> bool {
        family == ProtocolFamily::Codex && config.is_none()
    }
    fn codex_launcher(&self, _agent: &str) -> Launcher {
        Arc::clone(&self.launcher)
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Host {
    call: Arc<CallContext>,
    service: Arc<TaskService>,
    capabilities: Vec<String>,
    peers: mpsc::UnboundedReceiver<Peer>,
    project: aeroric_core::storage::Record,
    _root: PathBuf,
}

impl Host {
    async fn start(with_live: bool) -> Host {
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
        let (peers_tx, peers) = mpsc::unbounded_channel();
        let launcher: Launcher = Arc::new(move || {
            let (transport, from_client, to_client) = Transport::pair();
            let _ = peers_tx.send(Peer {
                from_client,
                to_client,
            });
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
            Arc::new(TestRouting { launcher }),
            Arc::clone(&sink) as Arc<_>,
            tokio::runtime::Handle::current(),
        );
        let families: Arc<dyn AgentFamilies> = Arc::new(CodexFamilies);
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
            families,
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

        let project_dir = root.join("workspace");
        std::fs::create_dir_all(&project_dir).expect("workspace");
        let (upserted, _) = ProjectService::new(Arc::clone(&context))
            .open_local(
                &project_dir.to_string_lossy(),
                "project-1",
                context.clock.now_ms() as i64,
            )
            .expect("open project");
        Host {
            call,
            service,
            capabilities,
            peers,
            project: upserted.project,
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
        self.service
            .create(
                &self.project,
                NewTask {
                    id: "task-1".into(),
                    project_id: "project-1".into(),
                    prompt: prompt.into(),
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
