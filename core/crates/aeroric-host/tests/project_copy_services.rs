//! Project-scoped copy destination authority, through real authenticated UDS.
//! "Outside" always means a sibling scratch directory, never real user files.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

mod host_harness;
use host_harness::Harness;

struct Rig {
    host: Harness,
    project: PathBuf,
    outside: PathBuf,
}

impl Rig {
    async fn start(tag: &str, read_only: bool) -> Self {
        let host = if read_only {
            Harness::start_read_only(tag).await
        } else {
            Harness::start(tag).await
        };
        let project = host.data_dir.join("copy project");
        let outside = host.data_dir.join("outside project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(project.join("source.txt"), "source bytes\n").unwrap();
        fs::set_permissions(
            project.join("source.txt"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        fs::write(outside.join("sentinel.txt"), "outside unchanged\n").unwrap();
        fs::write(
            host.data_dir.join("projects.json"),
            serde_json::to_vec(&json!([{"id": "p", "name": "copy", "path": project}])).unwrap(),
        )
        .unwrap();
        Self {
            host,
            project: project.canonicalize().unwrap(),
            outside: outside.canonicalize().unwrap(),
        }
    }

    async fn copy(&self, target: &str, sources: &[&str]) -> Frame {
        let (mut reader, mut writer) = self.host.connect().await;
        success(Harness::handshake(&mut reader, &mut writer, &self.host.token, "copy-test").await);
        Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(
                "project-copy",
                "files.copy_paths_to_directory",
                json!({"projectId": "p", "targetDir": target, "paths": sources}),
            ),
        )
        .await
    }

    fn outside_unchanged(&self) {
        assert_eq!(
            fs::read(self.outside.join("sentinel.txt")).unwrap(),
            b"outside unchanged\n"
        );
        assert_eq!(
            fs::read_dir(&self.outside).unwrap().count(),
            1,
            "no outside files/directories"
        );
        assert_eq!(
            fs::read(self.project.join("source.txt")).unwrap(),
            b"source bytes\n"
        );
    }
}

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(response) => response.result,
        other => panic!("expected success: {other:?}"),
    }
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(response) = reply else {
        panic!("expected refusal: {reply:?}")
    };
    assert_eq!(response.id, "project-copy");
    assert_eq!(response.error.code, code, "{response:?}");
    assert!(!response.error.retryable);
    response.error.message
}

#[tokio::test]
async fn a_destination_directory_link_cannot_write_outside_the_project() {
    let rig = Rig::start("copy-link", false).await;
    symlink(&rig.outside, rig.project.join("linked")).unwrap();
    failure(
        rig.copy("linked", &["source.txt"]).await,
        ErrorCode::InvalidArgument,
    );
    assert_eq!(
        fs::read_link(rig.project.join("linked")).unwrap(),
        rig.outside
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn a_linked_ancestor_cannot_create_an_outside_missing_suffix() {
    let rig = Rig::start("copy-parent", false).await;
    symlink(&rig.outside, rig.project.join("linked")).unwrap();
    failure(
        rig.copy("linked/missing/nested", &["source.txt"]).await,
        ErrorCode::InvalidArgument,
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn a_dangling_destination_leaf_is_an_overwrite_not_a_new_file() {
    let rig = Rig::start("copy-leaf", false).await;
    let directory = rig.project.join("out");
    fs::create_dir(&directory).unwrap();
    let outside_file = rig.outside.join("not-created.txt");
    symlink(&outside_file, directory.join("source.txt")).unwrap();
    let message = failure(
        rig.copy("out", &["source.txt"]).await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("Refusing to overwrite"), "{message}");
    assert_eq!(
        fs::read_link(directory.join("source.txt")).unwrap(),
        outside_file
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn in_root_directory_links_keep_reply_spelling_modes_and_non_overwrite() {
    let rig = Rig::start("copy-inside", false).await;
    fs::create_dir(rig.project.join("real")).unwrap();
    symlink(rig.project.join("real"), rig.project.join("alias")).unwrap();
    let expected = rig.project.join("alias/source.txt");
    assert_eq!(
        success(rig.copy("alias", &["source.txt"]).await),
        json!({"copied": [expected]})
    );
    assert_eq!(fs::read(&expected).unwrap(), b"source bytes\n");
    assert_eq!(
        fs::metadata(&expected).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::write(&expected, "do not overwrite\n").unwrap();
    let message = failure(
        rig.copy("alias", &["source.txt"]).await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("Refusing to overwrite"));
    assert_eq!(fs::read(&expected).unwrap(), b"do not overwrite\n");
    rig.outside_unchanged();
}

#[tokio::test]
async fn missing_nested_destinations_and_later_source_partial_failure_are_preserved() {
    let rig = Rig::start("copy-nested", false).await;
    assert_eq!(
        success(rig.copy("new/nested", &["source.txt"]).await),
        json!({"copied": [rig.project.join("new/nested/source.txt")]})
    );
    failure(
        rig.copy("partial/nested", &["source.txt", "../outside"])
            .await,
        ErrorCode::InvalidArgument,
    );
    assert_eq!(
        fs::read(rig.project.join("partial/nested/source.txt")).unwrap(),
        b"source bytes\n"
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn read_only_copy_is_refused_before_destination_or_registry_resolution() {
    let rig = Rig::start("copy-nowrite", true).await;
    symlink(&rig.outside, rig.project.join("linked")).unwrap();
    fs::write(rig.host.data_dir.join("projects.json"), b"invalid registry").unwrap();
    let message = failure(
        rig.copy("linked/missing", &["source.txt"]).await,
        ErrorCode::IoError,
    );
    assert!(message.contains("no write permission"));
    assert_eq!(
        fs::read(rig.host.data_dir.join("projects.json")).unwrap(),
        b"invalid registry"
    );
    rig.outside_unchanged();
}
