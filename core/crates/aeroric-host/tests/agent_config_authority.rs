//! Slice39: actual agent-config UDS, current writer and typed-settings refusal.
//! Even red uses only claude_gpt55 (no default path) or scratch custom profiles.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const PATH: &str = "settings.get_agent_config_file_path";
const READ: &str = "settings.read_agent_config_file";
const WRITE: &str = "settings.write_agent_config_file";
const AGENT: &str = "claude_gpt55";

struct Rig {
    host: Harness,
    tree: PathBuf,
    config: PathBuf,
}

impl Rig {
    async fn start(tag: &str, read_only: bool) -> Self {
        let host = if read_only {
            Harness::start_read_only(tag).await
        } else {
            Harness::start(tag).await
        };
        let tree = host.home.join("agent-config fixtures");
        fs::create_dir_all(&tree).unwrap();
        let config = tree.join("config 中文.json");
        fs::write(&config, "original\n配置").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tree, fs::Permissions::from_mode(0o750)).unwrap();
            fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        }
        let rig = Self { host, tree, config };
        rig.settings(rig.valid_settings());
        rig
    }

    fn valid_settings(&self) -> Value {
        json!({
            "claude_gpt55_config_path": self.config,
            "future_key": {"keep": [true, 42, "not a credential"]}
        })
    }

    fn settings(&self, fields: Value) {
        fs::write(
            self.host.data_dir.join("settings.json"),
            serde_json::to_vec_pretty(&fields).unwrap(),
        )
        .unwrap();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tree: tree(&self.tree),
            settings: node(&self.host.data_dir.join("settings.json")),
        }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply = Harness::handshake(&mut reader, &mut writer, &self.host.token, "cli").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }

    fn replace_lock(&self) {
        let lock = self.host.data_dir.join(".write.lock");
        fs::rename(&lock, lock.with_extension("lock-retired")).unwrap();
        fs::write(&lock, "new inode is not the held writer").unwrap();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Node {
    kind: &'static str,
    mode: u32,
    bytes: Vec<u8>,
}

fn node(path: &Path) -> Option<Node> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("snapshot {}: {error}", path.display()),
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = u32::from(metadata.permissions().readonly());
    assert!(!metadata.is_symlink(), "fixtures do not use links");
    Some(Node {
        kind: if metadata.is_dir() {
            "directory"
        } else {
            "file"
        },
        mode,
        bytes: if metadata.is_dir() {
            Vec::new()
        } else {
            fs::read(path).unwrap()
        },
    })
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Node> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Node>) {
        let entry = node(path).unwrap();
        let directory = entry.kind == "directory";
        out.insert(path.strip_prefix(root).unwrap().to_path_buf(), entry);
        if directory {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    tree: BTreeMap<PathBuf, Node>,
    settings: Option<Node>,
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
        &Harness::request("agent-config", method, params),
    )
    .await
}

fn success(frame: Frame) -> Value {
    let Frame::Success(reply) = frame else {
        panic!("expected success: {frame:?}");
    };
    reply.result
}

fn failure(frame: Frame, code: ErrorCode) -> String {
    let Frame::Failure(reply) = frame else {
        panic!("expected {code:?}: {frame:?}");
    };
    assert_eq!(reply.error.code, code, "{:?}", reply.error);
    assert!(!reply.error.retryable);
    reply.error.message
}

fn no_writer(frame: Frame) {
    let message = failure(frame, ErrorCode::IoError);
    assert!(message.contains("no write permission"), "{message}");
}

fn params() -> Value {
    json!({"agent": AGENT, "content": "replacement\n原样"})
}

#[tokio::test]
async fn read_only_cannot_overwrite_an_existing_agent_config() {
    let rig = Rig::start("ac-ro", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    let reply = call(&mut reader, &mut writer, WRITE, params()).await;
    assert_eq!(
        rig.snapshot(),
        before,
        "write refusal must preserve bytes/modes"
    );
    no_writer(reply);
}

#[tokio::test]
async fn read_only_cannot_create_agent_config_parent_directories() {
    let rig = Rig::start("ac-parent", true).await;
    let target = rig.tree.join("absent/nested/config.toml");
    rig.settings(json!({"claude_gpt55_config_path": target}));
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    let reply = call(&mut reader, &mut writer, WRITE, params()).await;
    assert_eq!(rig.snapshot(), before, "not even parents may be created");
    no_writer(reply);
}

#[tokio::test]
async fn a_replaced_lease_inode_revokes_writes_but_not_queries() {
    let rig = Rig::start("ac-inode", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    assert_eq!(
        success(call(&mut reader, &mut writer, WRITE, params()).await),
        Value::Null
    );
    rig.replace_lock();
    let before = rig.snapshot();
    let reply = call(
        &mut reader,
        &mut writer,
        WRITE,
        json!({"agent": AGENT, "content": "must not replace"}),
    )
    .await;
    assert_eq!(rig.snapshot(), before);
    no_writer(reply);
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        "replacement\n原样"
    );
    assert_eq!(
        success(call(&mut reader, &mut writer, PATH, json!({"agent": AGENT})).await),
        json!(rig.config)
    );
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn both_queries_remain_available_read_only_and_do_not_repair_files() {
    let rig = Rig::start("ac-queries", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    assert_eq!(
        success(call(&mut reader, &mut writer, PATH, json!({"agent": AGENT})).await),
        json!(rig.config)
    );
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        "original\n配置"
    );
    assert_eq!(rig.snapshot(), before);
    fs::remove_file(&rig.config).unwrap();
    let before = rig.snapshot();
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        Value::Null
    );
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn malformed_typed_settings_never_become_default_paths_or_null_results() {
    let rig = Rig::start("ac-types", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    let mut failures = Vec::new();
    for (field, value) in [
        ("claude_gpt55_config_path", json!(false)),
        ("custom_agents", json!(false)),
        ("custom_agents", json!([{"id": "missing-required-fields"}])),
        ("agent_proxy_enabled", json!({"claude": "yes"})),
        ("proxy_settings", json!(null)),
        ("send_shortcut", json!([])),
        ("terminal_shift_enter_newline", json!("false")),
        ("dsh_reasoning_efforts", json!(42)),
    ] {
        let mut settings = rig.valid_settings();
        settings[field] = value;
        rig.settings(settings);
        let before = rig.snapshot();
        for method in [PATH, READ, WRITE] {
            let reply = call(&mut reader, &mut writer, method, params()).await;
            let typed_error = matches!(&reply, Frame::Failure(reply)
                if reply.error.code == ErrorCode::IoError
                    && reply.error.message.contains("invalid stored app settings")
                    && !reply.error.retryable);
            if !typed_error {
                failures.push(format!("{field}/{method}: {reply:?}"));
            }
            assert_eq!(rig.snapshot(), before, "{field}/{method}");
        }
    }
    assert!(
        failures.is_empty(),
        "all24 pairs ran:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
async fn corrupt_settings_bytes_and_non_objects_remain_errors_without_repair() {
    let rig = Rig::start("ac-corrupt", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    for bytes in [
        b"{broken".as_slice(),
        b"null",
        b"[]",
        b"42",
        b"",
        b"\xff\xfe",
    ] {
        fs::write(rig.host.data_dir.join("settings.json"), bytes).unwrap();
        let before = rig.snapshot();
        for method in [PATH, READ, WRITE] {
            failure(
                call(&mut reader, &mut writer, method, params()).await,
                ErrorCode::IoError,
            );
            assert_eq!(rig.snapshot(), before);
        }
    }
}

#[tokio::test]
async fn string_parameter_decoding_precedes_writer_and_settings_io() {
    let rig = Rig::start("ac-params", true).await;
    fs::write(rig.host.data_dir.join("settings.json"), b"\xff").unwrap();
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    for method in [PATH, READ, WRITE] {
        for bad in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"agent": null, "content": "raw"}),
            json!({"agent": 3, "content": "raw"}),
        ] {
            let message = failure(
                call(&mut reader, &mut writer, method, bad).await,
                ErrorCode::InvalidArgument,
            );
            assert_eq!(message, "Missing param: agent");
        }
    }
    for bad in [
        json!({"agent": AGENT}),
        json!({"agent": AGENT, "content": null}),
        json!({"agent": AGENT, "content": []}),
        json!({"agent": AGENT, "content": true}),
    ] {
        assert_eq!(
            failure(
                call(&mut reader, &mut writer, WRITE, bad).await,
                ErrorCode::InvalidArgument
            ),
            "Missing param: content"
        );
    }
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn valid_string_writes_require_authority_before_settings_or_agent_resolution() {
    let rig = Rig::start("ac-order", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    for bytes in [b"{broken".as_slice(), b"{\"custom_agents\":false}", b"{}"] {
        fs::write(rig.host.data_dir.join("settings.json"), bytes).unwrap();
        let before = rig.snapshot();
        for agent in [AGENT, "unknown", " ", ""] {
            no_writer(
                call(
                    &mut reader,
                    &mut writer,
                    WRITE,
                    json!({"agent": agent, "content": ""}),
                )
                .await,
            );
            assert_eq!(rig.snapshot(), before);
        }
    }
}

#[tokio::test]
async fn permitted_write_preserves_raw_content_and_settings_and_returns_null() {
    let rig = Rig::start("ac-write", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    let settings_before = node(&rig.host.data_dir.join("settings.json"));
    // Slice41 corrects host direct-write modes to the actual legacy rename policy.
    let control = rig.host.home.join("legacy-atomic-mode-control");
    aeroric_core::storage::atomic::write_legacy_atomic(&control, "control").unwrap();
    let config_mode = node(&control).unwrap().mode;
    let directory_before = node(&rig.tree);
    let content = " \nnot parsed as JSON\0原文\r\n";
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"agent": AGENT, "content": content}),
            )
            .await
        ),
        Value::Null
    );
    assert_eq!(fs::read(&rig.config).unwrap(), content.as_bytes());
    assert_eq!(node(&rig.config).unwrap().mode, config_mode);
    assert_eq!(node(&rig.tree), directory_before);
    assert_eq!(
        node(&rig.host.data_dir.join("settings.json")),
        settings_before
    );
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        content
    );
    assert_eq!(fs::read_dir(&rig.tree).unwrap().count(), 1);
}

#[tokio::test]
async fn absent_settings_and_blank_paths_keep_the_existing_wire_and_unknown_agent_error() {
    let rig = Rig::start("ac-defaults", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    fs::remove_file(rig.host.data_dir.join("settings.json")).unwrap();
    for fields in [
        None,
        Some(json!({})),
        Some(json!({"claude_gpt55_config_path": " \t"})),
    ] {
        if let Some(fields) = fields {
            rig.settings(fields);
        }
        let before = rig.snapshot();
        assert_eq!(
            success(call(&mut reader, &mut writer, PATH, json!({"agent": AGENT})).await),
            ""
        );
        assert_eq!(
            success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
            Value::Null
        );
        assert_eq!(
            failure(
                call(&mut reader, &mut writer, WRITE, params()).await,
                ErrorCode::InvalidArgument
            ),
            "Agent config file path is not configured"
        );
        for method in [PATH, READ, WRITE] {
            assert_eq!(
                failure(
                    call(
                        &mut reader,
                        &mut writer,
                        method,
                        json!({"agent": " unknown ", "content": "ignored"}),
                    )
                    .await,
                    ErrorCode::InvalidArgument
                ),
                "Unknown agent:  unknown "
            );
        }
        assert_eq!(rig.snapshot(), before);
    }
}

#[tokio::test]
async fn custom_config_paths_keep_trim_and_existing_io_errors() {
    let rig = Rig::start("ac-custom", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.settings(json!({
        "custom_agents": [{
            "id": "fixture", "label": "Synthetic", "family": "claude",
            "path": format!(" \t{}  ", rig.config.display())
        }]
    }));
    fs::write(&rig.config, b"\xff").unwrap();
    let before = rig.snapshot();
    assert_eq!(
        success(call(&mut reader, &mut writer, PATH, json!({"agent": "fixture"})).await),
        json!(rig.config)
    );
    failure(
        call(&mut reader, &mut writer, READ, json!({"agent": "fixture"})).await,
        ErrorCode::IoError,
    );
    assert_eq!(rig.snapshot(), before);
    fs::remove_file(&rig.config).unwrap();
    fs::create_dir(&rig.config).unwrap();
    let before = rig.snapshot();
    for method in [READ, WRITE] {
        failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"agent": "fixture", "content": "must not overwrite a directory"}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert_eq!(rig.snapshot(), before);
    }
}

#[tokio::test]
async fn one_connection_observes_typed_corruption_and_recovery_without_repair() {
    let rig = Rig::start("ac-recovery", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        "original\n配置"
    );
    let mut invalid = rig.valid_settings();
    invalid["custom_agents"] = json!(false);
    rig.settings(invalid);
    let before = rig.snapshot();
    failure(
        call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await,
        ErrorCode::IoError,
    );
    assert_eq!(rig.snapshot(), before);
    rig.settings(rig.valid_settings());
    assert_eq!(
        success(call(&mut reader, &mut writer, WRITE, params()).await),
        Value::Null
    );
    assert_eq!(
        success(call(&mut reader, &mut writer, READ, json!({"agent": AGENT})).await),
        "replacement\n原样"
    );
}
