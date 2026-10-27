//! N4b: an agent configuration whose harness is Codex runs on its own shared
//! `codex app-server`, started with the configuration's managed home and the
//! gateway environment, and a gateway token minted for that process and revoked
//! when the process goes away (stopped, crashed or failed to start).
//!
//! Isolated: temporary data root, MemorySecrets, a fake `codex` script that
//! records its environment. The gateway really starts on loopback; no upstream
//! request is ever made.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aeroric_core::context::{CoreContext, MemorySecrets, SecretStore};
use aeroric_core::local_router::task_tokens;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::sessions::live::codex::server::{CodexServer, SERVER_IDLE_MS};
use aeroric_core::sessions::live::executor::{
    ClaudeKey, OmpKey, Route, Routing, ServerKey, StructuredExecutor,
};
use aeroric_core::sessions::live::LiveStatusSink;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::executor::{CancelTarget, Execution, ResetOutcome};
use aeroric_core::tasks::ProtocolFamily;
use aeroric_core::tasks::{
    LaunchRequest, PermissionMode, ResumeRequest, TaskExecutor, TaskStatus, WorkingDirectory,
};
use aeroric_host::live::{server_token_id, HostRouting};
use serde_json::json;

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

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Env {
    root: PathBuf,
    routing: Arc<HostRouting>,
    /// Unique per test: the gateway token table is process-wide, and tests run
    /// in parallel.
    codex_config: String,
}

impl Drop for Env {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

impl Env {
    fn env_file(&self) -> PathBuf {
        self.root.join("env.txt")
    }

    fn recorded(&self, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.env_file()).ok()?;
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")).map(str::to_string))
    }
}

/// A `codex` that answers `--version`, records its environment, handshakes,
/// then idles — or exits right after the handshake when `crash` exists.
fn fake_codex(root: &Path) -> PathBuf {
    let script = root.join("codex.sh");
    let env = root.join("env.txt");
    let crash = root.join("crash");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do last=\"$a\"; done\n\
             if [ \"$last\" = \"--version\" ]; then echo 'codex-cli 0.162.1'; exit 0; fi\n\
             printf '%s\\n' \"$@\" > '{argv}'\nenv > '{env}'\n\
             read line\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\n\
             if [ -f '{crash}' ]; then exit 0; fi\n\
             read init_done\nread resume\n\
             echo '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"th-old\"}}}}}}'\n\
             cat > /dev/null\n",
            env = env.display(),
            argv = root.join("argv.txt").display(),
            crash = crash.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

fn env() -> Env {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-gwcodex-{}-{seq}", std::process::id()));
    let codex_config = format!("cfg-codex-{}-{seq}", std::process::id());
    std::fs::create_dir_all(root.join("home/.aeroric")).unwrap();
    let codex = fake_codex(&root);
    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "gw-codex-test".into(),
                version: "0-test".into(),
            },
            &LocalProbe,
        )
        .unwrap()
        .with_secrets(Arc::new(MemorySecrets::default()) as Arc<dyn SecretStore>),
    );
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec(&json!({"codex_path": codex})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        paths.data_dir().join("agent-connections.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "connections": [{
                "id": "conn", "label": "Conn", "kind": "api_key", "vendor": "other",
                "base_url": "https://fixture.invalid/v1", "protocols": ["responses"],
                "models": [{"upstream_id": "up", "mapped_name": "mapped-model"}]
            }],
            "configs": [
                {"id": codex_config, "label": "Codex", "harness": "codex",
                 "target": {"kind": "connection", "connection_id": "conn"}},
                {"id": "cfg-claude", "label": "Claude", "harness": "claude",
                 "target": {"kind": "connection", "connection_id": "conn"}}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    Env {
        root,
        routing: HostRouting::new(context),
        codex_config,
    }
}

async fn until(what: &str, check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn codex_configurations_get_their_own_server_and_everything_else_stays_put() {
    let env = env();
    let route =
        |family, agent: &str, config: Option<&str>| env.routing.route(family, agent, config);
    let config = env.codex_config.as_str();
    assert_eq!(
        route(ProtocolFamily::Codex, "codex", Some(config)),
        Some(Route::Codex(ServerKey::Config(config.into())))
    );
    assert_eq!(
        route(ProtocolFamily::Codex, "codex", None),
        Some(Route::Codex(ServerKey::Agent("codex".into())))
    );
    assert_eq!(
        route(ProtocolFamily::Codex, "codex", Some("cfg-claude")),
        None,
        "a Claude harness under a Codex family is not ours"
    );
    assert_eq!(
        route(ProtocolFamily::Codex, "codex", Some("cfg-gone")),
        None,
        "the PTY path refuses it with its reason"
    );
    assert_eq!(
        route(ProtocolFamily::Claude, "claude", Some("cfg-claude")),
        Some(Route::Claude(ClaudeKey {
            agent: "claude".into(),
            config: Some("cfg-claude".into())
        })),
        "Claude configurations run natively per session (N5)"
    );
    assert_eq!(
        route(ProtocolFamily::Omp, "omp", None),
        Some(Route::Omp(OmpKey {
            agent: "omp".into(),
            config: None
        })),
        "local omp runs natively (ADR-043 phase two)"
    );
    assert_eq!(
        route(ProtocolFamily::Omp, "omp", Some("cfg-codex")),
        None,
        "a configuration of another harness is refused on the PTY"
    );
    assert_eq!(
        route(ProtocolFamily::Codex, "my-wrapper", None),
        None,
        "an unmigrated custom wrapper stays on the PTY (N6 D2)"
    );
    assert_eq!(
        route(ProtocolFamily::Claude, "my-claude-wrapper", None),
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_configuration_server_gets_its_home_and_gateway_and_its_token_dies_with_it() {
    let env = env();
    let server = CodexServer::new(
        env.routing
            .codex_launcher(&ServerKey::Config(env.codex_config.clone())),
    );
    server.connect().await.expect("handshake");
    until("the environment record", || {
        env.recorded("OPENAI_API_KEY").is_some()
    })
    .await;

    let home = env.recorded("CODEX_HOME").unwrap();
    assert!(home.contains("agent-homes"), "managed home, got {home}");
    let base = env.recorded("OPENAI_BASE_URL").unwrap();
    assert!(
        base.starts_with("http://127.0.0.1") && base.ends_with("/codex/v1"),
        "gateway, got {base}"
    );
    assert!(env.recorded("CODEX_API_KEY").is_none());
    assert!(env.recorded("OPENAI_ORG_ID").is_none());
    let token = env.recorded("OPENAI_API_KEY").unwrap();
    // N6a B5: Codex ignores OPENAI_BASE_URL; the provider comes from `-c`.
    assert_eq!(
        env.recorded("AERORIC_GATEWAY_TOKEN").as_deref(),
        Some(token.as_str())
    );
    let argv: Vec<String> = std::fs::read_to_string(env.root.join("argv.txt"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    for expected in [
        "model_provider=\"aeroric\"".to_string(),
        format!("model_providers.aeroric.base_url=\"{base}\""),
        "model_providers.aeroric.env_key=\"AERORIC_GATEWAY_TOKEN\"".to_string(),
        "model_providers.aeroric.wire_api=\"responses\"".to_string(),
    ] {
        let at = argv.iter().position(|a| *a == expected);
        assert!(
            at.is_some_and(|i| i > 0 && argv[i - 1] == "-c"),
            "missing -c {expected} in {argv:?}"
        );
    }
    assert_eq!(argv.last().map(String::as_str), Some("app-server"));
    assert!(
        argv.iter().all(|a| !a.contains(&token)),
        "the token never goes on the command line"
    );
    let grant = task_tokens::lookup(&token).expect("a live token while the server runs");
    assert_eq!(grant.task_id, server_token_id(&env.codex_config));
    assert_eq!(
        grant.agent_config_id.as_deref(),
        Some(env.codex_config.as_str())
    );

    assert!(
        server.stop_if_unused(now_ms() + SERVER_IDLE_MS).await,
        "an unused server stops"
    );
    assert!(
        task_tokens::lookup(&token).is_none(),
        "stopping revokes its token"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashed_server_s_token_is_revoked_and_a_restart_mints_a_new_one() {
    let env = env();
    std::fs::write(env.root.join("crash"), "").unwrap();
    let server = CodexServer::new(
        env.routing
            .codex_launcher(&ServerKey::Config(env.codex_config.clone())),
    );
    server.connect().await.expect("handshake");
    until("the environment record", || {
        env.recorded("OPENAI_API_KEY").is_some()
    })
    .await;
    let first = env.recorded("OPENAI_API_KEY").unwrap();
    let watched = Arc::clone(&server);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while watched.is_running().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the exit was never noticed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        task_tokens::lookup(&first).is_none(),
        "the exit revokes the token"
    );

    std::fs::remove_file(env.root.join("crash")).unwrap();
    std::fs::remove_file(env.env_file()).unwrap();
    server.connect().await.expect("restart");
    until("the second record", || {
        env.recorded("OPENAI_API_KEY").is_some()
    })
    .await;
    let second = env.recorded("OPENAI_API_KEY").unwrap();
    assert_ne!(first, second);
    assert!(task_tokens::lookup(&second).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_configuration_fails_to_start_with_its_reason() {
    let env = env();
    let server = CodexServer::new(
        env.routing
            .codex_launcher(&ServerKey::Config("cfg-gone".into())),
    );
    let refused = server.connect().await.err().expect("no such configuration");
    assert!(refused.contains("no longer exists"), "{refused}");
    assert!(task_tokens::lookup("anything").is_none());
    assert!(!env.env_file().exists(), "nothing was started");
}

#[derive(Default)]
struct Pty(parking_lot::Mutex<Vec<String>>);

impl TaskExecutor for Pty {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        self.0.lock().push(format!("launch {}", request.task_id));
        Ok(())
    }
    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        self.0.lock().push(format!("resume {}", request.task_id));
        Ok(())
    }
    fn cancel(&self, _: &str, _: &CancelTarget) -> Execution<()> {
        Ok(())
    }
    fn complete(&self, _: &str, _: &str) -> Execution<()> {
        Ok(())
    }
    fn reset(&self, _: &str) -> Execution<ResetOutcome> {
        Ok(ResetOutcome::default())
    }
}

#[derive(Default)]
struct Sink {
    statuses: parking_lot::Mutex<Vec<TaskStatus>>,
    sessions: parking_lot::Mutex<Vec<String>>,
}

impl LiveStatusSink for Sink {
    fn status(&self, _: &str, _: &str, _: u64, status: TaskStatus, _: Option<String>) {
        self.statuses.lock().push(status);
    }
    fn session(
        &self,
        _: &str,
        _: &str,
        _: ProtocolFamily,
        _: &str,
        session_id: &str,
        _: Option<&str>,
    ) {
        self.sessions.lock().push(session_id.to_string());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_gateway_task_continues_its_thread_on_the_driver() {
    let env = env();
    let pty = Arc::new(Pty::default());
    let sink = Arc::new(Sink::default());
    let executor = StructuredExecutor::new(
        Arc::clone(&pty) as Arc<dyn TaskExecutor>,
        Arc::clone(&env.routing) as Arc<dyn Routing>,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    let resume = |config: &str| ResumeRequest {
        task_id: format!("task-{config}"),
        project_id: "p".into(),
        operation_key: "op".into(),
        generation: 2,
        family: ProtocolFamily::Codex,
        agent: "codex".into(),
        session_id: "th-old".into(),
        session_path: None,
        permission_mode: PermissionMode::Ask,
        permission_mode_wire: "ask".into(),
        selected_model: Some("mapped-model".into()),
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: Some(config.to_string()),
        prompt: None,
        working_directory: WorkingDirectory::Local {
            path: env.root.to_string_lossy().into_owned(),
        },
        terminal_size: None,
    };
    executor.resume(&resume(&env.codex_config)).unwrap();
    until("done after thread/resume", || {
        sink.statuses.lock().last() == Some(&TaskStatus::Done)
    })
    .await;
    assert_eq!(*sink.sessions.lock(), ["th-old"]);
    assert!(env.recorded("CODEX_HOME").unwrap().contains("agent-homes"));
    assert!(pty.0.lock().is_empty(), "nothing went to the PTY executor");

    executor
        .resume(&ResumeRequest {
            family: ProtocolFamily::Omp,
            ..resume(&env.codex_config)
        })
        .unwrap();
    assert_eq!(pty.0.lock().len(), 1, "other families stay on the PTY");
}
