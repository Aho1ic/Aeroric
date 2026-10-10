//! Slice 19 regressions at the real Tauri command boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus};
use serde_json::{json, Value};
use tauri::Manager;

use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::run_registry::RunKey;
use crate::TaskManager;

const ACTIVE_METHOD: &str = "terminal.get_active_task_ids";
const TOKEN: &str = "active-task-ids-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("aat-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).expect("create scratch root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
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

#[derive(Debug)]
struct InertChild;

impl ChildKiller for InertChild {
    fn kill(&mut self) -> std::io::Result<()> {
        panic!("listing task ids must not kill a child")
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        panic!("listing task ids must not clone a child killer")
    }
}

impl Child for InertChild {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        panic!("listing task ids must not poll a child")
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        panic!("listing task ids must not wait for a child")
    }

    fn process_id(&self) -> Option<u32> {
        panic!("listing task ids must not inspect a child pid")
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        panic!("listing task ids must not inspect a child handle")
    }
}

fn insert_child(manager: &TaskManager, run: RunKey) {
    let child: Box<dyn Child + Send + Sync> = Box::new(InertChild);
    assert!(
        manager
            .child_handles
            .lock()
            .insert(run, Arc::new(Mutex::new(child)))
            .is_none(),
        "fixture run keys must be distinct"
    );
}

// Call the actual command with the same managed state and runtime as production.
async fn invoke(app: &MockApp) -> Result<Vec<String>, String> {
    crate::pty::get_active_task_ids(app.handle().clone(), app.state::<TaskManager>()).await
}

struct ExternalRig {
    // Drop the client-bearing app before ScriptedHost cleans its scratch directory.
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
            if method == ACTIVE_METHOD {
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
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);

        let app = tauri::test::mock_app();
        let manager = TaskManager::default();
        insert_child(&manager, RunKey::minted("local-only", 1));
        app.manage(manager);
        app.manage(gateway);
        Self { app, host, params }
    }

    fn assert_active_request(&self) {
        assert!(
            self.host
                .methods()
                .iter()
                .any(|method| method == ACTIVE_METHOD),
            "the real command must reach the existing host method"
        );
        assert_eq!(self.params.lock().as_slice(), &[json!({})]);
    }
}

#[tokio::test]
async fn embedded_preserves_child_duplicates_and_ignores_registry_only_runs() {
    // Declared first so the managed gateway/lease drops before directory cleanup.
    let scratch = ScratchRoot::new("embedded");
    let app = tauri::test::mock_app();
    let manager = TaskManager::default();
    insert_child(&manager, RunKey::minted("child-a", 1));
    insert_child(&manager, RunKey::minted("child-a", 2));
    insert_child(&manager, RunKey::minted("child-b", 1));
    insert_child(&manager, RunKey::unminted("shell:project:1:1"));
    manager.runs.register("registry-only", 1);
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(scratch.path(), Some(scratch.path())),
        "active-task-ids-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    app.manage(manager);
    app.manage(gateway);

    let mut ids = invoke(&app).await.expect("embedded child ids");
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "child-a".to_string(),
            "child-a".to_string(),
            "child-b".to_string(),
        ]
    );
}

#[tokio::test]
async fn external_uses_host_ids_and_empty_params() {
    let rig = ExternalRig::new("ati-ok", || Some(success(json!(["host-only"])))).await;

    let ids = invoke(&rig.app).await.expect("host task ids");
    assert_eq!(ids, vec!["host-only".to_string()]);
    rig.assert_active_request();
}

#[tokio::test]
async fn external_missing_method_refuses_local_ids() {
    let rig = ExternalRig::new("ati-missing", || {
        Some(failure(
            ErrorCode::UnsupportedCapability,
            "unknown method: terminal.get_active_task_ids",
        ))
    })
    .await;

    let error = invoke(&rig.app)
        .await
        .expect_err("missing host method must not return local task ids");
    assert!(error.contains("协调重启"), "{error}");
    rig.assert_active_request();
}

#[tokio::test]
async fn external_disconnect_refuses_local_ids() {
    let rig = ExternalRig::new("ati-disconnect", || None).await;

    let error = invoke(&rig.app)
        .await
        .expect_err("a disconnected host must not return local task ids");
    assert!(error.contains("连接失败"), "{error}");
    rig.assert_active_request();
}

#[tokio::test]
async fn external_malformed_reply_refuses_local_ids() {
    for (tag, payload) in [
        ("ati-object", json!({"ids": ["host-only"]})),
        ("ati-null", Value::Null),
        ("ati-mixed", json!(["host-only", 1])),
    ] {
        let rig = ExternalRig::new(tag, move || Some(success(payload.clone()))).await;

        assert!(
            invoke(&rig.app).await.is_err(),
            "{tag}: malformed replies must not become local ids or an empty list"
        );
        rig.assert_active_request();
    }
}

#[tokio::test]
async fn external_without_session_refuses_local_ids() {
    let rig = ExternalRig::new("ati-no-session", || Some(success(json!(["host-only"])))).await;
    {
        let gateway = rig.app.state::<CoreGateway>();
        let session = gateway.host().lock().await.take();
        assert!(session.is_some(), "fixture starts with an attached host");
        drop(session);
    }

    let error = invoke(&rig.app)
        .await
        .expect_err("an external gateway without a session must not return local task ids");
    assert!(error.contains("没有可用的宿主连接"), "{error}");
    assert!(rig.params.lock().is_empty());
    assert!(!rig
        .host
        .methods()
        .iter()
        .any(|method| method == ACTIVE_METHOD));
}
