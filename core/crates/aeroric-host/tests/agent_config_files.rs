//! Slice41: legacy leaf-replacement and explicit-path IO through real UDS.
use std::fs;
use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

mod host_harness;
use host_harness::Harness;

const READ: &str = "settings.read_agent_config_file";
const WRITE: &str = "settings.write_agent_config_file";
const RAW: &str = "raw\n配置\0\r\n";

struct Rig {
    host: Harness,
    root: PathBuf,
}

impl Rig {
    async fn new(tag: &str, read_only: bool) -> Self {
        let host = if read_only {
            Harness::start_read_only(tag).await
        } else {
            Harness::start(tag).await
        };
        let root = host.home.join("config file fixtures");
        fs::create_dir(&root).unwrap();
        Self { host, root }
    }

    async fn call(&self, method: &str, path: &Path, content: &str) -> Frame {
        // Even malformed/default handling is safe: this built-in has no fallback.
        let settings = serde_json::to_vec_pretty(&json!({
            "claude_gpt55_config_path": path,
            "future_field": {"retained": true}
        }))
        .unwrap();
        let file = self.host.data_dir.join("settings.json");
        fs::write(&file, &settings).unwrap();
        let (mut reader, mut writer) = self.host.connect().await;
        let auth = Harness::handshake(&mut reader, &mut writer, &self.host.token, "cli").await;
        assert!(matches!(auth, Frame::Success(_)), "{auth:?}");
        let reply = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(
                "config-files",
                method,
                json!({
                    "agent": "claude_gpt55", "content": content
                }),
            ),
        )
        .await;
        assert_eq!(
            fs::read(file).unwrap(),
            settings,
            "config IO must not rewrite settings"
        );
        reply
    }
}

fn success(reply: Frame) -> Value {
    let Frame::Success(reply) = reply else {
        panic!("expected legacy-compatible success: {reply:?}");
    };
    reply.result
}

fn io_error(reply: Frame) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected IO refusal: {reply:?}");
    };
    assert_eq!(reply.error.code, ErrorCode::IoError);
    assert!(!reply.error.retryable);
    reply.error.message
}

fn entries(path: &Path) -> Vec<String> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    entries.sort();
    entries
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_symlink_is_replaced_without_writing_its_target() {
    let rig = Rig::new("acf-link", false).await;
    let target = rig.root.join("target.txt");
    let leaf = rig.root.join("config.json");
    fs::write(&target, "keep target").unwrap();
    std::os::unix::fs::symlink(&target, &leaf).unwrap();
    assert_eq!(success(rig.call(WRITE, &leaf, RAW).await), Value::Null);
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep target");
    assert!(!fs::symlink_metadata(&leaf).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&leaf).unwrap(), RAW);
    assert_eq!(entries(&rig.root), ["config.json", "target.txt"]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_dangling_symlink_is_replaced_without_creating_its_target() {
    let rig = Rig::new("acf-dangling", false).await;
    let target = rig.root.join("must-stay-absent");
    let leaf = rig.root.join("config.yaml");
    std::os::unix::fs::symlink(&target, &leaf).unwrap();
    assert_eq!(success(rig.call(WRITE, &leaf, RAW).await), Value::Null);
    assert!(!target.exists(), "do not follow a dangling config leaf");
    assert!(!fs::symlink_metadata(&leaf).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&leaf).unwrap(), RAW);
    assert_eq!(entries(&rig.root), ["config.yaml"]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_directory_symlink_is_a_replaceable_leaf_not_the_actual_directory() {
    let rig = Rig::new("acf-dir-link", false).await;
    let target = rig.root.join("directory");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), "keep directory").unwrap();
    let leaf = rig.root.join("config.toml");
    std::os::unix::fs::symlink(&target, &leaf).unwrap();
    assert_eq!(success(rig.call(WRITE, &leaf, RAW).await), Value::Null);
    assert!(fs::symlink_metadata(&leaf).unwrap().is_file());
    assert_eq!(fs::read_to_string(&leaf).unwrap(), RAW);
    assert_eq!(
        fs::read_to_string(target.join("sentinel")).unwrap(),
        "keep directory"
    );
    assert_eq!(entries(&target), ["sentinel"]);
    assert_eq!(entries(&rig.root), ["config.toml", "directory"]);
}

#[tokio::test]
async fn writing_one_hardlink_does_not_modify_the_other_name() {
    let rig = Rig::new("acf-hardlink", false).await;
    let target = rig.root.join("alias.txt");
    let leaf = rig.root.join("config.ini");
    fs::write(&target, "keep alias").unwrap();
    fs::hard_link(&target, &leaf).unwrap();
    assert_eq!(success(rig.call(WRITE, &leaf, RAW).await), Value::Null);
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep alias");
    assert_eq!(fs::read_to_string(&leaf).unwrap(), RAW);
    assert_eq!(entries(&rig.root), ["alias.txt", "config.ini"]);
}

#[cfg(unix)]
#[tokio::test]
async fn reads_keep_existing_link_utf8_missing_and_directory_semantics() {
    let rig = Rig::new("acf-read", true).await;
    let target = rig.root.join("target.txt");
    let leaf = rig.root.join("config.json");
    fs::write(&target, RAW).unwrap();
    std::os::unix::fs::symlink(&target, &leaf).unwrap();
    assert_eq!(success(rig.call(READ, &leaf, "").await), RAW);
    assert_eq!(fs::read_link(&leaf).unwrap(), target);
    fs::write(&target, b"\xff").unwrap();
    io_error(rig.call(READ, &leaf, "").await);
    assert_eq!(fs::read(&target).unwrap(), b"\xff");
    fs::remove_file(&target).unwrap();
    assert_eq!(success(rig.call(READ, &leaf, "").await), Value::Null);
    assert!(fs::symlink_metadata(&leaf).unwrap().is_symlink());
    fs::create_dir(&target).unwrap();
    io_error(rig.call(READ, &leaf, "").await);
    assert!(fs::symlink_metadata(&leaf).unwrap().is_symlink());
    assert!(entries(&target).is_empty());
}

#[tokio::test]
async fn permitted_writes_create_parents_keep_raw_bytes_and_leave_no_temporaries() {
    let rig = Rig::new("acf-parents", false).await;
    let parent = rig.root.join("nested/with spaces");
    let leaf = parent.join("config.txt");
    for content in [RAW, "", "not JSON [ raw ]"] {
        assert_eq!(success(rig.call(WRITE, &leaf, content).await), Value::Null);
        assert_eq!(fs::read(&leaf).unwrap(), content.as_bytes());
        assert_eq!(success(rig.call(READ, &leaf, "").await), content);
        assert_eq!(entries(&parent), ["config.txt"]);
    }
    assert_eq!(entries(&rig.root), ["nested"]);
}

#[tokio::test]
async fn an_actual_directory_refuses_and_failed_rename_leaks_no_temp_copy() {
    let rig = Rig::new("acf-dir", false).await;
    let leaf = rig.root.join("config.json");
    fs::create_dir(&leaf).unwrap();
    fs::write(leaf.join("sentinel"), "unchanged").unwrap();
    io_error(rig.call(WRITE, &leaf, RAW).await);
    assert_eq!(
        fs::read_to_string(leaf.join("sentinel")).unwrap(),
        "unchanged"
    );
    assert_eq!(entries(&leaf), ["sentinel"]);
    assert_eq!(entries(&rig.root), ["config.json"]);
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_refusal_does_not_replace_a_leaf_or_create_parents() {
    let rig = Rig::new("acf-ro", true).await;
    let target = rig.root.join("target.txt");
    let leaf = rig.root.join("config.json");
    fs::write(&target, "keep readonly target").unwrap();
    std::os::unix::fs::symlink(&target, &leaf).unwrap();
    assert!(io_error(rig.call(WRITE, &leaf, RAW).await).contains("no write permission"));
    assert_eq!(fs::read_link(&leaf).unwrap(), target);
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep readonly target");
    assert!(
        io_error(rig.call(WRITE, &rig.root.join("absent/config"), RAW).await)
            .contains("no write permission")
    );
    assert_eq!(entries(&rig.root), ["config.json", "target.txt"]);
}
