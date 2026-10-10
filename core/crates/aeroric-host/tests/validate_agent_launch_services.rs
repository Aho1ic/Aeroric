//! P8-05 slice 26 & 55: agent launch validation over authenticated UDS.

use std::fs;
use std::path::Path;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request(id, method, params)).await
}

fn success_result(reply: Frame) -> Value {
    let Frame::Success(success) = reply else {
        panic!("expected a successful reply: {reply:?}");
    };
    success.result
}

#[tokio::test]
async fn validate_agent_launch_checks_project_path_and_params() {
    let harness = Harness::start("val-launch-host").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let project_dir = Path::new(&harness.home).join("valid-project");
    fs::create_dir_all(&project_dir).unwrap();

    // Invalid parameters rejected
    for (case, params) in [
        ("missing-params", json!({})),
        ("missing-project", json!({"agent": "claude"})),
        (
            "missing-agent",
            json!({"projectPath": project_dir.to_string_lossy()}),
        ),
        (
            "nonexistent-project",
            json!({"agent": "claude", "projectPath": "/nonexistent/path/that/cannot/exist"}),
        ),
    ] {
        let reply = query(
            &mut reader,
            &mut writer,
            case,
            "terminal.validate_agent_launch",
            params,
        )
        .await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must fail with InvalidArgument");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument, "{case}");
    }
}

#[tokio::test]
async fn validate_agent_launch_fake_agent_scenarios() {
    let harness = Harness::start("val-launch-fake").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let project_dir = Path::new(&harness.home).join("fake-project");
    fs::create_dir_all(&project_dir).unwrap();

    let bin_dir = harness.home.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // 1. Fake-agent success: executable that exits 0 on --version
    let success_script = bin_dir.join("fake-success.sh");
    fs::write(&success_script, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&success_script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // 2. Fake-agent non-zero exit: executable that exits non-zero on --version
    let fail_script = bin_dir.join("fake-fail.sh");
    fs::write(&fail_script, "#!/bin/sh\nexit 42\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fail_script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // 3. Fake-agent timeout-means-Ok: interactive script that does not exit immediately
    let timeout_script = bin_dir.join("fake-loop.sh");
    fs::write(
        &timeout_script,
        "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do\n  sleep 0.1\ndone\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&timeout_script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Write settings in harness.data_dir
    let settings = json!({
        "claude_path": success_script.to_string_lossy(),
        "dsh_path": harness.home.join("nonexistent-dsh").to_string_lossy(),
        "custom_agents": [
            {
                "id": "fake-fail-agent",
                "label": "Fake Fail",
                "path": fail_script.to_string_lossy()
            },
            {
                "id": "fake-timeout-agent",
                "label": "Fake Timeout",
                "path": timeout_script.to_string_lossy()
            }
        ]
    });
    fs::write(harness.data_dir.join("settings.json"), settings.to_string()).unwrap();

    // Scenario 1: fake-agent success
    let reply = query(
        &mut reader,
        &mut writer,
        "fake-success",
        "terminal.validate_agent_launch",
        json!({
            "agent": "claude",
            "projectPath": project_dir.to_string_lossy()
        }),
    )
    .await;
    let res = success_result(reply);
    assert_eq!(res, Value::Null);

    // Scenario 2: non-zero exit
    let reply = query(
        &mut reader,
        &mut writer,
        "fake-fail",
        "terminal.validate_agent_launch",
        json!({
            "agent": "fake-fail-agent",
            "projectPath": project_dir.to_string_lossy()
        }),
    )
    .await;
    let Frame::Failure(failure) = reply else {
        panic!("fake-fail must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::IoError);
    assert!(
        failure
            .error
            .message
            .contains("exited during validation with"),
        "unexpected failure message: {}",
        failure.error.message
    );

    // Scenario 3: NotFound with DSH message
    let reply = query(
        &mut reader,
        &mut writer,
        "dsh-not-found",
        "terminal.validate_agent_launch",
        json!({
            "agent": "dsh",
            "projectPath": project_dir.to_string_lossy()
        }),
    )
    .await;
    let Frame::Failure(failure) = reply else {
        panic!("dsh not found must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::NotFound);
    assert!(
        failure
            .error
            .message
            .contains("DeepSeek Harness is not installed or not found in PATH"),
        "unexpected failure message: {}",
        failure.error.message
    );

    // Scenario 4: timeout-means-Ok
    let reply = query(
        &mut reader,
        &mut writer,
        "fake-timeout",
        "terminal.validate_agent_launch",
        json!({
            "agent": "fake-timeout-agent",
            "projectPath": project_dir.to_string_lossy()
        }),
    )
    .await;
    let res = success_result(reply);
    assert_eq!(res, Value::Null);
}
