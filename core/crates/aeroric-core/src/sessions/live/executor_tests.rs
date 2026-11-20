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
        linked_folders: Vec::new(),
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
    // H3: every start carries the opening as developerInstructions.
    let mut params = start["params"].clone();
    let opening = params
        .as_object_mut()
        .and_then(|p| p.remove("developerInstructions"))
        .unwrap_or_default();
    assert!(
        opening
            .as_str()
            .is_some_and(|o| o.starts_with(crate::sessions::live::opening::PURPOSE_HEADER)),
        "{start}"
    );
    assert_eq!(
        params,
        json!({"cwd": "/w", "approvalPolicy": "untrusted", "sandbox": "workspace-write"})
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
            linked_folders: Vec::new(),
            agent_config_id: Some("cfg".into()),
            ..launch("t3", ProtocolFamily::Codex)
        })
        .unwrap();
    s.executor
        .launch(&LaunchRequest {
            linked_folders: Vec::new(),
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
    let queued = s
        .executor
        .deliveries()
        .enqueue(
            "t1",
            aeroric_protocol::delivery::DeliveryKind::Delegation,
            aeroric_protocol::delivery::DeliveryMode::AfterTurn,
            "委派",
            "结果",
            None,
        )
        .unwrap();
    s.executor
        .cancel(
            "t1",
            &CancelTarget::Local {
                project_path: "/w".into(),
            },
        )
        .unwrap();
    assert_eq!(
        s.executor.deliveries().get(&queued.id).unwrap().state,
        aeroric_protocol::delivery::DeliveryState::Paused,
        "the toolbar's stop pauses what has not gone out"
    );
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

struct Never;

impl IdleReleasePolicy for Never {
    fn enabled(&self) -> bool {
        false
    }
}

/// 2026-11-07: an hour, and never when the user turned release off.
#[tokio::test]
async fn the_sweep_keeps_idle_sessions_when_release_is_off() {
    assert_eq!(SESSION_IDLE_MS, 60 * 60_000);
    let mut s = setup();
    s.executor.set_idle_release_policy(Arc::new(Never));
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
    s.executor.sweep(now_ms() + 10 * SESSION_IDLE_MS).await;
    peer.silent().await;
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
        linked_folders: Vec::new(),
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
        linked_folders: Vec::new(),
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
        linked_folders: Vec::new(),
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

struct TestsOnly;

impl crate::sessions::live::auto_check::AutoCheckPolicy for TestsOnly {
    fn selection(
        &self,
        _project_id: &str,
    ) -> crate::sessions::live::auto_check::AutoCheckSelection {
        crate::sessions::live::auto_check::AutoCheckSelection {
            tests: true,
            diagnostics: false,
        }
    }
}

/// A05: a project that opted in gets its own tests run after a turn that
/// changed files — `running` first, then the verdict.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_changed_files_runs_the_projects_tests() {
    let repo = std::env::temp_dir().join(format!(
        "aeroric-exec-check-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(repo.join("src")).unwrap();
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
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"calc\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(repo.join(".gitignore"), "target\n").unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);

    let mut s = setup();
    s.executor.set_auto_check_policy(Arc::new(TestsOnly));
    let mut request = launch("tc", ProtocolFamily::Codex);
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
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[test]\nfn adds() { assert_eq!(add(1, 2), 3); }\n",
    )
    .unwrap();
    peer.notify(
        "turn/completed",
        json!({"threadId": "th1", "turn": {"id": "u1", "status": "completed"}}),
    );
    let checks = || {
        s.sink
            .updates
            .lock()
            .iter()
            .filter_map(|u| match &u.kind {
                crate::sessions::live::update::SessionUpdateKind::AutoCheck {
                    turn_id,
                    state,
                    items,
                    ..
                } => Some((turn_id.clone(), state.clone(), items.clone())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while !checks().iter().any(|c| c.1 != "running") {
        assert!(
            std::time::Instant::now() < deadline,
            "no verdict: {:?}",
            checks()
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let all = checks();
    assert_eq!(all[0].1, "running");
    let (turn_id, state, items) = all.last().unwrap().clone();
    assert_eq!(turn_id, "u1");
    assert_eq!(state, "passed", "{items:?}");
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].kind.as_str(), items[0].profile.as_str()),
        ("tests", "cargo")
    );
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn only_local_routes_get_the_local_context_server() {
    let remote = RemoteKey {
        connection_id: "ssh-1".into(),
        agent: "codex".into(),
        config: None,
    };
    assert!(Route::Codex(ServerKey::Agent("codex".into())).runs_locally());
    assert!(Route::Codex(ServerKey::Config("cfg".into())).runs_locally());
    assert!(!Route::Codex(ServerKey::Remote(remote.clone())).runs_locally());
    assert!(!Route::RemoteClaude(remote.clone()).runs_locally());
    assert!(!Route::RemoteOmp(remote).runs_locally());
}

/// C8 (2026-11-20): the tool list kept for a launch matches what the
/// session was really started with — the context server in Codex's
/// `thread/start` exactly when the list says injected; a launch with no
/// server set up says so.
#[tokio::test]
async fn the_kept_tool_list_matches_what_the_launch_was_given() {
    let mut s = setup();
    let dir = std::env::temp_dir().join(format!("aeroric-exec-tools-{}", uuid::Uuid::new_v4()));
    s.executor.tool_log().set_dir(dir);
    // No server set up: the list says so, and thread/start carries none.
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = launched(&mut s.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    let given = start["params"]["config"]["mcp_servers"]["aeroric_context"].is_object();
    let kept = s.executor.tool_log().latest("t1").unwrap();
    assert_eq!(kept.injected, given);
    assert!(!kept.injected);
    assert_eq!(kept.harness, "codex");

    // With the server: both say so.
    s.executor
        .set_context_program("aeroric-cli".into(), "/d".into());
    s.executor
        .launch(&LaunchRequest {
            agent_config_id: Some("native-a".into()),
            ..launch("t2", ProtocolFamily::Codex)
        })
        .unwrap();
    let mut peer = launched(&mut s.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    let given = start["params"]["config"]["mcp_servers"]["aeroric_context"].is_object();
    let kept = s.executor.tool_log().latest("t2").unwrap();
    assert!(given, "{start}");
    assert_eq!(kept.injected, given);
    assert!(kept.tools.iter().any(|t| t.name == "delegate" && t.ready));
    assert!(
        !kept
            .tools
            .iter()
            .any(|t| t.name == "memory_recall" && t.ready),
        "memory is off"
    );

    // OMP gets no server: a remote one and an OMP one each say why.
    let remote = RemoteKey {
        connection_id: "ssh-1".into(),
        agent: "claude".into(),
        config: None,
    };
    assert!(Route::RemoteClaude(remote.clone()).is_remote());
    assert!(!Route::Omp(OmpKey {
        agent: "omp".into(),
        config: None
    })
    .is_remote());
    assert_eq!(
        Route::Omp(OmpKey {
            agent: "omp".into(),
            config: None
        })
        .harness(),
        "omp"
    );
    assert_eq!(Route::RemoteClaude(remote).harness(), "claude");
}

/// C9 (2026-11-20): the capability table's "Aeroric 工具" agrees with what
/// the executor does — a family's local route gets the context server
/// exactly when the table says Yes; every remote route gets none and the
/// table says No. So no entry claims tools a harness cannot reach.
#[test]
fn the_capability_table_matches_who_gets_the_context_server() {
    use crate::sessions::live::capabilities::support;
    use aeroric_protocol::live_capability::{Feature, Place, SupportState};
    let remote = RemoteKey {
        connection_id: "ssh-1".into(),
        agent: "x".into(),
        config: None,
    };
    for family in ProtocolFamily::ALL {
        let local = match family {
            ProtocolFamily::Codex => Route::Codex(ServerKey::Agent("codex".into())),
            ProtocolFamily::Claude => Route::Claude(ClaudeKey {
                agent: "claude".into(),
                config: None,
            }),
            ProtocolFamily::Omp => Route::Omp(OmpKey {
                agent: "omp".into(),
                config: None,
            }),
            ProtocolFamily::Dsh => Route::Acp(AcpKey {
                family: ProtocolFamily::Dsh,
                agent: "dsh".into(),
                config: None,
            }),
        };
        let said = support(*family, Place::Local, Feature::ContextTools);
        assert_eq!(
            said.state == SupportState::Yes,
            local.runs_locally(),
            "{family:?}: {said:?}"
        );
        assert!(said.note.is_some(), "{family:?} says how or why not");
        let far = support(*family, Place::Remote, Feature::ContextTools);
        assert_eq!(far.state, SupportState::No, "{family:?} remote");
    }
    for route in [
        Route::Codex(ServerKey::Remote(remote.clone())),
        Route::RemoteClaude(remote.clone()),
        Route::RemoteOmp(remote),
    ] {
        assert!(route.is_remote() && !route.runs_locally());
    }
}

/// H1 (2026-11-20): the surfaces kept for a launch rest on what that
/// launch was given — Codex's `thread/start` carries the opening as
/// `developerInstructions` and the context server exactly when the record
/// says so, and the opening's length is the one sent.
#[tokio::test]
async fn the_kept_surfaces_match_what_the_launch_was_given() {
    use aeroric_protocol::checkpoint::HandoffReason;
    use aeroric_protocol::live_capability::{Feature, Place, SupportState, SurfaceLaunch};
    let mut s = setup();
    let dir = std::env::temp_dir().join(format!("aeroric-exec-surf-{}", uuid::Uuid::new_v4()));
    s.executor.surface_log().set_dir(dir.clone());
    let at = |kept: &SurfaceLaunch, feature: Feature| {
        kept.surfaces
            .iter()
            .find(|e| e.feature == feature)
            .cloned()
            .unwrap()
    };

    // Nothing to open with, no server set up.
    s.executor
        .launch(&launch("t1", ProtocolFamily::Codex))
        .unwrap();
    let mut peer = launched(&mut s.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    let kept = s.executor.surface_log().latest("t1").unwrap();
    assert_eq!(
        kept.surfaces.iter().map(|e| e.feature).collect::<Vec<_>>(),
        Feature::SURFACES.to_vec()
    );
    assert_eq!((kept.harness.as_str(), kept.place), ("codex", Place::Local));
    // H3: every launch gets the opening; without the server it says the
    // session has no Aeroric tools and points at nothing they would open.
    let given = start["params"]["developerInstructions"]
        .as_str()
        .unwrap_or_else(|| panic!("no opening: {start}"));
    assert!(given.starts_with(crate::sessions::live::opening::PURPOSE_HEADER));
    assert!(given.contains("没有接入它的工具"), "{given}");
    assert!(!given.contains(crate::sessions::live::opening::POINTERS_HEADER));
    let opening = at(&kept, Feature::SurfaceInstructions);
    assert_eq!(opening.state, SupportState::Yes);
    assert_eq!(
        opening.note,
        Some(format!(
            "这次启动从系统层给了开场内容（{} 字）",
            given.chars().count()
        ))
    );
    let tools = at(&kept, Feature::ContextTools);
    assert_eq!(tools.state, SupportState::No);
    assert_eq!(
        tools.note.as_deref(),
        Some("宿主没有准备 Aeroric 工具服务器")
    );

    // A source move arms a brief, and the server is set up: both reach
    // thread/start, and the record says so.
    s.executor
        .set_context_program("aeroric-cli".into(), "/d".into());
    s.executor
        .checkpoints()
        .arm("t2", HandoffReason::SourceMove);
    s.executor
        .launch(&LaunchRequest {
            agent_config_id: Some("native-a".into()),
            ..launch("t2", ProtocolFamily::Codex)
        })
        .unwrap();
    let mut peer = launched(&mut s.peers).await;
    peer.handshake().await;
    let start = peer.expect("thread/start").await;
    let kept = s.executor.surface_log().latest("t2").unwrap();
    let given = start["params"]["developerInstructions"]
        .as_str()
        .unwrap_or_else(|| panic!("no opening: {start}"));
    // H3: the opening with the tools' sentence, then the brief.
    assert!(given.starts_with(crate::sessions::live::opening::PURPOSE_HEADER));
    assert!(given.contains("Aeroric 上下文工具："), "{given}");
    assert!(given.contains("\n\n---\n\n"), "the brief follows: {given}");
    let opening = at(&kept, Feature::SurfaceInstructions);
    assert_eq!(opening.state, SupportState::Yes);
    assert_eq!(
        opening.note,
        Some(format!(
            "这次启动从系统层给了开场内容（{} 字）",
            given.chars().count()
        ))
    );
    assert!(start["params"]["config"]["mcp_servers"]["aeroric_context"].is_object());
    assert_eq!(at(&kept, Feature::ContextTools).state, SupportState::Yes);
    // Not exercised per launch: the family's table.
    assert_eq!(
        at(&kept, Feature::SurfaceSubagents),
        crate::sessions::live::capabilities::support(
            ProtocolFamily::Codex,
            Place::Local,
            Feature::SurfaceSubagents
        )
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// H1: a Claude launch hands its opening and context server to the
/// `claude` process (`--append-system-prompt`, `--mcp-config`), as the
/// kept surfaces say. The process is a fake that refuses to start, so
/// what it was asked to start with is all there is to see.
#[tokio::test]
async fn a_claude_launch_hands_what_its_surfaces_say() {
    use crate::sessions::live::claude::driver::ClaudeStart;
    use aeroric_protocol::checkpoint::HandoffReason;
    use aeroric_protocol::live_capability::{Feature, SupportState};

    struct Capture(Launcher, Arc<Mutex<Vec<ClaudeStart>>>);
    impl Routing for Capture {
        fn route(&self, _: ProtocolFamily, agent: &str, _: Option<&str>) -> Option<Route> {
            Some(Route::Claude(ClaudeKey {
                agent: agent.to_string(),
                config: None,
            }))
        }
        fn codex_launcher(&self, _key: &ServerKey) -> Launcher {
            Arc::clone(&self.0)
        }
        fn claude_launcher(&self, _key: &ClaudeKey) -> ClaudeLauncher {
            let starts = Arc::clone(&self.1);
            Arc::new(move |start: &ClaudeStart| {
                starts.lock().push(start.clone());
                Err("no claude in this test".to_string())
            })
        }
        fn route_remote(
            &self,
            _: ProtocolFamily,
            agent: &str,
            _: Option<&str>,
            connection_id: &str,
        ) -> Option<Route> {
            Some(Route::RemoteClaude(RemoteKey {
                connection_id: connection_id.to_string(),
                agent: agent.to_string(),
                config: None,
            }))
        }
        fn remote_claude_launcher(&self, _key: &RemoteKey) -> ClaudeLauncher {
            self.claude_launcher(&ClaudeKey {
                agent: "claude".into(),
                config: None,
            })
        }
    }

    let (codex, _peers) = launcher();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let executor = StructuredExecutor::new(
        Arc::new(Inner::default()) as Arc<dyn TaskExecutor>,
        Arc::new(Capture(codex, Arc::clone(&starts))),
        Arc::new(Sink::default()) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    let dir = std::env::temp_dir().join(format!("aeroric-exec-surf-c-{}", uuid::Uuid::new_v4()));
    executor.surface_log().set_dir(dir.clone());
    executor.set_context_program("aeroric-cli".into(), "/d".into());
    executor.checkpoints().arm("c1", HandoffReason::SourceMove);
    let _ = executor.launch(&LaunchRequest {
        agent: "claude".into(),
        ..launch("c1", ProtocolFamily::Claude)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while starts.lock().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "claude never asked to start"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let start = starts.lock()[0].clone();
    let kept = executor.surface_log().latest("c1").unwrap();
    assert_eq!(kept.harness, "claude");
    let entry = |f: Feature| kept.surfaces.iter().find(|e| e.feature == f).unwrap();
    let opening = start.append_system.expect("--append-system-prompt");
    assert!(opening.starts_with(crate::sessions::live::opening::PURPOSE_HEADER));
    assert!(opening.contains("Aeroric 上下文工具："), "{opening}");
    assert_eq!(entry(Feature::SurfaceInstructions).state, SupportState::Yes);
    assert_eq!(
        entry(Feature::SurfaceInstructions).note,
        Some(format!(
            "这次启动从系统层给了开场内容（{} 字）",
            opening.chars().count()
        ))
    );
    assert!(start.mcp_config.unwrap().contains("aeroric_context"));
    assert_eq!(entry(Feature::ContextTools).state, SupportState::Yes);

    // A remote Claude takes the opening the same way, and no tools.
    executor.checkpoints().arm("c2", HandoffReason::SourceMove);
    let _ = executor.launch(&LaunchRequest {
        agent: "claude".into(),
        working_directory: WorkingDirectory::Ssh {
            connection_id: "ssh-1".into(),
            remote_path: "/srv/w".into(),
        },
        ..launch("c2", ProtocolFamily::Claude)
    });
    while starts.lock().len() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "remote claude never asked to start"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let start = starts.lock()[1].clone();
    let kept = executor.surface_log().latest("c2").unwrap();
    assert_eq!(kept.place, aeroric_protocol::live_capability::Place::Remote);
    let entry = |f: Feature| kept.surfaces.iter().find(|e| e.feature == f).unwrap();
    assert!(start.append_system.is_some());
    assert_eq!(entry(Feature::SurfaceInstructions).state, SupportState::Yes);
    assert!(start.mcp_config.is_none());
    assert_eq!(entry(Feature::ContextTools).state, SupportState::No);
    assert_eq!(entry(Feature::SurfaceSkills).state, SupportState::No);
    std::fs::remove_dir_all(&dir).ok();
}

/// H2 (2026-11-20): an OMP session on this Mac gets the context server and
/// the opening at start, the record says so, and closing the session ends
/// its token.
#[tokio::test]
async fn an_omp_launch_gets_the_server_and_the_opening_and_closing_ends_its_token() {
    use crate::sessions::live::context_server::TOKEN_ENV;
    use crate::sessions::live::omp::driver::OmpStart;
    use aeroric_protocol::checkpoint::HandoffReason;
    use aeroric_protocol::live_capability::{Feature, SupportState};

    struct Capture(Launcher, Arc<Mutex<Vec<OmpStart>>>);
    impl Routing for Capture {
        fn route(&self, _: ProtocolFamily, agent: &str, _: Option<&str>) -> Option<Route> {
            Some(Route::Omp(OmpKey {
                agent: agent.to_string(),
                config: None,
            }))
        }
        fn codex_launcher(&self, _key: &ServerKey) -> Launcher {
            Arc::clone(&self.0)
        }
        fn claude_launcher(&self, _key: &ClaudeKey) -> ClaudeLauncher {
            Arc::new(|_: &crate::sessions::live::claude::driver::ClaudeStart| {
                Err("none".to_string())
            })
        }
        fn omp_launcher(&self, _key: &OmpKey) -> OmpLauncher {
            let starts = Arc::clone(&self.1);
            Arc::new(move |start: &OmpStart| {
                starts.lock().push(start.clone());
                Err("no omp in this test".to_string())
            })
        }
    }

    let (codex, _peers) = launcher();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let executor = StructuredExecutor::new(
        Arc::new(Inner::default()) as Arc<dyn TaskExecutor>,
        Arc::new(Capture(codex, Arc::clone(&starts))),
        Arc::new(Sink::default()) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    let dir = std::env::temp_dir().join(format!("aeroric-exec-omp-{}", uuid::Uuid::new_v4()));
    executor.surface_log().set_dir(dir.clone());
    executor.set_context_program("aeroric-cli".into(), "/d".into());
    executor.checkpoints().arm("o1", HandoffReason::SourceMove);
    let _ = executor.launch(&LaunchRequest {
        agent: "omp".into(),
        ..launch("o1", ProtocolFamily::Omp)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while starts.lock().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "omp never asked to start"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let start = starts.lock()[0].clone();
    let opening = start.append_system.expect("the opening");
    assert!(opening.starts_with(crate::sessions::live::opening::PURPOSE_HEADER));
    assert!(opening.contains("Aeroric 上下文工具："), "{opening}");
    let server = start.context.expect("the context server");
    let token = server
        .env
        .iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(
        executor.context_tokens().resolve(&token),
        Some(("o1".to_string(), "p1".to_string()))
    );
    let kept = executor.surface_log().latest("o1").unwrap();
    let entry = |f: Feature| kept.surfaces.iter().find(|e| e.feature == f).unwrap();
    assert_eq!(kept.harness, "omp");
    assert_eq!(entry(Feature::ContextTools).state, SupportState::Yes);
    assert_eq!(
        entry(Feature::SurfaceInstructions).note,
        Some(format!(
            "这次启动从系统层给了开场内容（{} 字）",
            opening.chars().count()
        ))
    );

    assert!(executor.close_session("o1"));
    assert_eq!(
        executor.context_tokens().resolve(&token),
        None,
        "closed: the token ends"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// H3 (2026-11-20): a session with no system layer (here a remote omp,
/// whose driver refuses it) gets the opening — and a brief, when one is
/// armed — in front of its first message, in the envelope the App shows as
/// "Aeroric 附加的上下文"; a resume with no message carries none.
#[tokio::test]
async fn with_no_system_layer_the_opening_goes_in_front_of_the_first_message() {
    use crate::sessions::live::omp::driver::{OmpSession, OmpSessionSpec, OmpStart};
    use crate::sessions::live::opening::{split_attached, ATTACHED_MARKER, PURPOSE_HEADER};
    use crate::sessions::live::surfaces::Opening;
    use aeroric_protocol::checkpoint::HandoffReason;
    let s = setup();
    let launcher: OmpLauncher = Arc::new(|_: &OmpStart| Err("not started".to_string()));
    let session: Arc<dyn SessionDriver> = OmpSession::start(
        OmpSessionSpec {
            task_id: "r1".into(),
            agent: "omp".into(),
            remote: true,
            ..Default::default()
        },
        launcher,
        Arc::new(Sink::default()) as Arc<dyn LiveStatusSink>,
        SharedLog::new("r1"),
        &tokio::runtime::Handle::current(),
    );
    let opening = format!("{PURPOSE_HEADER}\n用途");

    let (prompt, said) =
        s.executor
            .hand_handoff("r1", &session, Some("修好它".into()), false, Some(&opening));
    let prompt = prompt.unwrap();
    assert!(prompt.starts_with(ATTACHED_MARKER), "{prompt}");
    let (body, typed) = split_attached(&prompt).unwrap();
    assert_eq!((body, typed), (opening.as_str(), "修好它"));
    assert_eq!(
        said,
        Opening::Refused {
            brief: false,
            in_message: true
        }
    );

    // A brief rides in the same envelope, after the opening.
    s.executor.checkpoints().note_prompt("r1", "修好登录");
    s.executor
        .checkpoints()
        .arm("r1", HandoffReason::SourceMove);
    let (prompt, said) =
        s.executor
            .hand_handoff("r1", &session, Some("继续".into()), false, Some(&opening));
    let prompt = prompt.unwrap();
    let (body, typed) = split_attached(&prompt).unwrap();
    assert!(
        body.starts_with(PURPOSE_HEADER) && body.contains("\n\n---\n\n"),
        "{body}"
    );
    assert_eq!(typed, "继续");
    assert!(matches!(
        said,
        Opening::Refused {
            brief: true,
            in_message: true
        }
    ));

    // A resume with no message: nothing to carry it.
    let (prompt, said) = s
        .executor
        .hand_handoff("r1", &session, None, true, Some(&opening));
    assert_eq!(prompt, None);
    assert_eq!(
        said,
        Opening::Refused {
            brief: false,
            in_message: false
        }
    );
}
