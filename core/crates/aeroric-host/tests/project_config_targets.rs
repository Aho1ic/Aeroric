//! Slice30: ADR-006 targets must never fall through to local project-config IO.

use std::fs;
use std::path::Path;

use aeroric_core::settings::project_config::{ProjectConfig, DEFAULT_CONFIG};
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const READ: &str = "settings.read_project_config";
const INIT: &str = "settings.init_project_config";
const WRITE: &str = "settings.write_project_config";

fn config() -> Value {
    let mut config = ProjectConfig::default();
    config.agent.default = "synthetic-new-agent".into();
    config.agent.prompt_prefix = "new bytes\n中文".into();
    serde_json::to_value(config).unwrap()
}

fn remote_targets() -> [Value; 2] {
    [
        json!({"kind": "ssh", "connectionId": "synthetic", "remotePath": "/srv/fake"}),
        json!({"kind": "wsl", "distribution": "synthetic", "linuxPath": "/srv/fake"}),
    ]
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    u32::from(fs::metadata(path).unwrap().permissions().readonly())
}

fn seed(project: &Path, bytes: &[u8]) {
    let directory = project.join(".aeroric");
    fs::create_dir_all(&directory).unwrap();
    let file = directory.join("config.toml");
    fs::write(&file, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o640)).unwrap();
    }
}

fn snapshot(project: &Path) -> (Vec<u8>, u32, u32, Vec<String>) {
    let directory = project.join(".aeroric");
    let file = directory.join("config.toml");
    (
        fs::read(&file).unwrap(),
        mode(&file),
        mode(&directory),
        entries(&directory),
    )
}

async fn connect(harness: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = harness.connect().await;
    let reply = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request("config-target", method, params),
    )
    .await
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(response) = reply else {
        panic!("expected {code:?}: {reply:?}")
    };
    assert_eq!(response.error.code, code, "{:?}", response.error);
    response.error.message
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected local result: {reply:?}")
    };
    response.result
}

#[tokio::test]
async fn remote_read_target_never_returns_local_config_or_reads_a_poison_file() {
    let harness = Harness::start("config-target-read").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文 ");
    for bytes in [DEFAULT_CONFIG.as_bytes(), &[0xff, 0xfe], b"[invalid"] {
        seed(&project, bytes);
        let before = snapshot(&project);
        for target in remote_targets() {
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    READ,
                    json!({"projectPath": project, "target": target}),
                )
                .await,
                ErrorCode::UnsupportedCapability,
            );
            assert_eq!(snapshot(&project), before);
        }
    }
    assert!(!harness.data_dir.join("settings.json").exists());
}

#[tokio::test]
async fn remote_init_target_never_creates_local_directories_or_repairs_existing_config() {
    let harness = Harness::start("config-target-init").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("new project 中文 ");
    for target in remote_targets() {
        failure(
            call(
                &mut reader,
                &mut writer,
                INIT,
                json!({"projectPath": project, "target": target}),
            )
            .await,
            ErrorCode::UnsupportedCapability,
        );
        assert!(!project.exists());
    }
    seed(&project, &[0xff, 0xfe]);
    let before = snapshot(&project);
    for target in remote_targets() {
        failure(
            call(
                &mut reader,
                &mut writer,
                INIT,
                json!({"projectPath": project, "target": target}),
            )
            .await,
            ErrorCode::UnsupportedCapability,
        );
        assert_eq!(snapshot(&project), before);
    }
    assert!(!project.join(".aeroric/attachments").exists());
}

#[tokio::test]
async fn remote_write_target_never_overwrites_or_initializes_a_local_project() {
    let harness = Harness::start("config-target-write").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文 ");
    seed(&project, DEFAULT_CONFIG.as_bytes());
    let before = snapshot(&project);
    for target in remote_targets() {
        failure(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "config": config(), "target": target}),
            )
            .await,
            ErrorCode::UnsupportedCapability,
        );
        assert_eq!(snapshot(&project), before);
    }
    let absent = harness.home.join("absent");
    for target in remote_targets() {
        failure(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": absent, "config": config(), "target": target}),
            )
            .await,
            ErrorCode::UnsupportedCapability,
        );
        assert!(!absent.exists());
    }
}

#[tokio::test]
async fn invalid_or_conflicting_targets_and_bad_typed_fields_refuse_before_local_io() {
    let harness = Harness::start("config-target-invalid").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文 ");
    seed(&project, DEFAULT_CONFIG.as_bytes());
    let before = snapshot(&project);
    for method in [READ, INIT, WRITE] {
        for target in [
            Value::Null,
            json!(false),
            json!("local"),
            json!({}),
            json!({"kind":"ssh"}),
            json!({"kind":"local","path":project.to_str().unwrap().trim_end()}),
        ] {
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    json!({"projectPath": project, "config": config(), "target": target}),
                )
                .await,
                ErrorCode::InvalidArgument,
            );
            assert_eq!(snapshot(&project), before);
        }
        failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": 42, "config": config(), "target": remote_targets()[0]}),
            )
            .await,
            ErrorCode::InvalidArgument,
        );
    }
    // Complete typed decoding precedes target handling, as well as all IO.
    failure(
        call(
            &mut reader,
            &mut writer,
            WRITE,
            json!({"projectPath": project, "config": null, "target": remote_targets()[0]}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert_eq!(snapshot(&project), before);
    assert!(!harness.home.join("project 中文").exists());
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                READ,
                json!({"projectPath": project})
            )
            .await
        ),
        serde_json::to_value(ProjectConfig::default()).unwrap()
    );
}

#[tokio::test]
async fn matching_local_and_omitted_targets_keep_legacy_behavior_authentication_and_writer_authority(
) {
    let harness = Harness::start("config-target-local").await;
    let (mut reader, mut writer) = harness.connect().await;
    let project = harness.home.join("project 中文 ");
    for method in [READ, INIT, WRITE] {
        failure(call(&mut reader, &mut writer, method, json!({
            "projectPath": project, "config": config(), "target": {"kind":"local","path":project}
        })).await, ErrorCode::Unauthenticated);
        assert!(!project.exists());
    }
    assert!(matches!(
        Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await,
        Frame::Success(_)
    ));
    let defaults = serde_json::to_value(ProjectConfig::default()).unwrap();
    for with_target in [true, false] {
        let local = harness.home.join(format!("project-{with_target} 中文 "));
        let mut params = json!({"projectPath": local, "config": config()});
        if with_target {
            params["target"] = json!({"kind":"local","path":local});
        }
        assert_eq!(
            success(call(&mut reader, &mut writer, READ, params.clone()).await),
            defaults
        );
        assert!(!local.exists(), "read remains non-initializing");
        assert_eq!(
            success(call(&mut reader, &mut writer, INIT, params.clone()).await),
            defaults
        );
        assert_eq!(
            entries(&local.join(".aeroric")),
            ["attachments", "config.toml"]
        );
        assert_eq!(
            success(call(&mut reader, &mut writer, WRITE, params.clone()).await),
            Value::Null
        );
        assert_eq!(
            success(call(&mut reader, &mut writer, READ, params).await),
            config()
        );
        assert!(!harness
            .home
            .join(format!("project-{with_target} 中文"))
            .exists());
    }

    let read_only = Harness::start_read_only("config-target-read-only").await;
    let (mut reader, mut writer) = connect(&read_only).await;
    let absent = read_only.home.join("absent");
    for method in [INIT, WRITE] {
        failure(call(&mut reader, &mut writer, method, json!({
            "projectPath": absent, "config": config(), "target": {"kind":"local","path":absent}
        })).await, ErrorCode::IoError);
        assert!(!absent.exists());
    }
}
