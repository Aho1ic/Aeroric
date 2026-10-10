//! Slice 21 regressions at the real project-list command boundary.
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

use super::Project;
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;

const PROJECT_METHOD: &str = "project.load_projects";
const TOKEN: &str = "project-list-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("alp-{tag}-{}-{nonce}", std::process::id()));
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
async fn invoke(app: &MockApp) -> Result<Vec<Project>, String> {
    super::commands::load_projects(app.handle().clone()).await
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
            if method == PROJECT_METHOD {
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

    fn assert_project_request(&self) {
        assert!(
            self.host
                .methods()
                .iter()
                .any(|method| method == PROJECT_METHOD),
            "the real command must reach project.load_projects"
        );
        assert_eq!(self.params.lock().as_slice(), &[json!({})]);
    }
}

#[tokio::test]
async fn embedded_preserves_missing_full_and_corrupt_project_reads() {
    // Reverse local drop order keeps the app/lease ahead of the home guard and root.
    let root = ScratchRoot::new("embedded");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let data_dir = root.data_dir();
    let projects_file = data_dir.join("projects.json");
    let gateway_dir = root.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "project-list-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);

    assert!(
        !data_dir.exists(),
        "gateway startup must use its separate root"
    );
    assert!(invoke(&app).await.expect("missing project list").is_empty());
    assert!(data_dir.is_dir());
    assert!(!projects_file.exists());
    #[cfg(unix)]
    assert_mode(&data_dir, 0o700);

    let expected = json!([
        {
            "id": "local-b",
            "name": "Local B",
            "path": "/fictional/local-b",
            "location": {"kind": "local", "path": "/fictional/local-b"},
            "branch": "feature/local-b",
            "lastOpenedAt": 1700000000002_i64,
            "orderIndex": 8,
            "group": "Local group B",
            "hiddenFromRail": true,
            "pinned": true,
            "avatar": {"color": "blue", "emoji": "B", "label": "Local B"}
        },
        {
            "id": "local-a",
            "name": "Local A",
            "path": "/fictional/local-a",
            "location": {"kind": "local", "path": "/fictional/local-a"},
            "branch": null,
            "lastOpenedAt": 1700000000001_i64,
            "orderIndex": -1,
            "group": "Local group A",
            "hiddenFromRail": true,
            "pinned": true,
            "avatar": {"color": "green", "emoji": "A", "label": "Local A"}
        }
    ]);
    let raw = format!(
        "{}\n",
        serde_json::to_string_pretty(&expected).expect("handwritten project fixture")
    )
    .into_bytes();
    std::fs::write(&projects_file, &raw).expect("write local projects");
    #[cfg(unix)]
    {
        set_mode(&data_dir, 0o755);
        set_mode(&projects_file, 0o644);
        assert_mode(&data_dir, 0o755);
        assert_mode(&projects_file, 0o644);
    }

    let projects = invoke(&app).await.expect("complete local projects");
    assert_eq!(
        projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        vec!["local-b", "local-a"]
    );
    assert_eq!(serde_json::to_value(&projects).unwrap(), expected);
    assert_eq!(std::fs::read(&projects_file).unwrap(), raw);
    #[cfg(unix)]
    {
        assert_mode(&data_dir, 0o700);
        assert_mode(&projects_file, 0o600);
    }

    let corrupt = b"[{\"id\": \"unfinished\"";
    std::fs::write(&projects_file, corrupt).expect("write corrupt local fixture");
    assert!(
        invoke(&app).await.is_err(),
        "corrupt JSON must remain an error"
    );
    assert_eq!(std::fs::read(&projects_file).unwrap(), corrupt.to_vec());
}

#[tokio::test]
async fn external_preserves_host_order_duplicates_fields_and_local_bytes_and_modes() {
    let root = ScratchRoot::new("full");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let data_dir = root.data_dir();
    let projects_file = data_dir.join("projects.json");
    std::fs::create_dir(&data_dir).expect("local data directory");
    let local_raw = br#"[
  {"id":"local-only","name":"Local only","path":"/fictional/local-only","branch":null,"lastOpenedAt":1}
]
"#;
    std::fs::write(&projects_file, local_raw).expect("write distinct local project");
    #[cfg(unix)]
    {
        set_mode(&data_dir, 0o755);
        set_mode(&projects_file, 0o644);
        assert_mode(&data_dir, 0o755);
        assert_mode(&projects_file, 0o644);
    }

    let b = json!({
        "id": "b",
        "name": "Host-only B",
        "path": "/fictional/host-b",
        "location": {
            "kind": "ssh",
            "connectionId": "synthetic-connection",
            "remotePath": "/fictional/remote-b"
        },
        "branch": "feature/host-b",
        "lastOpenedAt": 1700000000020_i64,
        "orderIndex": 9,
        "group": "Host group B",
        "hiddenFromRail": true,
        "pinned": true,
        "avatar": {"color": "purple", "emoji": "B", "label": "Host B"}
    });
    let a = json!({
        "id": "a",
        "name": "Host-only A",
        "path": "/fictional/host-a",
        "location": {
            "kind": "wsl",
            "distribution": "synthetic-distribution",
            "linuxPath": "/fictional/linux-a"
        },
        "branch": null,
        "lastOpenedAt": 1700000000010_i64,
        "orderIndex": 0,
        "group": "Host group A",
        "hiddenFromRail": true,
        "pinned": true,
        "avatar": {"color": "orange", "emoji": "A", "label": "Host A"}
    });
    let expected = json!([b.clone(), a, b]);
    let mut host_projects = expected.clone();
    host_projects[0]["futureProjectField"] = json!({"synthetic": true});
    let rig = ExternalRig::new("lp-full", move || Some(success(host_projects.clone()))).await;

    let projects = invoke(&rig.app).await.expect("host project list");
    assert_eq!(
        projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a", "b"]
    );
    assert_eq!(serde_json::to_value(&projects).unwrap(), expected);
    rig.assert_project_request();
    assert_eq!(std::fs::read(&projects_file).unwrap(), local_raw.to_vec());
    #[cfg(unix)]
    {
        assert_mode(&data_dir, 0o755);
        assert_mode(&projects_file, 0o644);
    }
}

#[tokio::test]
async fn external_empty_projects_does_not_create_local_storage() {
    let root = ScratchRoot::new("empty");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let rig = ExternalRig::new("lp-empty", || Some(success(json!([])))).await;
    root.assert_local_absent();

    assert!(invoke(&rig.app).await.expect("empty host list").is_empty());
    rig.assert_project_request();
    root.assert_local_absent();
}

#[tokio::test]
async fn external_method_and_io_failures_do_not_fall_back_locally() {
    for (tag, code, message, expected_text) in [
        (
            "lp-unsupported",
            ErrorCode::UnsupportedCapability,
            "unknown method: project.load_projects",
            "协调重启",
        ),
        (
            "lp-io",
            ErrorCode::IoError,
            "synthetic project list IO failure",
            "synthetic project list IO failure",
        ),
    ] {
        // Each loop iteration releases SERIAL before the next root installs its guard.
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let rig = ExternalRig::new(tag, move || Some(failure(code.clone(), message))).await;
        root.assert_local_absent();

        let error = invoke(&rig.app)
            .await
            .expect_err("host failure must not fall back to local projects");
        assert!(error.contains(expected_text), "{tag}: {error}");
        rig.assert_project_request();
        root.assert_local_absent();
    }
}

#[tokio::test]
async fn external_disconnect_does_not_fall_back_locally() {
    let root = ScratchRoot::new("disconnect");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let rig = ExternalRig::new("lp-disconnect", || None).await;
    root.assert_local_absent();

    assert!(
        invoke(&rig.app).await.is_err(),
        "a disconnected host must not return a local list"
    );
    rig.assert_project_request();
    root.assert_local_absent();
}

#[tokio::test]
async fn external_malformed_payloads_are_errors_without_local_storage() {
    for (tag, payload) in [
        ("lp-null", Value::Null),
        ("lp-empty-wrapper", json!({"projects": []})),
        (
            "lp-full-wrapper",
            json!({
                "projects": [{
                    "id": "wrapped-valid",
                    "name": "Valid project in the intrinsic object wrapper",
                    "path": "/fictional/wrapped-valid",
                    "branch": null,
                    "lastOpenedAt": 1
                }],
                "futureWrapperMetadata": {"synthetic": true}
            }),
        ),
        ("lp-nested-empty", json!([[]])),
        (
            "lp-nested-list",
            json!([[{
                "id": "nested-valid",
                "name": "Valid project in a sequence wrapper",
                "path": "/fictional/nested-valid",
                "branch": null,
                "lastOpenedAt": 1
            }]]),
        ),
        ("lp-object", json!({})),
        ("lp-null-list", json!({"projects": null})),
        ("lp-text-list", json!("not an array")),
        ("lp-object-list", json!({"projects": {}})),
        (
            "lp-missing-id",
            json!([{
                "name": "Missing id",
                "path": "/fictional/no-id",
                "branch": null,
                "lastOpenedAt": 1
            }]),
        ),
        (
            "lp-missing-time",
            json!([{
                "id": "no-time",
                "name": "Missing timestamp",
                "path": "/fictional/no-time",
                "branch": null
            }]),
        ),
        (
            "lp-bad-id",
            json!([{
                "id": 42,
                "name": "Wrong id type",
                "path": "/fictional/bad-id",
                "branch": null,
                "lastOpenedAt": 1
            }]),
        ),
        (
            "lp-bad-time",
            json!([{
                "id": "bad-time",
                "name": "Wrong timestamp type",
                "path": "/fictional/bad-time",
                "branch": null,
                "lastOpenedAt": "not a timestamp"
            }]),
        ),
        (
            "lp-mixed",
            json!([
                {
                    "id": "valid",
                    "name": "Valid record",
                    "path": "/fictional/valid",
                    "branch": null,
                    "lastOpenedAt": 1
                },
                null
            ]),
        ),
    ] {
        // The root, guard, app and host are independent for every malformed reply.
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let rig = ExternalRig::new(tag, move || Some(success(payload.clone()))).await;
        root.assert_local_absent();

        let result = invoke(&rig.app).await;
        assert!(
            result.is_err(),
            "{tag}: malformed data must not become an empty or filtered list: {result:?}"
        );
        rig.assert_project_request();
        root.assert_local_absent();
    }
}

#[tokio::test]
async fn external_without_session_emits_no_project_request_or_local_io() {
    let root = ScratchRoot::new("no-session");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let rig = ExternalRig::new("lp-no-session", || Some(success(json!([])))).await;
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
        invoke(&rig.app).await.is_err(),
        "an External gateway without a session must not return local projects"
    );
    assert!(rig.params.lock().is_empty());
    assert!(!rig
        .host
        .methods()
        .iter()
        .any(|method| method == PROJECT_METHOD));
    root.assert_local_absent();
}
