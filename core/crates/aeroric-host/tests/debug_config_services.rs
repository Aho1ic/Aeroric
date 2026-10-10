//! Local definition IO through the real authenticated host; never execute configs.

use std::fs;
use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const READ: &str = "ide.read_debug_configs";
const WRITE: &str = "ide.write_debug_configs";

fn document(version: u32) -> Value {
    json!({"version": version, "configs": [
        {"type": "node", "id": "same", "name": " 中文 launch ", "request": "launch",
         "program": "missing.js", "cwd": ".", "attachHost": "127.0.0.1", "args": [],
         "env": {"FOO": " retained "}, "breakpoints": []},
        {"type": "python", "id": "same", "name": "attach", "request": "attach",
         "program": "", "cwd": ".", "attachHost": " debugger.local ", "attachPort": 5678,
         "args": ["--fake"], "env": {},
         "breakpoints": [{"file": "missing.py", "line": 3, "column": 1,
                          "condition": "count > 0", "logMessage": "中文 {count}"}]}
    ]})
}

fn config_path(project: &Path) -> PathBuf {
    project.join(".aeroric/debug-configs.json")
}

fn entries(directory: &Path) -> Vec<String> {
    let mut result: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    result.sort();
    result
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
    let file = config_path(project);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file.parent().unwrap(), fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o640)).unwrap();
    }
}

fn snapshot(project: &Path) -> (Vec<u8>, u32, u32, Vec<String>) {
    let file = config_path(project);
    let directory = file.parent().unwrap();
    (
        fs::read(&file).unwrap(),
        mode(&file),
        mode(directory),
        entries(directory),
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
        &Harness::request("debug-config", method, params),
    )
    .await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(response) = reply else {
        panic!("expected definition result: {reply:?}");
    };
    response.result
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(response) = reply else {
        panic!("expected {code:?}: {reply:?}");
    };
    assert_eq!(response.error.code, code, "{:?}", response.error);
    response.error.message
}

#[tokio::test]
async fn read_preserves_defaults_original_root_versions_duplicates_and_read_only_bytes() {
    let harness = Harness::start("debug-read").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文  ");
    let decoy = harness.home.join("project 中文");
    fs::create_dir(&project).unwrap();
    seed(&decoy, b"decoy must not be read");
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
        json!({"version": 1, "configs": []})
    );
    assert!(!project.join(".aeroric").exists());
    for version in [0, 1, 99] {
        seed(&project, &serde_json::to_vec(&document(version)).unwrap());
        let before = snapshot(&project);
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
            document(if version == 0 { 1 } else { version })
        );
        assert_eq!(snapshot(&project), before);
    }
    seed(&project, b"{}");
    let before = snapshot(&project);
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
        json!({"version": 1, "configs": []})
    );
    assert_eq!(snapshot(&project), before);
    assert_eq!(
        fs::read(config_path(&decoy)).unwrap(),
        b"decoy must not be read"
    );
    assert!(!project.join("missing.py").exists());
    assert!(!harness.data_dir.join("settings.json").exists());
    assert!(!harness.data_dir.join("projects.json").exists());
}

#[tokio::test]
async fn write_returns_normalized_document_and_atomically_replaces_with_inherited_mode() {
    let harness = Harness::start("debug-write").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文 ");
    seed(&project, b"old invalid bytes must be replaced");
    let expected = document(1);
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": document(99)})
            )
            .await
        ),
        expected
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
    let text = fs::read_to_string(config_path(&project)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
    assert!(text.starts_with("{\n  \"version\": 1,\n  \"configs\": ["));
    assert_eq!(entries(&project.join(".aeroric")), ["debug-configs.json"]);
    assert!(!harness.home.join("project 中文").exists());
    assert!(!project.join("missing.py").exists());
    let inherited = harness.home.join("mode-control");
    fs::write(&inherited, "control").unwrap();
    assert_eq!(mode(&config_path(&project)), mode(&inherited));
    #[cfg(unix)]
    assert_eq!(mode(&project.join(".aeroric")), 0o750);

    // Defaults are a real serde contract, not a malformed-result fallback.
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": {}})
            )
            .await
        ),
        json!({"version": 1, "configs": []})
    );
}

#[tokio::test]
async fn parameter_and_semantic_validation_precede_mutation_with_legacy_root_diagnostics() {
    let harness = Harness::start("debug-params").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("new");
    fs::create_dir(&project).unwrap();
    for method in [READ, WRITE] {
        for params in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"projectPath": 42}),
        ] {
            failure(
                call(&mut reader, &mut writer, method, params).await,
                ErrorCode::InvalidArgument,
            );
        }
        assert_eq!(
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    json!({"projectPath": "relative", "document": document(99)})
                )
                .await,
                ErrorCode::InvalidArgument
            ),
            "Project path must be absolute"
        );
        let absent = harness.home.join("absent");
        assert!(failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": absent, "document": document(99)})
            )
            .await,
            ErrorCode::InvalidArgument
        )
        .starts_with("Cannot resolve project path:"));
        assert!(!absent.exists());
    }
    for bad in [
        Value::Null,
        json!(false),
        json!({"version": "1"}),
        json!({"configs": {}}),
        json!({"configs": [{"type": "alien"}]}),
    ] {
        failure(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": bad}),
            )
            .await,
            ErrorCode::InvalidArgument,
        );
        assert!(!project.join(".aeroric").exists());
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
    for (field, value, diagnostic) in [
        ("id", "", "Debug config id cannot be empty"),
        ("name", " ", "Debug config name cannot be empty"),
        ("program", " ", "Debug program cannot be empty"),
        ("cwd", "../outside", "Path is outside project root"),
    ] {
        let mut invalid = document(99);
        invalid["configs"][0][field] = json!(value);
        assert_eq!(
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    WRITE,
                    json!({"projectPath": project, "document": invalid})
                )
                .await,
                ErrorCode::InvalidArgument
            ),
            diagnostic
        );
        assert!(!project.join(".aeroric").exists());
    }
    // A refused request does not poison the authenticated connection.
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": document(99)})
            )
            .await
        ),
        document(1)
    );
}

#[tokio::test]
async fn parse_io_and_atomic_errors_are_typed_do_not_repair_and_allow_connection_recovery() {
    let harness = Harness::start("debug-errors").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project");
    for bytes in [b"{broken".as_slice(), &[0xff, 0xfe]] {
        seed(&project, bytes);
        let before = snapshot(&project);
        failure(
            call(
                &mut reader,
                &mut writer,
                READ,
                json!({"projectPath": project}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert_eq!(snapshot(&project), before);
    }
    let mut invalid = document(1);
    invalid["configs"][1]["breakpoints"][0]["line"] = json!(0);
    seed(&project, &serde_json::to_vec(&invalid).unwrap());
    let before = snapshot(&project);
    assert_eq!(
        failure(
            call(
                &mut reader,
                &mut writer,
                READ,
                json!({"projectPath": project})
            )
            .await,
            ErrorCode::InvalidArgument
        ),
        "Breakpoint line must be at least 1"
    );
    assert_eq!(snapshot(&project), before);

    let file = config_path(&project);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    fs::write(file.join("keep"), "unchanged").unwrap();
    for method in [READ, WRITE] {
        failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": project, "document": document(99)}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert_eq!(entries(&project.join(".aeroric")), ["debug-configs.json"]);
        assert_eq!(fs::read_to_string(file.join("keep")).unwrap(), "unchanged");
    }
    fs::remove_file(file.join("keep")).unwrap();
    fs::remove_dir(&file).unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": document(99)})
            )
            .await
        ),
        document(1)
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
        document(1)
    );
}

#[tokio::test]
async fn read_only_context_can_read_but_refuses_writes_before_any_path_io() {
    let harness = Harness::start_read_only("debug-no-writer").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project");
    seed(&project, &serde_json::to_vec(&document(99)).unwrap());
    let before = snapshot(&project);
    for path in [
        project.clone(),
        harness.home.join("absent"),
        PathBuf::from("relative"),
    ] {
        let error = failure(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": path, "document": document(1)}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert!(
            error.contains("no write permission") && error.contains("lease"),
            "{error}"
        );
    }
    assert_eq!(snapshot(&project), before);
    assert!(!harness.home.join("absent").exists());
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
        document(99)
    );
}

#[tokio::test]
async fn unauthenticated_read_and_write_never_touch_the_project() {
    let harness = Harness::start("debug-no-auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    let project = harness.home.join("project");
    for method in [READ, WRITE] {
        failure(
            call(
                &mut reader,
                &mut writer,
                method,
                json!({"projectPath": project, "document": document(99)}),
            )
            .await,
            ErrorCode::Unauthenticated,
        );
        assert!(!project.exists());
    }
}

#[tokio::test]
async fn target_refusal_never_reads_or_mutates_local_files_and_matching_local_target_works() {
    let harness = Harness::start("debug-targets").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let project = harness.home.join("project 中文 ");
    seed(&project, &serde_json::to_vec(&document(99)).unwrap());
    let before = snapshot(&project);
    for method in [READ, WRITE] {
        for target in [
            json!({"kind": "ssh", "connectionId": "synthetic", "remotePath": "/srv/fake"}),
            json!({"kind": "wsl", "distribution": "synthetic", "linuxPath": "/srv/fake"}),
        ] {
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    json!({
                        "projectPath": project, "document": document(1), "target": target
                    }),
                )
                .await,
                ErrorCode::UnsupportedCapability,
            );
            assert_eq!(snapshot(&project), before);
        }
        for target in [
            Value::Null,
            json!(false),
            json!({}),
            json!({"kind": "ssh"}),
            json!({"kind": "unknown"}),
            json!({"kind": "local", "path": project.to_str().unwrap().trim_end()}),
        ] {
            failure(
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    json!({
                        "projectPath": project, "document": document(1), "target": target
                    }),
                )
                .await,
                ErrorCode::InvalidArgument,
            );
            assert_eq!(snapshot(&project), before);
        }
    }
    for method in [READ, WRITE] {
        assert_eq!(
            success(
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    json!({
                        "projectPath": project, "document": document(99),
                        "target": {"kind": "local", "path": project},
                    })
                )
                .await
            ),
            document(if method == READ { 99 } else { 1 }),
        );
    }
    assert!(!harness.home.join("project 中文").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn canonical_root_directory_and_leaf_links_keep_legacy_distinct_read_write_semantics() {
    use std::os::unix::fs::symlink;
    let harness = Harness::start("debug-links").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let actual = harness.home.join("actual");
    let project = harness.home.join("linked root ");
    let directory = harness.home.join("config-directory");
    fs::create_dir(&actual).unwrap();
    fs::create_dir(&directory).unwrap();
    symlink(&actual, &project).unwrap();
    symlink(&directory, actual.join(".aeroric")).unwrap();
    let target = harness.home.join("target.json");
    let bytes = serde_json::to_vec(&document(99)).unwrap();
    fs::write(&target, &bytes).unwrap();
    let file = directory.join("debug-configs.json");
    symlink(&target, &file).unwrap();
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
        document(99)
    );
    assert!(fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": document(99)})
            )
            .await
        ),
        document(1)
    );
    assert!(!fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(actual.join(".aeroric"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&target).unwrap(), bytes);
    fs::remove_file(&file).unwrap();
    let absent_target = harness.home.join("absent-target");
    symlink(&absent_target, &file).unwrap();
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
        json!({"version": 1, "configs": []})
    );
    assert!(fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                WRITE,
                json!({"projectPath": project, "document": {}})
            )
            .await
        ),
        json!({"version": 1, "configs": []})
    );
    assert!(!fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!absent_target.exists());
}
