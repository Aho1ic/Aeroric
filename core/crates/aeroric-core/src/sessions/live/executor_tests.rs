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

/// Codex goes structured without a configuration (built-in server) and with a
/// `native-` configuration (one server per configuration); anything else is PTY.
struct FakeRouting(Launcher, Mutex<Vec<ServerKey>>);

impl Routing for FakeRouting {
    fn route(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> Option<Route> {
        match (family, agent_config_id) {
            (ProtocolFamily::Codex, None) => {
                Some(Route::Codex(ServerKey::Agent(agent.to_string())))
            }
            (ProtocolFamily::Codex, Some(id)) if id.starts_with("native-") => {
                Some(Route::Codex(ServerKey::Config(id.to_string())))
            }
            (ProtocolFamily::Claude, config) => Some(Route::Claude(ClaudeKey {
                agent: agent.to_string(),
                config: config.map(str::to_string),
            })),
            _ => None,
        }
    }
    fn codex_launcher(&self, key: &ServerKey) -> Launcher {
        self.1.lock().push(key.clone());
        Arc::clone(&self.0)
    }
    fn claude_launcher(&self, _key: &ClaudeKey) -> ClaudeLauncher {
        Arc::new(|_: &crate::sessions::live::claude::driver::ClaudeStart| {
            Err("no claude in this test".to_string())
        })
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
        Arc::new(FakeRouting(launcher, Mutex::default())),
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

    // Claude goes to its own driver (N5), never to the PTY executor.
    s.executor
        .launch(&launch("t5", ProtocolFamily::Claude))
        .unwrap();
    assert_eq!(
        s.executor.registry().get("t5").map(|d| d.family()),
        Some(ProtocolFamily::Claude)
    );
    let sink = Arc::clone(&s.sink);
    until("the claude launch failure", move || {
        sink.statuses.lock().iter().any(|(status, reason, _)| {
            *status == TaskStatus::Failed && reason.as_deref() == Some("no claude in this test")
        })
    })
    .await;
    assert_eq!(s.inner.calls.lock().len(), 3);
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
        session_path: None,
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

#[tokio::test]
async fn each_configuration_gets_its_own_server_and_shares_it_across_tasks() {
    let (launcher, _peers) = launcher();
    let routing = Arc::new(FakeRouting(launcher, Mutex::default()));
    let executor = StructuredExecutor::new(
        Arc::new(Inner::default()) as Arc<dyn TaskExecutor>,
        Arc::clone(&routing) as Arc<dyn Routing>,
        Arc::new(Sink::default()) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    let with = |task: &str, config: Option<&str>| LaunchRequest {
        agent_config_id: config.map(str::to_string),
        prompt: String::new(),
        ..launch(task, ProtocolFamily::Codex)
    };
    executor.launch(&with("t1", None)).unwrap();
    executor.launch(&with("t2", Some("native-a"))).unwrap();
    executor.launch(&with("t3", Some("native-a"))).unwrap();
    executor.launch(&with("t4", Some("native-b"))).unwrap();
    assert_eq!(
        *routing.1.lock(),
        vec![
            ServerKey::Agent("codex".into()),
            ServerKey::Config("native-a".into()),
            ServerKey::Config("native-b".into()),
        ]
    );
    assert!(executor.registry().get("t3").is_some());
}

#[tokio::test]
async fn a_paginated_codex_session_resumes_on_the_driver() {
    // Codex 0.163's app-server writes and resumes paginated threads (N6a).
    let dir = std::env::temp_dir().join(format!("aeroric-paginated-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rollout = dir.join("rollout.jsonl");
    std::fs::write(
        &rollout,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"th9\",\"history_mode\":\"paginated\"}}\n",
    )
    .unwrap();
    let s = setup();
    let request = ResumeRequest {
        task_id: "t9".into(),
        project_id: "p1".into(),
        operation_key: "op".into(),
        generation: 2,
        family: ProtocolFamily::Codex,
        agent: "codex".into(),
        session_id: "th9".into(),
        session_path: Some(rollout.to_string_lossy().into_owned()),
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
    s.executor.resume(&request).expect("resumed");
    assert!(s.executor.registry().get("t9").is_some());
    assert!(s.inner.calls.lock().is_empty(), "not handed to the PTY");
    std::fs::remove_dir_all(dir).ok();
}

/// A05: a turn run in a git workspace gets a `turnChanges` update after it ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_in_a_git_workspace_reports_what_it_changed() {
    let repo = std::env::temp_dir().join(format!(
        "aeroric-exec-turn-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("calc.py"), "a\nb\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);

    let mut s = setup();
    let mut request = launch("tg", ProtocolFamily::Codex);
    request.working_directory = WorkingDirectory::Local {
        path: repo.to_string_lossy().into_owned(),
    };
    s.executor.launch(&request).unwrap();
    let mut peer = launched(&mut s.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    peer.reply(&start, json!({"thread": {"id": "th1"}}));
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th1", "turn": {"id": "u1"}}),
    );
    // Let the start snapshot land before the "agent" edits.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    std::fs::write(repo.join("calc.py"), "a\nB\nc\n").unwrap();
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": "completed"}}),
    );
    let sink = Arc::clone(&s.sink);
    until("turn changes", move || {
        sink.update_types().iter().any(|t| t == "turnChanges")
    })
    .await;
    let changes = s
        .sink
        .updates
        .lock()
        .iter()
        .find_map(|u| match &u.kind {
            crate::sessions::live::update::SessionUpdateKind::TurnChanges {
                turn_id,
                files,
                additions,
                deletions,
                ..
            } => Some((turn_id.clone(), files.clone(), *additions, *deletions)),
            _ => None,
        })
        .unwrap();
    assert_eq!(changes.0, "u1");
    assert_eq!(changes.1.len(), 1);
    assert_eq!(changes.1[0].path, "calc.py");
    assert_eq!((changes.2, changes.3), (2, 1));
    std::fs::remove_dir_all(&repo).ok();
}
