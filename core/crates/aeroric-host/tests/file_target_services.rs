//! Explicit file targets must never become local IO by being ignored.
//! Real UDS, scratch roots, counted scratch trash and unsupported .txt format only.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
type Connection = (BufReader<OwnedReadHalf>, OwnedWriteHalf);

#[derive(Clone, Copy, Debug)]
enum Op {
    ReadDir,
    ReadFile,
    Write,
    CreateFile,
    CreateDirectory,
    Delete,
    Rename,
    Format,
    ListHistory,
    ReadHistory,
    RestoreHistory,
    Copy,
}

const ALL: [Op; 12] = [
    Op::ReadDir,
    Op::ReadFile,
    Op::Write,
    Op::CreateFile,
    Op::CreateDirectory,
    Op::Delete,
    Op::Rename,
    Op::Format,
    Op::ListHistory,
    Op::ReadHistory,
    Op::RestoreHistory,
    Op::Copy,
];

impl Op {
    fn method(self) -> &'static str {
        match self {
            Self::ReadDir => "files.read_dir",
            Self::ReadFile => "files.read_file",
            Self::Write => "files.write_file",
            Self::CreateFile => "files.create_file",
            Self::CreateDirectory => "files.create_directory",
            Self::Delete => "files.delete_path",
            Self::Rename => "files.rename_path",
            Self::Format => "files.format_file",
            Self::ListHistory => "files.list_local_history",
            Self::ReadHistory => "files.read_local_history_entry",
            Self::RestoreHistory => "files.restore_local_history_entry",
            Self::Copy => "files.copy_paths_to_directory",
        }
    }

    fn query(self) -> bool {
        matches!(
            self,
            Self::ReadDir | Self::ReadFile | Self::ListHistory | Self::ReadHistory
        )
    }

    fn params(self, rig: &Rig) -> Value {
        let mut params = match self {
            Self::ReadDir => json!({"path": "src"}),
            Self::ReadFile => json!({"path": "src/query.txt"}),
            Self::Write => json!({"path": "src/write.txt", "content": "changed\n"}),
            Self::CreateFile => json!({"path": "src/new.txt"}),
            Self::CreateDirectory => json!({"path": "new-directory"}),
            Self::Delete => json!({"path": "src/delete.txt"}),
            Self::Rename => json!({"path": "src/rename.txt", "newName": "renamed.txt"}),
            // Absolute .txt reaches the old snapshot hook but cannot start a formatter.
            Self::Format => json!({"path": rig.project.join("src/format.txt")}),
            Self::ListHistory => json!({"path": "src/history.txt"}),
            Self::ReadHistory | Self::RestoreHistory => {
                json!({"path": "src/history.txt", "entryId": ENTRY})
            }
            Self::Copy => json!({"paths": ["src/copy.txt"], "targetDir": "copies"}),
        };
        params["projectId"] = json!("p");
        params
    }
}

struct Rig {
    host: Harness,
    project: PathBuf,
    other: PathBuf,
    alias: PathBuf,
    quarantine: PathBuf,
    trash_calls: Arc<AtomicUsize>,
}

impl Rig {
    async fn start(read_only: bool) -> Self {
        let trash_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&trash_calls);
        let wiring = move |data: &Path| {
            let project = data.join("file project");
            fs::create_dir_all(project.join("src")).unwrap();
            for name in [
                "query", "write", "delete", "rename", "format", "history", "copy",
            ] {
                let path = project.join(format!("src/{name}.txt"));
                fs::write(&path, format!("{name} original\n")).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
            }
            let history = project.join(".aeroric/local-history/src/history.txt");
            fs::create_dir_all(&history).unwrap();
            fs::write(history.join(format!("{ENTRY}.txt")), "older snapshot\n").unwrap();
            let other = data.join("other project");
            fs::create_dir(&other).unwrap();
            fs::write(other.join("sentinel"), "other unchanged\n").unwrap();
            symlink(&project, data.join("same-root-alias")).unwrap();
            fs::write(
                data.join("projects.json"),
                serde_json::to_vec(&json!([{"id": "p", "path": project, "name": "file-targets"}]))
                    .unwrap(),
            )
            .unwrap();
            let quarantine = data.join("scratch-trash");
            fs::create_dir(&quarantine).unwrap();
            HostWiring {
                files: FilePorts {
                    trash: Arc::new(move |root, relative| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        aeroric_core::files::read_write::delete_path_with_trash(
                            root,
                            relative,
                            |canonical| {
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
            Harness::start_read_only_with_wiring("ft", wiring).await
        } else {
            Harness::start_with_wiring("ft", wiring).await
        };
        Self {
            project: host.data_dir.join("file project").canonicalize().unwrap(),
            other: host.data_dir.join("other project"),
            alias: host.data_dir.join("same-root-alias"),
            quarantine: host.data_dir.join("scratch-trash"),
            host,
            trash_calls,
        }
    }

    async fn connect(&self) -> Connection {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply =
            Harness::handshake(&mut reader, &mut writer, &self.host.token, "file-targets").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }

    fn target(&self, case: &str) -> (Value, ErrorCode) {
        match case {
            "ssh" => (
                json!({"kind": "ssh", "connectionId": "fixture-only", "remotePath": self.project}),
                ErrorCode::UnsupportedCapability,
            ),
            "wsl" => (
                json!({"kind": "wsl", "distribution": "fixture-only", "linuxPath": self.project}),
                ErrorCode::UnsupportedCapability,
            ),
            "null" => (Value::Null, ErrorCode::InvalidArgument),
            "bad-local" => (
                json!({"kind": "local", "path": 3}),
                ErrorCode::InvalidArgument,
            ),
            "incomplete-ssh" => (
                json!({"kind": "ssh", "connectionId": "fixture-only"}),
                ErrorCode::InvalidArgument,
            ),
            "conflict" => (
                json!({"kind": "local", "path": self.other}),
                ErrorCode::InvalidArgument,
            ),
            "missing-local" => (
                json!({"kind": "local", "path": self.other.join("not-created")}),
                ErrorCode::InvalidArgument,
            ),
            "relative-local" => (
                json!({"kind": "local", "path": "relative"}),
                ErrorCode::InvalidArgument,
            ),
            _ => panic!("unknown fixture target"),
        }
    }

    fn snapshot(&self) -> (Tree, Tree, Tree, Node) {
        (
            tree(&self.project),
            tree(&self.other),
            tree(&self.quarantine),
            node(&self.host.data_dir.join("projects.json")),
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Node {
    kind: &'static str,
    mode: u32,
    bytes: Vec<u8>,
}

type Tree = BTreeMap<PathBuf, Node>;

fn node(path: &Path) -> Node {
    let metadata = fs::symlink_metadata(path).unwrap();
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
    Node {
        kind,
        mode: metadata.permissions().mode() & 0o777,
        bytes,
    }
}

fn tree(root: &Path) -> Tree {
    fn visit(root: &Path, path: &Path, entries: &mut Tree) {
        let entry = node(path);
        let directory = entry.kind == "dir";
        entries.insert(path.strip_prefix(root).unwrap().to_path_buf(), entry);
        if directory {
            for child in fs::read_dir(path).unwrap() {
                visit(root, &child.unwrap().path(), entries);
            }
        }
    }
    let mut entries = Tree::new();
    visit(root, root, &mut entries);
    entries
}

async fn call(connection: &mut Connection, operation: Op, params: Value) -> Frame {
    Harness::call(
        &mut connection.0,
        &mut connection.1,
        &Harness::request("file-target", operation.method(), params),
    )
    .await
}

fn refusal(reply: &Frame, code: ErrorCode) -> bool {
    matches!(reply, Frame::Failure(response) if response.id == "file-target"
        && response.error.code == code && !response.error.retryable
        && !response.error.message.is_empty())
}

fn label(reply: &Frame) -> String {
    match reply {
        Frame::Success(_) => "Success".to_string(),
        Frame::Failure(response) => format!("{:?}", response.error.code),
        _ => "not a response".to_string(),
    }
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected success: {reply:?}")
    };
    assert_eq!(response.id, "file-target");
    response.result
}

async fn rejected_targets(operation: Op) {
    let mut failures = Vec::new();
    // Do not stop at the first red: each target/operation gets a fresh fixture,
    // so pre-fix deletion/rename cannot invalidate the next reproduction.
    for case in [
        "ssh",
        "wsl",
        "null",
        "bad-local",
        "incomplete-ssh",
        "conflict",
        "missing-local",
        "relative-local",
    ] {
        let rig = Rig::start(false).await;
        let mut connection = rig.connect().await;
        let (target, code) = rig.target(case);
        let mut params = operation.params(&rig);
        params["target"] = target;
        let before = rig.snapshot();
        let reply = call(&mut connection, operation, params).await;
        let changed = rig.snapshot() != before;
        let calls = rig.trash_calls.load(Ordering::SeqCst);
        if !refusal(&reply, code) || changed || calls != 0 {
            failures.push(format!(
                "{case}: {}, changed={changed}, trash_calls={calls}",
                label(&reply)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {}",
        operation.method(),
        failures.join("; ")
    );
}

macro_rules! target_regression {
    ($test:ident, $op:ident) => {
        #[tokio::test]
        async fn $test() {
            rejected_targets(Op::$op).await;
        }
    };
}

target_regression!(read_dir_never_ignores_an_explicit_target, ReadDir);
target_regression!(read_file_never_ignores_an_explicit_target, ReadFile);
target_regression!(write_never_ignores_an_explicit_target, Write);
target_regression!(create_file_never_ignores_an_explicit_target, CreateFile);
target_regression!(
    create_directory_never_ignores_an_explicit_target,
    CreateDirectory
);
target_regression!(delete_never_ignores_an_explicit_target, Delete);
target_regression!(rename_never_ignores_an_explicit_target, Rename);
target_regression!(
    format_never_ignores_an_explicit_target_or_snapshots_locally,
    Format
);
target_regression!(list_history_never_ignores_an_explicit_target, ListHistory);
target_regression!(read_history_never_ignores_an_explicit_target, ReadHistory);
target_regression!(
    restore_history_never_ignores_an_explicit_target,
    RestoreHistory
);
target_regression!(copy_never_ignores_an_explicit_target, Copy);

fn assert_control(operation: Op, rig: &Rig, reply: Frame) {
    if matches!(operation, Op::Format) {
        assert!(refusal(&reply, ErrorCode::InvalidArgument), "{reply:?}");
        let Frame::Failure(response) = reply else {
            unreachable!()
        };
        assert!(response
            .error
            .message
            .contains("No formatter is configured"));
        assert_eq!(
            fs::read(rig.project.join("src/format.txt")).unwrap(),
            b"format original\n"
        );
        assert_eq!(
            fs::read_dir(rig.project.join(".aeroric/local-history/src/format.txt"))
                .unwrap()
                .count(),
            1,
            "authorized legacy formatter snapshots before unsupported-extension refusal"
        );
        return;
    }
    let result = success(reply);
    match operation {
        Op::ReadDir => assert_eq!(result["entries"].as_array().unwrap().len(), 7),
        Op::ReadFile => assert_eq!(result, json!({"content": "query original\n"})),
        Op::Write => {
            assert_eq!(result, json!({"written": true}));
            assert_eq!(
                fs::read(rig.project.join("src/write.txt")).unwrap(),
                b"changed\n"
            );
            assert_eq!(node(&rig.project.join("src/write.txt")).mode, 0o640);
        }
        Op::CreateFile => {
            assert_eq!(result, json!({"created": true}));
            assert_eq!(fs::read(rig.project.join("src/new.txt")).unwrap(), b"");
        }
        Op::CreateDirectory => {
            assert_eq!(result, json!({"created": true}));
            assert!(rig.project.join("new-directory").is_dir());
        }
        Op::Delete => {
            assert_eq!(result, json!({"deleted": true}));
            assert!(!rig.project.join("src/delete.txt").exists());
            assert_eq!(
                fs::read(rig.quarantine.join("delete.txt")).unwrap(),
                b"delete original\n"
            );
            assert_eq!(rig.trash_calls.load(Ordering::SeqCst), 1);
        }
        Op::Rename => {
            assert_eq!(result, json!({"renamed": true}));
            assert!(!rig.project.join("src/rename.txt").exists());
            assert_eq!(
                fs::read(rig.project.join("src/renamed.txt")).unwrap(),
                b"rename original\n"
            );
        }
        Op::ListHistory => {
            assert_eq!(result.as_array().unwrap().len(), 1);
            assert_eq!(result[0]["id"], ENTRY);
        }
        Op::ReadHistory => assert_eq!(result["content"], "older snapshot\n"),
        Op::RestoreHistory => {
            assert_eq!(result["content"], "older snapshot\n");
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
        }
        Op::Copy => {
            assert_eq!(
                result,
                json!({"copied": [rig.project.join("copies/copy.txt")]})
            );
            assert_eq!(
                fs::read(rig.project.join("copies/copy.txt")).unwrap(),
                b"copy original\n"
            );
        }
        Op::Format => unreachable!(),
    }
}

#[tokio::test]
async fn remote_and_malformed_targets_precede_writer_and_poison_registry_access() {
    let mut failures = Vec::new();
    for read_only in [false, true] {
        for operation in ALL {
            for case in ["ssh", "null"] {
                let rig = Rig::start(read_only).await;
                fs::write(rig.host.data_dir.join("projects.json"), b"invalid registry").unwrap();
                let mut connection = rig.connect().await;
                let (target, code) = rig.target(case);
                let mut params = operation.params(&rig);
                params["projectId"] = json!("unknown");
                params["target"] = target;
                let before = rig.snapshot();
                let reply = call(&mut connection, operation, params).await;
                if !refusal(&reply, code)
                    || rig.snapshot() != before
                    || rig.trash_calls.load(Ordering::SeqCst) != 0
                {
                    failures.push(format!(
                        "{operation:?}/{case}/readonly={read_only}: {}",
                        label(&reply)
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[tokio::test]
async fn omitted_matching_local_and_canonical_alias_targets_keep_all_operation_contracts() {
    for operation in ALL {
        for choice in ["omitted", "matching", "alias"] {
            let rig = Rig::start(false).await;
            let mut connection = rig.connect().await;
            let mut params = operation.params(&rig);
            match choice {
                "matching" => params["target"] = json!({"kind": "local", "path": rig.project}),
                "alias" => params["target"] = json!({"kind": "local", "path": rig.alias}),
                _ => {}
            }
            let other = tree(&rig.other);
            assert_control(
                operation,
                &rig,
                call(&mut connection, operation, params).await,
            );
            assert_eq!(tree(&rig.other), other);
            if !matches!(operation, Op::Delete) {
                assert_eq!(rig.trash_calls.load(Ordering::SeqCst), 0);
            }
        }
    }
}

#[tokio::test]
async fn read_only_local_queries_work_and_mutations_refuse_before_local_path_resolution() {
    for operation in ALL {
        let rig = Rig::start(true).await;
        let mut connection = rig.connect().await;
        let mut params = operation.params(&rig);
        params["target"] = json!({"kind": "local", "path": rig.alias});
        if !operation.query() {
            params["target"]["path"] = json!(rig.other.join("not-created"));
            fs::write(rig.host.data_dir.join("projects.json"), b"invalid registry").unwrap();
        }
        let before = rig.snapshot();
        let reply = call(&mut connection, operation, params).await;
        if operation.query() {
            assert_control(operation, &rig, reply);
        } else {
            assert!(refusal(&reply, ErrorCode::IoError), "{reply:?}");
            let Frame::Failure(response) = reply else {
                unreachable!()
            };
            assert!(response.error.message.contains("no write permission"));
        }
        assert_eq!(rig.snapshot(), before);
        assert_eq!(rig.trash_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn unauthenticated_targeted_requests_never_reach_local_file_ports() {
    let rig = Rig::start(false).await;
    let mut connection = rig.host.connect().await;
    let before = rig.snapshot();
    for operation in ALL {
        let mut params = operation.params(&rig);
        params["target"] = rig.target("ssh").0;
        let reply = call(&mut connection, operation, params).await;
        assert!(refusal(&reply, ErrorCode::Unauthenticated), "{reply:?}");
    }
    assert_eq!(rig.snapshot(), before);
    assert_eq!(rig.trash_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_conflicting_local_refusal_does_not_poison_the_same_connection() {
    let rig = Rig::start(false).await;
    let mut connection = rig.connect().await;
    let mut params = Op::Write.params(&rig);
    params["target"] = rig.target("conflict").0;
    let before = rig.snapshot();
    let reply = call(&mut connection, Op::Write, params.clone()).await;
    assert!(refusal(&reply, ErrorCode::InvalidArgument), "{reply:?}");
    assert_eq!(rig.snapshot(), before);
    params["target"] = json!({"kind": "local", "path": rig.alias});
    assert_control(
        Op::Write,
        &rig,
        call(&mut connection, Op::Write, params).await,
    );
}

// Slice37: a stored location is authoritative even without params.target.
// Reuse the same scratch ports/operations so every red remains safe.
fn store_location(rig: &Rig, location: Value, raw_path: Option<&Path>) {
    let mut record = json!({"id": "p", "name": "stored-file-location", "location": location});
    if let Some(path) = raw_path {
        record["path"] = json!(path);
    }
    fs::write(
        rig.host.data_dir.join("projects.json"),
        serde_json::to_vec(&json!([record])).unwrap(),
    )
    .unwrap();
}

fn stored_case(rig: &Rig, case: &str) -> (Value, ErrorCode) {
    match case {
        "ssh" | "wsl" | "null" | "bad-local" | "incomplete-ssh" => rig.target(case),
        "unknown" => (
            json!({"kind": "future-transport", "path": rig.project}),
            ErrorCode::InvalidArgument,
        ),
        "incomplete-wsl" => (
            json!({"kind": "wsl", "distribution": "fixture-only"}),
            ErrorCode::InvalidArgument,
        ),
        "array" => (json!([]), ErrorCode::InvalidArgument),
        _ => panic!("unknown stored location case"),
    }
}

fn location_refusal(reply: &Frame, code: ErrorCode) -> bool {
    refusal(reply, code)
        && matches!(reply, Frame::Failure(response)
            if response.error.detail.as_ref().and_then(|detail| detail.get("field"))
                == Some(&json!("location")))
}

async fn rejected_stored_locations(operation: Op) {
    let mut failures = Vec::new();
    for case in [
        "ssh",
        "wsl",
        "null",
        "unknown",
        "bad-local",
        "incomplete-ssh",
        "incomplete-wsl",
        "array",
    ] {
        for explicit_local in [false, true] {
            let rig = Rig::start(false).await;
            let (location, code) = stored_case(&rig, case);
            // A real local shadow path makes ignoring the remote record observable.
            store_location(&rig, location, Some(&rig.project));
            let mut connection = rig.connect().await;
            let mut params = operation.params(&rig);
            if explicit_local {
                params["target"] = json!({"kind": "local", "path": rig.alias});
            }
            let before = rig.snapshot();
            let reply = call(&mut connection, operation, params).await;
            let changed = rig.snapshot() != before;
            let calls = rig.trash_calls.load(Ordering::SeqCst);
            if !location_refusal(&reply, code) || changed || calls != 0 {
                failures.push(format!(
                    "{case}/explicit-local={explicit_local}: {}, changed={changed}, trash_calls={calls}",
                    label(&reply)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {}",
        operation.method(),
        failures.join("; ")
    );
}

macro_rules! stored_location_regression {
    ($test:ident, $op:ident) => {
        #[tokio::test]
        async fn $test() {
            rejected_stored_locations(Op::$op).await;
        }
    };
}

stored_location_regression!(stored_location_read_dir_refuses_local_shadow, ReadDir);
stored_location_regression!(stored_location_read_file_refuses_local_shadow, ReadFile);
stored_location_regression!(stored_location_write_refuses_local_shadow, Write);
stored_location_regression!(stored_location_create_file_refuses_local_shadow, CreateFile);
stored_location_regression!(
    stored_location_create_directory_refuses_local_shadow,
    CreateDirectory
);
stored_location_regression!(stored_location_delete_refuses_local_shadow, Delete);
stored_location_regression!(stored_location_rename_refuses_local_shadow, Rename);
stored_location_regression!(stored_location_format_refuses_snapshot, Format);
stored_location_regression!(
    stored_location_list_history_refuses_local_shadow,
    ListHistory
);
stored_location_regression!(
    stored_location_read_history_refuses_local_shadow,
    ReadHistory
);
stored_location_regression!(
    stored_location_restore_history_refuses_local_shadow,
    RestoreHistory
);
stored_location_regression!(stored_location_copy_refuses_local_shadow, Copy);

#[tokio::test]
async fn stored_location_local_controls_keep_legacy_path_and_all_operation_wire() {
    for operation in ALL {
        for choice in ["omitted", "matching", "alias", "opaque-target"] {
            let rig = Rig::start(false).await;
            match choice {
                "matching" | "alias" => {
                    let path = if choice == "alias" {
                        &rig.alias
                    } else {
                        &rig.project
                    };
                    store_location(
                        &rig,
                        json!({"kind": "local", "path": path}),
                        Some(&rig.project),
                    );
                }
                "opaque-target" => {
                    // The existing core identity rule reads location, not arbitrary
                    // metadata named target. Do not invent a second stored schema.
                    fs::write(
                        rig.host.data_dir.join("projects.json"),
                        serde_json::to_vec(&json!([{
                            "id": "p", "path": rig.project, "target": rig.target("ssh").0,
                        }]))
                        .unwrap(),
                    )
                    .unwrap();
                }
                _ => {}
            }
            let mut connection = rig.connect().await;
            let registry = node(&rig.host.data_dir.join("projects.json"));
            let other = tree(&rig.other);
            assert_control(
                operation,
                &rig,
                call(&mut connection, operation, operation.params(&rig)).await,
            );
            assert_eq!(node(&rig.host.data_dir.join("projects.json")), registry);
            assert_eq!(tree(&rig.other), other);
        }
    }
}

#[tokio::test]
async fn stored_location_does_not_move_record_io_before_current_writer_admission() {
    let mut failures = Vec::new();
    for operation in ALL {
        for case in ["ssh", "null"] {
            let rig = Rig::start(true).await;
            let (location, code) = stored_case(&rig, case);
            store_location(&rig, location, Some(&rig.project));
            let mut connection = rig.connect().await;
            let before = rig.snapshot();
            let reply = call(&mut connection, operation, operation.params(&rig)).await;
            let correct = if operation.query() {
                location_refusal(&reply, code)
            } else {
                refusal(&reply, ErrorCode::IoError)
                    && matches!(&reply, Frame::Failure(response)
                        if response.error.message.contains("no write permission"))
            };
            if !correct || rig.snapshot() != before || rig.trash_calls.load(Ordering::SeqCst) != 0 {
                failures.push(format!("{operation:?}/{case}: {}", label(&reply)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[tokio::test]
async fn stored_location_remote_refusal_precedes_missing_or_invalid_raw_path() {
    let rig = Rig::start(false).await;
    let mut connection = rig.connect().await;
    let missing = rig.other.join("not-created");
    let mut failures = Vec::new();
    for raw in [None, Some(missing.as_path())] {
        for operation in ALL {
            store_location(&rig, rig.target("ssh").0, raw);
            let before = rig.snapshot();
            let reply = call(&mut connection, operation, operation.params(&rig)).await;
            if !location_refusal(&reply, ErrorCode::UnsupportedCapability)
                || rig.snapshot() != before
                || rig.trash_calls.load(Ordering::SeqCst) != 0
            {
                failures.push(format!("{operation:?}/raw={raw:?}: {}", label(&reply)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[tokio::test]
async fn stored_location_is_rechecked_after_record_change_on_the_same_connection() {
    let rig = Rig::start(false).await;
    let mut connection = rig.connect().await;
    assert_control(
        Op::ReadFile,
        &rig,
        call(&mut connection, Op::ReadFile, Op::ReadFile.params(&rig)).await,
    );
    store_location(&rig, rig.target("ssh").0, Some(&rig.project));
    let before = rig.snapshot();
    let reply = call(&mut connection, Op::Write, Op::Write.params(&rig)).await;
    assert!(
        location_refusal(&reply, ErrorCode::UnsupportedCapability),
        "{reply:?}"
    );
    assert_eq!(rig.snapshot(), before);
    store_location(
        &rig,
        json!({"kind": "local", "path": rig.alias}),
        Some(&rig.project),
    );
    assert_control(
        Op::Write,
        &rig,
        call(&mut connection, Op::Write, Op::Write.params(&rig)).await,
    );
}
