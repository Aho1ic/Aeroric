//! Slice 26: frozen local project-config reads through a real authenticated host.
//! No HOME lookup, agents, credentials, remote systems or production data.

use std::fs;
use std::path::Path;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const METHOD: &str = "settings.read_project_config";
const RICH_TOML: &str = r#"
[agent]
default = "fixture-agent"
default_permission_mode = "ask"
prompt_prefix = "只读 fixture"
[git]
commit_prompt = "Fixture prompt"
commit_message_timeout_secs = 42
[editor]
format_on_save = true
file_browser_sort = { field = "name", direction = "asc" }
sftp_sort = { field = "size", direction = "asc" }
"#;
const DEFAULT_PROMPT: &str = "You are a git commit message generator. Based on the provided git diff, write a concise and descriptive commit message. Follow these rules:\n1. Use the imperative mood (e.g., \"Add feature\" not \"Added feature\")\n2. First line: type(scope): short summary (50 chars or less)\n   Types: feat, fix, docs, style, refactor, test, chore\n3. If needed, add a blank line then a brief body explaining what and why\n4. Output ONLY the commit message text, no explanations or markdown formatting";

fn expected_default() -> Value {
    json!({
        "agent": {
            "default": "claude",
            "default_permission_mode": "full_access",
            "prompt_prefix": ""
        },
        "git": {
            "commit_prompt": DEFAULT_PROMPT,
            "commit_message_timeout_secs": 15
        },
        "editor": {
            "format_on_save": false,
            "file_browser_sort": {"field": "modified", "direction": "desc"},
            "sftp_sort": {"field": "modified", "direction": "desc"}
        }
    })
}

fn expected_rich() -> Value {
    json!({
        "agent": {
            "default": "fixture-agent",
            "default_permission_mode": "ask",
            "prompt_prefix": "只读 fixture"
        },
        "git": {
            "commit_prompt": "Fixture prompt",
            "commit_message_timeout_secs": 42
        },
        "editor": {
            "format_on_save": true,
            "file_browser_sort": {"field": "name", "direction": "asc"},
            "sftp_sort": {"field": "size", "direction": "asc"}
        }
    })
}

fn write_config(project: &Path, bytes: &[u8]) {
    let directory = project.join(".aeroric");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("config.toml"), bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(
            directory.join("config.toml"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
}

fn assert_unchanged(project: &Path, bytes: &[u8]) {
    let directory = project.join(".aeroric");
    assert_eq!(fs::read(directory.join("config.toml")).unwrap(), bytes);
    let mut entries: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, vec![std::ffi::OsString::from("config.toml")]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(directory.join("config.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

async fn connect(harness: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");
    (reader, writer)
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request(id, METHOD, params)).await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected successful project config: {reply:?}");
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
async fn rich_and_legacy_configs_preserve_wire_bytes_modes_and_directories() {
    let harness = Harness::start("project-config-rich").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("fixture-project");
    write_config(&project, RICH_TOML.as_bytes());
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "rich",
                json!({"projectPath": project})
            )
            .await
        ),
        expected_rich()
    );
    assert_unchanged(&project, RICH_TOML.as_bytes());

    let legacy = b"[agent]\ndefault = 'legacy'\n[git]\ncommit_prompt = 'old prompt'\n";
    write_config(&project, legacy);
    let mut expected = expected_default();
    expected["agent"]["default"] = json!("legacy");
    expected["git"]["commit_prompt"] = json!("old prompt");
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "legacy",
                json!({"projectPath": project})
            )
            .await
        ),
        expected
    );
    assert_unchanged(&project, legacy);
}

#[tokio::test]
async fn missing_and_invalid_toml_return_defaults_without_repair_or_creation() {
    let harness = Harness::start("project-config-default").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("missing-project");
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "missing",
                json!({"projectPath": project})
            )
            .await
        ),
        expected_default()
    );
    assert!(!project.exists());

    for (id, bytes) in [
        ("empty", &b""[..]),
        ("invalid-toml", &b"[[not toml"[..]),
        (
            "missing-required-section",
            &b"[agent]\ndefault = 'ignored'\n"[..],
        ),
    ] {
        write_config(&project, bytes);
        assert_eq!(
            success(
                query(
                    &mut reader,
                    &mut writer,
                    id,
                    json!({"projectPath": project})
                )
                .await
            ),
            expected_default(),
            "{id}"
        );
        assert_unchanged(&project, bytes);
    }
}

#[tokio::test]
async fn invalid_params_and_io_errors_are_distinct_and_connection_recovers() {
    let harness = Harness::start("project-config-errors").await;
    let (mut reader, mut writer) = connect(&harness).await;
    for (id, params) in [
        ("missing", json!({})),
        ("null", Value::Null),
        ("array", json!([])),
        ("null-path", json!({"projectPath": null})),
        ("numeric-path", json!({"projectPath": 42})),
        ("object-path", json!({"projectPath": {}})),
    ] {
        failure(
            query(&mut reader, &mut writer, id, params).await,
            ErrorCode::InvalidArgument,
        );
    }

    let project = harness.home.join("io-project");
    let config = project.join(".aeroric").join("config.toml");
    fs::create_dir_all(&config).unwrap();
    failure(
        query(
            &mut reader,
            &mut writer,
            "directory",
            json!({"projectPath": project}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(config.is_dir());
    fs::remove_dir(&config).unwrap();
    write_config(&project, &[0xff, 0xfe]);
    failure(
        query(
            &mut reader,
            &mut writer,
            "utf8",
            json!({"projectPath": project}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_unchanged(&project, &[0xff, 0xfe]);

    write_config(&project, RICH_TOML.as_bytes());
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "recovered",
                json!({"projectPath": project})
            )
            .await
        ),
        expected_rich()
    );
    assert_unchanged(&project, RICH_TOML.as_bytes());
}

#[tokio::test]
async fn original_space_paths_and_existing_symlink_semantics_are_not_rewritten() {
    let harness = Harness::start("project-config-paths").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join(" original path ");
    let decoy = harness.home.join(" original path");
    write_config(&project, RICH_TOML.as_bytes());
    write_config(&decoy, b"invalid decoy TOML");
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "untrimmed",
                json!({"projectPath": project})
            )
            .await
        ),
        expected_rich()
    );
    assert_unchanged(&project, RICH_TOML.as_bytes());
    assert_unchanged(&decoy, b"invalid decoy TOML");

    #[cfg(unix)]
    {
        let linked = harness.home.join("linked-project");
        fs::create_dir(&linked).unwrap();
        std::os::unix::fs::symlink(project.join(".aeroric"), linked.join(".aeroric")).unwrap();
        assert_eq!(
            success(
                query(
                    &mut reader,
                    &mut writer,
                    "symlink",
                    json!({"projectPath": linked})
                )
                .await
            ),
            expected_rich()
        );
        assert!(fs::symlink_metadata(linked.join(".aeroric"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_unchanged(&project, RICH_TOML.as_bytes());
    }
}
