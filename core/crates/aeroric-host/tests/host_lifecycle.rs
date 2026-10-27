//! 生命周期（验收「host crash、socket 残留、二次启动均有测试」+ ready 帧形状）。
//! 用 `run_startup` 直连（不经 Harness）以测**拒绝分支**。

mod host_harness;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::startup::{run_startup, HostOptions};

struct LocalProbe;

impl StorageLocationProbe for LocalProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation {
            directory: data_dir.to_path_buf(),
            volume: VolumeKind::Local,
            private_permissions_enforced: true,
        }
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_root(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-life-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home")).expect("home");
    root
}

fn owner() -> LeaseOwner {
    LeaseOwner {
        application: "aeroric-host-test".to_string(),
        version: "0-test".to_string(),
    }
}

async fn startup_for(
    root: &Path,
    ready_fd: Option<std::os::unix::io::RawFd>,
) -> Result<aeroric_host::startup::StartedHost, aeroric_host::startup::StartupError> {
    let paths = AppPaths::from_user_home(root.join("home"));
    let socket_path = paths.data_dir().join("host.sock");
    run_startup(
        &HostOptions {
            paths,
            socket_path,
            ready_fd,
        },
        Arc::new(SystemClock),
        &owner(),
        &LocalProbe,
        "bh",
        "ph",
    )
    .await
}

#[tokio::test]
async fn second_startup_is_refused_while_the_first_holds_the_lease() {
    let root = fresh_root("double");
    let first = startup_for(&root, None).await.expect("first startup");
    let second = startup_for(&root, None).await;
    assert!(second.is_err(), "写租约必须拒绝第二个 host");
    let text = second.err().expect("err").to_string();
    assert!(
        text.starts_with("step 1 (lock):"),
        "错误必须带步骤前缀: {text}"
    );
    drop(first);
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn ready_frame_carries_the_supervisor_metadata_over_the_pipe() {
    let root = fresh_root("ready");
    // libc pipe：读端留测试手里，写端交给 run_startup 的第 5 步。
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);
    let started = startup_for(&root, Some(write_fd))
        .await
        .expect("startup with ready pipe");
    // 从读端取一行 JSON。
    let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
    use std::io::Read;
    let mut line = String::new();
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte).expect("read ready byte");
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0] as char);
    }
    let info: serde_json::Value = serde_json::from_str(&line).expect("ready line is JSON");
    assert_eq!(info["pid"], std::process::id());
    assert_eq!(info["buildHash"], "bh");
    assert_eq!(info["protocolHash"], "ph");
    assert!(info["epoch"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(info["revision"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(info["dataDirHash"].as_str().is_some_and(|s| s.len() == 16));
    assert!(info["socketPath"]
        .as_str()
        .is_some_and(|s| s.ends_with("host.sock")));
    assert!(info["startedAtMs"].as_u64().is_some());
    drop(started);
    std::mem::forget(reader); // write_fd 已被 startup 侧接管关闭；读端手动持有到此
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn after_shutdown_a_fresh_startup_reclaims_the_stale_socket() {
    // host「crash」的等价面：StartedHost 被 drop（进程退出同款资源退场——
    // listener 关闭、租约释放、socket 文件残留），重启必须清残留并成功。
    let root = fresh_root("restart");
    let first = startup_for(&root, None).await.expect("first");
    let socket = {
        let paths = AppPaths::from_user_home(root.join("home"));
        paths.data_dir().join("host.sock")
    };
    // 绑上后要有 serve 才会 accept；这里直接测 bind 层：first 还持 listener
    // 时，第二次 startup 已在 lock 步被拒（上一条测试）。模拟 crash：
    drop(first);
    assert!(socket.exists(), "drop 不移除 socket 文件——残留是常态");
    let second = startup_for(&root, None).await.expect("restart after crash");
    // 清残留+重绑在 startup 第 4 步内完成：重启后 socket 又是活的监听。
    assert!(socket.exists(), "重绑后监听文件必须在位");
    drop(second);
    std::fs::remove_dir_all(&root).ok();
}

use std::os::unix::io::FromRawFd;

#[tokio::test]
async fn graceful_shutdown_unlinks_the_socket_and_frees_the_lease() {
    let root = fresh_root("drain");
    let started = startup_for(&root, None).await.expect("first");
    let socket = started.socket_path.clone();
    let services = aeroric_host::dispatcher::HostServices::new(
        std::sync::Arc::clone(&started.token),
        std::sync::Arc::new(aeroric_core::events::EventLog::default()),
        std::sync::Arc::clone(&started.context),
    );
    let running = aeroric_host::startup::serve(started, services);
    assert!(socket.exists(), "serve 期间 socket 在位");
    running.shutdown();
    // unlink 发生在 serve 任务的退出序里：轮询等它（上限 2 s，正常 <100 ms）。
    let mut gone = false;
    for _ in 0..100 {
        if !socket.exists() {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(gone, "正常退出必须清理 socket 文件");
    // 租约已释放：重启在 lock 步就能过（不再被第二条测试的 AlreadyLive 挡）。
    let restarted = startup_for(&root, None)
        .await
        .expect("restart after clean shutdown");
    drop(restarted);
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn library_startup_does_not_migrate_legacy_agent_settings() {
    let root = fresh_root("agent-migration-off");
    let paths = AppPaths::from_user_home(root.join("home"));
    std::fs::create_dir_all(paths.data_dir()).expect("data dir");
    let settings = serde_json::to_vec_pretty(&serde_json::json!({
        "builtin_agent_credentials": {
            "claude": {
                "base_url": "https://anthropic.example",
                "api_key": "sk-library-must-not-copy",
                "models": ["claude-test"]
            }
        }
    }))
    .expect("settings");
    std::fs::write(paths.settings_file(), &settings).expect("legacy settings");
    let secrets = Arc::new(aeroric_core::context::MemorySecrets::default());

    let started = aeroric_host::startup::run_startup_with_secrets(
        &HostOptions {
            socket_path: paths.data_dir().join("host.sock"),
            paths: paths.clone(),
            ready_fd: None,
        },
        Arc::new(SystemClock),
        &owner(),
        &LocalProbe,
        "bh",
        "ph",
        secrets.clone(),
    )
    .await
    .expect("library startup");

    assert!(!paths.data_dir().join("agent-connections.json").exists());
    assert!(!aeroric_core::agent_connections::migration::report_path(paths.data_dir()).exists());
    assert!(secrets.keys().is_empty());
    assert_eq!(std::fs::read(paths.settings_file()).unwrap(), settings);
    drop(started);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn production_startup_migrates_legacy_agent_settings_before_ready() {
    let root = fresh_root("agent-migration-on");
    let paths = AppPaths::from_user_home(root.join("home"));
    std::fs::create_dir_all(paths.data_dir()).expect("data dir");
    let settings = serde_json::to_vec_pretty(&serde_json::json!({
        "builtin_agent_credentials": {
            "claude": {
                "base_url": "https://anthropic.example",
                "api_key": "sk-production-copy",
                "models": ["claude-test"]
            }
        }
    }))
    .expect("settings");
    std::fs::write(paths.settings_file(), &settings).expect("legacy settings");
    let secrets = Arc::new(aeroric_core::context::MemorySecrets::default());

    let started = aeroric_host::startup::run_startup_with_secrets_and_legacy_agent_migration(
        &HostOptions {
            socket_path: paths.data_dir().join("host.sock"),
            paths: paths.clone(),
            ready_fd: None,
        },
        Arc::new(SystemClock),
        &owner(),
        &LocalProbe,
        "bh",
        "ph",
        secrets.clone(),
    )
    .await
    .expect("production startup");

    let document = aeroric_core::agent_connections::store::load(
        &paths.data_dir().join("agent-connections.json"),
    )
    .expect("migrated document");
    assert_eq!(document.connections.len(), 1);
    assert_eq!(document.configs.len(), 1);
    assert_eq!(
        secrets
            .peek(&document.connections[0].secret_account())
            .as_deref(),
        Some("sk-production-copy")
    );
    let report: aeroric_core::agent_connections::migration::MigrationReport =
        serde_json::from_slice(
            &std::fs::read(aeroric_core::agent_connections::migration::report_path(
                paths.data_dir(),
            ))
            .expect("migration report"),
        )
        .expect("valid migration report");
    assert_eq!(report.migrated.len(), 1);
    assert!(report.skipped.is_empty());
    assert_eq!(std::fs::read(paths.settings_file()).unwrap(), settings);
    drop(started);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn production_startup_removes_hooks_older_builds_installed() {
    let root = fresh_root("legacy-hooks");
    let home = root.join("home");
    let paths = AppPaths::from_user_home(&home);
    std::fs::create_dir_all(paths.data_dir()).expect("data dir");
    let codex = home.join(".codex/config.toml");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let block = aeroric_core::agents::managed_home::build_codex_block("node", "aeroric-hook.mjs");
    std::fs::write(&codex, format!("model = \"o3\"\n\n{block}")).unwrap();
    let hooks = home.join(".aeroric/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("aeroric-hook.mjs"), "// old").unwrap();

    let started = aeroric_host::startup::run_startup_with_secrets_and_legacy_agent_migration(
        &HostOptions {
            socket_path: paths.data_dir().join("host.sock"),
            paths: paths.clone(),
            ready_fd: None,
        },
        Arc::new(SystemClock),
        &owner(),
        &LocalProbe,
        "bh",
        "ph",
        Arc::new(aeroric_core::context::MemorySecrets::default()),
    )
    .await
    .expect("production startup");

    assert_eq!(std::fs::read_to_string(&codex).unwrap(), "model = \"o3\"\n");
    assert!(!hooks.exists(), "the emptied hooks dir is removed");
    drop(started);
    std::fs::remove_dir_all(root).ok();
}
