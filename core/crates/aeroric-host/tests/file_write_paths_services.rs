//! Write/create leaf confinement through real authenticated UDS.
//! Every "outside" destination is another directory in the same scratch home.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

type Connection = (BufReader<OwnedReadHalf>, OwnedWriteHalf);

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
        let project = host.data_dir.join("leaf project");
        let outside = host.data_dir.join("outside project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&outside).unwrap();
        for path in [project.join("target.txt"), outside.join("sentinel.txt")] {
            fs::write(&path, "original bytes\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        }
        fs::write(
            host.data_dir.join("projects.json"),
            serde_json::to_vec(&json!([{"id": "p", "name": "leaves", "path": project}])).unwrap(),
        )
        .unwrap();
        Self {
            project: project.canonicalize().unwrap(),
            outside: outside.canonicalize().unwrap(),
            host,
        }
    }

    async fn connect(&self) -> Connection {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply =
            Harness::handshake(&mut reader, &mut writer, &self.host.token, "leaf-test").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }

    fn outside_unchanged(&self) {
        assert_eq!(fs::read_dir(&self.outside).unwrap().count(), 1);
        let file = self.outside.join("sentinel.txt");
        assert_eq!(fs::read(&file).unwrap(), b"original bytes\n");
        assert_eq!(mode(&file), 0o640);
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

async fn call(connection: &mut Connection, method: &str, params: Value) -> Frame {
    Harness::call(
        &mut connection.0,
        &mut connection.1,
        &Harness::request("leaf", method, params),
    )
    .await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected success: {reply:?}")
    };
    assert_eq!(response.id, "leaf");
    response.result
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(response) = reply else {
        panic!("expected refusal: {reply:?}")
    };
    assert_eq!(response.id, "leaf");
    assert_eq!(response.error.code, code, "{response:?}");
    assert!(!response.error.retryable);
    response.error.message
}

#[tokio::test]
async fn outside_existing_write_leaf_refuses_and_the_same_connection_recovers() {
    let rig = Rig::start("leaf-existing", false).await;
    let target = rig.outside.join("sentinel.txt");
    symlink(&target, rig.project.join("link.txt")).unwrap();
    let mut connection = rig.connect().await;
    let message = failure(
        call(
            &mut connection,
            "files.write_file",
            json!({"projectId": "p", "path": "link.txt", "content": "must not write"}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("escapes"), "{message}");
    assert_eq!(fs::read_link(rig.project.join("link.txt")).unwrap(), target);
    rig.outside_unchanged();
    assert_eq!(
        success(
            call(
                &mut connection,
                "files.write_file",
                json!({"projectId": "p", "path": "recovered.txt", "content": "ok\r\n"}),
            )
            .await
        ),
        json!({"written": true})
    );
    assert_eq!(
        fs::read(rig.project.join("recovered.txt")).unwrap(),
        b"ok\r\n"
    );
}

#[tokio::test]
async fn dangling_outside_write_leaf_cannot_create_its_referent() {
    let rig = Rig::start("leaf-dangling-out", false).await;
    let target = rig.outside.join("must-not-exist.txt");
    symlink(&target, rig.project.join("link.txt")).unwrap();
    let mut connection = rig.connect().await;
    failure(
        call(
            &mut connection,
            "files.write_file",
            json!({"projectId": "p", "path": "link.txt", "content": "must not write"}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(fs::read_link(rig.project.join("link.txt")).unwrap(), target);
    rig.outside_unchanged();
}

#[tokio::test]
async fn unresolved_inside_write_leaf_refuses_instead_of_creating_through_a_link() {
    let rig = Rig::start("leaf-dangling-in", false).await;
    symlink("missing.txt", rig.project.join("link.txt")).unwrap();
    let mut connection = rig.connect().await;
    failure(
        call(
            &mut connection,
            "files.write_file",
            json!({"projectId": "p", "path": "link.txt", "content": "must not create"}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(
        fs::read_link(rig.project.join("link.txt")).unwrap(),
        Path::new("missing.txt")
    );
    assert!(!rig.project.join("missing.txt").exists());
    rig.outside_unchanged();
}

#[tokio::test]
async fn create_refuses_dangling_leaves_even_when_the_referent_would_be_inside() {
    let rig = Rig::start("leaf-create", false).await;
    let mut connection = rig.connect().await;
    for (name, target) in [
        ("outside-link", rig.outside.join("must-not-exist.txt")),
        ("inside-link", rig.project.join("must-not-exist.txt")),
    ] {
        symlink(&target, rig.project.join(name)).unwrap();
        let message = failure(
            call(
                &mut connection,
                "files.create_file",
                json!({"projectId": "p", "path": name}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert_eq!(message, "file already exists");
        assert_eq!(fs::read_link(rig.project.join(name)).unwrap(), target);
        assert!(!target.exists());
    }
    rig.outside_unchanged();
}

#[tokio::test]
async fn valid_aliases_direct_writes_and_create_controls_preserve_wire_bytes_and_modes() {
    let rig = Rig::start("leaf-valid", false).await;
    let mut connection = rig.connect().await;
    symlink("target.txt", rig.project.join("alias.txt")).unwrap();
    fs::create_dir(rig.project.join("real")).unwrap();
    symlink("real", rig.project.join("parent-alias")).unwrap();
    let content = "\u{feff}你好\r\n";
    for relative in ["alias.txt", "parent-alias\\文件.txt", "direct.txt"] {
        assert_eq!(
            success(
                call(
                    &mut connection,
                    "files.write_file",
                    json!({"projectId": "p", "path": relative, "content": content}),
                )
                .await
            ),
            json!({"written": true})
        );
    }
    for relative in ["target.txt", "real/文件.txt", "direct.txt"] {
        assert_eq!(
            fs::read(rig.project.join(relative)).unwrap(),
            content.as_bytes()
        );
    }
    assert_eq!(mode(&rig.project.join("target.txt")), 0o640);
    assert_eq!(
        fs::read_link(rig.project.join("alias.txt")).unwrap(),
        Path::new("target.txt")
    );
    assert_eq!(
        success(
            call(
                &mut connection,
                "files.create_file",
                json!({"projectId": "p", "path": "new.txt"}),
            )
            .await
        ),
        json!({"created": true})
    );
    assert_eq!(fs::read(rig.project.join("new.txt")).unwrap(), b"");
    assert_eq!(
        mode(&rig.project.join("new.txt")),
        mode(&rig.project.join("direct.txt"))
    );
    for relative in ["target.txt", "alias.txt", "real", "parent-alias"] {
        let message = failure(
            call(
                &mut connection,
                "files.create_file",
                json!({"projectId": "p", "path": relative}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert_eq!(message, "file already exists");
    }
    assert_eq!(
        fs::read(rig.project.join("target.txt")).unwrap(),
        content.as_bytes()
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn parent_and_lexical_refusals_remain_effective_for_write_and_create() {
    let rig = Rig::start("leaf-parent", false).await;
    symlink(&rig.outside, rig.project.join("outside-dir")).unwrap();
    let mut connection = rig.connect().await;
    for method in ["files.write_file", "files.create_file"] {
        for relative in [
            "outside-dir/new.txt",
            "../escape",
            "./target.txt",
            "missing/leaf",
        ] {
            failure(
                call(
                    &mut connection,
                    method,
                    json!({"projectId": "p", "path": relative, "content": "must not write"}),
                )
                .await,
                ErrorCode::IoError,
            );
        }
    }
    assert!(!rig.project.join("missing").exists());
    assert_eq!(
        fs::read(rig.project.join("target.txt")).unwrap(),
        b"original bytes\n"
    );
    rig.outside_unchanged();
}

#[tokio::test]
async fn read_only_admission_precedes_registry_and_leaf_resolution() {
    let rig = Rig::start("leaf-readonly", true).await;
    symlink(&rig.outside, rig.project.join("outside-dir")).unwrap();
    fs::write(rig.host.data_dir.join("projects.json"), b"invalid registry").unwrap();
    let mut connection = rig.connect().await;
    for method in ["files.write_file", "files.create_file"] {
        let message = failure(
            call(
                &mut connection,
                method,
                json!({"projectId": "p", "path": "outside-dir/new.txt", "content": "no"}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert!(message.contains("no write permission"), "{message}");
    }
    assert_eq!(
        fs::read(rig.host.data_dir.join("projects.json")).unwrap(),
        b"invalid registry"
    );
    rig.outside_unchanged();
}
