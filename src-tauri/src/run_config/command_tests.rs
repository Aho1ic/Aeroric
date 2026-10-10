//! Slice29: actual read/write command placement, synthetic roots and scripted UDS.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use super::RunConfigDocument;
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;

type MockApp = tauri::App<tauri::test::MockRuntime>;
const TOKEN: &str = "synthetic-run-config-token";

#[derive(Clone, Copy, Debug)]
enum Operation {
    Read,
    Write,
}

impl Operation {
    fn method(self) -> &'static str {
        match self {
            Self::Read => "ide.read_run_configs",
            Self::Write => "ide.write_run_configs",
        }
    }

    fn params(self, path: &str, document: &RunConfigDocument) -> Value {
        match self {
            Self::Read => json!({"projectPath": path}),
            Self::Write => json!({"projectPath": path, "document": document}),
        }
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("arc-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Absent;
impl PlacementProbe for Absent {
    fn availability(&self, _: &Path) -> HostAvailability {
        HostAvailability::Absent
    }
    fn attach(&self, _: &Path) -> Result<(HostClient, bool), String> {
        panic!("absent fixture")
    }
}

fn embedded(scratch: &Scratch) -> MockApp {
    let data = scratch.0.join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&data, Some(&data)),
        "run-config-test",
        &Absent,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

struct External {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}
impl External {
    async fn new(
        tag: &str,
        operation: Operation,
        reply: impl Fn() -> Option<Frame> + Send + Sync + 'static,
    ) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if method == operation.method() {
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
        let app = tauri::test::mock_app();
        app.manage(gateway);
        Self { app, host, calls }
    }
    fn assert_call(&self, operation: Operation, params: Value) {
        assert_eq!(
            *self.calls.lock(),
            vec![(operation.method().to_string(), params)]
        );
        assert!(self.host.methods().iter().any(|m| m == operation.method()));
    }
}

fn wire(name: &str, version: u32) -> Value {
    json!({"version": version, "configs": [
        {"type": "shell", "id": "same", "name": name, "command": "echo not-executed", "cwd": ".", "env": {"LANGUAGE": "中文"}},
        {"type": "debug", "id": "same", "name": "debug", "debugType": "python",
         "program": "missing.py", "cwd": ".", "args": ["--fake"], "env": {},
         "breakpoints": [{"file": "missing.py", "line": 3, "column": 1}]}
    ]})
}

fn input() -> RunConfigDocument {
    serde_json::from_value(wire("input", 99)).unwrap()
}
fn config_path(project: &Path) -> PathBuf {
    project.join(".aeroric/run-configs.json")
}
fn seed(project: &Path, bytes: &[u8]) {
    let file = config_path(project);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            file.parent().unwrap(),
            std::fs::Permissions::from_mode(0o750),
        )
        .unwrap();
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
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

fn snapshot(project: &Path) -> (Vec<u8>, u32, u32, Vec<String>) {
    let file = config_path(project);
    let directory = file.parent().unwrap();
    let mut names: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    (
        std::fs::read(&file).unwrap(),
        mode(&file),
        mode(directory),
        names,
    )
}

// Exercise the actual generic Runtime commands registered in generate_handler.
async fn call(
    app: &MockApp,
    operation: Operation,
    project: String,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    match operation {
        Operation::Read => super::commands::read_run_configs(app.handle().clone(), project).await,
        Operation::Write => {
            super::commands::write_run_configs(app.handle().clone(), project, document).await
        }
    }
}

#[tokio::test]
async fn embedded_read_preserves_defaults_versions_duplicates_and_no_repair() {
    let scratch = Scratch::new("read");
    let _home = platform::install_scratch_home(scratch.0.clone());
    let app = embedded(&scratch);
    let project = scratch.0.join("project 中文 ");
    std::fs::create_dir(&project).unwrap();
    assert_eq!(
        serde_json::to_value(
            call(
                &app,
                Operation::Read,
                project.to_str().unwrap().into(),
                input()
            )
            .await
            .unwrap()
        )
        .unwrap(),
        json!({"version":1,"configs":[]})
    );
    assert!(!project.join(".aeroric").exists());
    for version in [0, 1, 99] {
        let mut expected = wire("local", version);
        let raw = serde_json::to_vec(&expected).unwrap();
        seed(&project, &raw);
        let before = snapshot(&project);
        expected["version"] = json!(if version == 0 { 1 } else { version });
        let result = call(
            &app,
            Operation::Read,
            project.to_str().unwrap().into(),
            input(),
        )
        .await
        .unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), expected);
        assert_eq!(snapshot(&project), before);
    }
}

#[tokio::test]
async fn embedded_write_forces_version_and_replaces_without_attachments_or_execution() {
    let scratch = Scratch::new("write");
    let _home = platform::install_scratch_home(scratch.0.clone());
    let app = embedded(&scratch);
    let project = scratch.0.join("project 中文 ");
    std::fs::create_dir(&project).unwrap();
    let result = call(
        &app,
        Operation::Write,
        project.to_str().unwrap().into(),
        input(),
    )
    .await
    .unwrap();
    assert_eq!(serde_json::to_value(&result).unwrap(), wire("input", 1));
    assert_eq!(
        std::fs::read_to_string(config_path(&project)).unwrap(),
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert!(!project.join(".aeroric/attachments").exists());
    assert!(!project.join("missing.py").exists());
    assert!(!scratch.0.join("project 中文").exists());
}

#[tokio::test]
async fn embedded_root_parse_and_semantic_errors_do_not_repair_or_create_config() {
    let scratch = Scratch::new("errors");
    let _home = platform::install_scratch_home(scratch.0.clone());
    let app = embedded(&scratch);
    for operation in [Operation::Read, Operation::Write] {
        assert_eq!(
            call(&app, operation, "relative".into(), input())
                .await
                .unwrap_err(),
            "Project path must be absolute"
        );
        let missing = scratch.0.join("absent");
        assert!(
            call(&app, operation, missing.to_str().unwrap().into(), input())
                .await
                .unwrap_err()
                .starts_with("Cannot resolve project path:")
        );
        assert!(!missing.exists());
    }
    let project = scratch.0.join("project");
    for bytes in [b"{malformed".as_slice(), &[0xff, 0xfe]] {
        seed(&project, bytes);
        let before = snapshot(&project);
        assert!(call(
            &app,
            Operation::Read,
            project.to_str().unwrap().into(),
            input()
        )
        .await
        .is_err());
        assert_eq!(snapshot(&project), before);
    }
    let untouched = scratch.0.join("untouched");
    std::fs::create_dir(&untouched).unwrap();
    let mut invalid = wire("bad", 7);
    invalid["configs"][0]["command"] = json!(" ");
    let error = call(
        &app,
        Operation::Write,
        untouched.to_str().unwrap().into(),
        serde_json::from_value(invalid).unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "Run config command cannot be empty");
    assert!(!untouched.join(".aeroric").exists());
}

#[tokio::test]
async fn external_results_and_dto_defaults_arrive_from_host_with_exact_params_and_no_local_io() {
    for operation in [Operation::Read, Operation::Write] {
        for (tag, payload, expected) in [
            ("rc-result", wire("host", 1), wire("host", 1)),
            ("rc-defaults", json!({}), json!({"version":1,"configs":[]})),
        ] {
            let scratch = Scratch::new(tag);
            let _home = platform::install_scratch_home(scratch.0.clone());
            let project = scratch.0.join("project 中文 ");
            seed(&project, &[0xff, 0xfe]);
            let before = snapshot(&project);
            let rig = External::new(tag, operation, move || Some(success(payload.clone()))).await;
            let path = project.to_str().unwrap().to_owned();
            let result = call(&rig.app, operation, path.clone(), input())
                .await
                .unwrap();
            assert_eq!(serde_json::to_value(result).unwrap(), expected);
            rig.assert_call(operation, operation.params(&path, &input()));
            assert_eq!(snapshot(&project), before);
        }
    }
}

#[tokio::test]
async fn external_rejects_invalid_host_documents_without_local_fallback() {
    for operation in [Operation::Read, Operation::Write] {
        for (tag, payload) in [
            ("rc-null", Value::Null),
            ("rc-string", json!("no document")),
            ("rc-bool", json!(false)),
            ("rc-version", json!({"version":"1"})),
            ("rc-configs", json!({"configs":"wrong"})),
            ("rc-kind", json!({"configs":[{"type":"alien"}]})),
        ] {
            let scratch = Scratch::new(tag);
            let _home = platform::install_scratch_home(scratch.0.clone());
            let project = scratch.0.join("project");
            seed(&project, &serde_json::to_vec(&wire("local", 1)).unwrap());
            let before = snapshot(&project);
            let rig = External::new(tag, operation, move || Some(success(payload.clone()))).await;
            let path = project.to_str().unwrap().to_owned();
            assert!(
                call(&rig.app, operation, path.clone(), input())
                    .await
                    .is_err(),
                "{tag} {operation:?}"
            );
            rig.assert_call(operation, operation.params(&path, &input()));
            assert_eq!(snapshot(&project), before);
        }
    }
}

#[tokio::test]
async fn external_host_failures_do_not_read_or_write_local_config() {
    for operation in [Operation::Read, Operation::Write] {
        for (tag, code, text, expected) in [
            (
                "rc-arg",
                ErrorCode::InvalidArgument,
                "synthetic invalid",
                "synthetic invalid",
            ),
            ("rc-io", ErrorCode::IoError, "synthetic IO", "synthetic IO"),
            (
                "rc-old",
                ErrorCode::UnsupportedCapability,
                "method missing",
                "协调重启",
            ),
        ] {
            let scratch = Scratch::new(tag);
            let _home = platform::install_scratch_home(scratch.0.clone());
            let project = scratch.0.join("project");
            seed(&project, &serde_json::to_vec(&wire("local", 1)).unwrap());
            let before = snapshot(&project);
            let rig =
                External::new(tag, operation, move || Some(failure(code.clone(), text))).await;
            let path = project.to_str().unwrap().to_owned();
            let error = call(&rig.app, operation, path.clone(), input())
                .await
                .unwrap_err();
            assert!(
                error.contains(expected) && error.contains(operation.method()),
                "{error}"
            );
            rig.assert_call(operation, operation.params(&path, &input()));
            assert_eq!(snapshot(&project), before);
        }
    }
}

#[tokio::test]
async fn external_disconnect_never_falls_back_to_local_io() {
    for operation in [Operation::Read, Operation::Write] {
        let scratch = Scratch::new("disconnect");
        let _home = platform::install_scratch_home(scratch.0.clone());
        let project = scratch.0.join("project");
        seed(&project, &serde_json::to_vec(&wire("local", 1)).unwrap());
        let before = snapshot(&project);
        let rig = External::new("rc-disconnect", operation, || None).await;
        let path = project.to_str().unwrap().to_owned();
        assert!(call(&rig.app, operation, path.clone(), input())
            .await
            .is_err());
        rig.assert_call(operation, operation.params(&path, &input()));
        assert_eq!(snapshot(&project), before);
    }
}

#[tokio::test]
async fn external_without_session_never_sends_or_runs_a_local_operation() {
    for operation in [Operation::Read, Operation::Write] {
        let scratch = Scratch::new("no-session");
        let _home = platform::install_scratch_home(scratch.0.clone());
        let project = scratch.0.join("project");
        seed(&project, &serde_json::to_vec(&wire("local", 1)).unwrap());
        let before = snapshot(&project);
        let rig = External::new("rc-no-session", operation, || {
            Some(success(wire("host", 1)))
        })
        .await;
        {
            let gateway = rig.app.state::<CoreGateway>();
            assert!(gateway.host().lock().await.take().is_some());
        }
        let error = call(
            &rig.app,
            operation,
            project.to_str().unwrap().into(),
            input(),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("没有可用的宿主连接") && error.contains(operation.method()),
            "{error}"
        );
        assert!(rig.calls.lock().is_empty());
        assert_eq!(rig.host.methods(), ["handshake"]);
        assert_eq!(snapshot(&project), before);
    }
}
