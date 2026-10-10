//! Existing file mutations must use the current writer, not merely an
//! authenticated connection and a known project. All side effects stay in scratch.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aeroric_host::dispatcher::HostWiring;
use aeroric_host::file_access::FilePorts;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const ENTRY: &str = "1700000000000";

struct Rig {
    host: Harness,
    project: PathBuf,
    quarantine: PathBuf,
    trash_calls: Arc<Mutex<Vec<PathBuf>>>,
}

impl Rig {
    async fn start(tag: &str, read_only: bool) -> Self {
        let trash_calls = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::clone(&trash_calls);
        let wiring = move |data: &Path| {
            let project = data.join("file project");
            fs::create_dir_all(project.join("src")).unwrap();
            for name in ["write", "delete", "rename", "format", "history", "copy"] {
                let path = project.join(format!("src/{name}.txt"));
                fs::write(&path, format!("{name} original\n")).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
                }
            }
            let history = project.join(".aeroric/local-history/src/history.txt");
            fs::create_dir_all(&history).unwrap();
            fs::write(history.join(format!("{ENTRY}.txt")), "older snapshot\n").unwrap();
            fs::write(
                data.join("projects.json"),
                serde_json::to_vec(&json!([{"id": "p", "path": project, "name": "files"}]))
                    .unwrap(),
            )
            .unwrap();
            let quarantine = data.join("scratch-trash");
            fs::create_dir(&quarantine).unwrap();
            HostWiring {
                files: FilePorts {
                    trash: Arc::new(move |root, relative| {
                        aeroric_core::files::read_write::delete_path_with_trash(
                            root,
                            relative,
                            |canonical| {
                                calls.lock().unwrap().push(canonical.to_path_buf());
                                fs::rename(
                                    canonical,
                                    quarantine.join(canonical.file_name().unwrap()),
                                )
                                .map_err(|error| error.to_string())
                            },
                        )
                    }),
                },
                ..HostWiring::default()
            }
        };
        let host = if read_only {
            Harness::start_read_only_with_wiring(tag, wiring).await
        } else {
            Harness::start_with_wiring(tag, wiring).await
        };
        Self {
            project: host.data_dir.join("file project").canonicalize().unwrap(),
            quarantine: host.data_dir.join("scratch-trash"),
            host,
            trash_calls,
        }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply =
            Harness::handshake(&mut reader, &mut writer, &self.host.token, "file-authority").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }

    fn mutations(&self) -> Vec<(&'static str, Value)> {
        vec![
            (
                "files.write_file",
                json!({"projectId": "p", "path": "src/write.txt", "content": "changed\n"}),
            ),
            (
                "files.create_file",
                json!({"projectId": "p", "path": "src/new.txt"}),
            ),
            (
                "files.create_directory",
                json!({"projectId": "p", "path": "src/new-directory"}),
            ),
            (
                "files.delete_path",
                json!({"projectId": "p", "path": "src/delete.txt"}),
            ),
            (
                "files.rename_path",
                json!({"projectId": "p", "path": "src/rename.txt", "newName": "renamed.txt"}),
            ),
            // .txt has no formatter: even the red run cannot launch a tool.
            // An absolute path reaches the old snapshot-before-extension check.
            (
                "files.format_file",
                json!({"projectId": "p", "path": self.project.join("src/format.txt")}),
            ),
            (
                "files.restore_local_history_entry",
                json!({"projectId": "p", "path": "src/history.txt", "entryId": ENTRY}),
            ),
            (
                "files.copy_paths_to_directory",
                json!({"projectId": "p", "paths": ["src/copy.txt"], "targetDir": "new-copy-directory"}),
            ),
        ]
    }

    fn snapshot(&self) -> FixtureSnapshot {
        FixtureSnapshot {
            project: tree(&self.project),
            quarantine: tree(&self.quarantine),
            registry: node(&self.host.data_dir.join("projects.json")),
        }
    }

    fn replace_lock(&self) {
        let lock = self.host.data_dir.join(".write.lock");
        assert!(lock.is_file(), "use the real startup lease");
        fs::rename(&lock, lock.with_extension("lock-retired")).unwrap();
        fs::write(&lock, b"replacement inode, not the held writer\n").unwrap();
        assert!(lock.is_file(), "existence alone must not authorize");
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Node {
    kind: &'static str,
    mode: u32,
    bytes: Vec<u8>,
}

fn node(path: &Path) -> Node {
    let metadata = fs::symlink_metadata(path).unwrap();
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = u32::from(metadata.permissions().readonly());
    let (kind, bytes) = if metadata.is_dir() {
        ("dir", Vec::new())
    } else if metadata.is_file() {
        ("file", fs::read(path).unwrap())
    } else {
        assert!(metadata.file_type().is_symlink());
        (
            "link",
            fs::read_link(path)
                .unwrap()
                .to_string_lossy()
                .as_bytes()
                .to_vec(),
        )
    };
    Node { kind, mode, bytes }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Node> {
    fn visit(root: &Path, path: &Path, nodes: &mut BTreeMap<PathBuf, Node>) {
        let entry = node(path);
        let directory = entry.kind == "dir";
        nodes.insert(path.strip_prefix(root).unwrap().to_path_buf(), entry);
        if directory {
            for child in fs::read_dir(path).unwrap() {
                visit(root, &child.unwrap().path(), nodes);
            }
        }
    }
    let mut nodes = BTreeMap::new();
    visit(root, root, &mut nodes);
    nodes
}

#[derive(Debug, PartialEq, Eq)]
struct FixtureSnapshot {
    project: BTreeMap<PathBuf, Node>,
    quarantine: BTreeMap<PathBuf, Node>,
    registry: Node,
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    let id = format!("authority-{method}");
    let reply = Harness::call(reader, writer, &Harness::request(&id, method, params)).await;
    match &reply {
        Frame::Success(response) => assert_eq!(response.id, id),
        Frame::Failure(response) => assert_eq!(response.id, id),
        other => panic!("expected response, got {other:?}"),
    }
    reply
}

fn refusal(reply: &Frame) {
    let Frame::Failure(response) = reply else {
        panic!("a non-writer must be refused, got {reply:?}");
    };
    assert_eq!(response.error.code, ErrorCode::IoError, "{reply:?}");
    assert!(
        response.error.message.contains("no write permission"),
        "not a missing project/path or formatter failure: {reply:?}"
    );
    assert!(!response.error.retryable);
}

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(response) => response.result,
        other => panic!("expected success: {other:?}"),
    }
}

async fn deny_one(index: usize) {
    let rig = Rig::start(&format!("fw-ro-{index}"), true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let (method, params) = rig.mutations().remove(index);
    let before = rig.snapshot();
    let reply = call(&mut reader, &mut writer, method, params).await;
    refusal(&reply);
    assert_eq!(rig.snapshot(), before, "{method} must have no side effects");
    assert!(rig.trash_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn read_only_cannot_write_file() {
    deny_one(0).await;
}

#[tokio::test]
async fn read_only_cannot_create_file() {
    deny_one(1).await;
}

#[tokio::test]
async fn read_only_cannot_create_directory() {
    deny_one(2).await;
}

#[tokio::test]
async fn read_only_cannot_trash_an_existing_file() {
    deny_one(3).await;
}

#[tokio::test]
async fn read_only_cannot_rename_file() {
    deny_one(4).await;
}

#[tokio::test]
async fn read_only_format_cannot_even_create_a_snapshot() {
    deny_one(5).await;
}

#[tokio::test]
async fn read_only_cannot_restore_or_snapshot_history() {
    deny_one(6).await;
}

#[tokio::test]
async fn read_only_cannot_create_a_copy_destination() {
    deny_one(7).await;
}

#[tokio::test]
async fn each_mutation_rechecks_a_replaced_lease_on_the_same_connection() {
    let rig = Rig::start("fw-lost", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    let (method, params) = rig.mutations().remove(0);
    assert_eq!(
        success(call(&mut reader, &mut writer, method, params).await),
        json!({"written": true})
    );
    rig.replace_lock();
    let before = rig.snapshot();
    let mut replies = Vec::new();
    for (method, params) in rig.mutations() {
        replies.push(call(&mut reader, &mut writer, method, params).await);
    }
    for reply in replies {
        refusal(&reply);
        let Frame::Failure(response) = reply else {
            unreachable!()
        };
        assert!(
            response.error.message.contains("lease was lost"),
            "{response:?}"
        );
    }
    assert_eq!(rig.snapshot(), before);
    assert!(rig.trash_calls.lock().unwrap().is_empty());
    // Losing write authority must not also prohibit a read.
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "files.read_file",
                json!({"projectId": "p", "path": "src/write.txt"})
            )
            .await
        ),
        json!({"content": "changed\n"})
    );
}

#[tokio::test]
async fn missing_authority_precedes_unknown_or_corrupt_project_io() {
    let rig = Rig::start("fw-preio", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    for corrupt in [false, true] {
        if corrupt {
            fs::write(
                rig.host.data_dir.join("projects.json"),
                b"[invalid registry",
            )
            .unwrap();
        }
        let before = rig.snapshot();
        for (method, mut params) in rig.mutations() {
            params["projectId"] = json!("not-registered");
            refusal(&call(&mut reader, &mut writer, method, params).await);
        }
        assert_eq!(rig.snapshot(), before, "no registry repair or path IO");
    }
    assert!(rig.trash_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn read_only_file_directory_and_history_queries_remain_available() {
    let rig = Rig::start("fw-queries", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    let listing = success(
        call(
            &mut reader,
            &mut writer,
            "files.read_dir",
            json!({"projectId": "p", "path": "src"}),
        )
        .await,
    );
    assert_eq!(listing["entries"].as_array().unwrap().len(), 6);
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "files.read_file",
                json!({"projectId": "p", "path": "src/write.txt"})
            )
            .await
        ),
        json!({"content": "write original\n"})
    );
    let history = success(
        call(
            &mut reader,
            &mut writer,
            "files.list_local_history",
            json!({"projectId": "p", "path": "src/history.txt"}),
        )
        .await,
    );
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["id"], ENTRY);
    let entry = success(
        call(
            &mut reader,
            &mut writer,
            "files.read_local_history_entry",
            json!({"projectId": "p", "path": "src/history.txt", "entryId": ENTRY}),
        )
        .await,
    );
    assert_eq!(entry["content"], "older snapshot\n");
    assert_eq!(rig.snapshot(), before);
    assert!(rig.trash_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn current_writer_preserves_results_history_and_injected_trash() {
    let rig = Rig::start("fw-writer", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    for (method, params) in rig.mutations() {
        let reply = call(&mut reader, &mut writer, method, params).await;
        if method == "files.format_file" {
            let Frame::Failure(response) = reply else {
                panic!("{reply:?}")
            };
            assert_eq!(response.error.code, ErrorCode::InvalidArgument);
            assert!(response
                .error
                .message
                .contains("No formatter is configured"));
            continue;
        }
        let value = success(reply);
        let expected = match method {
            "files.write_file" => json!({"written": true}),
            "files.create_file" | "files.create_directory" => json!({"created": true}),
            "files.delete_path" => json!({"deleted": true}),
            "files.rename_path" => json!({"renamed": true}),
            "files.copy_paths_to_directory" => json!({
                "copied": [rig.project.join("new-copy-directory/copy.txt")]
            }),
            "files.restore_local_history_entry" => {
                assert_eq!(value["content"], "older snapshot\n");
                continue;
            }
            _ => unreachable!(),
        };
        assert_eq!(value, expected, "{method}");
    }
    assert_eq!(
        fs::read(rig.project.join("src/write.txt")).unwrap(),
        b"changed\n"
    );
    assert_eq!(fs::read(rig.project.join("src/new.txt")).unwrap(), b"");
    assert!(rig.project.join("src/new-directory").is_dir());
    assert!(!rig.project.join("src/delete.txt").exists());
    assert_eq!(
        fs::read(rig.quarantine.join("delete.txt")).unwrap(),
        b"delete original\n"
    );
    assert_eq!(
        *rig.trash_calls.lock().unwrap(),
        [rig.project.join("src/delete.txt")]
    );
    assert!(!rig.project.join("src/rename.txt").exists());
    assert_eq!(
        fs::read(rig.project.join("src/renamed.txt")).unwrap(),
        b"rename original\n"
    );
    assert_eq!(
        fs::read(rig.project.join("new-copy-directory/copy.txt")).unwrap(),
        b"copy original\n"
    );
    assert_eq!(
        fs::read(rig.project.join("src/history.txt")).unwrap(),
        b"older snapshot\n"
    );
    assert_eq!(
        fs::read_dir(rig.project.join(".aeroric/local-history/src/history.txt"))
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        fs::read(rig.project.join("src/format.txt")).unwrap(),
        b"format original\n"
    );
    assert_eq!(
        fs::read_dir(rig.project.join(".aeroric/local-history/src/format.txt"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn unauthenticated_mutations_never_reach_the_file_ports() {
    let rig = Rig::start("fw-noauth", false).await;
    let (mut reader, mut writer) = rig.host.connect().await;
    let before = rig.snapshot();
    for (method, params) in rig.mutations() {
        let reply = call(&mut reader, &mut writer, method, params).await;
        let Frame::Failure(response) = reply else {
            panic!("{reply:?}")
        };
        assert_eq!(response.error.code, ErrorCode::Unauthenticated);
    }
    assert_eq!(rig.snapshot(), before);
    assert!(rig.trash_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn writer_argument_refusals_still_leave_files_unchanged() {
    let rig = Rig::start("fw-params", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    for (method, _) in rig.mutations() {
        let reply = call(&mut reader, &mut writer, method, json!({})).await;
        let Frame::Failure(response) = reply else {
            panic!("{reply:?}")
        };
        assert_eq!(response.error.code, ErrorCode::InvalidArgument, "{method}");
    }
    assert_eq!(rig.snapshot(), before);
    assert!(rig.trash_calls.lock().unwrap().is_empty());
}
