//! Actual registered command, with observable scratch-only local discovery.
use std::path::Path;
use std::sync::Arc;

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;

#[path = "../../../core/crates/aeroric-core/tests/common/conda_fixture.rs"]
mod conda_fixture;
#[path = "../../../core/crates/aeroric-core/tests/common/scratch.rs"]
mod scratch;
use conda_fixture::FakeConda;
use scratch::Scratch;

type MockApp = tauri::App<tauri::test::MockRuntime>;
const METHOD: &str = "infra.detect_conda_environments";
const TOKEN: &str = "scratch-conda-token";

struct Absent;
impl PlacementProbe for Absent {
    fn availability(&self, _: &Path) -> HostAvailability {
        HostAvailability::Absent
    }
    fn attach(&self, _: &Path) -> Result<(HostClient, bool), String> {
        panic!("absent fixture must not attach")
    }
}

fn embedded(root: &Scratch, probe: super::CondaProbe) -> MockApp {
    let data = root.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(root.path(), Some(&data)),
        "conda-test",
        &Absent,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app.manage(probe);
    app
}

struct External {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    fake: FakeConda,
}

impl External {
    async fn new(tag: &str, reply: impl Fn() -> Option<Frame> + Send + Sync + 'static) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if method == METHOD {
                observed.lock().push((method.to_owned(), params.clone()));
                reply()
            } else {
                handshake(method, params)
            }
        });
        let host = ScriptedHost::start(tag, script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "tauri").await.unwrap();
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);
        let fake = FakeConda::new(&host.data_dir.join("local must not run"));
        let app = tauri::test::mock_app();
        app.manage(gateway);
        // Safe even before the forwarding fix: a regression can only run this
        // owned script, and its marker makes local execution observable.
        app.manage(fake.probe());
        Self {
            app,
            host,
            calls,
            fake,
        }
    }

    fn assert_forwarded_only(&self) {
        assert_eq!(*self.calls.lock(), vec![(METHOD.to_string(), json!({}))]);
        assert!(self.host.methods().iter().any(|method| method == METHOD));
        assert!(
            !self.fake.marker.exists(),
            "External must never run local Conda"
        );
    }
}

async fn call(app: &MockApp) -> Result<Value, String> {
    let result = super::detect_conda_environments(app.handle().clone()).await?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

#[tokio::test]
async fn embedded_command_runs_the_shared_process_and_parser() {
    let root = Scratch::new("conda-desktop");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let fake = FakeConda::new(&root.path().join("local"));
    let app = embedded(&root, fake.probe());
    assert_eq!(call(&app).await.unwrap(), fake.wire);
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "env list --json\n"
    );
}

#[tokio::test]
async fn embedded_unavailable_tool_returns_empty_array() {
    let root = Scratch::new("conda-desktop-empty");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let fake = FakeConda::new(&root.path().join("local"));
    let mut probe = fake.probe();
    probe.candidates = Some(vec![root.path().join("unavailable")]);
    let app = embedded(&root, probe);
    assert_eq!(call(&app).await.unwrap(), json!([]));
    assert!(!fake.marker.exists());
}

#[tokio::test]
async fn external_success_is_host_only_including_empty_array() {
    let root = Scratch::new("conda-desktop-external");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    for expected in [
        json!([{"name":"宿主", "path":"/host only", "pythonPath":"/host only/bin/python"}]),
        json!([]),
    ] {
        let reply = expected.clone();
        let external = External::new("conda-ok", move || Some(success(reply.clone()))).await;
        assert_eq!(call(&external.app).await.unwrap(), expected);
        external.assert_forwarded_only();
    }
}

#[tokio::test]
async fn external_failures_and_disconnect_never_fall_back() {
    let root = Scratch::new("conda-desktop-fail");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    for code in [
        ErrorCode::UnsupportedCapability,
        ErrorCode::IoError,
        ErrorCode::InvalidArgument,
    ] {
        let external = External::new("conda-fail", move || {
            Some(failure(code.clone(), "fixture refusal"))
        })
        .await;
        assert!(call(&external.app)
            .await
            .unwrap_err()
            .contains("fixture refusal"));
        external.assert_forwarded_only();
    }
    let external = External::new("conda-eof", || None).await;
    assert!(call(&external.app).await.is_err());
    external.assert_forwarded_only();
}

#[tokio::test]
async fn external_malformed_replies_are_errors_not_local_results() {
    let root = Scratch::new("conda-desktop-bad");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    for reply in [
        Value::Null,
        json!({}),
        json!({"environments":[]}),
        json!([null]),
        json!([{"name":"bad","path":"/host"}]),
        json!([{"name":"bad","path":"/host","pythonPath":7}]),
    ] {
        let external = External::new("conda-bad", move || Some(success(reply.clone()))).await;
        assert!(call(&external.app).await.is_err());
        external.assert_forwarded_only();
    }
}

#[tokio::test]
async fn external_without_session_cannot_probe_locally() {
    let root = Scratch::new("conda-desktop-offline");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let external = External::new("conda-off", || Some(success(json!([])))).await;
    external
        .app
        .state::<CoreGateway>()
        .host()
        .lock()
        .await
        .take();
    assert!(call(&external.app)
        .await
        .unwrap_err()
        .contains("没有可用的宿主连接"));
    assert!(external.calls.lock().is_empty());
    assert!(!external.fake.marker.exists());
}

#[tokio::test]
async fn missing_gateway_does_not_guess_local_placement() {
    let root = Scratch::new("conda-no-gateway");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let fake = FakeConda::new(root.path());
    let app = tauri::test::mock_app();
    app.manage(fake.probe());
    assert!(call(&app).await.unwrap_err().contains("网关未安装"));
    assert!(!fake.marker.exists());
}
