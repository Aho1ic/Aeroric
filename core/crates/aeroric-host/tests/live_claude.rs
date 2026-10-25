//! N5: Claude Code runs natively, one process per session, started by the
//! host with the stream-json control flags, the user's model and permission
//! tier, `--resume` once a session exists, and — under an agent configuration —
//! the managed home and the gateway with a token for that task, revoked with
//! the process.
//!
//! Isolated: temporary data root and `CLAUDE_CONFIG_DIR`, MemorySecrets, a fake
//! `claude` script that records argv, cwd and environment. The gateway really
//! starts on loopback; no upstream request is ever made.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aeroric_core::context::{CoreContext, MemorySecrets, SecretStore};
use aeroric_core::local_router::task_tokens;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::sessions::live::executor::{Routing, StructuredExecutor};
use aeroric_core::sessions::live::LiveStatusSink;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::executor::{CancelTarget, Execution, ResetOutcome};
use aeroric_core::tasks::{
    LaunchRequest, PermissionMode, ProtocolFamily, ResumeRequest, TaskExecutor, TaskStatus,
    WorkingDirectory,
};
use aeroric_host::live::HostRouting;
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
    statuses: parking_lot::Mutex<Vec<(TaskStatus, Option<String>)>>,
    sessions: parking_lot::Mutex<Vec<String>>,
}

impl LiveStatusSink for Sink {
    fn status(&self, _: &str, _: &str, _: u64, status: TaskStatus, reason: Option<String>) {
        self.statuses.lock().push((status, reason));
    }
    fn session(
        &self,
        _: &str,
        _: &str,
        family: ProtocolFamily,
        _: &str,
        session_id: &str,
        _: Option<&str>,
    ) {
        assert_eq!(family, ProtocolFamily::Claude);
        self.sessions.lock().push(session_id.to_string());
    }
}

struct Env {
    root: PathBuf,
    executor: Arc<StructuredExecutor>,
    sink: Arc<Sink>,
    pty: Arc<Pty>,
    claude_config: String,
}

impl Drop for Env {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

impl Env {
    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap_or_default()
    }
    fn argv(&self) -> Vec<String> {
        self.file("argv.txt").lines().map(str::to_string).collect()
    }
    fn recorded(&self, key: &str) -> Option<String> {
        self.file("env.txt")
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")).map(str::to_string))
    }
    async fn until_status(&self, status: TaskStatus) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while self.sink.statuses.lock().last().map(|(s, _)| *s) != Some(status) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "no {status:?}: {:?}",
                self.sink.statuses.lock()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// A `claude` that answers `--version`, records argv / cwd / env, handshakes,
/// plays one turn for the first message, and idles.
fn fake_claude(root: &Path, version: &str) -> PathBuf {
    let script = root.join("claude.sh");
    let dir = root.display();
    let id = r#"sed 's/.*"request_id":"\([^"]*\)".*/\1/'"#;
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
for a in "$@"; do last="$a"; done
if [ "$last" = "--version" ]; then echo '{version} (Claude Code)'; exit 0; fi
printf '%s\n' "$@" > '{dir}/argv.txt'
pwd > '{dir}/cwd.txt'
env > '{dir}/env.txt'
read init; id=$(echo "$init" | {id})
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"'"$id"'","response":{{}}}}}}'
read user
echo '{{"type":"system","subtype":"init","session_id":"cs-host","model":"claude-sonnet"}}'
echo '{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"text","text":"ok"}}]}}}}'
echo '{{"type":"result","subtype":"success","is_error":false,"session_id":"cs-host","usage":{{"input_tokens":3,"output_tokens":2}}}}'
read usage; id=$(echo "$usage" | {id})
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"'"$id"'","response":{{"totalTokens":900,"maxTokens":200000}}}}}}'
cat > /dev/null
"#
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

fn env(version: &str) -> Env {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-claude-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    let claude = fake_claude(&root, version);
    let claude_config = format!("cfg-claude-{}-{seq}", std::process::id());
    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "live-claude-test".into(),
                version: "0-test".into(),
            },
            &LocalProbe,
        )
        .unwrap()
        .with_secrets(Arc::new(MemorySecrets::default()) as Arc<dyn SecretStore>),
    );
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec(&json!({"claude_path": claude})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        paths.data_dir().join("agent-connections.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "connections": [{
                "id": "conn", "label": "Conn", "kind": "api_key", "vendor": "other",
                "base_url": "https://fixture.invalid/v1", "protocols": ["messages"],
                "models": [{"upstream_id": "up", "mapped_name": "mapped-model"}]
            }],
            "configs": [{"id": claude_config, "label": "Claude", "harness": "claude",
                         "target": {"kind": "connection", "connection_id": "conn"}}]
        }))
        .unwrap(),
    )
    .unwrap();
    let pty = Arc::new(Pty::default());
    let sink = Arc::new(Sink::default());
    let executor = StructuredExecutor::new(
        Arc::clone(&pty) as Arc<dyn TaskExecutor>,
        HostRouting::new(context) as Arc<dyn Routing>,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        tokio::runtime::Handle::current(),
    );
    Env {
        root,
        executor,
        sink,
        pty,
        claude_config,
    }
}

fn launch(env: &Env, task: &str, config: Option<&str>) -> LaunchRequest {
    LaunchRequest {
        task_id: task.into(),
        project_id: "p".into(),
        operation_key: "op".into(),
        generation: 1,
        family: ProtocolFamily::Claude,
        agent: "claude".into(),
        permission_mode: PermissionMode::AutoEdit,
        permission_mode_wire: "auto_edit".into(),
        prompt: "hi".into(),
        prompt_override: None,
        images: vec![],
        texts: vec![],
        inject_prompt_into_terminal: false,
        session_id: None,
        selected_model: Some("sonnet".into()),
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: config.map(str::to_string),
        created_at_ms: 0,
        dsh_window: None,
        working_directory: WorkingDirectory::Local {
            path: env.root.join("workspace").to_string_lossy().into_owned(),
        },
        terminal_size: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_built_in_claude_starts_in_control_mode_and_runs_a_turn() {
    let env = env("2.1.299");
    env.executor.launch(&launch(&env, "t1", None)).unwrap();
    env.until_status(TaskStatus::Done).await;
    let argv = env.argv();
    for flag in [
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--allow-dangerously-skip-permissions",
    ] {
        assert!(argv.iter().any(|a| a == flag), "missing {flag} in {argv:?}");
    }
    let after = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].clone())
    };
    assert_eq!(after("--permission-mode").as_deref(), Some("acceptEdits"));
    assert_eq!(after("--model").as_deref(), Some("sonnet"));
    assert!(
        !argv
            .iter()
            .any(|a| a == "--resume" || a == "--system-prompt"),
        "{argv:?}"
    );
    assert!(env.file("cwd.txt").trim().ends_with("workspace"));
    assert_eq!(*env.sink.sessions.lock(), ["cs-host"]);
    assert!(
        env.recorded("ANTHROPIC_BASE_URL").is_none(),
        "the built-in Claude uses the user's own login"
    );
    assert!(env.pty.0.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resumed_session_starts_with_resume_on_its_first_prompt() {
    let env = env("2.1.299");
    let resume = ResumeRequest {
        task_id: "t2".into(),
        project_id: "p".into(),
        operation_key: "op".into(),
        generation: 2,
        family: ProtocolFamily::Claude,
        agent: "claude".into(),
        session_id: "cs-old".into(),
        session_path: None,
        permission_mode: PermissionMode::Ask,
        permission_mode_wire: "ask".into(),
        selected_model: None,
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: None,
        prompt: None,
        working_directory: WorkingDirectory::Local {
            path: env.root.join("workspace").to_string_lossy().into_owned(),
        },
        terminal_size: None,
    };
    env.executor.resume(&resume).unwrap();
    env.until_status(TaskStatus::Done).await;
    assert!(env.argv().is_empty(), "no process until a prompt");
    env.executor
        .registry()
        .get("t2")
        .unwrap()
        .prompt("continue".into())
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while env
        .sink
        .statuses
        .lock()
        .iter()
        .filter(|(s, _)| *s == TaskStatus::Done)
        .count()
        < 2
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{:?}",
            env.sink.statuses.lock()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let argv = env.argv();
    let after = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].clone())
    };
    assert_eq!(after("--resume").as_deref(), Some("cs-old"));
    assert_eq!(after("--permission-mode").as_deref(), Some("default"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_configuration_gets_its_home_and_gateway_and_the_task_token_dies_with_the_process() {
    let env = env("2.1.299");
    let config = env.claude_config.clone();
    env.executor
        .launch(&launch(&env, "t3", Some(&config)))
        .unwrap();
    env.until_status(TaskStatus::Done).await;
    assert!(env
        .recorded("CLAUDE_CONFIG_DIR")
        .unwrap()
        .contains("agent-homes"));
    let base = env.recorded("ANTHROPIC_BASE_URL").unwrap();
    assert!(
        base.starts_with("http://127.0.0.1") && base.ends_with("/claude"),
        "{base}"
    );
    assert!(env.recorded("ANTHROPIC_API_KEY").is_none());
    let token = env.recorded("ANTHROPIC_AUTH_TOKEN").unwrap();
    let grant = task_tokens::lookup(&token).expect("a live token while the process runs");
    assert_eq!(grant.task_id, "t3");
    assert_eq!(grant.agent_config_id.as_deref(), Some(config.as_str()));

    env.executor
        .cancel(
            "t3",
            &CancelTarget::Local {
                project_path: String::new(),
            },
        )
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while task_tokens::lookup(&token).is_some() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "closing the session revokes the token"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_claude_code_is_refused_with_an_upgrade_hint() {
    let env = env("2.0.9");
    env.executor.launch(&launch(&env, "t4", None)).unwrap();
    env.until_status(TaskStatus::Failed).await;
    let reason = env.sink.statuses.lock().last().unwrap().1.clone().unwrap();
    assert!(reason.contains("请升级"), "{reason}");
    assert!(env.argv().is_empty(), "never started");
}
