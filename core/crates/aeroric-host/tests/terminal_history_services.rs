//! P8-05 slice 24: terminal history reads over the authenticated UDS method.
//! All files are synthetic.

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
async fn padded_ids_select_the_legacy_canonical_history_not_a_padded_decoy() {
    let harness = Harness::start("hist-trim").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");
    let directory = harness.data_dir.join("terminal-history");
    fixture_dir(&directory);
    let canonical = directory.join("task-trim.log");
    let decoy = directory.join(" task-trim .log");
    write_fixture(&canonical, b"canonical terminal bytes\r\n");
    write_fixture(&decoy, b"wrong padded-name file");
    let modified = fs::metadata(&canonical).unwrap().modified().unwrap();

    for task_id in [
        " task-trim ",
        "\t\ntask-trim\r\n",
        "\u{2003}task-trim\u{2003}",
    ] {
        let result = query(
            &mut reader,
            &mut writer,
            "padded-read",
            "project.read_task_terminal_history",
            json!({"taskId": task_id}),
        )
        .await;
        assert_eq!(
            success_result(result),
            json!("canonical terminal bytes\r\n")
        );
    }
    assert_eq!(
        fs::read(&canonical).unwrap(),
        b"canonical terminal bytes\r\n"
    );
    assert_eq!(fs::read(&decoy).unwrap(), b"wrong padded-name file");
    assert_eq!(
        fs::metadata(&canonical).unwrap().modified().unwrap(),
        modified
    );
    #[cfg(unix)]
    {
        assert_eq!(
            fs::metadata(&canonical).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }
}

#[tokio::test]
async fn read_task_terminal_history_returns_content_or_empty_without_writes() {
    let harness = Harness::start("term-hist-content").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let hist_dir = harness.data_dir.join("terminal-history");
    fixture_dir(&hist_dir);
    let sample_task_id = "task-123_abc";
    let log_file = hist_dir.join(format!("{sample_task_id}.log"));
    write_fixture(&log_file, b"line 1\r\nline 2\r\n");

    // Existing file returns exact contents
    let reply = query(
        &mut reader,
        &mut writer,
        "read-existing",
        "project.read_task_terminal_history",
        json!({"taskId": sample_task_id}),
    )
    .await;
    let text = success_result(reply);
    assert_eq!(text, json!("line 1\r\nline 2\r\n"));

    // Missing file returns empty string, not an error
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "read-missing",
        "project.read_task_terminal_history",
        json!({"taskId": "nonexistent_task"}),
    )
    .await;
    let text_missing = success_result(reply_missing);
    assert_eq!(text_missing, json!(""));
}

#[tokio::test]
async fn read_task_terminal_history_rejects_invalid_task_id() {
    let harness = Harness::start("term-hist-invalid").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    for (case, params) in [
        ("missing-param", json!({})),
        ("null-param", json!(null)),
        ("number-id", json!({"taskId": 123})),
        ("empty-id", json!({"taskId": ""})),
        ("traversal-id", json!({"taskId": "../etc/passwd"})),
        ("control-char-id", json!({"taskId": "task\nid"})),
        ("space-id", json!({"taskId": "task id"})),
    ] {
        let reply = query(
            &mut reader,
            &mut writer,
            case,
            "project.read_task_terminal_history",
            params,
        )
        .await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must fail with InvalidArgument");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument, "{case}");
    }
}

/// **答复必须装得进一帧。** 2026-10-13 AP8 实测的真缺陷:把整个 8 MiB 尾部塞进
/// `result` 会让 `write_frame` 超限失败、宿主直接断连,客户端只看到
/// "host closed the connection" —— 于是"小历史读得到、大历史读不到",而大历史
/// 正是最需要它的时候。判据不是"大文件也能读",而是**大文件读出来的字节数受限且
/// 是一次成功的答复**。
#[tokio::test]
async fn a_history_larger_than_one_frame_is_read_back_bounded_not_by_dropping_the_connection() {
    use aeroric_protocol::MAX_FRAME_BYTES;

    let harness = Harness::start("hist-frame-budget").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let hist_dir = harness.data_dir.join("terminal-history");
    fixture_dir(&hist_dir);
    let task_id = "task-big_history";
    // 4 MiB:超过 1 MiB 帧上限,也在 8 MiB 的保留上限之内(即"合法大小"的历史)。
    let huge = "x".repeat(4 * 1024 * 1024);
    write_fixture(&hist_dir.join(format!("{task_id}.log")), huge.as_bytes());

    let reply = query(
        &mut reader,
        &mut writer,
        "read-huge",
        "project.read_task_terminal_history",
        json!({"taskId": task_id}),
    )
    .await;
    let text = success_result(reply);
    let returned = text.as_str().expect("history is a string").len();
    assert!(returned > 0, "a 4 MiB history must not come back empty");
    assert!(
        returned <= MAX_FRAME_BYTES,
        "the reply must fit one frame: got {returned} bytes against a {MAX_FRAME_BYTES} limit"
    );

    // 连接仍然可用 —— 这才是原缺陷真正丢掉的东西(宿主在超限时断连)。
    let follow_up = query(
        &mut reader,
        &mut writer,
        "after-huge",
        "project.read_task_terminal_history",
        json!({"taskId": "nonexistent_task"}),
    )
    .await;
    assert_eq!(success_result(follow_up), json!(""));
}

/// 尾部是**最新的**那一端:窗口钳过之后,丢的必须是开头而不是结尾。
/// 读历史的人要的是"刚才发生了什么",把结尾截掉等于把它变成最没用的那份。
#[tokio::test]
async fn a_bounded_history_read_keeps_the_tail_not_the_head() {
    let harness = Harness::start("hist-tail-window").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let hist_dir = harness.data_dir.join("terminal-history");
    fixture_dir(&hist_dir);
    let task_id = "task-tail_window";
    // 开头与结尾用**可区分**的标记:两份都是 `H` 的话,"保留尾部"与"保留头部"
    // 在断言上无法分辨,那条用例就测不到它声称的东西。
    let mut content = String::from("THE-VERY-START");
    content.push_str(&"H".repeat(2 * 1024 * 1024));
    content.push_str("THE-VERY-END");
    write_fixture(&hist_dir.join(format!("{task_id}.log")), content.as_bytes());

    let reply = query(
        &mut reader,
        &mut writer,
        "read-tail",
        "project.read_task_terminal_history",
        json!({"taskId": task_id}),
    )
    .await;
    let text = success_result(reply);
    let returned = text.as_str().expect("history is a string");
    assert!(
        returned.ends_with("THE-VERY-END"),
        "a bounded read must keep the newest bytes, not the oldest"
    );
    assert!(
        !returned.starts_with("THE-VERY-START"),
        "the oldest bytes are what gets dropped"
    );
}
