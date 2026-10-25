use super::*;
use crate::sessions::live::codex::fake::{launched, launcher, until, Peer, Sink};
use crate::tasks::{PermissionMode, TaskStatus};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Default)]
struct Inner {
    calls: Mutex<Vec<String>>,
}

impl TaskExecutor for Inner {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        self.calls
            .lock()
            .push(format!("launch {}", request.task_id));
        Ok(())
    }
    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        self.calls
            .lock()
            .push(format!("resume {}", request.task_id));
        Ok(())
    }
    fn cancel(&self, task_id: &str, _target: &CancelTarget) -> Execution<()> {
        self.calls.lock().push(format!("cancel {task_id}"));
        Ok(())
    }
    fn complete(&self, task_id: &str, _working_directory: &str) -> Execution<()> {
        self.calls.lock().push(format!("complete {task_id}"));
        Ok(())
    }
    fn reset(&self, task_id: &str) -> Execution<ResetOutcome> {
        self.calls.lock().push(format!("reset {task_id}"));
        Ok(ResetOutcome::default())
    }
}

struct FakeRouting(Launcher);

impl Routing for FakeRouting {
    fn structured(
        &self,
        family: ProtocolFamily,
        _agent: &str,
        agent_config_id: Option<&str>,
    ) -> bool {
        family == ProtocolFamily::Codex && agent_config_id.is_none()
    }
    fn codex_launcher(&self, _agent: &str) -> Launcher {
        Arc::clone(&self.0)
    }
}

struct Setup {
    executor: Arc<StructuredExecutor>,
    inner: Arc<Inner>,
    sink: Arc<Sink>,
    peers: mpsc::UnboundedReceiver<Peer>,
}

fn setup() -> Setup {
    let (launcher, peers) = launcher();
    let inner = Arc::new(Inner::default());
    let sink = Arc::new(Sink::default());
    let executor = StructuredExecutor::new(
        Arc::clone(&inner) as Arc<dyn TaskExecutor>,
        Arc::new(FakeRouting(launcher)),
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    Setup {
        executor,
        inner,
        sink,
        peers,
    }
}

fn launch(task_id: &str, family: ProtocolFamily) -> LaunchRequest {
    LaunchRequest {
        task_id: task_id.into(),
        project_id: "p1".into(),
        operation_key: "op".into(),
        generation: 1,
        family,
        agent: "codex".into(),
        permission_mode: PermissionMode::Ask,
        permission_mode_wire: "ask".into(),
        prompt: "hi".into(),
        prompt_override: None,
        images: vec![],
        texts: vec![],
        inject_prompt_into_terminal: false,
        session_id: None,
        selected_model: Some(String::new()),
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: None,
        created_at_ms: 0,
        dsh_window: None,
        working_directory: WorkingDirectory::Local { path: "/w".into() },
        terminal_size: None,
    }
}

/// Drive a structured launch until its turn is running.
async fn running(setup: &mut Setup) -> Peer {
    let mut peer = launched(&mut setup.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    assert_eq!(
        start["params"],
        json!({"cwd": "/w", "approvalPolicy": "on-request", "sandbox": "read-only"})
    );
    peer.reply(&start, json!({"thread": {"id": "th1"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th1", "turn": {"id": "u1"}}),
    );
    let sink = Arc::clone(&setup.sink);
    until("running", move || {
        sink.last_status() == Some(TaskStatus::Running)
    })
    .await;
    peer
}

#[tokio::test]
async fn codex_in_a_local_workspace_goes_to_the_driver_and_the_rest_to_the_pty_executor() {
    let mut s = setup();
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    running(&mut s).await;
    assert!(s.executor.registry().get("t1").is_some());

    s.executor
        .launch(&launch("t2", ProtocolFamily::Omp))
        .unwrap();
    s.executor
        .launch(&LaunchRequest {
            agent_config_id: Some("cfg".into()),
            ..launch("t3", ProtocolFamily::Codex)
        })
        .unwrap();
    s.executor
        .launch(&LaunchRequest {
            working_directory: WorkingDirectory::Ssh {
                connection_id: "c".into(),
                remote_path: "/r".into(),
            },
            ..launch("t4", ProtocolFamily::Codex)
        })
        .unwrap();
    assert_eq!(
        *s.inner.calls.lock(),
        vec!["launch t2", "launch t3", "launch t4"]
    );
    assert!(s.executor.registry().get("t2").is_none());
}

#[tokio::test]
async fn cancelling_a_structured_run_closes_its_session_and_others_are_delegated() {
    let mut s = setup();
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = running(&mut s).await;
    s.executor
        .cancel(
            "t1",
            &CancelTarget::Local {
                project_path: "/w".into(),
            },
        )
        .unwrap();
    let interrupt = peer.expect("turn/interrupt").await;
    peer.reply(&interrupt, json!({}));
    peer.expect("thread/unsubscribe").await;
    assert!(s.executor.registry().get("t1").is_none());
    assert!(
        s.executor.registry().log("t1").is_some(),
        "the log stays for replay"
    );

    s.executor
        .cancel(
            "t9",
            &CancelTarget::Local {
                project_path: "/w".into(),
            },
        )
        .unwrap();
    assert_eq!(*s.inner.calls.lock(), vec!["cancel t9"]);
}

#[tokio::test]
async fn the_sweep_suspends_sessions_idle_past_the_limit() {
    let mut s = setup();
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = running(&mut s).await;
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": "completed"}}),
    );
    let sink = Arc::clone(&s.sink);
    until("done", move || sink.last_status() == Some(TaskStatus::Done)).await;

    s.executor.sweep(now_ms()).await;
    peer.silent().await;
    s.executor.sweep(now_ms() + SESSION_IDLE_MS).await;
    peer.expect("thread/unsubscribe").await;
}

#[tokio::test]
async fn resuming_an_open_session_rebinds_it_instead_of_starting_another() {
    let mut s = setup();
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = running(&mut s).await;
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": "completed"}}),
    );
    let resume = ResumeRequest {
        task_id: "t1".into(),
        project_id: "p1".into(),
        operation_key: "op2".into(),
        generation: 2,
        family: ProtocolFamily::Codex,
        agent: "codex".into(),
        session_id: "th1".into(),
        permission_mode: PermissionMode::Ask,
        permission_mode_wire: "ask".into(),
        selected_model: None,
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: None,
        prompt: None,
        working_directory: WorkingDirectory::Local { path: "/w".into() },
        terminal_size: None,
    };
    s.executor.resume(&resume).unwrap();
    let sink = Arc::clone(&s.sink);
    until("done under generation 2", move || {
        sink.statuses
            .lock()
            .last()
            .is_some_and(|(status, _, generation)| *status == TaskStatus::Done && *generation == 2)
    })
    .await;
    peer.silent().await;
    assert!(s.peers.try_recv().is_err());
}

#[tokio::test]
async fn a_closed_session_s_log_is_kept_for_the_retention_then_dropped() {
    let mut s = setup();
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = running(&mut s).await;
    s.executor
        .cancel(
            "t1",
            &CancelTarget::Local {
                project_path: "/w".into(),
            },
        )
        .unwrap();
    let interrupt = peer.expect("turn/interrupt").await;
    peer.reply(&interrupt, json!({}));
    s.executor.sweep(now_ms()).await;
    assert!(
        s.executor.registry().log("t1").is_some(),
        "kept for clients catching up"
    );
    s.executor
        .sweep(now_ms() + super::super::CLOSED_LOG_RETENTION_MS)
        .await;
    assert!(s.executor.registry().log("t1").is_none());
}
