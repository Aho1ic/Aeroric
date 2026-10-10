//! P8-05 slice 22: exact projectId reads over the frozen, authenticated UDS method.
//! All files are synthetic. These tests preserve lexical ID semantics, not symlink policy.

use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use aeroric_core::context::CoreContext;
use aeroric_core::paths::AppPaths;
use aeroric_core::storage::tasks::load_tasks;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

#[derive(Debug, PartialEq, Eq)]
enum EntrySnapshot {
    Missing,
    File {
        bytes: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Directory {
        entries: Vec<OsString>,
        #[cfg(unix)]
        mode: u32,
    },
}

fn snapshots(paths: &[PathBuf]) -> Vec<(PathBuf, EntrySnapshot)> {
    paths
        .iter()
        .map(|path| {
            let entry = match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    EntrySnapshot::Missing
                }
                Err(error) => panic!("cannot inspect scratch fixture {}: {error}", path.display()),
                Ok(metadata) if metadata.is_file() => EntrySnapshot::File {
                    bytes: fs::read(path).expect("read scratch fixture"),
                    #[cfg(unix)]
                    mode: metadata.permissions().mode(),
                },
                Ok(metadata) if metadata.is_dir() => {
                    let mut entries: Vec<_> = fs::read_dir(path)
                        .expect("scratch directory")
                        .map(|entry| entry.expect("scratch directory entry").file_name())
                        .collect();
                    entries.sort();
                    EntrySnapshot::Directory {
                        entries,
                        #[cfg(unix)]
                        mode: metadata.permissions().mode(),
                    }
                }
                Ok(_) => panic!("unexpected fixture type at {}", path.display()),
            };
            (path.clone(), entry)
        })
        .collect()
}

fn fixture_dir(path: &Path) {
    fs::create_dir_all(path).expect("create scratch fixture directory");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o750)).expect("fixture directory mode");
}

fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write scratch fixture");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o640)).expect("fixture file mode");
}

fn write_json_fixture(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("synthetic JSON");
    bytes.extend_from_slice(b"\n \n");
    write_fixture(path, &bytes);
}

fn raw_task(id: &str, prompt: &str) -> Value {
    json!({
        "id": id,
        "projectId": "stored-owner",
        "prompt": prompt,
        "agent": "codex",
        "permissionMode": "default",
        "status": "done",
        "createdAt": 0,
        "future": {"nested": [null, true, {"prompt": prompt}], "unknown": "保留"}
    })
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request(id, "project.load_project_tasks", params),
    )
    .await
}

fn success_result(reply: Frame) -> Value {
    let Frame::Success(success) = reply else {
        panic!("expected a successful bare task array: {reply:?}");
    };
    success.result
}

#[tokio::test]
async fn project_tasks_preserve_orphan_raw_records_and_original_padded_id() {
    let harness = Harness::start("tasks-raw").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let padded_id = " orphan-A ";
    let projects_dir = harness.data_dir.join("projects");
    let padded_dir = projects_dir.join(padded_id);
    let trimmed_dir = projects_dir.join("orphan-A");
    fixture_dir(&projects_dir);
    fixture_dir(&padded_dir);
    fixture_dir(&trimmed_dir);
    let selected_file = padded_dir.join("tasks.json");
    let decoy_file = trimmed_dir.join("tasks.json");
    let registry_file = harness.data_dir.join("projects.json");
    assert!(
        !registry_file.exists(),
        "the task fixtures are unregistered"
    );

    let records = json!([
        raw_task("b", "first b"),
        raw_task("a", "middle a"),
        raw_task("b", "second b")
    ]);
    let decoy = json!([raw_task("decoy", "trimmed directory")]);
    write_json_fixture(&selected_file, &records);
    write_json_fixture(&decoy_file, &decoy);
    let paths = vec![
        harness.data_dir.clone(),
        projects_dir,
        padded_dir,
        trimmed_dir,
        selected_file.clone(),
        decoy_file.clone(),
        selected_file.with_extension("json.bak"),
        decoy_file.with_extension("json.bak"),
        registry_file.clone(),
    ];
    let read_only = CoreContext::read_only(AppPaths::with_data_dir(
        &harness.home,
        Some(&harness.data_dir),
    ));

    for (registry_case, corrupt_registry) in [("absent", false), ("corrupt", true)] {
        if corrupt_registry {
            write_fixture(&registry_file, b"{ broken registry");
        }
        let before = snapshots(&paths);
        let direct =
            load_tasks(&read_only, padded_id).expect("existing reader needs no writer lease");
        assert_eq!(serde_json::to_value(direct).expect("raw records"), records);
        assert_eq!(snapshots(&paths), before, "read-only core control");

        for (case, project_id, expected) in [
            ("padded", padded_id, &records),
            ("trimmed", "orphan-A", &decoy),
        ] {
            let result = success_result(
                query(
                    &mut reader,
                    &mut writer,
                    &format!("{registry_case}-{case}"),
                    json!({"projectId": project_id}),
                )
                .await,
            );
            assert_eq!(
                &result, expected,
                "{registry_case}/{case}: preserve raw fields, b,a,b and stored projectId"
            );
            assert_eq!(snapshots(&paths), before, "{registry_case}/{case}");
        }
    }
}

#[tokio::test]
async fn project_tasks_preserve_absence_and_read_errors_without_writes_or_disconnect() {
    let harness = Harness::start("tasks-errors").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let project_id = "selected-project";
    let projects_dir = harness.data_dir.join("projects");
    let project_dir = projects_dir.join(project_id);
    let tasks_file = project_dir.join("tasks.json");
    let sentinel = harness.data_dir.join("query-sentinel.txt");
    write_fixture(&sentinel, b"do not change this sentinel");
    let paths = vec![
        harness.data_dir.clone(),
        projects_dir,
        project_dir.clone(),
        tasks_file.clone(),
        tasks_file.with_extension("json.bak"),
        harness.data_dir.join("projects.json"),
        sentinel,
    ];
    assert!(!project_dir.exists());

    for (case, create_directory) in [("missing-directory", false), ("missing-file", true)] {
        if create_directory {
            fixture_dir(&project_dir);
        }
        assert!(!tasks_file.exists());
        let before = snapshots(&paths);
        let result = success_result(
            query(
                &mut reader,
                &mut writer,
                case,
                json!({"projectId": project_id}),
            )
            .await,
        );
        assert_eq!(result, json!([]), "{case}");
        assert_eq!(snapshots(&paths), before, "{case} must stay absent");
    }

    write_fixture(&tasks_file, b" [ \n ] \n");
    let before = snapshots(&paths);
    let empty = success_result(
        query(
            &mut reader,
            &mut writer,
            "empty",
            json!({"projectId": project_id}),
        )
        .await,
    );
    assert_eq!(empty, json!([]));
    assert_eq!(snapshots(&paths), before, "empty array is not rewritten");

    let recovered_records = json!([raw_task("recovered", "same connection after error")]);
    let invalid: [(&str, &[u8]); 4] = [
        ("bad-json", b"{ broken"),
        ("object", br#"{"tasks":[]}"#),
        ("null", b"null"),
        ("mixed-array", br#"[{"id":"not-a-partial-success"},false]"#),
    ];
    for (case, bytes) in invalid {
        write_fixture(&tasks_file, bytes);
        let before = snapshots(&paths);
        let reply = query(
            &mut reader,
            &mut writer,
            case,
            json!({"projectId": project_id}),
        )
        .await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must fail rather than become an empty/partial array: {reply:?}");
        };
        assert_eq!(failure.error.code, ErrorCode::IoError, "{case}");
        assert_eq!(snapshots(&paths), before, "{case}");

        write_json_fixture(&tasks_file, &recovered_records);
        let recovered_before = snapshots(&paths);
        let recovered = success_result(
            query(
                &mut reader,
                &mut writer,
                &format!("recover-{case}"),
                json!({"projectId": project_id}),
            )
            .await,
        );
        assert_eq!(recovered, recovered_records, "{case}");
        assert_eq!(snapshots(&paths), recovered_before, "{case} recovery");
    }
}

#[tokio::test]
async fn project_tasks_reject_invalid_parameters_and_ids_without_filesystem_changes() {
    let harness = Harness::start("tasks-invalid").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let project_id = "safe-project";
    let projects_dir = harness.data_dir.join("projects");
    let project_dir = projects_dir.join(project_id);
    fixture_dir(&projects_dir);
    fixture_dir(&project_dir);
    let tasks_file = project_dir.join("tasks.json");
    let records = json!([raw_task("valid", "after invalid arguments")]);
    write_json_fixture(&tasks_file, &records);
    let sentinel = harness.home.join("outside-data-sentinel.txt");
    write_fixture(
        &sentinel,
        b"synthetic sentinel outside the managed data directory",
    );
    let paths = vec![
        harness.home.clone(),
        harness.data_dir.clone(),
        projects_dir,
        project_dir.clone(),
        tasks_file.clone(),
        tasks_file.with_extension("json.bak"),
        harness.data_dir.join("projects.json"),
        sentinel,
    ];
    let before = snapshots(&paths);
    let mut invalid_params = vec![
        ("missing", json!({})),
        ("null-id", json!({"projectId": null})),
        ("number-id", json!({"projectId": 7})),
        ("bool-id", json!({"projectId": true})),
        ("array-id", json!({"projectId": []})),
        ("object-id", json!({"projectId": {}})),
        ("null-container", json!(null)),
        ("array-container", json!([{"projectId": project_id}])),
        ("string-container", json!(project_id)),
        ("number-container", json!(7)),
        ("bool-container", json!(false)),
        (
            "path-only",
            json!({"projectPath": project_dir.to_string_lossy()}),
        ),
    ];
    for (case, id) in [
        ("empty-id", ""),
        ("blank-id", " \t\n "),
        ("dot-id", "."),
        ("parent-id", ".."),
        ("escape-id", "../escape"),
        ("slash-id", "/"),
        ("slash-component", "a/b"),
        ("double-slash", "a//b"),
        ("backslash-id", "\\"),
        ("backslash-component", "a\\b"),
        ("colon-id", ":"),
        ("colon-component", "a:b"),
        ("internal-space", "a b"),
        ("nul-id", "a\0b"),
        ("non-ascii-id", "项目"),
    ] {
        invalid_params.push((case, json!({"projectId": id})));
    }
    for (case, params) in invalid_params {
        let reply = query(&mut reader, &mut writer, case, params).await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must be InvalidArgument: {reply:?}");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument, "{case}");
        assert_eq!(snapshots(&paths), before, "{case}");
    }

    let recovered = success_result(
        query(
            &mut reader,
            &mut writer,
            "valid-after-refusals",
            json!({"projectId": project_id}),
        )
        .await,
    );
    assert_eq!(recovered, records);
    assert_eq!(
        snapshots(&paths),
        before,
        "valid recovery must also be read-only"
    );
}
