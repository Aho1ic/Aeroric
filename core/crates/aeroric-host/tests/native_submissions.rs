//! Authenticated UDS, isolated data, MemorySecrets and an in-memory executor.
//! The concrete PTY adapter is registered but NEVER invoked by these requests.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use aeroric_core::clock::SystemClock;
use aeroric_core::context::MemorySecrets;
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::projects::ProjectService;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    CancelTarget, ExecutorFailure, LaunchRequest, NoLiveRuns, ResetOutcome, ResumeRequest,
    TaskExecutor, TaskService,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::executor::CliPtyExecutor;
use aeroric_host::families::SettingsFamiliesCli;
use aeroric_host::startup::{run_startup_with_secrets, RunningHost};
use aeroric_protocol::envelope::{encode_line, Frame};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::submission::{
    ConfigurationCatalog, SubmissionReceipt, SubmissionRequest, SubmissionState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

mod host_harness;
use host_harness::Harness;

static SEQ: AtomicU64 = AtomicU64::new(0);
struct Local;
impl StorageLocationProbe for Local {
    fn probe(&self, path: &Path) -> StorageLocation {
        StorageLocation::new(path, VolumeKind::Local)
    }
}

#[derive(Default)]
struct FakeExecutor {
    launches: Mutex<Vec<LaunchRequest>>,
    fail: AtomicBool,
    data_dir: PathBuf,
}
impl TaskExecutor for FakeExecutor {
    fn launch(&self, request: &LaunchRequest) -> Result<(), ExecutorFailure> {
        let pending: Value = serde_json::from_slice(
            &std::fs::read(self.data_dir.join("task-submissions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            pending["receipts"][&request.task_id]["state"], "pending",
            "receipt is durable before execution"
        );
        self.launches.lock().unwrap().push(request.clone());
        if self.fail.load(Ordering::SeqCst) {
            Err(ExecutorFailure::Spawn {
                reason: "fake partial execution".into(),
            })
        } else {
            Ok(())
        }
    }
    fn resume(&self, _: &ResumeRequest) -> Result<(), ExecutorFailure> {
        panic!("no resume")
    }
    fn cancel(&self, _: &str, _: &CancelTarget) -> Result<(), ExecutorFailure> {
        panic!("no cancel")
    }
    fn complete(&self, _: &str, _: &str) -> Result<(), ExecutorFailure> {
        panic!("no complete")
    }
    fn reset(&self, _: &str) -> Result<ResetOutcome, ExecutorFailure> {
        panic!("no reset")
    }
}

struct Fixture {
    root: PathBuf,
    data: PathBuf,
    socket: PathBuf,
    token: String,
    executor: Arc<FakeExecutor>,
    running: Option<RunningHost>,
}
impl Fixture {
    async fn start() -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "an-sub-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let data = root.join("data");
        let socket = data.join("host.sock");
        let started = run_startup_with_secrets(
            &aeroric_host::startup::HostOptions {
                paths: AppPaths::with_data_dir(root.join("synthetic-home"), Some(&data)),
                socket_path: socket.clone(),
                ready_fd: None,
            },
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "native-submission-fixture".into(),
                version: "test".into(),
            },
            &Local,
            "fixture",
            "fixture",
            Arc::new(MemorySecrets::default()),
        )
        .await
        .unwrap();
        aeroric_core::storage::save_settings(
            &started.context,
            json!({"custom_agents":[{
                "id":"custom-fixture", "label":"Fixture", "path":root.join("never-executed"),
                "codex_like":true, "family":"codex", "models":["fixture/model"]
            }]})
            .as_object()
            .unwrap(),
        )
        .unwrap();
        ProjectService::new(started.context.clone())
            .open_local(root.join("project").to_str().unwrap(), "fixture-project", 1)
            .unwrap();
        let executor = Arc::new(FakeExecutor {
            data_dir: data.clone(),
            ..Default::default()
        });
        let families = Arc::new(SettingsFamiliesCli::new(started.context.clone()));
        let tasks = Arc::new(
            TaskService::new(
                started.context.clone(),
                families.clone(),
                executor.clone(),
                Arc::new(NoLiveRuns),
            )
            .unwrap(),
        );
        let registry = Arc::new(TaskRegistry::default());
        let terminal = Arc::new(TerminalService::default());
        let log = Arc::new(EventLog::default());
        let unused_pty = CliPtyExecutor::new(
            started.context.clone(),
            registry.clone(),
            terminal.clone(),
            log.clone(),
        );
        unused_pty.attach(&tasks);
        let services = HostServices::with_tasks(
            started.token.clone(),
            log,
            started.context.clone(),
            tasks,
            families,
            unused_pty,
            terminal,
            registry,
        );
        assert!(services
            .capabilities
            .iter()
            .any(|capability| capability.name == "task.submissions" && capability.enabled));
        let token = std::fs::read_to_string(data.join("host.token"))
            .unwrap()
            .trim()
            .to_string();
        let running = Some(aeroric_host::startup::serve(started, services));
        Self {
            root,
            data,
            socket,
            token,
            executor,
            running,
        }
    }
    async fn connect(
        &self,
    ) -> (
        BufReader<tokio::net::unix::OwnedReadHalf>,
        tokio::net::unix::OwnedWriteHalf,
    ) {
        let stream = UnixStream::connect(&self.socket).await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        assert!(matches!(
            Harness::handshake(&mut read, &mut write, &self.token, "fixture").await,
            Frame::Success(_)
        ));
        (read, write)
    }
    async fn call(&self, method: &str, params: Value) -> Frame {
        let (mut read, mut write) = self.connect().await;
        Harness::call(
            &mut read,
            &mut write,
            &Harness::request("fixture-request", method, params),
        )
        .await
    }
    async fn request(&self) -> SubmissionRequest {
        let directory: ConfigurationCatalog = serde_json::from_value(success(
            self.call(
                "agent.configurations",
                json!({"projectId":"fixture-project"}),
            )
            .await,
        ))
        .unwrap();
        let wire = serde_json::to_string(&directory).unwrap();
        assert!(!wire.contains("FAKE-SECRET"));
        assert!(!wire.contains(self.root.to_str().unwrap()));
        let mut request = aeroric_protocol::submission::fixture_request();
        request.configuration_version = directory.configurations.last().unwrap().version.clone();
        request.fingerprint = aeroric_core::tasks::submissions::fingerprint(&request);
        request
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
        // Only this fixture's disposable root, never a host/guest or real home.
        std::fs::remove_dir_all(&self.root).ok();
    }
}
fn success(frame: Frame) -> Value {
    match frame {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success: {other:?}"),
    }
}

#[tokio::test]
async fn actual_uds_adopts_custom_identity_model_permission_and_reuses_receipt() {
    let fixture = Fixture::start().await;
    let request = fixture.request().await;
    let first = success(fixture.call("task.run", json!(request)).await);
    let receipt: SubmissionReceipt = serde_json::from_value(first.clone()).unwrap();
    assert_eq!(receipt.state, SubmissionState::Accepted);
    assert_eq!(
        receipt.task_id.as_deref(),
        Some(request.submission_id.as_str())
    );
    assert_eq!(
        success(fixture.call("task.run", json!(request)).await),
        first
    );
    let lookup = success(
        fixture
            .call(
                "task.submission.get",
                json!({
                    "submissionId":request.submission_id,"fingerprint":request.fingerprint
                }),
            )
            .await,
    );
    assert_eq!(lookup["receipt"], first);
    let launches = fixture.executor.launches.lock().unwrap();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].agent, "custom-fixture");
    assert_eq!(launches[0].selected_model.as_deref(), Some("fixture/model"));
    assert_eq!(launches[0].permission_mode_wire, "ask");
    assert_eq!(launches[0].prompt, request.prompt);
    // A legacy entry is not an agent configuration: no gateway pinning.
    assert_eq!(launches[0].agent_config_id, None);
}

#[tokio::test]
async fn agent_configuration_entry_pins_the_task_to_that_configuration() {
    let fixture = Fixture::start().await;
    std::fs::write(
        fixture.data.join("agent-connections.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "connections": [{
                "id": "conn", "label": "Conn", "kind": "api_key", "vendor": "other",
                "base_url": "https://fixture.invalid/v1", "protocols": ["messages"],
                "models": [{"upstream_id": "up", "mapped_name": "mapped-model"}]
            }],
            "configs": [{
                "id": "cfg", "label": "Gateway Claude", "harness": "claude",
                "target": {"kind": "connection", "connection_id": "conn"}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let directory: ConfigurationCatalog = serde_json::from_value(success(
        fixture
            .call(
                "agent.configurations",
                json!({"projectId":"fixture-project"}),
            )
            .await,
    ))
    .unwrap();
    let entry = directory
        .configurations
        .iter()
        .find(|item| item.id == "agent-config:cfg")
        .expect("configuration offered");
    assert!(
        entry.blocked_reasons.is_empty(),
        "{:?}",
        entry.blocked_reasons
    );
    assert!(!serde_json::to_string(&directory)
        .unwrap()
        .contains("fixture.invalid"));

    let mut request = aeroric_protocol::submission::fixture_request();
    request.configuration_id = entry.id.clone();
    request.configuration_version = entry.version.clone();
    request.agent = "claude".into();
    request.permission_agent = "claude".into();
    request.selected_model = Some("mapped-model".into());
    request.fingerprint = aeroric_core::tasks::submissions::fingerprint(&request);
    let receipt: SubmissionReceipt =
        serde_json::from_value(success(fixture.call("task.run", json!(request)).await)).unwrap();
    assert_eq!(receipt.state, SubmissionState::Accepted, "{receipt:?}");
    let launches = fixture.executor.launches.lock().unwrap();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].agent, "claude");
    assert_eq!(launches[0].agent_config_id.as_deref(), Some("cfg"));
    assert_eq!(launches[0].selected_model.as_deref(), Some("mapped-model"));
}

#[tokio::test]
async fn lost_reply_and_concurrent_retries_do_not_create_another_run() {
    let fixture = Arc::new(Fixture::start().await);
    let request = fixture.request().await;
    let (read, mut write) = fixture.connect().await;
    write
        .write_all(
            encode_line(&Harness::request("lost", "task.run", json!(request)))
                .unwrap()
                .as_bytes(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture.executor.launches.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(read);
    drop(write); // deliberately do not consume the positive reply
    let mut calls = Vec::new();
    for _ in 0..8 {
        let fixture = fixture.clone();
        let request = request.clone();
        calls.push(tokio::spawn(async move {
            success(fixture.call("task.run", json!(request)).await)
        }));
    }
    for call in calls {
        assert_eq!(call.await.unwrap()["state"], "accepted");
    }
    assert_eq!(fixture.executor.launches.lock().unwrap().len(), 1);
    let lookup = success(
        fixture
            .call(
                "task.submission.get",
                json!({
                    "submissionId":request.submission_id,"fingerprint":request.fingerprint
                }),
            )
            .await,
    );
    assert_eq!(lookup["receipt"]["state"], "accepted");
}

#[tokio::test]
async fn option_refusal_stale_version_conflicts_and_missing_lookup_are_distinct() {
    let fixture = Fixture::start().await;
    let mut request = fixture.request().await;
    request.attachments = vec!["/synthetic/never-read".into()];
    request.worktree = Some("/synthetic/never-created".into());
    request.team = true;
    request.review = true;
    request.evolution = true;
    request.recipe_id = Some("fixture".into());
    request.fingerprint = aeroric_core::tasks::submissions::fingerprint(&request);
    let receipt: SubmissionReceipt =
        serde_json::from_value(success(fixture.call("task.run", json!(request)).await)).unwrap();
    assert_eq!(receipt.state, SubmissionState::RejectedBeforeSideEffect);
    for option in [
        "附件",
        "工作目录",
        "团队协作",
        "独立审查",
        "任务工具进化",
        "工作配方",
    ] {
        assert!(receipt.reasons.iter().any(|reason| reason.contains(option)));
    }
    assert!(fixture.executor.launches.lock().unwrap().is_empty());
    request.prompt.push_str("changed");
    request.fingerprint = aeroric_core::tasks::submissions::fingerprint(&request);
    let Frame::Failure(conflict) = fixture.call("task.run", json!(request)).await else {
        panic!("expected conflict")
    };
    assert_eq!(conflict.error.code, ErrorCode::RevisionConflict);
    request.submission_id = "new-attempt".into();
    request.configuration_version = "stale-version".into();
    request.fingerprint = aeroric_core::tasks::submissions::fingerprint(&request);
    let receipt = success(fixture.call("task.run", json!(request)).await);
    assert_eq!(receipt["state"], "rejected_before_side_effect");
    assert!(receipt["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("版本")));
    let lookup = success(
        fixture
            .call(
                "task.submission.get",
                json!({
                    "submissionId":"never-submitted","fingerprint":request.fingerprint
                }),
            )
            .await,
    );
    assert!(lookup["receipt"].is_null());
    assert!(fixture.executor.launches.lock().unwrap().is_empty());
}

#[tokio::test]
async fn execution_error_and_malformed_storage_preserve_unknown_without_replay() {
    let fixture = Fixture::start().await;
    fixture.executor.fail.store(true, Ordering::SeqCst);
    let request = fixture.request().await;
    let first = success(fixture.call("task.run", json!(request)).await);
    assert_eq!(first["state"], "unknown");
    assert_eq!(
        success(fixture.call("task.run", json!(request)).await),
        first
    );
    assert_eq!(fixture.executor.launches.lock().unwrap().len(), 1);
    let path = fixture.data.join("task-submissions.json");
    std::fs::write(&path, b"{corrupt").unwrap();
    assert!(matches!(
        fixture.call("task.run", json!(request)).await,
        Frame::Failure(_)
    ));
    assert_eq!(std::fs::read(path).unwrap(), b"{corrupt");
    assert_eq!(fixture.executor.launches.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn unconsumed_profile_connection_and_options_are_refused_before_execution() {
    let fixture = Fixture::start().await;
    let path = fixture.data.join("settings.json");
    let baseline: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (key, value) in [
        ("api_key", json!("FAKE-SECRET-NOT-A-CREDENTIAL")),
        ("base_url", json!("https://fixture.invalid")),
        ("enable_1m_context", json!(true)),
        ("disable_artifact_tool", json!(true)),
        ("enable_chat_completions_proxy", json!(true)),
        ("bridge_python_path", json!("/synthetic/never-executed")),
    ] {
        let mut settings = baseline.clone();
        settings["custom_agents"][0][key] = value;
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let mut request = fixture.request().await;
        request.submission_id = format!("blocked-{key}");
        let receipt = success(fixture.call("task.run", json!(request)).await);
        assert_eq!(receipt["state"], "rejected_before_side_effect", "{key}");
        assert!(receipt["taskId"].is_null());
        assert!(fixture.executor.launches.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn unavailable_runtime_and_malformed_settings_do_not_become_empty_success() {
    let harness = Harness::start("native-catalog-readonly").await;
    let (mut read, mut write) = harness.connect().await;
    assert!(matches!(
        Harness::handshake(&mut read, &mut write, &harness.token, "fixture").await,
        Frame::Success(_)
    ));
    // This harness does not register a task executor. Its catalog remains
    // explicitly unavailable even with a stored custom profile.
    std::fs::write(
        harness.data_dir.join("projects.json"),
        json!([{
            "id":"p","path":harness.home.join("synthetic-project")
        }])
        .to_string(),
    )
    .unwrap();
    let result = success(
        Harness::call(
            &mut read,
            &mut write,
            &Harness::request("catalog", "agent.configurations", json!({"projectId":"p"})),
        )
        .await,
    );
    assert!(result["configurations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|config| {
            config["blockedReasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("没有装配"))
        }));
    let settings = harness.data_dir.join("settings.json");
    std::fs::write(&settings, br#"{"custom_agents":[{"id":"bad","path":42}]}"#).unwrap();
    let before = std::fs::read(&settings).unwrap();
    assert!(matches!(
        Harness::call(
            &mut read,
            &mut write,
            &Harness::request("bad", "agent.configurations", json!({"projectId":"p"}))
        )
        .await,
        Frame::Failure(_)
    ));
    assert_eq!(std::fs::read(settings).unwrap(), before);
}
