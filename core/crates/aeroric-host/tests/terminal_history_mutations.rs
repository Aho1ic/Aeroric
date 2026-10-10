//! Slice28: real authenticated terminal-history deletion, synthetic files only.

use std::fs;
use std::path::Path;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const DELETE: &str = "project.delete_task_terminal_histories";
const READ: &str = "project.read_task_terminal_history";

async fn connect(harness: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = harness.connect().await;
    let result = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(result, Frame::Success(_)), "{result:?}");
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
        &Harness::request("hist-mutation", method, params),
    )
    .await
}

fn success(reply: Frame) -> Value {
    let Frame::Success(reply) = reply else {
        panic!("expected success, got {reply:?}");
    };
    reply.result
}

fn failure(reply: Frame, expected: ErrorCode) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected {expected:?}, got {reply:?}");
    };
    assert_eq!(reply.error.code, expected, "{:?}", reply.error);
    reply.error.message
}

fn seed(directory: &Path) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("first.log"), "first bytes").unwrap();
    fs::write(directory.join("later.log"), "later bytes").unwrap();
    fs::write(directory.join(" first .log"), "padded filename decoy").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(
            directory.join("later.log"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
}

fn assert_all_kept(directory: &Path) {
    assert_eq!(
        fs::read_to_string(directory.join("first.log")).unwrap(),
        "first bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join(" first .log")).unwrap(),
        "padded filename decoy"
    );
}

#[tokio::test]
async fn deletes_only_selected_injected_history_with_legacy_trim_and_null_reply() {
    let harness = Harness::start("hist-delete").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    seed(&directory);
    let decoy = harness.home.join("other-data/terminal-history");
    seed(&decoy);
    // There is deliberately no project/task registry entry for either log.
    fs::write(
        harness.data_dir.join("projects.json"),
        "unrelated invalid registry",
    )
    .unwrap();
    let params = json!({"taskIds": [" first ", "first", "missing"]});
    assert_eq!(
        success(call(&mut reader, &mut writer, DELETE, params).await),
        Value::Null
    );
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join(" first .log")).unwrap(),
        "padded filename decoy"
    );
    assert_all_kept(&decoy);
    assert_eq!(
        fs::read_to_string(harness.data_dir.join("projects.json")).unwrap(),
        "unrelated invalid registry"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(directory.join("later.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    fs::remove_file(directory.join("later.log")).unwrap();
    fs::remove_file(directory.join(" first .log")).unwrap();
    fs::remove_dir(&directory).unwrap();
    for params in [
        json!({"taskIds": []}),
        json!({"taskIds": ["absent", "absent"]}),
    ] {
        assert_eq!(
            success(call(&mut reader, &mut writer, DELETE, params).await),
            Value::Null
        );
        assert!(
            !directory.exists(),
            "a deletion must not initialize directories"
        );
    }
}

#[tokio::test]
async fn invalid_id_after_a_delete_is_partial_and_the_connection_remains_usable() {
    let harness = Harness::start("hist-bad-id").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    for bad in ["", " \n\t", "../escape", "a/b", "a\\b", "a b", "a\nb"] {
        seed(&directory);
        failure(
            call(
                &mut reader,
                &mut writer,
                DELETE,
                json!({"taskIds": ["first", bad, "later"]}),
            )
            .await,
            ErrorCode::InvalidArgument,
        );
        assert!(
            !directory.join("first.log").exists(),
            "must not prevalidate every string: {bad:?}"
        );
        assert_eq!(
            fs::read_to_string(directory.join("later.log")).unwrap(),
            "later bytes"
        );
        assert_eq!(
            success(call(&mut reader, &mut writer, READ, json!({"taskId": "later"})).await),
            json!("later bytes")
        );
    }
}

#[tokio::test]
async fn malformed_container_or_element_types_refuse_before_any_deletion() {
    let harness = Harness::start("hist-shape").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    seed(&directory);
    for params in [
        Value::Null,
        json!([]),
        json!([["first"]]),
        json!("first"),
        json!({}),
        json!({"taskIds": null}),
        json!({"taskIds": "first"}),
        json!({"taskIds": {}}),
        json!({"taskIds": ["first", null, "later"]}),
        json!({"taskIds": ["first", 1, "later"]}),
        json!({"taskIds": ["first", false, "later"]}),
        json!({"taskIds": ["first", {}, "later"]}),
        json!({"task_ids": ["first"]}),
    ] {
        failure(
            call(&mut reader, &mut writer, DELETE, params).await,
            ErrorCode::InvalidArgument,
        );
        assert_all_kept(&directory);
    }
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                DELETE,
                json!({"taskIds": ["first"]})
            )
            .await
        ),
        Value::Null
    );
    assert!(!directory.join("first.log").exists());
}

#[tokio::test]
async fn directory_error_keeps_prior_removals_and_stops_before_later_ids() {
    let harness = Harness::start("hist-io").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    seed(&directory);
    fs::create_dir(directory.join("blocked.log")).unwrap();
    fs::write(directory.join("blocked.log/sentinel"), "keep").unwrap();
    failure(
        call(
            &mut reader,
            &mut writer,
            DELETE,
            json!({"taskIds": ["first", "blocked", "later"]}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join("blocked.log/sentinel")).unwrap(),
        "keep"
    );
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                DELETE,
                json!({"taskIds": ["later"]})
            )
            .await
        ),
        Value::Null
    );
    assert!(!directory.join("later.log").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn keeps_dangling_links_unlinks_live_leaf_links_and_follows_directory_links() {
    use std::os::unix::fs::symlink;

    let harness = Harness::start("hist-links").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    let linked_directory = harness.home.join("linked-history");
    fs::create_dir(&linked_directory).unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::remove_dir(&directory).unwrap();
    symlink(&linked_directory, &directory).unwrap();
    let target = harness.home.join("leaf-target");
    fs::write(&target, "target remains").unwrap();
    let missing_target = harness.home.join("no-target");
    symlink(&target, directory.join("live.log")).unwrap();
    symlink(&missing_target, directory.join("dangling.log")).unwrap();
    fs::write(
        directory.join("regular.log"),
        "delete through directory link",
    )
    .unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                DELETE,
                json!({"taskIds": ["live", "dangling", "regular"]})
            )
            .await
        ),
        Value::Null
    );
    assert!(fs::symlink_metadata(directory.join("live.log")).is_err());
    assert!(fs::symlink_metadata(directory.join("dangling.log"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!linked_directory.join("regular.log").exists());
    assert_eq!(fs::read_to_string(target).unwrap(), "target remains");
    assert!(!missing_target.exists());
    assert!(fs::symlink_metadata(directory)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[tokio::test]
async fn read_only_host_refuses_mutations_before_io_but_still_decodes_params_first() {
    let harness = Harness::start_read_only("hist-readonly").await;
    let (mut reader, mut writer) = connect(&harness).await;
    let directory = harness.data_dir.join("terminal-history");
    seed(&directory);
    for params in [json!({"taskIds": ["first"]}), json!({"taskIds": []})] {
        let message = failure(
            call(&mut reader, &mut writer, DELETE, params).await,
            ErrorCode::IoError,
        );
        assert!(message.contains("no write permission"), "{message}");
        assert_all_kept(&directory);
    }
    failure(
        call(
            &mut reader,
            &mut writer,
            DELETE,
            json!({"taskIds": ["first", 1]}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert_all_kept(&directory);
}

#[tokio::test]
async fn unauthenticated_deletion_never_reaches_the_filesystem() {
    let harness = Harness::start("hist-unauth").await;
    let (mut reader, mut writer) = harness.connect().await;
    let directory = harness.data_dir.join("terminal-history");
    seed(&directory);
    failure(
        call(
            &mut reader,
            &mut writer,
            DELETE,
            json!({"taskIds": ["first"]}),
        )
        .await,
        ErrorCode::Unauthenticated,
    );
    assert_all_kept(&directory);
}
