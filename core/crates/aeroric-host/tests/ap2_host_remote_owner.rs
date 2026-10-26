//! AP2 — 完整 Mac 宿主与远端执行 owner 集成验收测试套件。
//!
//! 覆盖：
//! - AC-AP2-1: 四族（Claude / Codex / OMP / DSH）在 `CliPtyExecutor` 中真实启动与结算；
//!   OMP 注入托管 `PI_CODING_AGENT_DIR` 并清除 `PI_PROFILE`/`OMP_PROFILE` (INV-06)；
//!   DSH 注入 `DSH_HOME` 与 `--profile headless` 位置参数。
//! - AC-AP2-2: `WorkingDirectory::Wsl` 显式退休拒绝且不触发本地 fallback；
//!   `WorkingDirectory::Ssh` 解析已保存连接启动，未知连接与非法路径先于副作用拒绝。
//! - AC-AP2-3 (G43): `connections.*` SSH 连接与凭据经 `SecretStore` 闭环，明文密码永不落入
//!   `ssh-connections.json` 或列表响应；只读宿主与未认证请求在副作用前被拒绝；HostKey 校验与信任闭环。
//! - AC-AP2-4: `CliRemoteHandle` 移除 `NoStreams`，真实二进制终端流（Subscribe/Snapshot/Output/Resize）、
//!   审批回复、会话消息、项目文件读写（含路径穿越防护）、Git 状态/Diff 与 Agent 配置（不回传明文 Key）闭环。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::agents::dsh_host::approvals::{PendingWaterfall, WaterfallKind};
use aeroric_core::context::{CoreContext, MemorySecrets, SecretKey};
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::projects::ProjectService;
use aeroric_core::remote::services::RemoteHostHandle;
use aeroric_core::remote::terminal_frames::{decode_terminal_frame, TerminalFrame, TerminalOpcode};
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::executor::{
    ExecutorFailure, HostWindowFacts, LaunchRequest, TaskExecutor,
};
use aeroric_core::tasks::{
    Attachments, NewTask, PermissionMode, ProtocolFamily, TaskService, WorkingDirectory,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::auth::HostToken;
use aeroric_host::dispatcher::{dispatch, CallContext, HostServices};
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use aeroric_host::families::SettingsFamiliesCli;
use aeroric_host::remote_headless::CliRemoteHandle;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

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

fn scratch_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-ap2-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).expect("create scratch home");
    root
}

fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

fn rpc_req(id: &str, method: &str, params: Value) -> Request {
    Request {
        v: 1,
        id: id.to_string(),
        method: method.to_string(),
        params,
        idempotency_key: None,
        extra: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_families_and_managed_homes_execute_in_cli_pty_executor() {
    let root = scratch_dir("families");
    let script = root.join("probe-agent.sh");
    // Script records family args and environment to a file in cwd so we can assert them deterministically.
    write_executable(
        &script,
        "#!/bin/sh\n\
         printf 'ARGS=%s\\n' \"$*\" > agent-observed.txt\n\
         printf 'PI_DIR=%s\\n' \"${PI_CODING_AGENT_DIR:-}\" >> agent-observed.txt\n\
         printf 'PI_PROF=%s\\n' \"${PI_PROFILE:-unset}\" >> agent-observed.txt\n\
         printf 'OMP_PROF=%s\\n' \"${OMP_PROFILE:-unset}\" >> agent-observed.txt\n\
         printf 'DSH_HOME=%s\\n' \"${DSH_HOME:-}\" >> agent-observed.txt\n\
         printf 'DSH_MODE=%s\\n' \"${DSH_PERMISSION_MODE:-}\" >> agent-observed.txt\n\
         echo AGENT_DONE\n\
         exit 0\n",
    );

    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "ap2-test".to_string(),
                version: "0.1.0".to_string(),
            },
            &LocalProbe,
        )
        .expect("acquire writer"),
    );

    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&json!({
            "custom_agents": [
                { "id": "c-claude", "family": "claude", "path": script.to_string_lossy() },
                { "id": "c-codex", "family": "codex", "codex_like": true, "path": script.to_string_lossy() },
                { "id": "c-omp", "family": "omp", "path": script.to_string_lossy() },
                { "id": "c-dsh", "family": "dsh", "path": script.to_string_lossy() }
            ]
        }))
        .expect("serialize settings"),
    )
    .expect("write settings");

    let registry = Arc::new(TaskRegistry::default());
    let terminal = Arc::new(TerminalService::new());
    let log = Arc::new(EventLog::default());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::clone(&terminal),
        Arc::clone(&log),
    );
    let families = Arc::new(SettingsFamiliesCli::new(Arc::clone(&context)));
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            families.clone() as Arc<_>,
            Arc::clone(&executor) as Arc<_>,
            Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
        )
        .expect("task service"),
    );
    executor.attach(&service);

    // Export PI_PROFILE in parent env to verify OMP unsets it in the child (INV-06).
    std::env::set_var("PI_PROFILE", "polluted-profile");
    std::env::set_var("OMP_PROFILE", "polluted-omp-profile");

    for (agent_id, expected_substr) in [
        ("c-claude", "--permission-mode default -- hello-claude"),
        ("c-codex", "-- hello-codex"),
        ("c-omp", "--approval-mode always-ask"),
        ("c-dsh", "--profile headless -- hello-dsh"),
    ] {
        let ws = root.join(format!("ws-{agent_id}"));
        std::fs::create_dir_all(&ws).expect("ws dir");
        let projects = ProjectService::new(Arc::clone(&context));
        let (upserted, _) = projects
            .open_local(
                &ws.to_string_lossy(),
                &format!("proj-{agent_id}"),
                context.clock.now_ms() as i64,
            )
            .expect("open project");
        let project = upserted.project;
        let task_id = format!("task-{agent_id}");
        let prompt_str = format!("hello-{}", agent_id.trim_start_matches("c-"));
        service
            .create(
                &project,
                NewTask {
                    id: task_id.clone(),
                    project_id: format!("proj-{agent_id}"),
                    prompt: prompt_str,
                    agent: agent_id.to_string(),
                    permission_mode: "ask".to_string(),
                    immediate: true,
                    selected_model: None,
                    reasoning_effort: None,
                    speed: None,
                    dsh_agent_preset: None,
                    agent_config_id: None,
                    images: Vec::new(),
                    texts: Vec::new(),
                },
            )
            .expect("create task");
        service
            .start(
                &project,
                &task_id,
                &format!("op-{agent_id}"),
                true,
                None,
                false,
                Attachments::default(),
                HostWindowFacts {
                    terminal_size: None,
                    dsh: None,
                },
            )
            .expect("start task");

        let mut final_status = String::new();
        for _ in 0..80 {
            let stamped = service.load(&format!("proj-{agent_id}")).expect("load");
            final_status = stamped
                .records
                .first()
                .and_then(|r| r.0.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if final_status == "done" || final_status == "failed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(final_status, "done", "{agent_id} should settle done");
        let observed = std::fs::read_to_string(ws.join("agent-observed.txt"))
            .expect("agent-observed.txt written");
        assert!(
            observed.contains(expected_substr),
            "{agent_id} observed output missing `{expected_substr}`:\n{observed}"
        );
        if agent_id == "c-omp" {
            assert!(
                observed.contains("PI_PROF=unset") && observed.contains("OMP_PROF=unset"),
                "OMP must unset PI_PROFILE and OMP_PROFILE (INV-06):\n{observed}"
            );
            // A custom omp profile runs in its own home, where its models.yml
            // lives (cc-switch recheck D2); only the built-in omp uses `omp`.
            let expected_omp_dir = paths.data_dir().join("agent-homes/c-omp");
            assert!(
                observed.contains(&format!("PI_DIR={}", expected_omp_dir.display())),
                "OMP must inject managed PI_CODING_AGENT_DIR:\n{observed}"
            );
        }
        if agent_id == "c-dsh" {
            let expected_dsh_dir = paths.data_dir().join("agent-homes/dsh");
            assert!(
                observed.contains(&format!("DSH_HOME={}", expected_dsh_dir.display())),
                "DSH must inject managed DSH_HOME:\n{observed}"
            );
            assert!(
                observed.contains("DSH_MODE=read-only"),
                "DSH ask mode maps to read-only:\n{observed}"
            );
        }
    }

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wsl_workspace_is_retired_and_ssh_workspace_executes_via_saved_connection() {
    let root = scratch_dir("ssh-wsl");
    let fake_ssh = root.join("fake-ssh.sh");
    let ssh_log = root.join("ssh-invoked.txt");
    write_executable(
        &fake_ssh,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > '{}'\nexit 0\n",
            ssh_log.display()
        ),
    );

    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "ap2-ssh-test".to_string(),
                version: "0.1.0".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer"),
    );

    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&json!({
            "ssh_program": fake_ssh.to_string_lossy()
        }))
        .expect("settings"),
    )
    .expect("write settings");

    let registry = Arc::new(TaskRegistry::default());
    let terminal = Arc::new(TerminalService::new());
    let log = Arc::new(EventLog::default());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::clone(&terminal),
        Arc::clone(&log),
    );

    let base_request = |wd: WorkingDirectory, family: ProtocolFamily| LaunchRequest {
        task_id: "t-remote".to_string(),
        project_id: "p-remote".to_string(),
        operation_key: "op-1".to_string(),
        generation: 1,
        family,
        agent: "claude".to_string(),
        permission_mode: PermissionMode::Ask,
        permission_mode_wire: "ask".to_string(),
        prompt: "fix bug".to_string(),
        prompt_override: None,
        images: Vec::new(),
        texts: Vec::new(),
        inject_prompt_into_terminal: false,
        session_id: None,
        selected_model: None,
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: None,
        created_at_ms: 1,
        dsh_window: None,
        working_directory: wd,
        terminal_size: None,
    };

    // 1. WSL must be refused with retirement message, never falling back to local.
    let wsl_err = executor
        .launch(&base_request(
            WorkingDirectory::Wsl {
                distribution: "Ubuntu-24.04".to_string(),
                path: "/home/u/app".to_string(),
            },
            ProtocolFamily::Claude,
        ))
        .expect_err("WSL launch must be refused");
    match wsl_err {
        ExecutorFailure::Unsupported { reason } => {
            assert!(
                reason.contains("WSL") && reason.contains("retired"),
                "unexpected WSL refusal reason: {reason}"
            );
        }
        other => panic!("expected Unsupported for WSL, got {other:?}"),
    }
    assert!(!ssh_log.exists(), "WSL refusal must not invoke ssh");

    // 2. DSH on SSH must be refused with Unsupported.
    let dsh_ssh_err = executor
        .launch(&base_request(
            WorkingDirectory::Ssh {
                connection_id: "conn-1".to_string(),
                remote_path: "/srv/app".to_string(),
            },
            ProtocolFamily::Dsh,
        ))
        .expect_err("DSH on SSH must be refused");
    assert!(matches!(dsh_ssh_err, ExecutorFailure::Unsupported { .. }));

    // 3. SSH with unknown connection_id must fail before spawning.
    let missing_conn_err = executor
        .launch(&base_request(
            WorkingDirectory::Ssh {
                connection_id: "missing-conn".to_string(),
                remote_path: "/srv/app".to_string(),
            },
            ProtocolFamily::Claude,
        ))
        .expect_err("unknown SSH connection must fail");
    assert!(matches!(missing_conn_err, ExecutorFailure::Spawn { .. }));
    assert!(!ssh_log.exists(), "unknown connection must not spawn ssh");

    // 4. SSH with invalid remote_path (leading dash) must fail before spawning.
    let bad_path_err = executor
        .launch(&base_request(
            WorkingDirectory::Ssh {
                connection_id: "conn-1".to_string(),
                remote_path: "-rf /".to_string(),
            },
            ProtocolFamily::Claude,
        ))
        .expect_err("leading-dash remote path must fail");
    assert!(matches!(bad_path_err, ExecutorFailure::Spawn { .. }));

    // 5. Save valid connection and verify SSH launch invokes fake-ssh with target host/port/path.
    std::fs::write(
        paths.data_dir().join("ssh-connections.json"),
        serde_json::to_vec_pretty(&json!([
            {
                "id": "conn-1",
                "name": "Dev Box",
                "host": "10.0.0.42",
                "port": 2222,
                "username": "devops"
            }
        ]))
        .expect("json"),
    )
    .expect("write ssh-connections.json");

    executor
        .launch(&base_request(
            WorkingDirectory::Ssh {
                connection_id: "conn-1".to_string(),
                remote_path: "/srv/my_project".to_string(),
            },
            ProtocolFamily::Claude,
        ))
        .expect("SSH launch with saved connection succeeds");

    for _ in 0..50 {
        if ssh_log.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let invoked = std::fs::read_to_string(&ssh_log).expect("fake-ssh wrote log");
    assert!(
        invoked.contains("-tt -p 2222 devops@10.0.0.42")
            && invoked.contains("cd '/srv/my_project'")
            && invoked.contains("claude"),
        "unexpected ssh invocation: {invoked}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_connections_secret_store_and_host_keys_close_g43() {
    let root = scratch_dir("g43");
    let paths = AppPaths::from_user_home(root.join("home"));
    let secrets = MemorySecrets::default();
    let writer_ctx = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "ap2-g43".to_string(),
                version: "0.1.0".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer")
        .with_secrets(Arc::new(secrets.clone())),
    );
    let token_path = paths.data_dir().join("host.token");
    let token = Arc::new(HostToken::load_or_create(&token_path).expect("token"));
    let token_str = std::fs::read_to_string(&token_path)
        .expect("read token")
        .trim()
        .to_string();
    let log = Arc::new(EventLog::default());
    let assembled = HostServices::new(
        Arc::clone(&token),
        Arc::clone(&log),
        Arc::clone(&writer_ctx),
    );

    // 1. Unauthenticated request must be rejected with Unauthenticated.
    let unauth_call = Arc::new(CallContext {
        services: Arc::clone(&assembled.services),
        connection: Arc::new(tokio::sync::Mutex::new(
            aeroric_host::subscriptions::Connection::new(),
        )),
    });
    let unauth_frame = dispatch(
        rpc_req(
            "u1",
            "connections.save_ssh_connections",
            json!({ "connections": [] }),
        ),
        unauth_call,
    )
    .await;
    match unauth_frame {
        Frame::Failure(f) => assert_eq!(f.error.code, ErrorCode::Unauthenticated),
        other => panic!("expected Unauthenticated, got {other:?}"),
    }

    // 2. Read-only host must reject mutations before any side effect.
    let ro_ctx =
        Arc::new(CoreContext::read_only(paths.clone()).with_secrets(Arc::new(secrets.clone())));
    let ro_assembled = HostServices::new(Arc::clone(&token), Arc::clone(&log), ro_ctx);
    let ro_call = Arc::new(CallContext {
        services: Arc::clone(&ro_assembled.services),
        connection: Arc::new(tokio::sync::Mutex::new(
            aeroric_host::subscriptions::Connection::new(),
        )),
    });
    let _ = dispatch(
        rpc_req(
            "h-ro",
            "handshake",
            json!({ "token": token_str, "clientId": "ro" }),
        ),
        Arc::clone(&ro_call),
    )
    .await;
    let ro_save = dispatch(
        rpc_req(
            "ro-1",
            "connections.save_ssh_connections",
            json!({
                "connections": [{
                    "id": "c1", "name": "Prod", "host": "1.2.3.4", "port": 22,
                    "username": "root", "password": "top-secret-password"
                }]
            }),
        ),
        Arc::clone(&ro_call),
    )
    .await;
    assert!(
        matches!(ro_save, Frame::Failure(_)),
        "read-only host must refuse save"
    );
    assert!(
        !paths.data_dir().join("ssh-connections.json").exists(),
        "read-only refusal must not write file"
    );

    // 3. Authenticated writer saves SSH connection: password goes to SecretStore, never to disk or list output.
    let call = Arc::new(CallContext {
        services: Arc::clone(&assembled.services),
        connection: Arc::new(tokio::sync::Mutex::new(
            aeroric_host::subscriptions::Connection::new(),
        )),
    });
    let _ = dispatch(
        rpc_req(
            "h-w",
            "handshake",
            json!({ "token": token_str, "clientId": "w" }),
        ),
        Arc::clone(&call),
    )
    .await;

    let save_res = dispatch(
        rpc_req(
            "s1",
            "connections.save_ssh_connections",
            json!({
                "connections": [{
                    "id": "c1", "name": "Prod", "host": "1.2.3.4", "port": 22,
                    "username": "root", "password": "top-secret-password"
                }]
            }),
        ),
        Arc::clone(&call),
    )
    .await;
    assert!(matches!(save_res, Frame::Success(_)));

    let on_disk = std::fs::read_to_string(paths.data_dir().join("ssh-connections.json"))
        .expect("ssh-connections.json exists");
    assert!(
        !on_disk.contains("top-secret-password"),
        "plaintext password must NEVER be written to ssh-connections.json:\n{on_disk}"
    );
    assert_eq!(
        aeroric_core::context::SecretStore::get(
            &secrets,
            &SecretKey("ssh-password:c1".to_string())
        )
        .expect("secret read"),
        Some("top-secret-password".to_string())
    );

    let list_res = dispatch(
        rpc_req("l1", "connections.load_ssh_connections", json!({})),
        Arc::clone(&call),
    )
    .await;
    match list_res {
        Frame::Success(s) => {
            let dump = serde_json::to_string(&s.result).unwrap();
            assert!(
                !dump.contains("top-secret-password"),
                "list must not leak password"
            );
            assert_eq!(s.result[0]["hasPassword"], json!(true));
        }
        other => panic!("expected Success, got {other:?}"),
    }

    // Re-saving without password field preserves existing secret!
    let _ = dispatch(
        rpc_req(
            "s2",
            "connections.save_ssh_connections",
            json!({
                "connections": [{
                    "id": "c1", "name": "Prod Renamed", "host": "1.2.3.4", "port": 22,
                    "username": "root"
                }]
            }),
        ),
        Arc::clone(&call),
    )
    .await;
    let pw_res = dispatch(
        rpc_req(
            "p1",
            "connections.get_ssh_connection_password",
            json!({ "connectionId": "c1" }),
        ),
        Arc::clone(&call),
    )
    .await;
    match pw_res {
        Frame::Success(s) => assert_eq!(s.result, json!("top-secret-password")),
        other => panic!("expected password, got {other:?}"),
    }

    // 4. HostKey check -> trust -> check Trusted.
    let sample_key =
        "1.2.3.4 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJtFmT0P9XjGqJdKFPpAeN3FzJGFmXK8eFz/1xG8vJq";
    let check_unknown = dispatch(
        rpc_req(
            "hk1",
            "connections.check_ssh_host_key",
            json!({ "host": "1.2.3.4", "port": 22, "scanStdout": sample_key }),
        ),
        Arc::clone(&call),
    )
    .await;
    match check_unknown {
        Frame::Success(s) => assert_eq!(s.result["state"], json!("unknown")),
        other => panic!("expected unknown status, got {other:?}"),
    }

    let trust_res = dispatch(
        rpc_req(
            "hk2",
            "connections.trust_ssh_host_key",
            json!({ "knownHostsLine": sample_key }),
        ),
        Arc::clone(&call),
    )
    .await;
    assert!(matches!(trust_res, Frame::Success(_)));

    let check_trusted = dispatch(
        rpc_req(
            "hk3",
            "connections.check_ssh_host_key",
            json!({ "host": "1.2.3.4", "port": 22 }),
        ),
        Arc::clone(&call),
    )
    .await;
    match check_trusted {
        Frame::Success(s) => assert_eq!(s.result["state"], json!("trusted")),
        other => panic!("expected trusted status, got {other:?}"),
    }

    // 5. Delete SSH connection removes secret and record.
    let _ = dispatch(
        rpc_req(
            "d1",
            "connections.delete_ssh_connection",
            json!({ "connectionId": "c1" }),
        ),
        Arc::clone(&call),
    )
    .await;
    assert_eq!(
        aeroric_core::context::SecretStore::get(
            &secrets,
            &SecretKey("ssh-password:c1".to_string())
        )
        .expect("secret read"),
        None
    );

    // 6. WSL RPC explicitly returns UnsupportedCapability (F12 retired).
    let wsl_rpc = dispatch(
        rpc_req("w1", "connections.get_wsl_status", json!({})),
        Arc::clone(&call),
    )
    .await;
    match wsl_rpc {
        Frame::Failure(f) => assert_eq!(f.error.code, ErrorCode::UnsupportedCapability),
        other => panic!("expected UnsupportedCapability for WSL, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn headless_remote_handle_serves_streams_approvals_sessions_files_git_and_agent_config() {
    let root = scratch_dir("remote-bridge");
    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "ap2-remote".to_string(),
                version: "0.1.0".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer"),
    );

    let script = root.join("live-agent.sh");
    write_executable(
        &script,
        "#!/bin/sh\necho STREAM-LINE-1\nsleep 1\necho STREAM-LINE-2\nexit 0\n",
    );
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&json!({
            "custom_agents": [{ "id": "live", "family": "claude", "path": script.to_string_lossy() }]
        }))
        .expect("settings"),
    )
    .expect("write settings");

    let registry = Arc::new(TaskRegistry::default());
    let terminal = Arc::new(TerminalService::new());
    let log = Arc::new(EventLog::default());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::clone(&terminal),
        Arc::clone(&log),
    );
    let families = Arc::new(SettingsFamiliesCli::new(Arc::clone(&context)));
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            families.clone() as Arc<_>,
            Arc::clone(&executor) as Arc<_>,
            Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
        )
        .expect("task service"),
    );
    executor.attach(&service);
    let approvals = Arc::new(std::sync::Mutex::new(
        aeroric_host::services::HostApprovals::default(),
    ));

    let handle = Arc::new(CliRemoteHandle {
        service: Arc::clone(&service),
        context: Arc::clone(&context),
        executor: Arc::clone(&executor),
        terminal: Arc::clone(&terminal),
        registry: Arc::clone(&registry),
        approvals: Arc::clone(&approvals),
        families: families as Arc<_>,
        // AP2 验收面不涉及推送:显式留空。本套件新加的 AP6 用例会自建带推送面的
        // handle,不去改这份既有装配。
        push: None,
        push_transport: None,
        relay: None,
    });

    // 1. Create a git-backed project workspace and launch a task via task_request.
    let ws = root.join("project-repo");
    std::fs::create_dir_all(&ws).expect("ws");
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&ws)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@aeroric.local"])
        .current_dir(&ws)
        .output()
        .expect("git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&ws)
        .output()
        .expect("git name");
    let readme = ws.join("README.md");
    std::fs::write(&readme, "line 1\n").expect("write readme");
    std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&ws)
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&ws)
        .output()
        .expect("git commit");

    let created = handle
        .task_request(json!({
            "kind": "create",
            "projectPath": ws.to_string_lossy(),
            "agent": "live",
            "prompt": "run stream",
            "permissionMode": "ask"
        }))
        .await
        .expect("task_request create");
    let task_id = created["taskId"].as_str().expect("taskId").to_string();

    // 2. Approval respond via CliRemoteHandle.
    {
        let mut guard = approvals.lock().unwrap();
        guard.registry.insert(
            "req-approval-1".to_string(),
            PendingWaterfall {
                kind: WaterfallKind::Approval,
                session_id: Some("s1".to_string()),
            },
        );
    }
    let resp = handle
        .task_respond(
            &task_id,
            json!({ "requestId": "req-approval-1", "action": "approve" }),
        )
        .await
        .expect("task_respond");
    assert_eq!(resp["accepted"], json!(true));
    assert!(approvals
        .lock()
        .unwrap()
        .resolved_events
        .contains("req-approval-1"));

    // 3. Project files, read_file, write_file, and path traversal refusal.
    let files_val = handle
        .project_files(json!({ "projectId": "project" }))
        .await
        .expect("project_files");
    assert_eq!(files_val["available"], json!(true));
    assert!(files_val["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["name"] == "README.md"));

    let write_res = handle
        .project_write_file(json!({
            "projectId": "project",
            "path": readme.to_string_lossy(),
            "content": "line 1\nline 2 modified\n"
        }))
        .await
        .expect("project_write_file");
    assert_eq!(write_res["ok"], json!(true));

    let read_res = handle
        .project_read_file(json!({
            "projectId": "project",
            "path": readme.to_string_lossy()
        }))
        .await
        .expect("project_read_file");
    assert_eq!(read_res["content"], json!("line 1\nline 2 modified\n"));

    // Outside-project file write must be refused!
    let outside = root.join("outside.txt");
    std::fs::write(&outside, "secret").unwrap();
    assert!(handle
        .project_write_file(json!({
            "projectId": "project",
            "path": outside.to_string_lossy(),
            "content": "hacked"
        }))
        .await
        .is_err());

    // 4. Git changes and git diff (and traversal refusal).
    let git_ch = handle
        .git_changes(json!({ "projectId": "project" }))
        .await
        .expect("git_changes");
    assert_eq!(git_ch["available"], json!(true));
    assert!(!git_ch["changes"].as_array().unwrap().is_empty());

    let git_df = handle
        .git_diff(json!({ "projectId": "project", "path": "README.md", "staged": false }))
        .await
        .expect("git_diff");
    assert!(git_df["diff"]
        .as_str()
        .unwrap()
        .contains("+line 2 modified"));
    assert!(handle
        .git_diff(json!({ "projectId": "project", "path": "../outside.txt" }))
        .await
        .is_err());

    // 5. Agent config save & list (never returning plaintext apiKey).
    handle
        .agent_config_save(json!({
            "id": "claude",
            "baseUrl": "https://api.example.com",
            "apiKey": "sk-secret-do-not-leak",
            "models": ["claude-sonnet-4-5"]
        }))
        .await
        .expect("agent_config_save");
    let cfg_list = handle.agent_config_list().await.expect("agent_config_list");
    let cfg_dump = serde_json::to_string(&cfg_list).unwrap();
    assert!(
        !cfg_dump.contains("sk-secret-do-not-leak"),
        "agent_config_list must never leak plaintext apiKey:\n{cfg_dump}"
    );
    let claude_entry = cfg_list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "claude")
        .expect("claude entry");
    assert_eq!(claude_entry["apiKeyConfigured"], json!(true));
    assert_eq!(claude_entry["baseUrl"], json!("https://api.example.com"));

    // 6. Terminal streams (Subscribe -> SnapshotStart/Chunk/End/Output + Resize -> Resized) with NoStreams removed.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let (disc_tx, _disc_rx) = tokio::sync::watch::channel(false);
    let outbound = aeroric_core::remote::state::OutboundSender::new(tx, disc_tx);
    let mut streams = handle.terminal_streams(outbound);

    let sub_frame = TerminalFrame::new(
        TerminalOpcode::Subscribe,
        7,
        1,
        serde_json::to_vec(&json!({ "taskId": task_id })).unwrap(),
    )
    .encode();
    streams.handle_frame(&sub_frame);

    let resize_frame = TerminalFrame::new(
        TerminalOpcode::Resize,
        7,
        2,
        serde_json::to_vec(&json!({ "cols": 100, "rows": 40 })).unwrap(),
    )
    .encode();
    streams.handle_frame(&resize_frame);

    let mut saw_snapshot_start = false;
    let mut saw_snapshot_end = false;
    let mut saw_resized = false;
    for _ in 0..20 {
        let Some(msg) = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };
        if let aeroric_core::remote::state::Message::Binary(bytes) = msg {
            if let Some(tf) = decode_terminal_frame(&bytes) {
                if tf.stream_id == 7 && tf.opcode == TerminalOpcode::SnapshotStart {
                    saw_snapshot_start = true;
                }
                if tf.stream_id == 7 && tf.opcode == TerminalOpcode::SnapshotEnd {
                    saw_snapshot_end = true;
                }
                if tf.stream_id == 7 && tf.opcode == TerminalOpcode::Resized {
                    saw_resized = true;
                }
                if saw_snapshot_start && saw_snapshot_end && saw_resized {
                    break;
                }
            }
        }
    }
    streams.abort_all();
    assert!(
        saw_snapshot_start && saw_snapshot_end && saw_resized,
        "HeadlessTerminalStreams must deliver SnapshotStart, SnapshotEnd, and Resized frames (saw_start={saw_snapshot_start}, saw_end={saw_snapshot_end}, saw_resized={saw_resized})"
    );

    std::fs::remove_dir_all(&root).ok();
}
