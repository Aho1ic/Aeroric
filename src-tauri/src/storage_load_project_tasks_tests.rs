//! Slice 22 regressions at the registered project-task-list command boundary.
//!
//! Keep each scratch-home guard alive until its app and scripted host have dropped.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use super::Task;
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;

const PROJECT_TASKS_METHOD: &str = "project.load_project_tasks";
const TOKEN: &str = "project-task-list-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("alpt-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).expect("create scratch root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn data_dir(&self) -> PathBuf {
        self.path().join(".aeroric")
    }

    fn assert_local_absent(&self) {
        assert!(
            !self.data_dir().exists(),
            "External must not create the local .aeroric directory"
        );
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("set scratch permissions");
}

#[cfg(unix)]
fn assert_mode(path: &Path, expected: u32) {
    use std::os::unix::fs::PermissionsExt;
    let actual = std::fs::metadata(path)
        .expect("scratch metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(actual, expected, "permissions for {}", path.display());
}

struct AbsentProbe;

impl PlacementProbe for AbsentProbe {
    fn availability(&self, _data_dir: &Path) -> HostAvailability {
        HostAvailability::Absent
    }

    fn attach(&self, _data_dir: &Path) -> Result<(HostClient, bool), String> {
        panic!("an absent-host fixture must not attach")
    }
}

// Invoke the actual registered IPC command, not the retained synchronous helper.
async fn invoke(app: &MockApp, project_id: &str) -> Result<Vec<Task>, String> {
    super::commands::load_project_tasks(app.handle().clone(), project_id.to_owned()).await
}

struct ExternalRig {
    // Fields drop in declaration order: release the client before removing the host root.
    app: MockApp,
    host: ScriptedHost,
    params: Arc<Mutex<Vec<Value>>>,
}

impl ExternalRig {
    async fn new(tag: &str, reply: impl Fn() -> Option<Frame> + Send + Sync + 'static) -> Self {
        let params = Arc::new(Mutex::new(Vec::new()));
        let observed_params = Arc::clone(&params);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, request_params| {
            if method == PROJECT_TASKS_METHOD {
                observed_params.lock().push(request_params.clone());
                reply()
            } else {
                handshake(method, request_params)
            }
        });
        let host = ScriptedHost::start(tag, script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "tauri")
            .await
            .expect("authenticated scratch handshake");
        assert!(host.methods().iter().any(|method| method == "handshake"));
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);

        let app = tauri::test::mock_app();
        app.manage(gateway);
        Self { app, host, params }
    }

    fn assert_task_request(&self, project_id: &str) {
        assert!(
            self.host
                .methods()
                .iter()
                .any(|method| method == PROJECT_TASKS_METHOD),
            "the real command must reach project.load_project_tasks"
        );
        assert_eq!(
            self.params.lock().as_slice(),
            &[json!({"projectId": project_id})]
        );
    }
}

fn minimal_task(id: &str, project_id: &str) -> Value {
    json!({
        "id": id,
        "projectId": project_id,
        "prompt": "Synthetic task prompt",
        "agent": "claude",
        "permissionMode": "default",
        "status": "todo",
        "createdAt": 1700000000001_i64
    })
}

fn detailed_task(id: &str, project_id: &str) -> Value {
    json!({
        "id": id,
        "projectId": project_id,
        "name": "Synthetic task Δ",
        "prompt": "Synthetic completed task\nwith a second line",
        "agent": "claude",
        "permissionMode": "default",
        "selectedModel": "synthetic-model",
        "reasoningEffort": "high",
        "speed": "fast",
        "status": "done",
        "createdAt": 1700000000002_i64,
        "attentionRequestedAt": 1700000000010_i64,
        "completedAt": 1700000000020_i64,
        "claudeSessionId": "synthetic-session",
        "claudeSessionPath": "/fictional/session.jsonl",
        "sessionAgent": "claude",
        "sessionCodexLike": false,
        "sessionFamily": "claude",
        "launchFamily": "claude",
        "resumeFamily": "claude",
        "starred": true,
        "worktreePath": "/fictional/worktree",
        "worktreeBranch": "synthetic/task",
        "baseBranch": "main",
        "worktreeDiscarded": false,
        "additions": 3000000000_i64,
        "deletions": 2
    })
}

#[tokio::test]
async fn embedded_preserves_invalid_id_missing_full_corrupt_and_permission_behavior() {
    // Reverse local drop order keeps the app/lease ahead of the home guard and root.
    let root = ScratchRoot::new("embedded");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let data_dir = root.data_dir();
    let projects_dir = data_dir.join("projects");
    let project_id = "embedded-project";
    let project_dir = projects_dir.join(project_id);
    let tasks_file = project_dir.join("tasks.json");
    let gateway_dir = root.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "project-task-list-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);

    assert!(
        !data_dir.exists(),
        "gateway startup must use its separate root"
    );
    assert!(
        invoke(&app, "../escape").await.is_err(),
        "Embedded must retain the old invalid-ID rejection"
    );
    // The old helper creates the common directories BEFORE validating the ID.
    assert!(data_dir.is_dir());
    assert!(projects_dir.is_dir());
    assert!(std::fs::read_dir(&projects_dir).unwrap().next().is_none());
    assert!(!data_dir.join("escape").exists());
    #[cfg(unix)]
    {
        assert_mode(&data_dir, 0o700);
        assert_mode(&projects_dir, 0o700);
    }

    assert!(invoke(&app, project_id)
        .await
        .expect("missing project task list")
        .is_empty());
    assert!(project_dir.is_dir());
    assert!(!tasks_file.exists());
    #[cfg(unix)]
    assert_mode(&project_dir, 0o700);

    let records = json!([
        detailed_task("local-b", "stored-local-b"),
        minimal_task("local-a", "stored-local-a")
    ]);
    let expected: Vec<Task> =
        serde_json::from_value(records.clone()).expect("handwritten complete Task fixtures");
    let raw = format!(
        "{}\n",
        serde_json::to_string_pretty(&records).expect("handwritten task list")
    )
    .into_bytes();
    std::fs::write(&tasks_file, &raw).expect("write local tasks");
    #[cfg(unix)]
    {
        for directory in [&data_dir, &projects_dir, &project_dir] {
            set_mode(directory, 0o755);
            assert_mode(directory, 0o755);
        }
        set_mode(&tasks_file, 0o644);
        assert_mode(&tasks_file, 0o644);
    }

    let tasks = invoke(&app, project_id)
        .await
        .expect("complete local tasks");
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["local-b", "local-a"]
    );
    assert_eq!(
        serde_json::to_value(&tasks).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(std::fs::read(&tasks_file).unwrap(), raw);
    #[cfg(unix)]
    {
        for directory in [&data_dir, &projects_dir, &project_dir] {
            assert_mode(directory, 0o700);
        }
        assert_mode(&tasks_file, 0o600);
    }

    let corrupt = b"[{\"id\": \"unfinished\"";
    std::fs::write(&tasks_file, corrupt).expect("write corrupt local task fixture");
    #[cfg(unix)]
    set_mode(&tasks_file, 0o644);
    assert!(
        invoke(&app, project_id).await.is_err(),
        "corrupt JSON must remain an error"
    );
    assert_eq!(std::fs::read(&tasks_file).unwrap(), corrupt.to_vec());
    #[cfg(unix)]
    assert_mode(&tasks_file, 0o600);
}

#[tokio::test]
async fn external_preserves_host_order_duplicates_stored_owner_fields_and_local_bytes_and_modes() {
    let root = ScratchRoot::new("full");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project_id = "requested-project";
    let data_dir = root.data_dir();
    let projects_dir = data_dir.join("projects");
    let project_dir = projects_dir.join(project_id);
    let tasks_file = project_dir.join("tasks.json");
    std::fs::create_dir_all(&project_dir).expect("local project task directory");
    let local_raw = br#"[
  {"id":"local-only","projectId":"requested-project","prompt":"Local only","agent":"claude","permissionMode":"default","status":"todo","createdAt":1}
]
"#;
    std::fs::write(&tasks_file, local_raw).expect("write distinct local task");
    #[cfg(unix)]
    {
        for directory in [&data_dir, &projects_dir, &project_dir] {
            set_mode(directory, 0o755);
            assert_mode(directory, 0o755);
        }
        set_mode(&tasks_file, 0o644);
        assert_mode(&tasks_file, 0o644);
    }

    let b = detailed_task("b", "stored-owner-b");
    let a = minimal_task("a", "stored-owner-a");
    let records = json!([b.clone(), a, b]);
    let expected: Vec<Task> =
        serde_json::from_value(records.clone()).expect("handwritten host Task fixtures");
    let mut host_tasks = records;
    host_tasks[0]["futureTaskField"] = json!({"synthetic": true, "order": [3, 1, 3]});
    let rig = ExternalRig::new("lpt-full", move || Some(success(host_tasks.clone()))).await;

    let tasks = invoke(&rig.app, project_id)
        .await
        .expect("host project task list");
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a", "b"]
    );
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.project_id.as_str())
            .collect::<Vec<_>>(),
        vec!["stored-owner-b", "stored-owner-a", "stored-owner-b"]
    );
    let projected = serde_json::to_value(&tasks).unwrap();
    assert_eq!(projected, serde_json::to_value(&expected).unwrap());
    assert!(
        projected[0].get("futureTaskField").is_none(),
        "unknown fields follow the existing Task projection, not a new DTO"
    );
    rig.assert_task_request(project_id);
    assert_eq!(std::fs::read(&tasks_file).unwrap(), local_raw.to_vec());
    #[cfg(unix)]
    {
        for directory in [&data_dir, &projects_dir, &project_dir] {
            assert_mode(directory, 0o755);
        }
        assert_mode(&tasks_file, 0o644);
    }
}

#[tokio::test]
async fn external_empty_tasks_does_not_create_local_storage() {
    let root = ScratchRoot::new("empty");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project_id = "empty-project";
    let rig = ExternalRig::new("lpt-empty", || Some(success(json!([])))).await;
    root.assert_local_absent();

    assert!(invoke(&rig.app, project_id)
        .await
        .expect("empty host task list")
        .is_empty());
    rig.assert_task_request(project_id);
    root.assert_local_absent();
}

#[tokio::test]
async fn external_preserves_exact_padded_and_invalid_project_id_parameters() {
    for (tag, project_id, valid) in [
        ("lpt-padded", "  padded-project  ", true),
        ("lpt-empty-id", "", false),
        ("lpt-space-id", " \t ", false),
        ("lpt-invalid-id", "../escape", false),
    ] {
        // Each iteration drops its app/host and SERIAL guard before installing the next.
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let rig = ExternalRig::new(tag, move || {
            if valid {
                Some(success(json!([])))
            } else {
                Some(failure(
                    ErrorCode::InvalidArgument,
                    "synthetic invalid project ID",
                ))
            }
        })
        .await;
        root.assert_local_absent();

        let result = invoke(&rig.app, project_id).await;
        if valid {
            assert!(result.expect("padded valid ID reaches the host").is_empty());
        } else {
            let error = result.expect_err("the host rejects invalid IDs without local IO");
            assert!(
                error.contains("synthetic invalid project ID"),
                "{tag}: {error}"
            );
        }
        rig.assert_task_request(project_id);
        root.assert_local_absent();
    }
}

#[tokio::test]
async fn external_method_and_io_failures_do_not_fall_back_locally() {
    for (tag, code, message, expected_text) in [
        (
            "lpt-unsupported",
            ErrorCode::UnsupportedCapability,
            "unknown method: project.load_project_tasks",
            "协调重启",
        ),
        (
            "lpt-io",
            ErrorCode::IoError,
            "synthetic project task list IO failure",
            "synthetic project task list IO failure",
        ),
    ] {
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let project_id = "failed-project";
        let rig = ExternalRig::new(tag, move || Some(failure(code.clone(), message))).await;
        root.assert_local_absent();

        let error = invoke(&rig.app, project_id)
            .await
            .expect_err("host failure must not fall back to local tasks");
        assert!(error.contains(expected_text), "{tag}: {error}");
        rig.assert_task_request(project_id);
        root.assert_local_absent();
    }
}

#[tokio::test]
async fn external_disconnect_does_not_fall_back_locally() {
    let root = ScratchRoot::new("disconnect");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project_id = "disconnected-project";
    let rig = ExternalRig::new("lpt-disconnect", || None).await;
    root.assert_local_absent();

    assert!(
        invoke(&rig.app, project_id).await.is_err(),
        "a disconnected host must not return a local task list"
    );
    rig.assert_task_request(project_id);
    root.assert_local_absent();
}

#[tokio::test]
async fn external_malformed_payloads_are_errors_without_partial_results_or_local_storage() {
    let valid = minimal_task("host-valid", "stored-owner");
    let mut cases: Vec<(String, Value)> = [
        ("lpt-null", Value::Null),
        ("lpt-object", json!({})),
        ("lpt-single-record", valid.clone()),
        ("lpt-empty-wrapper", json!({"tasks": []})),
        (
            "lpt-full-wrapper",
            json!({
                "tasks": [valid.clone()],
                "futureWrapperMetadata": {"synthetic": true}
            }),
        ),
        ("lpt-projects-wrapper", json!({"projects": []})),
        ("lpt-text-list", json!("not an array")),
        ("lpt-null-item", json!([null])),
        ("lpt-number-item", json!([42])),
        ("lpt-text-item", json!(["not a task"])),
        ("lpt-nested-empty", json!([[]])),
        ("lpt-nested-list", json!([[valid.clone()]])),
        ("lpt-empty-record", json!([{}])),
        ("lpt-mixed-null", json!([valid.clone(), null])),
    ]
    .into_iter()
    .map(|(tag, payload)| (tag.to_owned(), payload))
    .collect();

    for field in [
        "id",
        "projectId",
        "prompt",
        "agent",
        "permissionMode",
        "status",
        "createdAt",
    ] {
        let mut missing = valid.clone();
        assert!(missing.as_object_mut().unwrap().remove(field).is_some());
        cases.push((format!("lpt-missing-{field}"), json!([missing])));

        let mut wrong_type = valid.clone();
        wrong_type[field] = if field == "createdAt" {
            json!("not a timestamp")
        } else {
            json!(42)
        };
        cases.push((format!("lpt-type-{field}"), json!([wrong_type])));
    }
    let mut missing_id = valid.clone();
    missing_id.as_object_mut().unwrap().remove("id");
    cases.push(("lpt-mixed-missing".to_owned(), json!([valid, missing_id])));

    for (tag, payload) in cases {
        // No nested scratch-home guards: each malformed reply has an independent lifetime.
        let root = ScratchRoot::new(&tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let project_id = "bad-reply-project";
        let rig = ExternalRig::new(&tag, move || Some(success(payload.clone()))).await;
        root.assert_local_absent();

        let result = invoke(&rig.app, project_id).await;
        assert!(
            result.is_err(),
            "{tag}: malformed data must not become an empty or filtered task list: {result:?}"
        );
        rig.assert_task_request(project_id);
        root.assert_local_absent();
    }
}

#[tokio::test]
async fn external_without_session_emits_no_task_request_or_local_io() {
    let root = ScratchRoot::new("no-session");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project_id = "no-session-project";
    let rig = ExternalRig::new("lpt-no-session", || Some(success(json!([])))).await;
    root.assert_local_absent();
    {
        let gateway = rig.app.state::<CoreGateway>();
        let session = gateway.host().lock().await.take();
        assert!(
            session.is_some(),
            "fixture starts with an authenticated session"
        );
        drop(session);
    }

    assert!(
        invoke(&rig.app, project_id).await.is_err(),
        "an External gateway without a session must not return local tasks"
    );
    assert!(rig.params.lock().is_empty());
    assert!(
        rig.host
            .methods()
            .iter()
            .all(|method| method == "handshake"),
        "removing the session must prevent any query after the authenticated handshake"
    );
    root.assert_local_absent();
}
