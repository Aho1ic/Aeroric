//! Slices 24/28: regressions at the actual terminal-history command boundaries.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
use crate::storage::commands;

const TERMINAL_HISTORY_METHOD: &str = "project.read_task_terminal_history";
const DELETE_METHOD: &str = "project.delete_task_terminal_histories";
const TOKEN: &str = "term-hist-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("arth-{tag}-{}-{nonce}", std::process::id()));
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

struct AbsentProbe;

impl PlacementProbe for AbsentProbe {
    fn availability(&self, _data_dir: &Path) -> HostAvailability {
        HostAvailability::Absent
    }

    fn attach(&self, _data_dir: &Path) -> Result<(HostClient, bool), String> {
        panic!("an absent-host fixture must not attach")
    }
}

fn embedded_app(scratch: &ScratchRoot) -> MockApp {
    let gateway_dir = scratch.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "term-hist-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

struct ExternalRig {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl ExternalRig {
    async fn new(
        tag: &str,
        reply: impl Fn(&str, &Value) -> Option<Frame> + Send + Sync + 'static,
    ) -> Self {
        Self::for_method(tag, TERMINAL_HISTORY_METHOD, reply).await
    }

    async fn for_method(
        tag: &str,
        method: &'static str,
        reply: impl Fn(&str, &Value) -> Option<Frame> + Send + Sync + 'static,
    ) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |called, params| {
            if called == method {
                observed.lock().push((called.to_owned(), params.clone()));
                reply(called, params)
            } else {
                handshake(called, params)
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
        Self { app, host, calls }
    }

    fn assert_single_call(&self, method: &str, params: Value) {
        assert!(
            self.host.methods().iter().any(|seen| seen == method),
            "the real command must reach {method}"
        );
        assert_eq!(self.calls.lock().as_slice(), &[(method.to_owned(), params)]);
    }
}

#[tokio::test]
async fn embedded_answers_from_local_scratch() {
    let scratch = ScratchRoot::new("embedded");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let app = embedded_app(&scratch);

    let hist_dir = scratch.data_dir().join("terminal-history");
    std::fs::create_dir_all(&hist_dir).expect("create hist dir");
    let log_file = hist_dir.join("task-embedded-1.log");
    std::fs::write(&log_file, "local terminal log bytes").expect("write log");

    let history =
        commands::read_task_terminal_history(app.handle().clone(), "task-embedded-1".to_string())
            .await
            .expect("embedded read succeeds");
    assert_eq!(history, "local terminal log bytes");

    let missing =
        commands::read_task_terminal_history(app.handle().clone(), "missing-task".to_string())
            .await
            .expect("missing returns empty string");
    assert_eq!(missing, "");
}

#[tokio::test]
async fn external_answers_from_host_without_local_io() {
    let scratch = ScratchRoot::new("external");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("term-hist-ext", |_method, _params| {
        Some(success(json!("host-terminal-bytes\r\n")))
    })
    .await;
    scratch.assert_local_absent();

    let history =
        commands::read_task_terminal_history(rig.app.handle().clone(), " task-host-1 ".to_string())
            .await
            .expect("external read succeeds from host");
    assert_eq!(history, "host-terminal-bytes\r\n");
    rig.assert_single_call(TERMINAL_HISTORY_METHOD, json!({"taskId": " task-host-1 "}));
    scratch.assert_local_absent();
}

#[tokio::test]
async fn external_host_failures_do_not_fall_back_locally() {
    for (tag, code, message, expected_text) in [
        (
            "th-unsupported",
            ErrorCode::UnsupportedCapability,
            "unknown method: project.read_task_terminal_history",
            "协调重启",
        ),
        (
            "th-io",
            ErrorCode::IoError,
            "synthetic io error reading terminal log",
            "synthetic io error reading terminal log",
        ),
    ] {
        let scratch = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let rig = ExternalRig::new(tag, move |_method, _params| {
            Some(failure(code.clone(), message))
        })
        .await;
        scratch.assert_local_absent();

        let error =
            commands::read_task_terminal_history(rig.app.handle().clone(), "task-fail".to_string())
                .await
                .expect_err("external host failure must not fall back to local");
        assert!(error.contains(expected_text), "{tag}: {error}");
        rig.assert_single_call(TERMINAL_HISTORY_METHOD, json!({"taskId": "task-fail"}));
        scratch.assert_local_absent();
    }
}

#[tokio::test]
async fn embedded_padded_history_ids_read_the_canonical_file_without_modifying_the_decoy() {
    let scratch = ScratchRoot::new("read-padded");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let app = embedded_app(&scratch);
    let directory = seed_local_history(&scratch);
    let before = history_snapshot(&directory);
    for id in [" first ", "\t\nfirst\r\n", "\u{2003}first\u{2003}"] {
        assert_eq!(
            commands::read_task_terminal_history(app.handle().clone(), id.into())
                .await
                .unwrap(),
            "first local log"
        );
    }
    assert_eq!(history_snapshot(&directory), before);
}

fn seed_local_history(scratch: &ScratchRoot) -> PathBuf {
    let directory = scratch.data_dir().join("terminal-history");
    std::fs::create_dir_all(&directory).unwrap();
    for (name, bytes) in [
        ("first.log", "first local log"),
        ("later.log", "later local log"),
        (" first .log", "padded filename decoy"),
    ] {
        let file = directory.join(name);
        std::fs::write(&file, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o750)).unwrap();
    }
    directory
}

fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    u32::from(std::fs::metadata(path).unwrap().permissions().readonly())
}

type HistorySnapshot = (u32, Vec<(String, Vec<u8>, u32)>);

fn history_snapshot(directory: &Path) -> HistorySnapshot {
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                std::fs::read(entry.path()).unwrap(),
                mode(&entry.path()),
            )
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    (mode(directory), files)
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

// Always call the actual command registered in generate_handler![].
async fn call_delete_command(app: &MockApp, task_ids: Vec<String>) -> Result<(), String> {
    commands::delete_task_terminal_histories(app.handle().clone(), task_ids).await
}

#[tokio::test]
async fn mutations_embedded_preserves_empty_missing_duplicates_and_padded_ids() {
    let scratch = ScratchRoot::new("delete-embedded");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let app = embedded_app(&scratch);
    for task_ids in [vec![], ids(&["missing"])] {
        call_delete_command(&app, task_ids).await.unwrap();
        scratch.assert_local_absent();
    }
    let directory = seed_local_history(&scratch);
    let before_mode = mode(&directory);
    call_delete_command(&app, ids(&[" first ", "first", "missing"]))
        .await
        .unwrap();
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        std::fs::read_to_string(directory.join("later.log")).unwrap(),
        "later local log"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join(" first .log")).unwrap(),
        "padded filename decoy"
    );
    assert_eq!(mode(&directory), before_mode);
    assert!(!scratch.data_dir().join("projects").exists());
}

#[tokio::test]
async fn mutations_embedded_stops_after_a_later_invalid_id_or_io_error() {
    for (tag, bad) in [("invalid-id", "../unsafe"), ("directory", "blocked")] {
        let scratch = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let app = embedded_app(&scratch);
        let directory = seed_local_history(&scratch);
        if bad == "blocked" {
            std::fs::create_dir(directory.join("blocked.log")).unwrap();
            std::fs::write(directory.join("blocked.log/sentinel"), "keep").unwrap();
        }
        let error = call_delete_command(&app, ids(&["first", bad, "later"]))
            .await
            .expect_err("later failure must stop the batch");
        if bad == "../unsafe" {
            assert_eq!(error, "Invalid task id");
        } else {
            assert_eq!(
                std::fs::read_to_string(directory.join("blocked.log/sentinel")).unwrap(),
                "keep"
            );
        }
        assert!(
            !directory.join("first.log").exists(),
            "earlier delete remains applied"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("later.log")).unwrap(),
            "later local log"
        );
        assert!(directory.join(" first .log").exists());
    }
}

#[tokio::test]
async fn mutations_external_success_forwards_exact_ids_without_local_validation_or_deletion() {
    let scratch = ScratchRoot::new("delete-external");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let directory = seed_local_history(&scratch);
    let before = history_snapshot(&directory);
    let rig = ExternalRig::for_method("hist-delete-ok", DELETE_METHOD, |_, _| {
        Some(success(Value::Null))
    })
    .await;
    // The scripted authority accepts this vector. The client must not trim,
    // prevalidate, filter or perform a local deletion before asking it.
    let task_ids = ids(&[" first ", "first", "../host-decides", "later"]);
    call_delete_command(&rig.app, task_ids.clone())
        .await
        .unwrap();
    rig.assert_single_call(DELETE_METHOD, json!({"taskIds": task_ids}));
    assert_eq!(history_snapshot(&directory), before);
}

#[tokio::test]
async fn mutations_external_host_errors_never_delete_valid_local_history() {
    for (tag, code, text, expected) in [
        (
            "hist-delete-arg",
            ErrorCode::InvalidArgument,
            "synthetic bad IDs",
            "synthetic bad IDs",
        ),
        (
            "hist-delete-io",
            ErrorCode::IoError,
            "synthetic delete IO",
            "synthetic delete IO",
        ),
        (
            "hist-delete-old",
            ErrorCode::UnsupportedCapability,
            "unknown delete method",
            "协调重启",
        ),
    ] {
        let scratch = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let directory = seed_local_history(&scratch);
        let before = history_snapshot(&directory);
        let rig = ExternalRig::for_method(tag, DELETE_METHOD, move |_, _| {
            Some(failure(code.clone(), text))
        })
        .await;
        let error = call_delete_command(&rig.app, ids(&["first", "later"]))
            .await
            .unwrap_err();
        assert!(error.contains(expected), "{tag}: {error}");
        assert!(error.contains(DELETE_METHOD), "{error}");
        rig.assert_single_call(DELETE_METHOD, json!({"taskIds": ["first", "later"]}));
        assert_eq!(history_snapshot(&directory), before);
    }
}

#[tokio::test]
async fn mutations_external_rejects_non_null_success_without_local_fallback() {
    for (tag, payload) in [
        ("hist-delete-object", json!({})),
        ("hist-delete-array", json!([])),
        ("hist-delete-bool", json!(true)),
        ("hist-delete-string", json!("ok")),
        ("hist-delete-number", json!(0)),
    ] {
        let scratch = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let directory = seed_local_history(&scratch);
        let before = history_snapshot(&directory);
        let rig = ExternalRig::for_method(tag, DELETE_METHOD, move |_, _| {
            Some(success(payload.clone()))
        })
        .await;
        assert!(
            call_delete_command(&rig.app, ids(&["first"]))
                .await
                .is_err(),
            "{tag}"
        );
        rig.assert_single_call(DELETE_METHOD, json!({"taskIds": ["first"]}));
        assert_eq!(history_snapshot(&directory), before);
    }
}

#[tokio::test]
async fn mutations_external_disconnect_never_deletes_locally() {
    let scratch = ScratchRoot::new("delete-disconnect");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let directory = seed_local_history(&scratch);
    let before = history_snapshot(&directory);
    let rig = ExternalRig::for_method("hist-delete-gone", DELETE_METHOD, |_, _| None).await;
    assert!(call_delete_command(&rig.app, ids(&["first"]))
        .await
        .is_err());
    rig.assert_single_call(DELETE_METHOD, json!({"taskIds": ["first"]}));
    assert_eq!(history_snapshot(&directory), before);
}

#[tokio::test]
async fn mutations_external_without_session_never_deletes_or_sends_a_request() {
    let scratch = ScratchRoot::new("delete-no-session");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let directory = seed_local_history(&scratch);
    let before = history_snapshot(&directory);
    let rig = ExternalRig::for_method("hist-delete-nosess", DELETE_METHOD, |_, _| {
        Some(success(Value::Null))
    })
    .await;
    {
        let gateway = rig.app.state::<CoreGateway>();
        assert!(gateway.host().lock().await.take().is_some());
    }
    let error = call_delete_command(&rig.app, ids(&["first"]))
        .await
        .unwrap_err();
    assert!(error.contains("没有可用的宿主连接"), "{error}");
    assert!(error.contains(DELETE_METHOD), "{error}");
    assert!(rig.calls.lock().is_empty());
    assert_eq!(rig.host.methods(), ["handshake"]);
    assert_eq!(history_snapshot(&directory), before);
}
