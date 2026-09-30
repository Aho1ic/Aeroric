//! 认证面（P3-06 Action 5/验收「错 token/错 UID/未知方法被正确拒绝」；
//! 跨 UID 的活体拒绝需第二用户，超出集成测试边界——accept 循环里
//! peer-UID 先于任何协议字节，判据本体在 auth.rs 单测）。

mod host_harness;

use host_harness::Harness;
use serde_json::json;

fn error_code(frame: &aeroric_protocol::envelope::Frame) -> aeroric_protocol::error::ErrorCode {
    match frame {
        aeroric_protocol::envelope::Frame::Failure(f) => f.error.code.clone(),
        other => panic!("expected failure frame, got {other:?}"),
    }
}

#[tokio::test]
async fn unauthenticated_mutation_is_refused_without_reaching_dispatch() {
    let harness = Harness::start("auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("1", "tasks.start", json!({"x":1})),
    )
    .await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::Unauthenticated
    );
    // 连接不被拒绝帧杀死——gate 是协议回答，不是断连。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "nope", json!({})),
    )
    .await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::Unauthenticated
    );
}

#[tokio::test]
async fn wrong_token_is_refused_and_the_connection_keeps_its_unauthed_state() {
    let harness = Harness::start("auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    let frame = Harness::handshake(&mut reader, &mut writer, "not-the-token", "c").await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::Unauthenticated
    );
    // 失败握手不翻状态：随后非握手方法仍是 Unauthenticated（不是「已认证但方法不存在」）。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "events.since", json!({"after":"0"})),
    )
    .await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::Unauthenticated
    );
    // 同一连接上补一次正确握手仍可成功（token 错不拉黑连接——拉黑是 UID 闸的事）。
    let frame = Harness::handshake(&mut reader, &mut writer, &harness.token, "c1").await;
    assert!(
        matches!(frame, aeroric_protocol::envelope::Frame::Success(s) if s.result["clientId"] == "c1")
    );
}

#[tokio::test]
async fn authenticated_unknown_method_gets_unsupported_capability() {
    let harness = Harness::start("auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "c").await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "tasks.start", json!({})),
    )
    .await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::UnsupportedCapability
    );
}

#[tokio::test]
async fn re_handshake_on_an_authenticated_connection_is_refused() {
    let harness = Harness::start("auth").await;
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "first").await;
    let frame = Harness::handshake(&mut reader, &mut writer, &harness.token, "second").await;
    assert_eq!(
        error_code(&frame),
        aeroric_protocol::error::ErrorCode::Unauthenticated
    );
    // 身份不被覆盖：events.since 仍能走（连接活着，只是不再接受换身份）。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("9", "events.since", json!({"after":"0"})),
    )
    .await;
    assert!(matches!(
        frame,
        aeroric_protocol::envelope::Frame::Success(_)
    ));
}
