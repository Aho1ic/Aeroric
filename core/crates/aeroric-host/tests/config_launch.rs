//! Launching from an agent configuration instead of a legacy wrapper (Q5).
//!
//! Isolated: temporary data root, MemorySecrets, fake harness/wrapper scripts. The
//! gateway really starts on loopback, but no upstream request is ever made. The
//! executor is driven directly so launch and resume take the same assertions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::context::{CoreContext, MemorySecrets, SecretKey, SecretStore};
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    AgentFamilies, ExecutorFailure, LaunchRequest, PermissionMode, ProtocolFamily, ResumeRequest,
    TaskExecutor, WorkingDirectory,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_core::terminal::stream::TerminalIdentity;
use aeroric_host::executor::CliPtyExecutor;
use aeroric_host::families::SettingsFamiliesCli;
use serde_json::json;

const UPSTREAM_SECRET: &str = "sk-upstream-secret-must-not-leak";

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
    context: Arc<CoreContext>,
    executor: Arc<CliPtyExecutor>,
    terminal: Arc<TerminalService>,
}

impl Drop for Env {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn script(path: &Path, marker: &str) {
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\n\
             echo \"RAN={marker}\"\n\
             echo \"ARGV=$*\"\n\
             echo \"CLAUDE_CONFIG_DIR=$CLAUDE_CONFIG_DIR\"\n\
             echo \"CODEX_HOME=$CODEX_HOME\"\n\
             echo \"ANTHROPIC_BASE_URL=$ANTHROPIC_BASE_URL\"\n\
             echo \"ANTHROPIC_AUTH_TOKEN=$ANTHROPIC_AUTH_TOKEN\"\n\
             echo \"ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY\"\n\
             echo \"OPENAI_BASE_URL=$OPENAI_BASE_URL\"\n\
             echo \"OPENAI_API_KEY=$OPENAI_API_KEY\"\n\
             echo \"SECRET_HITS=$(env | grep -c {UPSTREAM_SECRET})\"\n\
             echo END-OF-FAKE\n"
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// `settings` gets `claude_path`/`codex_path` pointing at the fake harness and a
/// legacy `ds` wrapper profile unless `with_ds_profile` is false. `migrated` adds
/// `custom:ds → cfg-legacy` to the migration report.
fn env(tag: &str, with_ds_profile: bool, migrated: bool) -> Env {
    let root = PathBuf::from("/tmp").join(format!(
        "ah-cfg-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(root.join("home/.aeroric")).unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    let harness = root.join("harness.sh");
    let wrapper = root.join("wrapper.sh");
    script(&harness, "harness");
    script(&wrapper, "wrapper");

    let paths = AppPaths::from_user_home(root.join("home"));
    let secrets = Arc::new(MemorySecrets::default());
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "config-launch-test".into(),
                version: "0-test".into(),
            },
            &LocalProbe,
        )
        .unwrap()
        .with_secrets(Arc::clone(&secrets) as Arc<dyn SecretStore>),
    );
    let data = paths.data_dir().to_path_buf();
    let mut profiles = vec![json!({"id": "plain", "label": "Plain", "path": wrapper, "args": []})];
    if with_ds_profile {
        profiles.push(
            json!({"id": "ds", "label": "DS", "path": wrapper, "family": "claude", "args": []}),
        );
    }
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec(&json!({
            "claude_path": harness,
            "codex_path": harness,
            "custom_agents": profiles
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        data.join("agent-connections.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "connections": [{
                "id": "conn", "label": "Conn", "kind": "api_key", "vendor": "other",
                "base_url": "https://fixture.invalid/v1", "protocols": ["messages"],
                "models": [{"upstream_id": "up", "mapped_name": "mapped-model"}]
            }],
            "configs": [
                {"id": "cfg-claude", "label": "Claude", "harness": "claude",
                 "target": {"kind": "connection", "connection_id": "conn"}},
                {"id": "cfg-codex", "label": "Codex", "harness": "codex",
                 "target": {"kind": "connection", "connection_id": "conn"}},
                {"id": "cfg-legacy", "label": "DS", "harness": "claude",
                 "target": {"kind": "connection", "connection_id": "conn"}},
                {"id": "cfg-omp", "label": "OMP", "harness": "omp",
                 "target": {"kind": "route"}}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    secrets
        .set(
            &SecretKey("connection:conn:credential".into()),
            UPSTREAM_SECRET,
        )
        .unwrap();
    if migrated {
        std::fs::write(
            data.join("agent-connections-migration-report.json"),
            serde_json::to_vec(&json!({
                "version": 2,
                "migrated": [{"source": "custom:ds", "connectionId": "conn",
                              "configId": "cfg-legacy", "sourceFingerprint": "0".repeat(64)}],
                "alreadyPresent": [], "skipped": [], "warnings": []
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let terminal = Arc::new(TerminalService::new());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::new(TaskRegistry::default()),
        Arc::clone(&terminal),
        Arc::new(EventLog::default()),
    );
    Env {
        root,
        context,
        executor,
        terminal,
    }
}

impl Env {
    fn workspace(&self) -> WorkingDirectory {
        WorkingDirectory::Local {
            path: self.root.join("workspace").to_string_lossy().into_owned(),
        }
    }

    fn launch_request(&self, task: &str, agent: &str, config: Option<&str>) -> LaunchRequest {
        LaunchRequest {
            task_id: task.into(),
            project_id: "project".into(),
            operation_key: format!("op-{task}"),
            generation: 1,
            family: ProtocolFamily::Claude,
            agent: agent.into(),
            permission_mode: PermissionMode::Ask,
            permission_mode_wire: "ask".into(),
            prompt: String::new(),
            prompt_override: None,
            images: vec![],
            texts: vec![],
            inject_prompt_into_terminal: false,
            session_id: None,
            selected_model: Some("mapped-model".into()),
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
            agent_config_id: config.map(str::to_string),
            created_at_ms: 0,
            dsh_window: None,
            working_directory: self.workspace(),
            terminal_size: Some((120, 30)),
        }
    }

    fn resume_request(&self, task: &str, agent: &str, config: Option<&str>) -> ResumeRequest {
        ResumeRequest {
            task_id: task.into(),
            project_id: "project".into(),
            operation_key: format!("op-{task}"),
            generation: 1,
            family: ProtocolFamily::Claude,
            agent: agent.into(),
            session_id: "session-1".into(),
            session_path: None,
            permission_mode: PermissionMode::Ask,
            permission_mode_wire: "ask".into(),
            selected_model: Some("mapped-model".into()),
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
            agent_config_id: config.map(str::to_string),
            prompt: None,
            working_directory: self.workspace(),
            terminal_size: Some((120, 30)),
        }
    }

    fn home(&self, name: &str) -> String {
        self.context
            .paths
            .data_dir()
            .join("agent-homes")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    /// Fake script output, as `KEY=value` lines.
    async fn output(&self, task: &str) -> std::collections::BTreeMap<String, String> {
        let identity = TerminalIdentity::new(task, 1);
        for _ in 0..200 {
            if let Ok(chunk) = self.terminal.read_from(&identity, 0, 64 * 1024) {
                let text = String::from_utf8_lossy(&chunk.bytes).replace('\r', "");
                if text.contains("END-OF-FAKE") {
                    return text
                        .lines()
                        .filter_map(|line| line.split_once('='))
                        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                        .collect();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("fake agent for {task} produced no complete output");
    }
}

fn assert_gateway_claude(out: &std::collections::BTreeMap<String, String>, home: &str) {
    assert_eq!(out["RAN"], "harness", "never the legacy wrapper: {out:?}");
    assert_eq!(out["CLAUDE_CONFIG_DIR"], home);
    assert!(
        out["ANTHROPIC_BASE_URL"].starts_with("http://127.0.0.1:")
            && out["ANTHROPIC_BASE_URL"].ends_with("/claude"),
        "{out:?}"
    );
    assert!(!out["ANTHROPIC_AUTH_TOKEN"].is_empty());
    assert_eq!(out["ANTHROPIC_API_KEY"], "");
    assert_eq!(
        out["SECRET_HITS"], "0",
        "upstream credential reached the process"
    );
    assert!(out["ARGV"].contains("--model mapped-model"), "{out:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pinned_configuration_launches_the_harness_through_the_gateway() {
    let env = env("pinned", true, false);
    env.executor
        .launch(&env.launch_request("t-claude", "claude", Some("cfg-claude")))
        .unwrap();
    let out = env.output("t-claude").await;
    assert_gateway_claude(&out, &env.home(&format!("config-{}", hex("cfg-claude"))));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out["CLAUDE_CONFIG_DIR"])
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    // The token dies with the process.
    let token = out["ANTHROPIC_AUTH_TOKEN"].clone();
    let mut revoked = false;
    for _ in 0..200 {
        if aeroric_core::local_router::task_tokens::lookup(&token).is_none() {
            revoked = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(revoked, "router token outlived the process");

    env.executor
        .launch(&env.launch_request("t-codex", "codex", Some("cfg-codex")))
        .unwrap();
    let out = env.output("t-codex").await;
    assert_eq!(out["RAN"], "harness");
    assert_eq!(
        out["CODEX_HOME"],
        env.home(&format!("config-{}", hex("cfg-codex")))
    );
    assert!(out["OPENAI_BASE_URL"].ends_with("/codex/v1"), "{out:?}");
    assert!(!out["OPENAI_API_KEY"].is_empty());
    assert_eq!(out["SECRET_HITS"], "0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_or_unsupported_configuration_never_falls_back() {
    let env = env("missing", true, false);
    for (task, config) in [("t-gone", "gone"), ("t-omp", "cfg-omp")] {
        match env
            .executor
            .launch(&env.launch_request(task, "claude", Some(config)))
        {
            Err(ExecutorFailure::Spawn { reason }) => assert!(reason.contains(config), "{reason}"),
            other => panic!("expected a spawn refusal, got {other:?}"),
        }
        assert!(env
            .terminal
            .read_from(&TerminalIdentity::new(task, 1), 0, 1024)
            .is_err_or_empty());
    }
    // A remote workspace cannot reach this Mac's loopback gateway.
    let mut remote = env.launch_request("t-remote", "claude", Some("cfg-claude"));
    remote.working_directory = WorkingDirectory::Ssh {
        connection_id: "nowhere".into(),
        remote_path: "/srv".into(),
    };
    assert!(matches!(
        env.executor.launch(&remote),
        Err(ExecutorFailure::Unsupported { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_migrated_legacy_task_resumes_through_its_configuration_without_the_profile() {
    let env = env("legacy", false, true);
    env.executor
        .resume(&env.resume_request("t-legacy", "ds", None))
        .unwrap();
    let out = env.output("t-legacy").await;
    // Same home as the legacy wrapper, so the old sessions are there to resume.
    assert_gateway_claude(&out, &env.home("ds"));
    assert!(out["ARGV"].contains("--resume session-1"), "{out:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_migrated_legacy_profile_switches_even_while_it_still_exists() {
    let env = env("switch", true, true);
    env.executor
        .launch(&env.launch_request("t-switch", "ds", None))
        .unwrap();
    assert_gateway_claude(&env.output("t-switch").await, &env.home("ds"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unmigrated_legacy_profile_keeps_its_wrapper_and_no_gateway() {
    let env = env("plain", true, true);
    env.executor
        .launch(&env.launch_request("t-plain", "plain", None))
        .unwrap();
    let out = env.output("t-plain").await;
    assert_eq!(out["RAN"], "wrapper");
    assert_eq!(out["ANTHROPIC_BASE_URL"], "");
    assert_eq!(out["ANTHROPIC_AUTH_TOKEN"], "");
    assert_eq!(out["CLAUDE_CONFIG_DIR"], "");
}

#[test]
fn families_follow_the_migrated_configuration_after_the_profile_is_gone() {
    let mapped = env("families", false, true);
    let families = SettingsFamiliesCli::new(Arc::clone(&mapped.context));
    assert_eq!(families.launch_family("ds"), ProtocolFamily::Claude);
    assert_eq!(families.session_family("ds"), ProtocolFamily::Claude);
    // Without the report the same id falls back to the unknown-agent guess.
    let unmapped = env("families-none", false, false);
    let families = SettingsFamiliesCli::new(Arc::clone(&unmapped.context));
    assert_eq!(families.launch_family("ds"), ProtocolFamily::Codex);
}

fn hex(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

trait ErrOrEmpty {
    fn is_err_or_empty(&self) -> bool;
}

impl<E> ErrOrEmpty for Result<aeroric_core::terminal::stream::StreamChunk, E> {
    fn is_err_or_empty(&self) -> bool {
        self.as_ref().map_or(true, |chunk| chunk.bytes.is_empty())
    }
}
