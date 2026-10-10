//! P8-05 slice 25: session id and native resume capability reads over authenticated UDS.
//! All fixtures are synthetic.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

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

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request(id, method, params)).await
}

fn success_result(reply: Frame) -> Value {
    let Frame::Success(success) = reply else {
        panic!("expected a successful reply: {reply:?}");
    };
    success.result
}

#[tokio::test]
async fn read_session_id_resolves_id_or_reports_missing() {
    let harness = Harness::start("session-id-query").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let sessions_dir = harness
        .home
        .join(".claude")
        .join("projects")
        .join("sample-project");
    fixture_dir(&sessions_dir);
    let session_file = sessions_dir.join("test-session.jsonl");
    write_fixture(&session_file, b"{\"sessionId\":\"claude-sess-999\"}\n");

    // Existing session file resolves id
    let reply = query(
        &mut reader,
        &mut writer,
        "read-id-ok",
        "session.read_session_id",
        json!({
            "sessionPath": session_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": false,
            "family": "claude"
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, json!("claude-sess-999"));

    // Missing session file returns error
    let missing_path = sessions_dir.join("missing.jsonl");
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "read-id-missing",
        "session.read_session_id",
        json!({
            "sessionPath": missing_path.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": false,
            "family": "claude"
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_missing else {
        panic!("missing session file must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::IoError);
}

#[tokio::test]
async fn session_supports_native_resume_evaluates_history_mode() {
    let harness = Harness::start("session-resume-query").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let codex_dir = harness.home.join(".codex").join("sessions");
    fixture_dir(&codex_dir);

    // Standard session: supports native resume
    let standard_file = codex_dir.join("standard.jsonl");
    write_fixture(
        &standard_file,
        b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-1\"}}\n",
    );
    let reply_std = query(
        &mut reader,
        &mut writer,
        "resume-std",
        "session.session_supports_native_resume",
        json!({
            "sessionPath": standard_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": true,
            "family": "codex"
        }),
    )
    .await;
    assert_eq!(success_result(reply_std), json!(true));

    // Paginated codex session: does not support native resume
    let paginated_file = codex_dir.join("paginated.jsonl");
    write_fixture(
        &paginated_file,
        b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-2\",\"history_mode\":\"paginated\"}}\n",
    );
    let reply_pag = query(
        &mut reader,
        &mut writer,
        "resume-pag",
        "session.session_supports_native_resume",
        json!({
            "sessionPath": paginated_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": true,
            "family": "codex"
        }),
    )
    .await;
    assert_eq!(success_result(reply_pag), json!(false));
}

#[tokio::test]
async fn read_session_message_page_pages_codex_and_dsh_or_reports_missing() {
    let harness = Harness::start("session-page-query").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // Codex: a single user event_msg line.
    let codex_dir = harness.home.join(".codex").join("sessions");
    fixture_dir(&codex_dir);
    let codex_file = codex_dir.join("page.jsonl");
    write_fixture(
        &codex_file,
        b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\"}}\n",
    );
    let reply = query(
        &mut reader,
        &mut writer,
        "page-codex",
        "session.read_session_message_page",
        json!({
            "sessionPath": codex_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": true,
            "family": "codex",
            "cursor": null
        }),
    )
    .await;
    let page = success_result(reply);
    assert_eq!(page["messages"][0]["role"], json!("user"));
    assert_eq!(page["messages"][0]["content"][0]["text"], json!("hello"));
    assert_eq!(page["messages"][0]["messageId"], Value::Null);
    assert_eq!(page["nextCursor"], Value::Null);
    assert_eq!(page["hasMore"], json!(false));

    // DSH plain transcript (uncompressed): family dsh branch.
    let dsh_dir = harness.home.join(".dsh").join("sessions");
    fixture_dir(&dsh_dir);
    let dsh_file = dsh_dir.join("dsh-page.jsonl");
    write_fixture(
        &dsh_file,
        b"{\"type\":\"user/message\",\"seq\":1,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"hello dsh\"}]}}\n",
    );
    let reply_dsh = query(
        &mut reader,
        &mut writer,
        "page-dsh",
        "session.read_session_message_page",
        json!({
            "sessionPath": dsh_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": false,
            "family": "dsh",
            "cursor": null
        }),
    )
    .await;
    let page_dsh = success_result(reply_dsh);
    assert_eq!(page_dsh["messages"][0]["role"], json!("user"));
    assert_eq!(
        page_dsh["messages"][0]["content"][0]["text"],
        json!("hello dsh")
    );

    // Missing session file → IoError.
    let missing = codex_dir.join("missing.jsonl");
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "page-missing",
        "session.read_session_message_page",
        json!({
            "sessionPath": missing.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": true,
            "family": "codex",
            "cursor": null
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_missing else {
        panic!("missing session file must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::IoError);

    // Missing sessionPath param → InvalidArgument.
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "page-bad",
        "session.read_session_message_page",
        json!({ "isCodex": true }),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing sessionPath must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // cursor continuation on a single-page file: with an earlier cursor the dsh
    // compressed-only first-page rule must not apply (plain transcript), and the
    // page still resolves with hasMore false.
    let reply_cursor = query(
        &mut reader,
        &mut writer,
        "page-cursor",
        "session.read_session_message_page",
        json!({
            "sessionPath": codex_file.to_string_lossy(),
            "projectPath": "/tmp/dummy-project",
            "isCodex": true,
            "family": "codex",
            "cursor": 1
        }),
    )
    .await;
    let cursor_page = success_result(reply_cursor);
    assert_eq!(cursor_page["hasMore"], json!(false));
}
