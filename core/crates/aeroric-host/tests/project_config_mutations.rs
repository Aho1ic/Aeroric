//! Slice 27: real authenticated project-local init/write, synthetic paths only.

use std::fs;
use std::path::Path;

use aeroric_core::settings::project_config::{read_project_config, ProjectConfig, DEFAULT_CONFIG};
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const INIT: &str = "settings.init_project_config";
const WRITE: &str = "settings.write_project_config";
const READ: &str = "settings.read_project_config";

fn replacement() -> Value {
    json!({
        "agent": {
            "default": " fixture-agent ",
            "default_permission_mode": "ask",
            "prompt_prefix": "中文\nsecond line"
        },
        "git": {"commit_prompt": " fixture commit ", "commit_message_timeout_secs": 42},
        "editor": {
            "format_on_save": true,
            "file_browser_sort": {"field": "unknown-field", "direction": "arbitrary"},
            "sftp_sort": {"field": "size", "direction": "asc"}
        }
    })
}

fn entries(path: &Path) -> Vec<String> {
    let mut result: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    result.sort();
    result
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
        &Harness::request("config-mutation", method, params),
    )
    .await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected mutation success: {reply:?}");
    };
    response.result
}

fn failure(reply: Frame, code: ErrorCode) {
    let Frame::Failure(response) = reply else {
        panic!("expected {code:?}: {reply:?}");
    };
    assert_eq!(response.error.code, code, "{:?}", response.error);
}

#[tokio::test]
async fn init_creates_template_and_attachments_but_never_repairs_existing_toml() {
    let harness = Harness::start("config-init").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("new project 中文 ");
    let directory = project.join(".aeroric");
    let config = directory.join("config.toml");
    let defaults = serde_json::to_value(ProjectConfig::default()).unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                INIT,
                json!({"projectPath": project})
            )
            .await
        ),
        defaults
    );
    assert_eq!(fs::read_to_string(&config).unwrap(), DEFAULT_CONFIG);
    assert_eq!(entries(&directory), ["attachments", "config.toml"]);

    for text in [
        "# original comment\n[agent]\ndefault = 'custom'\n[git]\ncommit_prompt = 'kept'\n",
        "[malformed",
        "",
    ] {
        fs::write(&config, text).unwrap();
        let before = fs::metadata(&config).unwrap().modified().unwrap();
        let expected =
            serde_json::to_value(read_project_config(project.to_str().unwrap()).unwrap()).unwrap();
        assert_eq!(
            success(
                call(
                    &mut reader,
                    &mut writer,
                    INIT,
                    json!({"projectPath": project})
                )
                .await
            ),
            expected
        );
        assert_eq!(fs::read_to_string(&config).unwrap(), text);
        assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), before);
    }
    assert!(!harness.data_dir.join("settings.json").exists());
    assert!(!harness.data_dir.join("projects.json").exists());
}

#[tokio::test]
async fn write_is_a_complete_replacement_at_the_original_path_without_attachments() {
    let harness = Harness::start("config-write").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文  ");
    let decoy = harness.home.join("project 中文");
    fs::create_dir_all(decoy.join(".aeroric")).unwrap();
    fs::write(decoy.join(".aeroric/config.toml"), "decoy").unwrap();
    fs::create_dir_all(project.join(".aeroric")).unwrap();
    let config = project.join(".aeroric/config.toml");
    fs::write(&config, "# remove old comment\nunknown = 'old'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(project.join(".aeroric"), fs::Permissions::from_mode(0o750)).unwrap();
    }
    let expected = replacement();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "config": expected})
            )
            .await
        ),
        Value::Null
    );
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
        expected
    );
    assert_eq!(entries(&project.join(".aeroric")), ["config.toml"]);
    let text = fs::read_to_string(&config).unwrap();
    assert!(!text.contains("remove old comment") && !text.contains("unknown = 'old'"));
    assert_eq!(
        fs::read_to_string(decoy.join(".aeroric/config.toml")).unwrap(),
        "decoy"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let control = harness.home.join("inherited-mode");
        fs::write(&control, "control").unwrap();
        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            fs::metadata(control).unwrap().permissions().mode() & 0o777
        );
        assert_eq!(
            fs::metadata(project.join(".aeroric"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
    }
}

#[tokio::test]
async fn invalid_params_precede_io_and_real_io_errors_preserve_legacy_side_effects() {
    let harness = Harness::start("config-errors").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("must-stay-absent");
    for method in [INIT, WRITE] {
        for params in [Value::Null, json!({}), json!({"projectPath": 42})] {
            failure(
                call(&mut reader, &mut writer, method, params).await,
                ErrorCode::InvalidArgument,
            );
        }
    }
    for bad_config in [Value::Null, json!({}), json!([]), json!({"agent": 4})] {
        failure(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "config": bad_config}),
            )
            .await,
            ErrorCode::InvalidArgument,
        );
        assert!(!project.exists());
    }
    failure(
        call(
            &mut reader,
            &mut writer,
            WRITE,
            json!({"projectPath": project}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(!project.exists());

    let directory = project.join(".aeroric");
    let file = directory.join("config.toml");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&file, [0xff, 0xfe]).unwrap();
    failure(
        call(
            &mut reader,
            &mut writer,
            INIT,
            json!({"projectPath": project}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(
        directory.join("attachments").is_dir(),
        "legacy init creates attachments before read"
    );
    assert_eq!(fs::read(&file).unwrap(), [0xff, 0xfe]);

    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    fs::write(file.join("keep"), "unchanged").unwrap();
    failure(
        call(
            &mut reader,
            &mut writer,
            WRITE,
            json!({"projectPath": project, "config": replacement()}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(fs::read_to_string(file.join("keep")).unwrap(), "unchanged");
    assert_eq!(
        entries(&directory),
        ["attachments", "config.toml"],
        "failed rename cleans temp"
    );
    fs::remove_file(file.join("keep")).unwrap();
    fs::remove_dir(&file).unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "config": replacement()})
            )
            .await
        ),
        Value::Null
    );
}

#[tokio::test]
async fn read_only_service_context_refuses_both_mutations_before_any_directory_or_file_change() {
    let harness = Harness::start_read_only("config-no-writer").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let missing = harness.home.join("absent");
    let existing = harness.home.join("existing");
    let directory = existing.join(".aeroric");
    fs::create_dir_all(&directory).unwrap();
    let file = directory.join("config.toml");
    fs::write(&file, "original bytes").unwrap();
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    for project in [&missing, &existing] {
        for method in [INIT, WRITE] {
            let reply = call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": project, "config": replacement()}),
            )
            .await;
            let Frame::Failure(response) = reply else {
                panic!("a read-only service must refuse: {reply:?}");
            };
            assert_eq!(response.error.code, ErrorCode::IoError);
            assert!(
                response.error.message.contains("no write permission"),
                "{:?}",
                response.error
            );
        }
    }
    assert!(!missing.exists());
    assert_eq!(entries(&directory), ["config.toml"]);
    assert_eq!(fs::read_to_string(&file).unwrap(), "original bytes");
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
}

#[tokio::test]
async fn unauthenticated_mutations_never_touch_the_project() {
    let harness = Harness::start("config-no-auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    let project = harness.home.join("unauthenticated-project");
    for method in [INIT, WRITE] {
        failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": project, "config": replacement()}),
            )
            .await,
            ErrorCode::Unauthenticated,
        );
    }
    assert!(!project.exists());
}

#[tokio::test]
async fn two_clients_initializing_and_saving_cannot_replace_an_explicit_save_with_defaults() {
    let harness = Harness::start("config-concurrent").await;
    let (mut reader_a, mut writer_a) = connect(&harness).await;
    let (mut reader_b, mut writer_b) = connect(&harness).await;
    for iteration in 0..16 {
        let project = harness.home.join(format!("race-{iteration}"));
        let (initialized, written) = tokio::join!(
            call(
                &mut reader_a,
                &mut writer_a,
                INIT,
                json!({"projectPath": project})
            ),
            call(
                &mut reader_b,
                &mut writer_b,
                WRITE,
                json!({"projectPath": project, "config": replacement()})
            ),
        );
        let initialized = success(initialized);
        assert!(
            initialized == replacement()
                || initialized == serde_json::to_value(ProjectConfig::default()).unwrap()
        );
        assert_eq!(success(written), Value::Null);
        assert_eq!(
            success(
                call(
                    &mut reader_a,
                    &mut writer_a,
                    READ,
                    json!({"projectPath": project})
                )
                .await
            ),
            replacement()
        );
        assert_eq!(
            entries(&project.join(".aeroric")),
            ["attachments", "config.toml"]
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn existing_directory_and_leaf_symlink_semantics_remain_distinct() {
    let harness = Harness::start("config-symlink").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("linked-project");
    let directory = harness.home.join("linked-config-directory");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&directory).unwrap();
    std::os::unix::fs::symlink(&directory, project.join(".aeroric")).unwrap();
    let target = harness.home.join("config-target.toml");
    fs::write(&target, DEFAULT_CONFIG).unwrap();
    let link = directory.join("config.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    success(
        call(
            &mut reader,
            &mut writer,
            INIT,
            json!({"projectPath": project}),
        )
        .await,
    );
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), DEFAULT_CONFIG);
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "config": replacement()})
            )
            .await
        ),
        Value::Null
    );
    assert!(!fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(project.join(".aeroric"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), DEFAULT_CONFIG);
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
        replacement()
    );
}
