//! Slice 23 regressions at the registered platform command boundary.
//!
//! These call the real registered commands (`crate::platform::…`), never a copy of
//! their bodies, against an authenticated scripted host and a scratch home.

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

const RUNTIME_INFO_METHOD: &str = "platform.get_platform_runtime_info";
const RUNNABLE_COMMAND_METHOD: &str = "platform.build_runnable_file_command";
const TOKEN: &str = "platform-command-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("aplc-{tag}-{}-{nonce}", std::process::id()));
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
        "platform-command-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

async fn invoke_runtime_info(app: &MockApp) -> Result<platform::PlatformRuntimeInfo, String> {
    platform::get_platform_runtime_info(app.handle().clone()).await
}

async fn invoke_runnable_command(
    app: &MockApp,
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
    remote: bool,
) -> Result<platform::RunnableFileCommand, String> {
    platform::build_runnable_file_command(
        app.handle().clone(),
        file_path.to_owned(),
        conda_path.map(str::to_owned),
        conda_python_path.map(str::to_owned),
        remote,
    )
    .await
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
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if method == RUNTIME_INFO_METHOD || method == RUNNABLE_COMMAND_METHOD {
                observed.lock().push((method.to_owned(), params.clone()));
                reply(method, params)
            } else {
                handshake(method, params)
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
async fn embedded_answers_from_this_process_without_a_host() {
    let scratch = ScratchRoot::new("embedded");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let app = embedded_app(&scratch);

    let info = invoke_runtime_info(&app)
        .await
        .expect("the Embedded answer is available synchronously");
    assert_eq!(
        serde_json::to_value(&info).unwrap(),
        serde_json::to_value(platform::local_platform_runtime_info()).unwrap()
    );
    assert_eq!(info.os, std::env::consts::OS);

    let command = invoke_runnable_command(&app, "/srv/run.py", None, None, true)
        .await
        .expect("the Embedded builder accepts this file");
    assert_eq!(
        command.command.as_deref(),
        Some("'python3' '/srv/run.py'\r")
    );
    assert_eq!(command.unavailable_reason, None);

    let refused = invoke_runnable_command(&app, "/srv/notes.txt", None, None, false)
        .await
        .expect("a non-script is a successful answer with a reason");
    assert_eq!(refused.command, None);
    assert_eq!(
        refused.unavailable_reason.as_deref(),
        Some("Only Python or shell scripts can be run.")
    );

    let control = invoke_runnable_command(&app, "/srv/a\nb.py", None, None, false)
        .await
        .expect_err("control characters stay rejected");
    assert!(control.contains("control characters"), "{control}");
    scratch.assert_local_absent();
}

#[tokio::test]
async fn external_runtime_info_comes_from_the_host() {
    let scratch = ScratchRoot::new("info");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let host_info = json!({
        "os": "synthetic-os",
        "arch": "synthetic-arch",
        "shellKind": "bash",
        "shellLabel": "bash",
        "pathSeparator": "/",
        "canRunShellScripts": true,
        "shellScriptUnavailableReason": ""
    });
    let rig = ExternalRig::new("plc-info", {
        let host_info = host_info.clone();
        move |_method, _params| Some(success(host_info.clone()))
    })
    .await;
    scratch.assert_local_absent();

    let info = invoke_runtime_info(&rig.app)
        .await
        .expect("the host's platform answer");
    assert_eq!(serde_json::to_value(&info).unwrap(), host_info);
    assert_ne!(
        info.os,
        std::env::consts::OS,
        "the host answer replaces this process's"
    );
    rig.assert_single_call(RUNTIME_INFO_METHOD, json!({}));
    scratch.assert_local_absent();
}

#[tokio::test]
async fn external_runnable_command_sends_exact_parameters_and_returns_host_bytes() {
    let scratch = ScratchRoot::new("runnable");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("plc-runnable", |_method, _params| {
        Some(success(json!({
            "command": "HOST-COMMAND\r",
            "unavailableReason": null
        })))
    })
    .await;
    scratch.assert_local_absent();

    let command = invoke_runnable_command(
        &rig.app,
        "/srv/项目/my script.py",
        None,
        Some("/opt/py 3"),
        true,
    )
    .await
    .expect("the host's runnable command");
    assert_eq!(command.command.as_deref(), Some("HOST-COMMAND\r"));
    assert_eq!(command.unavailable_reason, None);
    rig.assert_single_call(
        RUNNABLE_COMMAND_METHOD,
        json!({
            "filePath": "/srv/项目/my script.py",
            "condaPath": null,
            "condaPythonPath": "/opt/py 3",
            "remote": true,
        }),
    );
    scratch.assert_local_absent();
}

#[tokio::test]
async fn external_method_and_io_failures_do_not_fall_back_locally() {
    for (tag, code, message, expected_text) in [
        (
            "plc-unsupported",
            ErrorCode::UnsupportedCapability,
            "unknown method: platform.get_platform_runtime_info",
            "协调重启",
        ),
        (
            "plc-io",
            ErrorCode::IoError,
            "synthetic platform failure",
            "synthetic platform failure",
        ),
    ] {
        let scratch = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let rig = ExternalRig::new(tag, move |_method, _params| {
            Some(failure(code.clone(), message))
        })
        .await;
        scratch.assert_local_absent();

        let error = invoke_runtime_info(&rig.app)
            .await
            .expect_err("a failing host must not fall back to the local platform");
        assert!(error.contains(expected_text), "{tag}: {error}");
        rig.assert_single_call(RUNTIME_INFO_METHOD, json!({}));
        scratch.assert_local_absent();
    }
}

#[tokio::test]
async fn external_disconnect_does_not_fall_back_locally() {
    let scratch = ScratchRoot::new("disconnect");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("plc-disconnect", |_method, _params| None).await;
    scratch.assert_local_absent();

    assert!(
        invoke_runtime_info(&rig.app).await.is_err(),
        "a disconnected host must not answer with this process's platform"
    );
    assert!(
        invoke_runnable_command(&rig.app, "/srv/run.py", None, None, true)
            .await
            .is_err(),
        "a disconnected host must not answer with this process's command builder"
    );
    assert!(rig
        .host
        .methods()
        .iter()
        .any(|method| method == RUNTIME_INFO_METHOD));
    scratch.assert_local_absent();
}

#[tokio::test]
async fn external_malformed_platform_replies_are_errors() {
    let runtime_info_cases: Vec<(String, Value)> = [
        ("plc-info-null", Value::Null),
        ("plc-info-array", json!([])),
        ("plc-info-text", json!("os")),
        ("plc-info-empty", json!({})),
        (
            "plc-info-partial",
            json!({"os": "synthetic-os", "arch": "synthetic-arch"}),
        ),
        (
            "plc-info-type",
            json!({
                "os": "synthetic-os",
                "arch": "synthetic-arch",
                "shellKind": 7,
                "shellLabel": "bash",
                "pathSeparator": "/",
                "canRunShellScripts": true,
                "shellScriptUnavailableReason": ""
            }),
        ),
        (
            "plc-info-extra-type",
            json!({
                "os": "synthetic-os",
                "arch": "synthetic-arch",
                "shellKind": "bash",
                "shellLabel": "bash",
                "pathSeparator": "/",
                "canRunShellScripts": "yes",
                "shellScriptUnavailableReason": ""
            }),
        ),
    ]
    .into_iter()
    .map(|(tag, payload)| (tag.to_owned(), payload))
    .collect();

    for (tag, payload) in runtime_info_cases {
        let scratch = ScratchRoot::new(&tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let rig =
            ExternalRig::new(&tag, move |_method, _params| Some(success(payload.clone()))).await;
        scratch.assert_local_absent();
        let result = invoke_runtime_info(&rig.app).await;
        assert!(
            result.is_err(),
            "{tag}: malformed info must not answer locally: {result:?}"
        );
        rig.assert_single_call(RUNTIME_INFO_METHOD, json!({}));
        scratch.assert_local_absent();
    }

    let runnable_cases: Vec<(String, Value)> = [
        ("plc-command-null", Value::Null),
        ("plc-command-array", json!([])),
        ("plc-command-text", json!("command")),
        ("plc-command-empty", json!({})),
        (
            "plc-command-missing-reason",
            json!({"command": "/usr/bin/python3 run.py\r"}),
        ),
        (
            "plc-command-missing-command",
            json!({"unavailableReason": "nope"}),
        ),
        (
            "plc-command-both",
            json!({"command": "x\r", "unavailableReason": "y"}),
        ),
        (
            "plc-command-neither",
            json!({"command": null, "unavailableReason": null}),
        ),
        (
            "plc-command-type",
            json!({"command": 7, "unavailableReason": null}),
        ),
        (
            "plc-command-reason-type",
            json!({"command": null, "unavailableReason": false}),
        ),
    ]
    .into_iter()
    .map(|(tag, payload)| (tag.to_owned(), payload))
    .collect();

    for (tag, payload) in runnable_cases {
        let scratch = ScratchRoot::new(&tag);
        let _home = platform::install_scratch_home(scratch.path().to_path_buf());
        let rig =
            ExternalRig::new(&tag, move |_method, _params| Some(success(payload.clone()))).await;
        scratch.assert_local_absent();
        let result = invoke_runnable_command(&rig.app, "/srv/run.py", None, None, true).await;
        assert!(
            result.is_err(),
            "{tag}: malformed command must not answer locally: {result:?}"
        );
        rig.assert_single_call(
            RUNNABLE_COMMAND_METHOD,
            json!({
                "filePath": "/srv/run.py",
                "condaPath": null,
                "condaPythonPath": null,
                "remote": true,
            }),
        );
        scratch.assert_local_absent();
    }
}

#[tokio::test]
async fn external_without_session_returns_no_local_answer() {
    let scratch = ScratchRoot::new("no-session");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("plc-no-session", |_method, _params| {
        Some(success(json!({
            "os": "synthetic-os",
            "arch": "synthetic-arch",
            "shellKind": "bash",
            "shellLabel": "bash",
            "pathSeparator": "/",
            "canRunShellScripts": true,
            "shellScriptUnavailableReason": ""
        })))
    })
    .await;
    scratch.assert_local_absent();
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
        invoke_runtime_info(&rig.app).await.is_err(),
        "an External gateway without a session must not answer locally"
    );
    assert!(
        invoke_runnable_command(&rig.app, "/srv/run.py", None, None, true)
            .await
            .is_err(),
        "an External gateway without a session must not build a local command"
    );
    assert!(
        rig.calls.lock().is_empty(),
        "no platform query after the session is gone"
    );
    scratch.assert_local_absent();
}
