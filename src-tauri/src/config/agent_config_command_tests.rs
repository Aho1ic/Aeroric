//! Actual IPC placement. Even the red/Embedded loader runs in an env-cleared
//! child with scratch HOME and fake shell/which; it never reads a real shell rc.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use crate::app_settings::AppSettings;
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;

const GET: &str = "settings.get_agent_config_file_path";
const READ: &str = "settings.read_agent_config_file";
const WRITE: &str = "settings.write_agent_config_file";
const METHODS: [&str; 3] = [GET, READ, WRITE];
const TOKEN: &str = "agent-config-ipc-synthetic-token";
const CASE: &str = "AERORIC_CONFIG_IPC_CASE";
const ROOT: &str = "AERORIC_CONFIG_IPC_ROOT";
type MockApp = tauri::App<tauri::test::MockRuntime>;

struct OwnedRoot(PathBuf);

impl Drop for OwnedRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only the owned synthetic tree");
    }
}

fn put(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn isolated(name: &str, run: impl FnOnce(Fixture)) {
    if std::env::var(CASE).as_deref() == Ok(name) {
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        assert_eq!(PathBuf::from(std::env::var_os("HOME").unwrap()), root);
        assert_eq!(
            PathBuf::from(std::env::var_os("PATH").unwrap()),
            root.join("bin")
        );
        let _home = crate::platform::install_scratch_home(root.clone());
        run(Fixture { root });
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        OwnedRoot(PathBuf::from("/tmp").join(format!("ac-ipc-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    let bin = root.0.join("bin");
    let shell = bin.join("fixture-shell");
    // /bin/sh only interprets our non-login script; no env/rc is inherited.
    put(
        &shell,
        format!(
            "#!/bin/sh\nprintf 'probe\\n' >> '{0}/shell-probed'\nprintf '__AERORIC_ENV_START__\\000PATH={0}/bin\\000HOME={0}\\000'\n",
            root.0.display()
        )
        .as_bytes(),
        0o700,
    );
    put(
        &bin.join("which"),
        format!(
            "#!/bin/sh\nprintf 'probe\\n' >> '{0}/which-probed'\ncase \"$1\" in '{0}/'*) [ -f \"$1\" ] && printf '%s\\n' \"$1\";; *) exit 1;; esac\n",
            root.0.display()
        )
        .as_bytes(),
        0o700,
    );
    put(
        &root.0.join("never-run-agent"),
        format!(
            "#!/bin/sh\nprintf 'BUG: agent executed' > '{}/agent-ran'\nexit 1\n",
            root.0.display()
        )
        .as_bytes(),
        0o700,
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("config::agent_config_command_tests::{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(CASE, name)
        .env(ROOT, &root.0)
        .env("HOME", &root.0)
        .env("USERPROFILE", &root.0)
        .env("AERORIC_HOME", root.0.join(".aeroric"))
        .env("PATH", &bin)
        .env("SHELL", &shell)
        .env("TMPDIR", &root.0)
        .current_dir(&root.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        !timed_out
            && output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("running 1 test")
            && String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored"),
        "{name}: {}, timeout={timed_out}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !root.0.join("agent-ran").exists(),
        "only probes may execute"
    );
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn settings(&self) -> PathBuf {
        self.root.join(".aeroric/settings.json")
    }

    fn config(&self) -> PathBuf {
        self.root.join("configs/local.raw")
    }

    fn seed(&self, kind: &str) {
        put(&self.config(), b"LOCAL raw\0contents\r\n", 0o640);
        let program = self
            .root
            .join("never-run-agent")
            .to_string_lossy()
            .into_owned();
        let settings = AppSettings {
            claude_path: program.clone(),
            claude_gpt55_path: program.clone(),
            codex_path: program.clone(),
            dsh_path: program.clone(),
            omp_path: program,
            // Exercise the unchanged Embedded loader's tilde/trim behavior.
            codex_config_path: " ~/configs/local.raw ".into(),
            ..Default::default()
        };
        match kind {
            "valid" => put(
                &self.settings(),
                &serde_json::to_vec(&settings).unwrap(),
                0o644,
            ),
            "poison" => put(&self.settings(), b"\xff\xfe\x80\n", 0o644),
            "absent" => {
                if self.settings().exists() {
                    fs::remove_file(self.settings()).unwrap();
                }
            }
            _ => panic!("unknown fixture kind"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, all: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        all.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
                mode: meta.permissions().mode() & 0o7777,
                modified: meta.modified().unwrap(),
                bytes: meta.is_file().then(|| fs::read(path).unwrap()),
                link: meta.is_symlink().then(|| fs::read_link(path).unwrap()),
            },
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), all);
            }
        }
    }
    let mut all = BTreeMap::new();
    visit(root, root, &mut all);
    all
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

struct Absent;
impl PlacementProbe for Absent {
    fn availability(&self, _: &Path) -> HostAvailability {
        HostAvailability::Absent
    }
    fn attach(&self, _: &Path) -> Result<(HostClient, bool), String> {
        panic!("an Embedded fixture must not attach");
    }
}

fn embedded(fixture: &Fixture) -> MockApp {
    let data = fixture.root.join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&data, Some(&data)),
        "config-ipc-test",
        &Absent,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

struct Rig {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Rig {
    async fn new(reply: Option<Frame>) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if method == "handshake" {
                return handshake(method, params);
            }
            seen.lock().push((method.to_owned(), params.clone()));
            reply.clone()
        });
        let host = ScriptedHost::start("ac-ipc", script, Some(TOKEN)).await;
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

    fn assert_request(&self, method: &str, agent: &str, content: &str) {
        let params = if method == WRITE {
            json!({"agent": agent, "content": content})
        } else {
            json!({"agent": agent})
        };
        assert_eq!(self.calls.lock().as_slice(), &[(method.to_owned(), params)]);
        assert!(self.host.methods().iter().any(|seen| seen == method));
    }
}

// Exercise exactly the commands registered in generate_handler![], not route_for.
async fn call(app: &MockApp, method: &str, agent: &str, content: &str) -> Result<Value, String> {
    let value = match method {
        GET => serde_json::to_value(
            super::commands::get_agent_config_file_path(app.handle().clone(), agent.to_owned())
                .await?,
        ),
        READ => serde_json::to_value(
            super::commands::read_agent_config_file(app.handle().clone(), agent.to_owned()).await?,
        ),
        WRITE => serde_json::to_value(
            super::commands::write_agent_config_file(
                app.handle().clone(),
                agent.to_owned(),
                content.to_owned(),
            )
            .await?,
        ),
        _ => panic!("unknown test method"),
    };
    value.map_err(|error| error.to_string())
}

#[test]
fn embedded_retains_loader_normalization_permission_and_raw_io() {
    isolated(
        "embedded_retains_loader_normalization_permission_and_raw_io",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                let app = embedded(&fixture);
                let original = fs::read(fixture.settings()).unwrap();
                assert_eq!(
                    call(&app, GET, "codex", "").await.unwrap(),
                    json!(fixture.config())
                );
                assert_eq!(
                    fs::metadata(fixture.settings())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
                assert_ne!(fs::read(fixture.settings()).unwrap(), original);
                assert!(fixture.root.join("shell-probed").exists());
                assert!(fixture.root.join("which-probed").exists());
                let before = snapshot(&fixture.root);
                assert_eq!(
                    call(&app, READ, "codex", "").await.unwrap(),
                    "LOCAL raw\0contents\r\n"
                );
                assert_eq!(
                    snapshot(&fixture.root),
                    before,
                    "cached read still performs no repairs"
                );
                assert_eq!(
                    call(&app, WRITE, "codex", " \0新内容\r\n").await.unwrap(),
                    Value::Null
                );
                assert_eq!(
                    fs::read(fixture.config()).unwrap(),
                    " \0新内容\r\n".as_bytes()
                );
                assert!(!fixture.root.join("agent-ran").exists());
            });
        },
    );
}

#[test]
fn embedded_retains_missing_errors_and_atomic_link_leaf_policy() {
    isolated(
        "embedded_retains_missing_errors_and_atomic_link_leaf_policy",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                let app = embedded(&fixture);
                assert_eq!(call(&app, GET, "claude_gpt55", "").await.unwrap(), "");
                assert_eq!(
                    call(&app, READ, "claude_gpt55", "").await.unwrap(),
                    Value::Null
                );
                assert_eq!(
                    call(&app, WRITE, "claude_gpt55", "no").await.unwrap_err(),
                    "Agent config file path is not configured"
                );
                assert_eq!(
                    call(&app, GET, " codex", "").await.unwrap_err(),
                    "Unknown agent:  codex"
                );
                fs::remove_file(fixture.config()).unwrap();
                assert_eq!(call(&app, READ, "codex", "").await.unwrap(), Value::Null);
                put(&fixture.config(), b"\xff", 0o640);
                assert!(call(&app, READ, "codex", "").await.is_err());
                fs::remove_file(fixture.config()).unwrap();
                let target = fixture.root.join("link-target");
                put(&target, b"alias target must survive", 0o600);
                symlink(&target, fixture.config()).unwrap();
                assert_eq!(
                    call(&app, WRITE, "codex", "new leaf").await.unwrap(),
                    Value::Null
                );
                assert!(!fs::symlink_metadata(fixture.config()).unwrap().is_symlink());
                assert_eq!(fs::read(target).unwrap(), b"alias target must survive");
                assert_eq!(fs::read(fixture.config()).unwrap(), b"new leaf");
                assert_eq!(
                    fs::read_dir(fixture.config().parent().unwrap())
                        .unwrap()
                        .count(),
                    1
                );
            });
        },
    );
}

fn external_success(name: &str, method: &'static str, values: Vec<Value>) {
    isolated(name, |fixture| {
        runtime().block_on(async {
            for kind in ["valid", "poison", "absent"] {
                fixture.seed(kind);
                for agent in ["codex", " codex \n"] {
                    for value in &values {
                        let rig = Rig::new(Some(success(value.clone()))).await;
                        let before = snapshot(&fixture.root);
                        let content = " \0raw hosted 内容\r\n";
                        assert_eq!(
                            call(&rig.app, method, agent, content).await.unwrap(),
                            *value
                        );
                        rig.assert_request(method, agent, content);
                        assert_eq!(
                            snapshot(&fixture.root),
                            before,
                            "External must not load/repair settings or run a probe"
                        );
                    }
                }
            }
        });
    });
}

#[test]
fn external_path_comes_only_from_host_without_normalizing_agent_or_result() {
    external_success(
        "external_path_comes_only_from_host_without_normalizing_agent_or_result",
        GET,
        vec![json!(" ~/host path \n"), json!("")],
    );
}

#[test]
fn external_read_preserves_host_raw_empty_and_null_without_local_reads() {
    external_success(
        "external_read_preserves_host_raw_empty_and_null_without_local_reads",
        READ,
        vec![json!("host\0内容\r\n"), json!(""), Value::Null],
    );
}

#[test]
fn external_write_sends_exact_content_without_local_loader_or_write() {
    external_success(
        "external_write_sends_exact_content_without_local_loader_or_write",
        WRITE,
        vec![Value::Null],
    );
}

#[test]
fn external_rejects_malformed_result_types_instead_of_defaults_or_local_success() {
    isolated(
        "external_rejects_malformed_result_types_instead_of_defaults_or_local_success",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                for method in METHODS {
                    let invalid = match method {
                        GET => vec![Value::Null, json!(false), json!(17), json!({}), json!([])],
                        READ => vec![json!(false), json!(17), json!({}), json!([])],
                        _ => vec![json!(""), json!(false), json!(17), json!({}), json!([])],
                    };
                    for value in invalid {
                        let rig = Rig::new(Some(success(value.clone()))).await;
                        let before = snapshot(&fixture.root);
                        assert!(
                            call(&rig.app, method, "codex", "not local").await.is_err(),
                            "{method}: {value}"
                        );
                        rig.assert_request(method, "codex", "not local");
                        assert_eq!(snapshot(&fixture.root), before);
                    }
                }
            });
        },
    );
}

#[test]
fn external_host_errors_never_retry_through_the_local_loader() {
    isolated(
        "external_host_errors_never_retry_through_the_local_loader",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                for method in METHODS {
                    for code in [
                        ErrorCode::UnsupportedCapability,
                        ErrorCode::InvalidArgument,
                        ErrorCode::IoError,
                    ] {
                        let rig = Rig::new(Some(failure(code, "synthetic host refusal"))).await;
                        let before = snapshot(&fixture.root);
                        let error = call(&rig.app, method, "codex", "not local")
                            .await
                            .unwrap_err();
                        assert!(error.contains("synthetic host refusal"), "{error}");
                        rig.assert_request(method, "codex", "not local");
                        assert_eq!(snapshot(&fixture.root), before);
                    }
                }
            });
        },
    );
}

#[test]
fn external_disconnect_never_falls_back_to_local_success() {
    isolated(
        "external_disconnect_never_falls_back_to_local_success",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                for method in METHODS {
                    let rig = Rig::new(None).await;
                    let before = snapshot(&fixture.root);
                    assert!(call(&rig.app, method, "codex", "not local").await.is_err());
                    rig.assert_request(method, "codex", "not local");
                    assert_eq!(snapshot(&fixture.root), before);
                }
            });
        },
    );
}

#[test]
fn external_without_session_refuses_before_local_work_or_any_request() {
    isolated(
        "external_without_session_refuses_before_local_work_or_any_request",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                let rig = Rig::new(Some(success(Value::Null))).await;
                let gateway = rig.app.state::<CoreGateway>();
                gateway.host().lock().await.take();
                assert_eq!(gateway.placement(), CorePlacement::External);
                let before = snapshot(&fixture.root);
                for method in METHODS {
                    let error = call(&rig.app, method, "codex", "not local")
                        .await
                        .unwrap_err();
                    assert!(
                        error.contains("external") && error.contains("没有可用"),
                        "{error}"
                    );
                }
                assert!(rig.calls.lock().is_empty());
                assert_eq!(snapshot(&fixture.root), before);
            });
        },
    );
}

#[test]
fn missing_gateway_refuses_instead_of_guessing_embedded() {
    isolated(
        "missing_gateway_refuses_instead_of_guessing_embedded",
        |fixture| {
            runtime().block_on(async {
                fixture.seed("valid");
                let app = tauri::test::mock_app();
                let before = snapshot(&fixture.root);
                for method in METHODS {
                    let error = call(&app, method, "codex", "not local").await.unwrap_err();
                    assert!(error.contains("网关未安装"), "{error}");
                }
                assert_eq!(snapshot(&fixture.root), before);
            });
        },
    );
}
