//! Root ownership through actual UDS. Every test re-execs itself with scratch
//! environment roots before touching a default, including behavioral-red runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

mod host_harness;
use host_harness::Harness;

const CASE_ENV: &str = "AERORIC_CONFIG_ROOT_TEST_CASE";
const ROOT_ENV: &str = "AERORIC_CONFIG_ROOT_TEST_DIR";
const GET: &str = "settings.get_agent_config_file_path";
const READ: &str = "settings.read_agent_config_file";
const WRITE: &str = "settings.write_agent_config_file";
static SEQ: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove this test's own decoys");
    }
}

fn isolated(name: &str, run: impl FnOnce(&Path)) {
    if std::env::var(CASE_ENV).as_deref() == Ok(name) {
        let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("parent scratch root"));
        let ambient = root.join("ambient-home");
        assert_eq!(PathBuf::from(std::env::var_os("HOME").unwrap()), ambient);
        assert_eq!(
            PathBuf::from(std::env::var_os("AERORIC_HOME").unwrap()),
            root.join("env-data")
        );
        let previous = aeroric_core::settings::set_data_dir_for_tests(root.join("global-data"));
        run(&root);
        aeroric_core::settings::restore_data_dir(previous);
        return;
    }

    let root = Scratch(PathBuf::from("/tmp").join(format!(
        "acr-child-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    )));
    fs::create_dir(&root.0).unwrap();
    for child in ["ambient-home", "env-data", "global-data"] {
        fs::create_dir(root.0.join(child)).unwrap();
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CASE_ENV, name)
        .env(ROOT_ENV, &root.0)
        .env("HOME", root.0.join("ambient-home"))
        .env("USERPROFILE", root.0.join("ambient-home"))
        .env("AERORIC_HOME", root.0.join("env-data"))
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
        !timed_out && output.status.success(),
        "{name}: child status {}, timeout {timed_out}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Bytes, mode and directory entries, deliberately excluding read atime.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u32, Option<Vec<u8>>)> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, (u32, Option<Vec<u8>>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.is_symlink(), "fixtures contain no links");
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        out.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            (mode, metadata.is_file().then(|| fs::read(path).unwrap())),
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn seed_decoys(root: &Path) {
    for relative in [
        "ambient-home/.claude/settings.json",
        "ambient-home/.codex/config.toml",
        "ambient-home/.aeroric/agent-homes/omp/config.yml",
        "ambient-home/.aeroric/agent-homes/local_dsh/settings.yaml",
        "env-data/agent-homes/local_dsh/settings.yaml",
        "env-data/agent-homes/custom-dsh/settings.yaml",
        "global-data/agent-homes/local_dsh/settings.yaml",
        "global-data/agent-homes/custom-dsh/settings.yaml",
    ] {
        write(&root.join(relative), "DECOY: must remain unchanged");
    }
}

fn settings(host: &Harness) -> Value {
    json!({
        "future_field": {"keep": true},
        "custom_agents": [
            {"id": "custom-dsh", "label": "DSH", "path": "/not-a-config",
             "family": " dsh ", "codex_like": false},
            {"id": "custom-omp", "label": "OMP", "path": "/not-a-config",
             "family": "omp", "codex_like": false},
            {"id": "DSH", "label": "Reserved", "path": "/not-a-config",
             "family": "dsh", "codex_like": false},
            {"id": "...", "label": "Invalid home", "path": "/not-a-config",
             "family": "dsh", "codex_like": false},
            {"id": "custom-codex", "label": "Configured", "path": host.home.join("raw.conf"),
             "family": "codex", "codex_like": true}
        ]
    })
}

fn save_settings(host: &Harness, settings: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(settings).unwrap();
    fs::write(host.data_dir.join("settings.json"), &bytes).unwrap();
    bytes
}

fn expected(host: &Harness, agent: &str) -> PathBuf {
    match agent {
        "claude" => host.home.join("home/.claude/settings.json"),
        "codex" => host.home.join("home/.codex/config.toml"),
        "dsh" | "DSH" => host.data_dir.join("agent-homes/local_dsh/settings.yaml"),
        "custom-dsh" => host.data_dir.join("agent-homes/custom-dsh/settings.yaml"),
        "omp" | "custom-omp" => host.data_dir.join("agent-homes/omp/config.yml"),
        _ => panic!("no default expected for {agent}"),
    }
}

async fn call(host: &Harness, method: &str, agent: &str, content: &str) -> Frame {
    let (mut reader, mut writer) = host.connect().await;
    let auth = Harness::handshake(&mut reader, &mut writer, &host.token, "cli").await;
    assert!(matches!(auth, Frame::Success(_)), "{auth:?}");
    Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("roots", method, json!({"agent": agent, "content": content})),
    )
    .await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(reply) = reply else {
        panic!("expected success, got {reply:?}");
    };
    reply.result
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn official_defaults_use_the_injected_user_home_not_the_process_home() {
    isolated(
        "official_defaults_use_the_injected_user_home_not_the_process_home",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let host = Harness::start_relocated("acr-off", false).await;
                let settings_before = save_settings(&host, &settings(&host));
                for agent in ["claude", "codex"] {
                    let path = expected(&host, agent);
                    write(&path, "CONTEXT raw\0配置\r\n");
                    assert_eq!(success(call(&host, GET, agent, "").await), json!(path));
                    assert_eq!(
                        success(call(&host, READ, agent, "").await),
                        "CONTEXT raw\0配置\r\n"
                    );
                }
                assert_eq!(snapshot(root), decoys);
                assert_eq!(
                    fs::read(host.data_dir.join("settings.json")).unwrap(),
                    settings_before
                );
            });
        },
    );
}

#[test]
fn managed_defaults_follow_relocated_data_and_preserve_actual_family_names() {
    isolated(
        "managed_defaults_follow_relocated_data_and_preserve_actual_family_names",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let host = Harness::start_relocated("acr-man", false).await;
                save_settings(&host, &settings(&host));
                let mut mismatches = Vec::new();
                for agent in ["dsh", "omp", "custom-dsh", "custom-omp", "DSH"] {
                    let path = expected(&host, agent);
                    write(&path, "INJECTED");
                    let got = success(call(&host, GET, agent, "").await);
                    if got != json!(path) {
                        mismatches.push(format!("{agent}: {got} != {path:?}"));
                    }
                    let got = success(call(&host, READ, agent, "").await);
                    if got != json!("INJECTED") {
                        mismatches.push(format!("{agent} read: {got}"));
                    }
                }
                assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
                assert_eq!(snapshot(root), decoys);
                assert!(!host.home.join("home/.aeroric").exists());
                assert!(!host.data_dir.join("agent-homes/dsh").exists());
                assert!(!host.data_dir.join("agent-homes/local_omp").exists());
            });
        },
    );
}

#[test]
fn default_writes_update_only_the_injected_official_and_managed_targets() {
    isolated(
        "default_writes_update_only_the_injected_official_and_managed_targets",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let host = Harness::start_relocated("acr-wr", false).await;
                let settings_before = save_settings(&host, &settings(&host));
                let mut mismatches = Vec::new();
                for agent in ["claude", "codex", "dsh", "omp", "custom-dsh", "custom-omp"] {
                    let path = expected(&host, agent);
                    write(&path, "before");
                    assert_eq!(
                        success(call(&host, WRITE, agent, "new raw\0\n").await),
                        Value::Null
                    );
                    if fs::read(&path).unwrap() != b"new raw\0\n" {
                        mismatches.push(agent);
                    }
                }
                assert!(mismatches.is_empty(), "wrong targets: {mismatches:?}");
                assert_eq!(snapshot(root), decoys, "no writes to any global decoy");
                assert_eq!(
                    fs::read(host.data_dir.join("settings.json")).unwrap(),
                    settings_before
                );
            });
        },
    );
}

#[test]
fn missing_defaults_remain_null_and_read_only_queries_create_no_roots() {
    isolated(
        "missing_defaults_remain_null_and_read_only_queries_create_no_roots",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let host = Harness::start_relocated("acr-ro", true).await;
                save_settings(&host, &settings(&host));
                let before = snapshot(&host.home);
                for agent in ["claude", "codex", "dsh", "omp", "custom-dsh", "custom-omp"] {
                    assert_eq!(
                        success(call(&host, GET, agent, "").await),
                        json!(expected(&host, agent))
                    );
                    assert_eq!(success(call(&host, READ, agent, "").await), Value::Null);
                    let Frame::Failure(reply) = call(&host, WRITE, agent, "refused").await else {
                        panic!("read-only write must fail");
                    };
                    assert_eq!(reply.error.code, ErrorCode::IoError);
                    assert!(reply.error.message.contains("no write permission"));
                }
                assert_eq!(snapshot(&host.home), before);
                assert_eq!(snapshot(root), decoys);
            });
        },
    );
}

#[test]
fn configured_paths_none_and_unknown_controls_do_not_change_with_root_injection() {
    isolated(
        "configured_paths_none_and_unknown_controls_do_not_change_with_root_injection",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let host = Harness::start_relocated("acr-cfg", false).await;
                let path = host.home.join("configured file");
                write(&path, "configured raw");
                let mut settings = settings(&host);
                for field in [
                    "claude_config_path",
                    "claude_gpt55_config_path",
                    "codex_config_path",
                    "dsh_config_path",
                ] {
                    settings[field] = json!(format!(" \t{}\n", path.display()));
                }
                save_settings(&host, &settings);
                for agent in ["claude", "claude_gpt55", "codex", "dsh"] {
                    assert_eq!(success(call(&host, GET, agent, "").await), json!(path));
                    assert_eq!(
                        success(call(&host, READ, agent, "").await),
                        "configured raw"
                    );
                }
                assert_eq!(success(call(&host, GET, "...", "").await), "");
                assert_eq!(success(call(&host, READ, "...", "").await), Value::Null);
                settings["claude_gpt55_config_path"] = json!(" ");
                save_settings(&host, &settings);
                assert_eq!(success(call(&host, GET, "claude_gpt55", "").await), "");
                assert_eq!(
                    success(call(&host, READ, "claude_gpt55", "").await),
                    Value::Null
                );
                let Frame::Failure(reply) = call(&host, GET, " claude", "").await else {
                    panic!("an ID must not be trimmed");
                };
                assert_eq!(reply.error.code, ErrorCode::InvalidArgument);
                assert_eq!(snapshot(root), decoys);
            });
        },
    );
}

#[test]
fn simultaneous_hosts_keep_distinct_roots_despite_shared_process_globals() {
    isolated(
        "simultaneous_hosts_keep_distinct_roots_despite_shared_process_globals",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let decoys = snapshot(root);
                let a = Harness::start_relocated("acr-a", false).await;
                let b = Harness::start_relocated("acr-b", false).await;
                for (host, raw) in [(&a, "host a"), (&b, "host b")] {
                    save_settings(host, &settings(host));
                    for agent in ["claude", "omp", "dsh"] {
                        let path = expected(host, agent);
                        write(&path, raw);
                        assert_eq!(success(call(host, GET, agent, "").await), json!(path));
                        assert_eq!(success(call(host, READ, agent, "").await), raw);
                    }
                }
                assert_eq!(snapshot(root), decoys);
            });
        },
    );
}

#[test]
fn trusted_config_home_override_cannot_relocate_the_managed_data_root() {
    isolated(
        "trusted_config_home_override_cannot_relocate_the_managed_data_root",
        |root| {
            runtime().block_on(async {
                seed_decoys(root);
                let external = root.join("config-only-external");
                fs::create_dir(&external).unwrap();
                let port = external.clone();
                let host = Harness::start_with_wiring("acr-port", move |_| {
                    aeroric_host::dispatcher::HostWiring {
                        agent_config_external_home: Some(port),
                        ..Default::default()
                    }
                })
                .await;
                let settings_before = save_settings(&host, &settings(&host));
                for (agent, path) in [
                    ("claude", external.join(".claude/settings.json")),
                    ("codex", external.join(".codex/config.toml")),
                    (
                        "dsh",
                        host.data_dir.join("agent-homes/local_dsh/settings.yaml"),
                    ),
                    ("omp", host.data_dir.join("agent-homes/omp/config.yml")),
                ] {
                    write(&path, "before");
                    assert_eq!(success(call(&host, GET, agent, "").await), json!(path));
                    assert_eq!(
                        success(call(&host, WRITE, agent, "after").await),
                        Value::Null
                    );
                    assert_eq!(fs::read(path).unwrap(), b"after");
                }
                assert!(!external.join(".aeroric").exists());
                assert!(!host.home.join("home/.claude").exists());
                assert!(!host.home.join("home/.codex").exists());
                assert_eq!(
                    fs::read(host.data_dir.join("settings.json")).unwrap(),
                    settings_before
                );
                assert_eq!(
                    fs::read(root.join("ambient-home/.claude/settings.json")).unwrap(),
                    b"DECOY: must remain unchanged"
                );
            });
        },
    );
}

#[test]
fn legacy_global_helpers_keep_their_original_roots_and_missing_home_errors() {
    isolated(
        "legacy_global_helpers_keep_their_original_roots_and_missing_home_errors",
        |root| {
            use aeroric_core::settings::{builtin, normalize};
            let before = snapshot(root);
            for (agent, relative) in [
                ("claude", "ambient-home/.claude/settings.json"),
                ("codex", "ambient-home/.codex/config.toml"),
                ("claude_gpt55", "ambient-home/.claude/start-gpt55.sh"),
                ("dsh", "global-data/agent-homes/local_dsh/settings.yaml"),
                ("omp", "ambient-home/.aeroric/agent-homes/omp/config.yml"),
            ] {
                assert_eq!(
                    builtin::default_builtin_agent_config_path(agent).unwrap(),
                    root.join(relative)
                );
            }
            assert_eq!(
                normalize::custom_agent_home(" Team/DSH.. ").unwrap(),
                root.join("global-data/agent-homes/team_dsh")
            );
            assert!(normalize::custom_agent_home("...")
                .unwrap_err()
                .contains("Invalid"));
            // Process-local and scratch-only: preserve the legacy eager HOME
            // error even when DSH could otherwise use the global managed root.
            std::env::remove_var("HOME");
            assert_eq!(
                builtin::default_builtin_agent_config_path("dsh").unwrap_err(),
                "Cannot find home directory"
            );
            assert_eq!(snapshot(root), before);
        },
    );
}
