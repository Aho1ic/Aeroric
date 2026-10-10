//! Slice 26 regressions at the project-config command boundary.
//!
//! Keep the scratch-home guard alive until the mock app and scripted host drop.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use crate::config::ProjectConfig;
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;

const METHOD: &str = "settings.read_project_config";
const INIT: &str = "settings.init_project_config";
const WRITE: &str = "settings.write_project_config";
const TOKEN: &str = "project-config-synthetic-token";
const POISON: &[u8] = b"\xff\xfe\x80\n";
const LOCAL_TOML: &str = r#"# Synthetic local config; a read must not rewrite this comment.
[agent]
default = "codex"
default_permission_mode = "ask"
prompt_prefix = "local prefix\nsecond line"

[git]
commit_prompt = "local commit prompt"
commit_message_timeout_secs = 29

[editor]
format_on_save = true
file_browser_sort = { field = "name", direction = "asc" }
sftp_sort = { field = "size", direction = "desc" }
"#;

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("apc-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).expect("create independent scratch root");
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

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("set synthetic fixture permissions");
}

fn write_local_config(project: &Path, bytes: &[u8]) {
    let directory = project.join(".aeroric");
    let config = directory.join("config.toml");
    std::fs::create_dir_all(&directory).expect("create synthetic project");
    std::fs::write(&config, bytes).expect("write synthetic config");
    #[cfg(unix)]
    {
        set_mode(project, 0o755);
        set_mode(&directory, 0o755);
        set_mode(&config, 0o644);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LocalEntry {
    relative_path: PathBuf,
    #[cfg(unix)]
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
}

// Only inspect our local fixture root, never the separate host's socket directory.
// Include directories and mtimes to catch attachments, backups and same-byte rewrites.
fn local_snapshot(root: &Path) -> Vec<LocalEntry> {
    fn visit(root: &Path, path: &Path, entries: &mut Vec<LocalEntry>) {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::symlink_metadata(path).expect("fixture metadata");
        assert!(
            metadata.is_file() || metadata.is_dir(),
            "local fixtures contain only regular files and directories"
        );
        entries.push(LocalEntry {
            relative_path: path.strip_prefix(root).unwrap().to_path_buf(),
            #[cfg(unix)]
            mode: metadata.permissions().mode() & 0o7777,
            modified: metadata.modified().expect("fixture modification time"),
            bytes: metadata
                .is_file()
                .then(|| std::fs::read(path).expect("read synthetic fixture bytes")),
        });
        if metadata.is_dir() {
            let mut children: Vec<_> = std::fs::read_dir(path)
                .expect("fixture directory entries")
                .map(|entry| entry.expect("fixture entry").path())
                .collect();
            children.sort();
            for child in children {
                visit(root, &child, entries);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

fn path_string(path: &Path) -> String {
    path.to_str().expect("synthetic UTF-8 path").to_owned()
}

fn local_wire() -> Value {
    json!({
        "agent": {
            "default": "codex",
            "default_permission_mode": "ask",
            "prompt_prefix": "local prefix\nsecond line"
        },
        "git": {
            "commit_prompt": "local commit prompt",
            "commit_message_timeout_secs": 29
        },
        "editor": {
            "format_on_save": true,
            "file_browser_sort": {"field": "name", "direction": "asc"},
            "sftp_sort": {"field": "size", "direction": "desc"}
        }
    })
}

fn wire_with_optional_defaults(agent: &str, commit_prompt: &str) -> Value {
    json!({
        "agent": {
            "default": agent,
            "default_permission_mode": "full_access",
            "prompt_prefix": ""
        },
        "git": {
            "commit_prompt": commit_prompt,
            "commit_message_timeout_secs": 15
        },
        "editor": {
            "format_on_save": false,
            "file_browser_sort": {"field": "modified", "direction": "desc"},
            "sftp_sort": {"field": "modified", "direction": "desc"}
        }
    })
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

fn embedded_app(root: &ScratchRoot) -> MockApp {
    let directory = root.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&directory, Some(&directory)),
        "project-config-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

// Always exercise the actual command registered in generate_handler![].
async fn call_command(app: &MockApp, path: String) -> Result<ProjectConfig, String> {
    crate::config::commands::read_project_config(app.handle().clone(), path).await
}

// All three helpers exercise the actual generate_handler! registrations.
async fn call_init_command(app: &MockApp, path: String) -> Result<ProjectConfig, String> {
    crate::config::commands::init_project_config(app.handle().clone(), path).await
}

async fn call_write_command(
    app: &MockApp,
    path: String,
    config: ProjectConfig,
) -> Result<(), String> {
    crate::config::commands::write_project_config(app.handle().clone(), path, config).await
}

struct ExternalRig {
    // Fields drop in declaration order: client before host socket/root.
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl ExternalRig {
    async fn new(tag: &str, reply: impl Fn() -> Option<Frame> + Send + Sync + 'static) -> Self {
        Self::new_for_method(tag, METHOD, reply).await
    }

    async fn new_for_method(
        tag: &str,
        method: &'static str,
        reply: impl Fn() -> Option<Frame> + Send + Sync + 'static,
    ) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |requested, params| {
            if requested == "handshake" {
                return handshake(requested, params);
            }
            observed.lock().push((requested.to_owned(), params.clone()));
            if requested == method {
                reply()
            } else {
                Some(failure(
                    ErrorCode::UnsupportedCapability,
                    "unexpected project-config command method",
                ))
            }
        });
        let host = ScriptedHost::start(tag, script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "tauri")
            .await
            .expect("authenticated synthetic UDS handshake");
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

    fn assert_single_request(&self, original_path: &str) {
        self.assert_request(METHOD, json!({"projectPath": original_path}));
    }

    fn assert_request(&self, method: &str, params: Value) {
        assert!(
            self.host.methods().iter().any(|seen| seen == method),
            "the command must reach the frozen {method} method"
        );
        assert_eq!(self.calls.lock().as_slice(), &[(method.to_owned(), params)]);
    }
}

#[tokio::test]
async fn embedded_preserves_local_results_bytes_modes_and_directories() {
    let root = ScratchRoot::new("embedded");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let app = embedded_app(&root);
    let defaults = serde_json::to_value(ProjectConfig::default()).unwrap();

    let missing = root.path().join("missing project");
    let before = local_snapshot(root.path());
    let result = call_command(&app, path_string(&missing))
        .await
        .expect("missing project returns defaults");
    assert_eq!(serde_json::to_value(result).unwrap(), defaults);
    assert!(!missing.exists(), "a read must not create the project");
    assert_eq!(local_snapshot(root.path()), before);

    for (tag, raw, expected) in [
        ("valid", LOCAL_TOML.as_bytes(), Some(local_wire())),
        (
            "legacy",
            b"[agent]\ndefault = \"codex\"\n[git]\ncommit_prompt = \"legacy local\"\n".as_slice(),
            Some(wire_with_optional_defaults("codex", "legacy local")),
        ),
        (
            "bad-toml",
            b"[agent\nbroken = true\n".as_slice(),
            Some(defaults.clone()),
        ),
        ("bad-utf8", POISON, None),
    ] {
        let project = root.path().join(tag);
        write_local_config(&project, raw);
        let before = local_snapshot(root.path());
        let result = call_command(&app, path_string(&project)).await;
        match expected {
            Some(expected) => assert_eq!(
                serde_json::to_value(result.expect("local config read")).unwrap(),
                expected,
                "{tag}"
            ),
            None => assert!(result.is_err(), "{tag}: invalid UTF-8 must be an error"),
        }
        assert_eq!(local_snapshot(root.path()), before, "{tag}: read-only");
    }

    let empty = root.path().join("empty config directory");
    std::fs::create_dir_all(empty.join(".aeroric")).unwrap();
    let before = local_snapshot(root.path());
    let result = call_command(&app, path_string(&empty))
        .await
        .expect("missing file returns defaults");
    assert_eq!(serde_json::to_value(result).unwrap(), defaults);
    assert_eq!(local_snapshot(root.path()), before);
}

#[tokio::test]
async fn external_uses_typed_host_config_and_exact_space_path_without_local_io() {
    let rich = json!({
        "agent": {
            "default": "omp",
            "default_permission_mode": "auto_edit",
            "prompt_prefix": "host prefix\nonly"
        },
        "git": {
            "commit_prompt": "host commit prompt",
            "commit_message_timeout_secs": 83
        },
        "editor": {
            "format_on_save": false,
            "file_browser_sort": {"field": "size", "direction": "asc"},
            "sftp_sort": {"field": "name", "direction": "desc"}
        }
    });
    for (tag, payload, expected) in [
        ("pc-rich", rich.clone(), rich),
        (
            "pc-defaults",
            json!({"agent": {"default": "omp"}, "git": {"commit_prompt": "host minimal"}}),
            wire_with_optional_defaults("omp", "host minimal"),
        ),
    ] {
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let original = path_string(&root.path().join("project with spaces  "));
        write_local_config(Path::new(&original), POISON);
        // Trimming would select this different, valid config instead of the poison file.
        write_local_config(Path::new(original.trim_end()), LOCAL_TOML.as_bytes());
        assert_ne!(expected, local_wire());
        let rig = ExternalRig::new(tag, move || Some(success(payload.clone()))).await;
        let before = local_snapshot(root.path());

        let result = call_command(&rig.app, original.clone())
            .await
            .expect("host config must bypass the local invalid-UTF-8 file");
        assert_eq!(serde_json::to_value(result).unwrap(), expected, "{tag}");
        rig.assert_single_request(&original);
        assert_eq!(local_snapshot(root.path()), before, "{tag}: no local IO");
    }
}

#[tokio::test]
async fn external_rejects_malformed_host_configs_without_local_fallback() {
    for (tag, payload) in [
        ("pc-null", Value::Null),
        ("pc-string", json!("not a ProjectConfig")),
        ("pc-array", json!([])),
        ("pc-empty", json!({})),
        ("pc-wrapper", json!({"config": local_wire()})),
        ("pc-no-agent", json!({"git": {"commit_prompt": "host"}})),
        ("pc-no-git", json!({"agent": {"default": "omp"}})),
        (
            "pc-no-default",
            json!({"agent": {}, "git": {"commit_prompt": "host"}}),
        ),
        (
            "pc-no-prompt",
            json!({"agent": {"default": "omp"}, "git": {}}),
        ),
        (
            "pc-agent-type",
            json!({"agent": {"default": 7}, "git": {"commit_prompt": "host"}}),
        ),
        (
            "pc-time-type",
            json!({
                "agent": {"default": "omp"},
                "git": {"commit_prompt": "host", "commit_message_timeout_secs": "15"}
            }),
        ),
        (
            "pc-editor-type",
            json!({
                "agent": {"default": "omp"},
                "git": {"commit_prompt": "host"},
                "editor": {"format_on_save": "false"}
            }),
        ),
    ] {
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let project = root.path().join("local project");
        write_local_config(&project, LOCAL_TOML.as_bytes());
        let original = path_string(&project);
        let rig = ExternalRig::new(tag, move || Some(success(payload.clone()))).await;
        let before = local_snapshot(root.path());

        let result = call_command(&rig.app, original.clone()).await;
        assert!(
            result.is_err(),
            "{tag}: malformed host data must not become defaults or local data: {result:?}"
        );
        rig.assert_single_request(&original);
        assert_eq!(local_snapshot(root.path()), before, "{tag}: no fallback");
    }
}

#[tokio::test]
async fn external_host_errors_do_not_fall_back_to_valid_local_config() {
    for (tag, code, message, expected_text) in [
        (
            "pc-io",
            ErrorCode::IoError,
            "synthetic project-config IO failure",
            "synthetic project-config IO failure",
        ),
        (
            "pc-unsupported",
            ErrorCode::UnsupportedCapability,
            "unknown method: settings.read_project_config",
            "协调重启",
        ),
    ] {
        let root = ScratchRoot::new(tag);
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let project = root.path().join("local project");
        write_local_config(&project, LOCAL_TOML.as_bytes());
        let original = path_string(&project);
        let rig = ExternalRig::new(tag, move || Some(failure(code.clone(), message))).await;
        let before = local_snapshot(root.path());

        let error = call_command(&rig.app, original.clone())
            .await
            .expect_err("host error must not fall back to valid local config");
        assert!(error.contains(expected_text), "{tag}: {error}");
        assert!(error.contains(METHOD), "{tag}: {error}");
        rig.assert_single_request(&original);
        assert_eq!(local_snapshot(root.path()), before, "{tag}: no fallback");
    }
}

#[tokio::test]
async fn external_disconnect_does_not_fall_back_to_valid_local_config() {
    let root = ScratchRoot::new("disconnect");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project = root.path().join("local project");
    write_local_config(&project, LOCAL_TOML.as_bytes());
    let original = path_string(&project);
    let rig = ExternalRig::new("pc-disconnect", || None).await;
    let before = local_snapshot(root.path());

    let result = call_command(&rig.app, original.clone()).await;
    assert!(
        result.is_err(),
        "disconnection must not return valid local config"
    );
    rig.assert_single_request(&original);
    assert_eq!(local_snapshot(root.path()), before);
}

#[tokio::test]
async fn external_without_session_refuses_without_request_or_local_io() {
    let root = ScratchRoot::new("no-session");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project = root.path().join("local project");
    write_local_config(&project, LOCAL_TOML.as_bytes());
    let rig = ExternalRig::new("pc-no-session", || Some(success(local_wire()))).await;
    {
        let gateway = rig.app.state::<CoreGateway>();
        let session = gateway.host().lock().await.take();
        assert!(session.is_some(), "fixture starts authenticated");
        drop(session);
    }
    let before = local_snapshot(root.path());

    let error = call_command(&rig.app, path_string(&project))
        .await
        .expect_err("External without a session must not run the local reader");
    assert!(error.contains("没有可用的宿主连接"), "{error}");
    assert!(error.contains(METHOD), "{error}");
    assert!(rig.calls.lock().is_empty());
    assert_eq!(rig.host.methods(), vec!["handshake".to_owned()]);
    assert_eq!(local_snapshot(root.path()), before);
}

#[tokio::test]
async fn mutations_embedded_init_keeps_template_and_existing_error_side_effects() {
    let root = ScratchRoot::new("init-embedded");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let app = embedded_app(&root);
    let project = root.path().join("new project 中文 ");
    let config_path = project.join(".aeroric/config.toml");
    let result = call_init_command(&app, path_string(&project))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(ProjectConfig::default()).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        aeroric_core::settings::project_config::DEFAULT_CONFIG
    );
    assert!(project.join(".aeroric/attachments").is_dir());

    for (name, bytes) in [
        ("valid", LOCAL_TOML.as_bytes()),
        ("bad-toml", b"[malformed".as_slice()),
        ("bad-utf8", POISON),
    ] {
        let project = root.path().join(name);
        write_local_config(&project, bytes);
        let result = call_init_command(&app, path_string(&project)).await;
        if name == "bad-utf8" {
            assert!(result.is_err());
        } else {
            let expected = if name == "valid" {
                local_wire()
            } else {
                serde_json::to_value(ProjectConfig::default()).unwrap()
            };
            assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), expected);
        }
        assert_eq!(
            std::fs::read(project.join(".aeroric/config.toml")).unwrap(),
            bytes
        );
        assert!(project.join(".aeroric/attachments").is_dir());
    }
}

#[tokio::test]
async fn mutations_embedded_write_keeps_complete_toml_replacement_and_failed_rename_cleanup() {
    let root = ScratchRoot::new("write-embedded");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let app = embedded_app(&root);
    let project = root.path().join("project  ");
    write_local_config(&project, b"# prior contents\nunknown = true\n");
    let config_path = project.join(".aeroric/config.toml");
    let config: ProjectConfig = serde_json::from_value(local_wire()).unwrap();
    let raw = toml::to_string_pretty(&config).unwrap();
    call_write_command(&app, path_string(&project), config.clone())
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), raw);
    assert!(!project.join(".aeroric/attachments").exists());
    assert_eq!(
        std::fs::read_dir(project.join(".aeroric")).unwrap().count(),
        1
    );

    std::fs::remove_file(&config_path).unwrap();
    std::fs::create_dir(&config_path).unwrap();
    std::fs::write(config_path.join("keep"), b"existing directory").unwrap();
    assert!(call_write_command(&app, path_string(&project), config)
        .await
        .is_err());
    assert_eq!(
        std::fs::read(config_path.join("keep")).unwrap(),
        b"existing directory"
    );
    assert_eq!(
        std::fs::read_dir(project.join(".aeroric")).unwrap().count(),
        1
    );
}

#[tokio::test]
async fn mutations_external_init_uses_typed_host_result_without_creating_attachments() {
    let root = ScratchRoot::new("init-external");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project = root.path().join("original path 中文  ");
    write_local_config(&project, POISON);
    let payload = json!({"agent": {"default": "host-27"}, "git": {"commit_prompt": "host prompt"}});
    let rig = ExternalRig::new_for_method("init-external", INIT, move || {
        Some(success(payload.clone()))
    })
    .await;
    let before = local_snapshot(root.path());
    let result = call_init_command(&rig.app, path_string(&project))
        .await
        .expect("External init must not read the local poison file");
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        wire_with_optional_defaults("host-27", "host prompt")
    );
    rig.assert_request(INIT, json!({"projectPath": project}));
    assert_eq!(local_snapshot(root.path()), before);
}

#[tokio::test]
async fn mutations_external_write_sends_exact_config_and_never_writes_locally() {
    let root = ScratchRoot::new("write-external");
    let _home = platform::install_scratch_home(root.path().to_path_buf());
    let project = root.path().join("project 中文  ");
    write_local_config(&project, POISON);
    let rig =
        ExternalRig::new_for_method("write-external", WRITE, || Some(success(Value::Null))).await;
    let before = local_snapshot(root.path());
    call_write_command(
        &rig.app,
        path_string(&project),
        serde_json::from_value(local_wire()).unwrap(),
    )
    .await
    .unwrap();
    rig.assert_request(
        WRITE,
        json!({"projectPath": project, "config": local_wire()}),
    );
    assert_eq!(local_snapshot(root.path()), before);
}

async fn call_mutation(app: &MockApp, method: &str, project: &Path) -> Result<(), String> {
    if method == INIT {
        call_init_command(app, path_string(project))
            .await
            .map(|_| ())
    } else {
        call_write_command(
            app,
            path_string(project),
            serde_json::from_value(local_wire()).unwrap(),
        )
        .await
    }
}

fn mutation_params(method: &str, project: &Path) -> Value {
    if method == INIT {
        json!({"projectPath": project})
    } else {
        json!({"projectPath": project, "config": local_wire()})
    }
}

#[tokio::test]
async fn mutations_external_rejects_malformed_results_instead_of_local_success() {
    for method in [INIT, WRITE] {
        let payloads = if method == INIT {
            vec![
                Value::Null,
                json!({}),
                json!([]),
                json!({"config": local_wire()}),
            ]
        } else {
            vec![json!({}), json!(true), json!("ok"), local_wire()]
        };
        for payload in payloads {
            let root = ScratchRoot::new("mutation-malformed");
            let _home = platform::install_scratch_home(root.path().to_path_buf());
            let project = root.path().join("local project");
            write_local_config(&project, LOCAL_TOML.as_bytes());
            let rig = ExternalRig::new_for_method("mutation-malformed", method, move || {
                Some(success(payload.clone()))
            })
            .await;
            let before = local_snapshot(root.path());
            assert!(
                call_mutation(&rig.app, method, &project).await.is_err(),
                "{method}: a bad host result must not become local success"
            );
            rig.assert_request(method, mutation_params(method, &project));
            assert_eq!(local_snapshot(root.path()), before);
        }
    }
}

#[tokio::test]
async fn mutations_external_error_or_disconnect_never_retries_locally() {
    for method in [INIT, WRITE] {
        for code in [
            Some(ErrorCode::IoError),
            Some(ErrorCode::UnsupportedCapability),
            None,
        ] {
            let root = ScratchRoot::new("mutation-failure");
            let _home = platform::install_scratch_home(root.path().to_path_buf());
            let project = root.path().join("must remain absent");
            let rig = ExternalRig::new_for_method("mutation-failure", method, move || {
                code.clone()
                    .map(|code| failure(code, "synthetic mutation failure"))
            })
            .await;
            let before = local_snapshot(root.path());
            let error = call_mutation(&rig.app, method, &project)
                .await
                .expect_err("External failure must not create the local project");
            assert!(error.contains(method), "{error}");
            rig.assert_request(method, mutation_params(method, &project));
            assert_eq!(local_snapshot(root.path()), before);
        }
    }
}

#[tokio::test]
async fn mutations_external_without_session_refuses_before_local_io() {
    for method in [INIT, WRITE] {
        let root = ScratchRoot::new("mutation-no-session");
        let _home = platform::install_scratch_home(root.path().to_path_buf());
        let project = root.path().join("must remain absent");
        let rig = ExternalRig::new_for_method("mutation-no-session", method, || {
            Some(success(Value::Null))
        })
        .await;
        {
            let gateway = rig.app.state::<CoreGateway>();
            let session = gateway.host().lock().await.take();
            assert!(session.is_some());
            drop(session);
        }
        let before = local_snapshot(root.path());
        let error = call_mutation(&rig.app, method, &project).await.unwrap_err();
        assert!(error.contains("没有可用的宿主连接"), "{error}");
        assert!(error.contains(method), "{error}");
        assert!(rig.calls.lock().is_empty());
        assert_eq!(local_snapshot(root.path()), before);
    }
}
