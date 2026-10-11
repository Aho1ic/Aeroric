//! Real CLI/root composition, including pre-startup home refusal. Server and
//! client environments, data, configs and deliberately wrong roots are scratch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    data: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "acr-cli-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("external home");
        fs::create_dir(&home).unwrap();
        Self {
            home,
            data: root.join("managed"),
            root,
        }
    }

    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aeroric-cli"));
        command
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("AERORIC_HOME", self.root.join("wrong-global-data"))
            .arg("--data-dir")
            .arg(&self.data)
            .stdin(Stdio::null());
        command
    }

    fn server(&self) -> Server {
        let child = self
            .cli()
            .arg("serve")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut server = Server(Some(child));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.data.join("host.sock").exists() && self.data.join("host.token").exists() {
                return server;
            }
            if let Some(status) = server.0.as_mut().unwrap().try_wait().unwrap() {
                let output = server.0.take().unwrap().wait_with_output().unwrap();
                panic!(
                    "serve exited {status}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(Instant::now() < deadline, "serve ready timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn rpc(&self, method: &str, params: Value) -> Value {
        let file = self.root.join("params.json");
        fs::write(&file, serde_json::to_vec(&params).unwrap()).unwrap();
        let output = self
            .cli()
            .args(["rpc", method, "--params-file"])
            .arg(file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{method}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove only this fixture");
    }
}

struct Server(Option<Child>);

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn default_configs_use_external_home_and_explicit_managed_data_through_cli() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.data).unwrap();
    let settings = b"{\"future_field\":{\"retained\":true}}\n";
    fs::write(fixture.data.join("settings.json"), settings).unwrap();
    let decoys = [
        fixture.home.join(".aeroric/agent-homes/omp/config.yml"),
        fixture
            .root
            .join("wrong-global-data/agent-homes/local_dsh/settings.yaml"),
    ];
    for path in &decoys {
        write(path, "DECOY");
    }
    let configs = [
        ("claude", fixture.home.join(".claude/settings.json")),
        ("codex", fixture.home.join(".codex/config.toml")),
        (
            "dsh",
            fixture.data.join("agent-homes/local_dsh/settings.yaml"),
        ),
        ("omp", fixture.data.join("agent-homes/omp/config.yml")),
    ];
    for (_, path) in &configs {
        write(path, "CONTEXT");
    }
    let _server = fixture.server();

    // Validate every actual path before any read/write. An incomplete fix that
    // leaves CLI's old user_home="/" must never send a request toward /.claude.
    let mut wrong = Vec::new();
    for (agent, path) in &configs {
        let got = fixture.rpc(
            "settings.get_agent_config_file_path",
            json!({"agent": agent}),
        );
        if got != json!(path) {
            wrong.push(format!("{agent}: {got} != {path:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    for (agent, path) in &configs {
        assert_eq!(
            fixture.rpc("settings.read_agent_config_file", json!({"agent": agent})),
            "CONTEXT"
        );
        assert_eq!(
            fixture.rpc(
                "settings.write_agent_config_file",
                json!({"agent": agent, "content": "raw\0配置\r\n"})
            ),
            Value::Null
        );
        assert_eq!(fs::read(path).unwrap(), "raw\0配置\r\n".as_bytes());
    }
    for path in &decoys {
        assert_eq!(fs::read(path).unwrap(), b"DECOY");
    }
    assert_eq!(
        fs::read(fixture.data.join("settings.json")).unwrap(),
        settings
    );
    assert!(!fixture.data.join("agent-homes/dsh").exists());
    assert!(!fixture.data.join("agent-homes/local_omp").exists());
}

#[test]
fn invalid_external_home_refuses_before_startup_or_managed_directory_creation() {
    let mut failures = Vec::new();
    for home in [None, Some(""), Some("relative-home"), Some(" \t ")] {
        let fixture = Fixture::new();
        let mut command = fixture.cli();
        command.env_remove("HOME").env_remove("USERPROFILE");
        if let Some(home) = home {
            command.env("HOME", home).env("USERPROFILE", home);
        }
        let child = command
            .arg("serve")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut server = Server(Some(child));
        // 防挂看门狗（不是行为断言）：切片95–100 后二进制变大，并行冷启动的
        // 签名校验能超过 2s——真挂（serve 循环不退）仍会被杀并记 stayed_running。
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stayed_running = false;
        while server.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                stayed_running = true;
                server.0.as_mut().unwrap().kill().unwrap();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = server.0.take().unwrap().wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stayed_running
            || output.status.code() != Some(3)
            || !output.stdout.is_empty()
            || !stderr.contains("external user home")
            || fixture.data.exists()
        {
            failures.push(format!(
                "home={home:?}: running={stayed_running}, status={}, data_created={}, stderr={stderr}",
                output.status,
                fixture.data.exists()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn doctor_and_client_refusals_remain_data_only_without_a_home() {
    let fixture = Fixture::new();
    let output = fixture
        .cli()
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .args(["rpc", "settings.get_agent_config_file_path"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(
        !fixture.data.exists(),
        "a client refusal must not start a writer"
    );
    let output = fixture
        .cli()
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["dataDir"], json!(fixture.data));
    assert!(fs::read_dir(&fixture.home).unwrap().next().is_none());
    assert!(!fixture.root.join("wrong-global-data").exists());
}
