//! P8-05 slice 21: project.list keeps its wrapper; project.load_projects returns an array.
//! Fixtures are handwritten under the harness root; no project.open or project probe.

use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::json;

mod host_harness;
use host_harness::Harness;

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    bytes: Vec<u8>,
    #[cfg(unix)]
    mode: u32,
}

fn file_snapshot(path: &Path) -> FileSnapshot {
    FileSnapshot {
        bytes: fs::read(path).expect("read scratch projects fixture"),
        #[cfg(unix)]
        mode: fs::metadata(path)
            .expect("fixture metadata")
            .permissions()
            .mode(),
    }
}

fn write_fixture(path: &Path, bytes: &[u8]) -> FileSnapshot {
    fs::write(path, bytes).expect("write scratch projects fixture");
    // A read must not silently tighten an existing file to 0600.
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o640)).expect("fixture permissions");
    file_snapshot(path)
}

fn directory_entries(path: &Path) -> Vec<OsString> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .expect("scratch data directory")
        .map(|entry| entry.expect("data directory entry").file_name())
        .collect();
    entries.sort();
    entries
}

#[tokio::test]
async fn project_list_preserves_raw_records_order_duplicates_and_files() {
    let harness = Harness::start("project-list-full").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let projects_path = harness.data_dir.join("projects.json");
    let project_dirs = [
        harness.data_dir.join("missing-project-b"),
        harness.data_dir.join("missing-project-a"),
        harness.data_dir.join("missing-project-b-again"),
    ];
    for path in &project_dirs {
        assert!(!path.exists());
    }
    let records = json!([
        {
            "id": "b",
            "name": "Beta",
            "path": project_dirs[0].to_string_lossy(),
            "location": {"kind": "local", "path": project_dirs[0].to_string_lossy()},
            "branch": "beta-branch",
            "lastOpenedAt": 30,
            "orderIndex": 2,
            "group": "work",
            "hiddenFromRail": true,
            "pinned": false,
            "avatar": {"color": "blue", "emoji": "🧪", "label": "B"},
            "future": {"nested": [null, true, {"value": "保留"}], "version": 1}
        },
        {
            "id": "a",
            "name": "Alpha",
            "path": project_dirs[1].to_string_lossy(),
            "location": null,
            "branch": null,
            "lastOpenedAt": 10,
            "orderIndex": 1,
            "group": null,
            "hiddenFromRail": false,
            "pinned": true,
            "avatar": null,
            "future": ["alpha", {"unknown": 42}]
        },
        {
            "id": "b",
            "name": "Beta duplicate",
            "path": project_dirs[2].to_string_lossy(),
            "location": {"kind": "local", "path": project_dirs[2].to_string_lossy()},
            "branch": "other-beta-branch",
            "lastOpenedAt": 20,
            "orderIndex": 0,
            "group": "other",
            "hiddenFromRail": false,
            "pinned": true,
            "avatar": {"color": "orange", "emoji": null, "label": "B2"},
            "future": {"duplicate": true, "values": [3, 2, 1]}
        }
    ]);
    let mut bytes = serde_json::to_vec_pretty(&records).expect("raw projects JSON");
    bytes.extend_from_slice(b"\n \n");
    let before = write_fixture(&projects_path, &bytes);
    let entries_before = directory_entries(&harness.data_dir);

    for (method, expected) in [
        ("project.list", json!({"projects": &records})),
        ("project.load_projects", records.clone()),
    ] {
        let reply = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(&format!("full-{method}"), method, json!({})),
        )
        .await;
        let Frame::Success(success) = reply else {
            panic!("{method} failed: {reply:?}");
        };
        assert_eq!(
            success.result, expected,
            "{method} must keep its own reply shape, every raw field and the original b,a,b order"
        );
        assert_eq!(file_snapshot(&projects_path), before, "{method}");
        assert!(!projects_path.with_extension("json.bak").exists());
        assert_eq!(directory_entries(&harness.data_dir), entries_before);
        for path in &project_dirs {
            assert!(!path.exists(), "listing must not create {}", path.display());
        }
    }
}

#[tokio::test]
async fn project_list_preserves_absence_and_refuses_corruption_without_disconnect() {
    let harness = Harness::start("project-list-errors").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let projects_path = harness.data_dir.join("projects.json");
    let entries_without_file = directory_entries(&harness.data_dir);
    assert!(!projects_path.exists());
    for (method, expected) in [
        ("project.list", json!({"projects": []})),
        ("project.load_projects", json!([])),
    ] {
        let absent = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(&format!("absent-{method}"), method, json!({})),
        )
        .await;
        let Frame::Success(success) = absent else {
            panic!("absent {method} failed: {absent:?}");
        };
        assert_eq!(success.result, expected, "{method}");
        assert!(!projects_path.exists(), "a missing list must stay absent");
        assert!(!projects_path.with_extension("json.bak").exists());
        assert_eq!(directory_entries(&harness.data_dir), entries_without_file);
    }

    let empty_before = write_fixture(&projects_path, b" [ \n ] \n");
    let entries_with_file = directory_entries(&harness.data_dir);
    for (method, expected) in [
        ("project.list", json!({"projects": []})),
        ("project.load_projects", json!([])),
    ] {
        let empty = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(&format!("empty-{method}"), method, json!({})),
        )
        .await;
        let Frame::Success(success) = empty else {
            panic!("empty {method} failed: {empty:?}");
        };
        assert_eq!(success.result, expected, "{method}");
        assert_eq!(file_snapshot(&projects_path), empty_before, "{method}");
        assert!(!projects_path.with_extension("json.bak").exists());
        assert_eq!(directory_entries(&harness.data_dir), entries_with_file);
    }

    let recovery_dir = harness.data_dir.join("missing-recovery-project");
    assert!(!recovery_dir.exists());
    let recovery_records = json!([{
        "id": "recovered",
        "name": "Recovered",
        "path": recovery_dir.to_string_lossy(),
        "branch": null,
        "lastOpenedAt": 1,
        "future": {"stillPreserved": [false, null]}
    }]);
    let recovery_bytes = serde_json::to_vec_pretty(&recovery_records).expect("recovery JSON");
    let invalid: [(&str, &[u8]); 4] = [
        ("bad-json", b"{ broken"),
        ("object", br#"{"projects":[]}"#),
        ("null", b"null"),
        ("mixed-array", br#"[{"id":"not-a-partial-success"},false]"#),
    ];
    for (case, payload) in invalid {
        for (method, expected_recovery) in [
            ("project.list", json!({"projects": &recovery_records})),
            ("project.load_projects", recovery_records.clone()),
        ] {
            let before = write_fixture(&projects_path, payload);
            let reply = Harness::call(
                &mut reader,
                &mut writer,
                &Harness::request(&format!("{case}-{method}"), method, json!({})),
            )
            .await;
            let Frame::Failure(failure) = reply else {
                panic!("{method}/{case} must be refused, not returned as an empty list: {reply:?}");
            };
            assert_eq!(failure.error.code, ErrorCode::IoError, "{method}/{case}");
            assert_eq!(file_snapshot(&projects_path), before, "{method}/{case}");
            assert!(
                !projects_path.with_extension("json.bak").exists(),
                "{method}/{case}"
            );
            assert_eq!(directory_entries(&harness.data_dir), entries_with_file);
            assert!(!recovery_dir.exists());

            // Keep the same authenticated socket: an ordinary read error must not close it.
            let recovered_before = write_fixture(&projects_path, &recovery_bytes);
            let recovered = Harness::call(
                &mut reader,
                &mut writer,
                &Harness::request(&format!("recover-{case}-{method}"), method, json!({})),
            )
            .await;
            let Frame::Success(success) = recovered else {
                panic!("{method}/{case} disconnected or prevented recovery: {recovered:?}");
            };
            assert_eq!(success.result, expected_recovery, "{method}/{case}");
            assert_eq!(file_snapshot(&projects_path), recovered_before);
            assert!(!projects_path.with_extension("json.bak").exists());
            assert_eq!(directory_entries(&harness.data_dir), entries_with_file);
            assert!(!recovery_dir.exists());
        }
    }
}
#[tokio::test]
async fn project_save_projects_writes_atomic_records_and_roundtrips_through_load_projects() {
    let harness = Harness::start("project-list-save").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // 1. Invalid arguments rejected
    for (case, params) in [
        ("missing-params", json!({})),
        ("non-array-projects", json!({ "projects": "not an array" })),
        (
            "non-object-element",
            json!({ "projects": ["string-instead-of-object"] }),
        ),
    ] {
        let reply = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(case, "project.save_projects", params),
        )
        .await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must fail with InvalidArgument");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument, "{case}");
    }

    // 2. Successful save
    let initial_projects = json!([
        { "id": "proj-1", "name": "Project One", "path": "/path/to/p1", "customField": 42 },
        { "id": "proj-2", "name": "Project Two", "path": "/path/to/p2" }
    ]);
    let save_reply = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "save-1",
            "project.save_projects",
            json!({ "projects": initial_projects }),
        ),
    )
    .await;
    assert!(matches!(save_reply, Frame::Success(_)), "{save_reply:?}");

    // 3. Roundtrip through project.load_projects (bare array)
    let load_reply = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("load-1", "project.load_projects", json!({})),
    )
    .await;
    let Frame::Success(load_success) = load_reply else {
        panic!("load_projects must succeed: {load_reply:?}");
    };
    assert_eq!(load_success.result, initial_projects);

    // 4. Overwrite with empty projects list
    let empty_save = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "save-empty",
            "project.save_projects",
            json!({ "projects": [] }),
        ),
    )
    .await;
    assert!(matches!(empty_save, Frame::Success(_)), "{empty_save:?}");

    let load_empty = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("load-empty", "project.load_projects", json!({})),
    )
    .await;
    let Frame::Success(empty_success) = load_empty else {
        panic!("load_projects must succeed: {load_empty:?}");
    };
    assert_eq!(empty_success.result, json!([]));

    // 5. Read-only harness has no writer lease, must refuse save
    let ro_harness = Harness::start_read_only("project-list-save-ro").await;
    let (mut ro_reader, mut ro_writer) = ro_harness.connect().await;
    let _ = Harness::handshake(&mut ro_reader, &mut ro_writer, &ro_harness.token, "cli").await;
    let ro_reply = Harness::call(
        &mut ro_reader,
        &mut ro_writer,
        &Harness::request(
            "save-ro",
            "project.save_projects",
            json!({ "projects": [] }),
        ),
    )
    .await;
    let Frame::Failure(ro_failure) = ro_reply else {
        panic!("read-only host must fail on save_projects: {ro_reply:?}");
    };
    assert_eq!(ro_failure.error.code, ErrorCode::IoError);
    assert!(ro_failure.error.message.contains("no write permission"));
}
