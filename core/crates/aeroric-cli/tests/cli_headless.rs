//! Real CLI subprocess checks. All paths and HOME belong to this test.
//! Lifecycle/approval/interactive attach coverage is still required for P3-07.

use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::path::PathBuf::from("/tmp").join(format!("ac-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("scratch directory");
        Self(path)
    }

    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aeroric-cli"));
        command.env("HOME", &self.0).stdin(Stdio::null());
        command
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove only this test's scratch directory");
    }
}

#[test]
fn mutations_without_data_dir_exit_two_without_writing() {
    let scratch = Scratch::new();
    let cases: &[&[&str]] = &[
        &["project", "open", "."],
        &[
            "run",
            "--project",
            ".",
            "--agent",
            "fake",
            "--prompt-file",
            "-",
        ],
        &["task", "cancel", "fake-task"],
        &["task", "complete", "fake-task"],
        &["task", "resume", "fake-task"],
        &["task", "input", "fake-task", "--text-file", "missing"],
        &[
            "approval",
            "respond",
            "fake-event",
            "--outcome-file",
            "missing",
        ],
        &["rpc", "task.run"],
    ];
    for args in cases {
        let output = scratch.cli().args(*args).output().expect("CLI subprocess");
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(
            output.stdout.is_empty(),
            "usage diagnostics belong on stderr"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--data-dir"),
            "the missing data directory must be identified"
        );
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
    }
}

#[test]
fn non_tty_attach_refuses_before_connecting_or_creating_data() {
    let scratch = Scratch::new();
    let missing_data = scratch.0.join("data not created");
    let output = scratch
        .cli()
        .arg("--data-dir")
        .arg(&missing_data)
        .args(["attach", "fake-task"])
        .output()
        .expect("CLI subprocess");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("events"));
    assert!(diagnostic.contains("session history"));
    assert!(
        !missing_data.exists(),
        "attach refusal must not create data"
    );
}

#[test]
fn approval_and_task_lifecycle_real_cli_e2e() {
    let scratch = Scratch::new();
    let data_dir = scratch.0.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let server = Command::new(env!("CARGO_BIN_EXE_aeroric-cli"))
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    struct ServerGuard(std::process::Child);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut guard = ServerGuard(server);

    let sock = data_dir.join("host.sock");
    let token = data_dir.join("host.token");
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(Some(status)) = guard.0.try_wait() {
            let mut stderr = String::new();
            if let Some(mut err) = guard.0.stderr.take() {
                use std::io::Read;
                let _ = err.read_to_string(&mut stderr);
            }
            panic!("server exited early with {status}: stderr: {stderr}");
        }
        if sock.exists() && token.exists() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(ready, "serve must become ready within 5s");

    let proj_dir = scratch.0.join("test_proj");
    std::fs::create_dir_all(&proj_dir).expect("create test proj");
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["project", "open", proj_dir.to_str().unwrap()])
        .output()
        .expect("project open");
    assert_eq!(out.status.code(), Some(0));

    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["approval", "list"])
        .output()
        .expect("approval list");
    assert_eq!(out.status.code(), Some(0));
    let val: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    assert_eq!(val["approvals"].as_array().unwrap().len(), 0);

    let inject_file = scratch.0.join("inject_ev1.json");
    std::fs::write(
        &inject_file,
        serde_json::to_string(&serde_json::json!({
            "eventId": "ev-1",
            "kind": "approval",
            "sessionId": "sess-1"
        }))
        .unwrap(),
    )
    .unwrap();
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "rpc",
            "approval.test_request",
            "--params-file",
            inject_file.to_str().unwrap(),
        ])
        .output()
        .expect("rpc approval.test_request");
    assert_eq!(out.status.code(), Some(0));

    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["approval", "list"])
        .output()
        .expect("approval list");
    assert_eq!(out.status.code(), Some(0));
    let val: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    assert_eq!(val["approvals"].as_array().unwrap().len(), 1);
    assert_eq!(val["approvals"][0]["eventId"], "ev-1");
    assert_eq!(val["approvals"][0]["kind"], "approval");

    let allow_file = scratch.0.join("allow.json");
    std::fs::write(&allow_file, r#"{"kind":"allow","value":true}"#).unwrap();
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "approval",
            "respond",
            "ev-1",
            "--outcome-file",
            allow_file.to_str().unwrap(),
        ])
        .output()
        .expect("approval respond allow");
    assert_eq!(out.status.code(), Some(0));
    let val: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    assert_eq!(val["accepted"], true);

    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "approval",
            "respond",
            "ev-1",
            "--outcome-file",
            allow_file.to_str().unwrap(),
        ])
        .output()
        .expect("approval respond replay");
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("resolved or expired") || stderr.contains("cannot answer approval"),
        "stderr: {stderr}"
    );

    let inject_file2 = scratch.0.join("inject_ev2.json");
    std::fs::write(
        &inject_file2,
        serde_json::to_string(&serde_json::json!({
            "eventId": "ev-2",
            "kind": "question",
            "sessionId": "sess-2"
        }))
        .unwrap(),
    )
    .unwrap();
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "rpc",
            "approval.test_request",
            "--params-file",
            inject_file2.to_str().unwrap(),
        ])
        .output()
        .expect("rpc approval.test_request 2");
    assert_eq!(out.status.code(), Some(0));

    let deny_file = scratch.0.join("deny.json");
    std::fs::write(&deny_file, r#"{"kind":"deny"}"#).unwrap();
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "approval",
            "respond",
            "ev-2",
            "--outcome-file",
            deny_file.to_str().unwrap(),
        ])
        .output()
        .expect("approval respond deny");
    assert_eq!(out.status.code(), Some(0));

    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "approval",
            "respond",
            "ev-non-existent",
            "--outcome-file",
            deny_file.to_str().unwrap(),
        ])
        .output()
        .expect("approval respond missing");
    assert_ne!(out.status.code(), Some(0));

    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["task", "resume", "task-missing"])
        .output()
        .expect("task resume missing");
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotFound") || stderr.contains("no task"));

    let input_file = scratch.0.join("input.txt");
    std::fs::write(&input_file, "hello\n").unwrap();
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "task",
            "input",
            "task-missing",
            "--text-file",
            input_file.to_str().unwrap(),
        ])
        .output()
        .expect("task input missing");
    assert_ne!(out.status.code(), Some(0));
}

/// 续跑验证 2 的具名遗留收口：SIGTERM 默认处置直接死进程、`Drop` 不跑，
/// 所以恢复钩子是「真 PTY 子进程收 SIGTERM 后 termios 回到 raw 前」的
/// 机器判据，不是文档声明。
#[cfg(unix)]
#[test]
fn sigterm_killed_attach_restores_the_local_termios() {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::ExitStatusExt as _;

    let scratch = Scratch::new();
    let data_dir = scratch.0.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let server = Command::new(env!("CARGO_BIN_EXE_aeroric-cli"))
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn serve");
    struct ServerGuard(std::process::Child);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _server_guard = ServerGuard(server);
    let sock = data_dir.join("host.sock");
    let token = data_dir.join("host.token");
    let mut ready = false;
    for _ in 0..100 {
        if sock.exists() && token.exists() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(ready, "serve must become ready within 5s");

    // 真 PTY：attach 的「本地终端」＝这个 slave；master 用来读 pty 的
    // termios（BSD pty 的 termios 在 master/slave 两侧一致）。
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "openpty"
    );

    fn lflag(fd: libc::c_int) -> libc::tcflag_t {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(fd, &mut t), 0, "tcgetattr");
            t.c_lflag
        }
    }
    // 显式把 ECHO 立起来当判定位：raw 期间 cfmakeraw 清掉它，恢复后回来。
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut t), 0, "tcgetattr initial");
        t.c_lflag |= libc::ECHO;
        assert_eq!(
            libc::tcsetattr(slave, libc::TCSANOW, &t),
            0,
            "tcsetattr initial"
        );
    }
    assert_ne!(lflag(slave) & libc::ECHO, 0, "ECHO must start set");

    // 先把一个不退场的 fake 任务跑起来：否则 task.output 立刻 gap、attach
    // 16ms 内就 detach，SIGTERM 没有窗口。
    let proj_dir = scratch.0.join("sigterm_proj");
    std::fs::create_dir_all(&proj_dir).expect("proj");
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["project", "open", proj_dir.to_str().unwrap()])
        .output()
        .expect("project open");
    assert_eq!(out.status.code(), Some(0));
    let agent = scratch.0.join("fake-sigterm.sh");
    std::fs::write(
        &agent,
        "#!/bin/sh\necho 'SIGTERM CANARY'\ntrap 'exit 143' TERM\nsleep 60 &\nwait $!\n",
    )
    .expect("agent script");
    let _ = std::fs::set_permissions(&agent, std::os::unix::fs::PermissionsExt::from_mode(0o755));
    // 没有这条 settings，executor 找不到 custom profile、回退 PATH 查找
    // "fake-sigterm" 直接 spawn 失败（本次测试首跑的实证教训）。
    std::fs::write(
        data_dir.join("settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "custom_agents": [
                { "id": "fake-sigterm", "path": agent, "args": [], "family": "claude", "codex_like": false }
            ]
        }))
        .unwrap(),
    )
    .expect("settings");
    let run_params = scratch.0.join("run-params.json");
    std::fs::write(
        &run_params,
        serde_json::to_string(&serde_json::json!({
            "projectPath": proj_dir,
            "agent": "fake-sigterm",
            "prompt": "stay running",
            "taskId": "sigterm-task"
        }))
        .unwrap(),
    )
    .expect("params");
    let out = scratch
        .cli()
        .arg("--data-dir")
        .arg(&data_dir)
        .args([
            "rpc",
            "task.run",
            "--params-file",
            run_params.to_str().unwrap(),
        ])
        .output()
        .expect("rpc task.run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // attach 进真 PTY：stdin/stdout ＝ slave 的副本，stderr 走 null。
    let mut attach = Command::new(env!("CARGO_BIN_EXE_aeroric-cli"))
        .arg("--data-dir")
        .arg(&data_dir)
        .args(["attach", "sigterm-task"])
        .stdin(unsafe { Stdio::from(std::fs::File::from_raw_fd(libc::dup(slave))) })
        .stdout(unsafe { Stdio::from(std::fs::File::from_raw_fd(libc::dup(slave))) })
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn attach");

    // 等 raw 生效（ECHO 清零）。
    let mut went_raw = false;
    for _ in 0..200 {
        if lflag(master) & libc::ECHO == 0 {
            went_raw = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // 临时诊断：raw 未生效时，抓 attach 的存活状态与 stderr。
    if !went_raw {
        let state = attach.try_wait().expect("try_wait");
        let mut stderr_buf = String::new();
        let _ = std::io::Read::read_to_string(&mut attach.stderr.take().unwrap(), &mut stderr_buf);
        panic!(
            "DIAG: attach_state={state:?} slave_lflag={:#x} master_lflag={:#x} stderr={stderr_buf:?}",
            lflag(slave),
            lflag(master)
        );
    }
    assert!(went_raw, "attach must put the local terminal into raw mode");

    // SIGTERM：默认处置会留 raw——钩子必须先恢复再以信号死法收场。
    assert_eq!(
        unsafe { libc::kill(attach.id() as libc::c_int, libc::SIGTERM) },
        0
    );
    let status = attach.wait().expect("wait attach");
    assert!(
        status.signal() == Some(libc::SIGTERM),
        "attach must die by SIGTERM to prove the handler re-raised it, got {status:?}"
    );

    // 恢复判据：ECHO 回来（处理器写了存档）。
    let mut restored = false;
    for _ in 0..200 {
        if lflag(master) & libc::ECHO != 0 {
            restored = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        restored,
        "SIGTERM-killed attach must restore the local termios (the flagged leftover)"
    );
    unsafe {
        libc::close(master);
        libc::close(slave);
    }
}
