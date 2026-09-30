//! 帧层拒绝面（验收「超长 JSONL/partial frame 被正确拒绝」）：
//! 三种 codec 拒绝都必须**断连**，不许回帧、不许猜。

mod host_harness;

use host_harness::{assert_closed, Harness};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn overlong_line_without_newline_kills_the_connection() {
    let harness = Harness::start("frames").await;
    let (mut reader, mut writer) = harness.connect().await;
    let chunk = vec![b'x'; 64 * 1024];
    for _ in 0..20 {
        // 20×64 KiB = 1.25 MiB > 1 MiB；无换行。写侧可能因对端早断而 Err——
        // 那是预期，不断言写成功。
        if writer.write_all(&chunk).await.is_err() {
            break;
        }
    }
    assert_closed(&mut reader).await;
}

#[tokio::test]
async fn closed_mid_frame_kills_the_connection() {
    let harness = Harness::start("frames").await;
    let (mut reader, mut writer) = harness.connect().await;
    writer
        .write_all(b"{\"v\":1,\"id\":\"1\"")
        .await
        .expect("partial write");
    writer.flush().await.ok();
    drop(writer); // 半帧后关闭：host 不许把它当请求
                  // 读侧：host 对这条连接再无响应（它自己也会看到 EOF 断）。这里断言
                  // 我们读不到任何帧——EOF 即「拒绝且没猜」。
    assert_closed(&mut reader).await;
}

#[tokio::test]
async fn malformed_json_line_kills_the_connection() {
    let harness = Harness::start("frames").await;
    let (mut reader, mut writer) = harness.connect().await;
    writer
        .write_all(b"{not json at all}\n")
        .await
        .expect("write malformed");
    assert_closed(&mut reader).await;
}

#[tokio::test]
async fn a_valid_request_still_works_after_a_sibling_connection_died() {
    // 「slow client/坏 client 不阻塞其他客户端」的最小面：一条连接被断，
    // 另一条照常握手。
    let harness = Harness::start("frames").await;
    let (mut bad_reader, mut bad_writer) = harness.connect().await;
    bad_writer.write_all(b"junk\n").await.ok();
    assert_closed(&mut bad_reader).await;

    let (mut reader, mut writer) = harness.connect().await;
    let frame = Harness::handshake(&mut reader, &mut writer, &harness.token, "ok").await;
    assert!(matches!(
        frame,
        aeroric_protocol::envelope::Frame::Success(_)
    ));
}

#[tokio::test]
async fn a_slow_client_holding_a_half_frame_blocks_nobody_else() {
    // 验收「slow client 不阻塞其他客户端」的正面断言：一条连接停在半帧
    // （写了 `{` 不写完也不关），另一条照常握手＋调用。每连接独立任务的
    // 形状在这里过真机。
    let harness = Harness::start("frames").await;
    let (_slow_reader, mut slow_writer) = harness.connect().await;
    slow_writer
        .write_all(b"{\"v\":1,")
        .await
        .expect("half frame parked");
    // 不 flush 后的半帧已在内核缓冲；不读、不关——它就是慢客户端。

    let (mut reader, mut writer) = harness.connect().await;
    let frame = Harness::handshake(&mut reader, &mut writer, &harness.token, "fast").await;
    assert!(matches!(
        frame,
        aeroric_protocol::envelope::Frame::Success(_)
    ));
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "events.since", serde_json::json!({"after":"0"})),
    )
    .await;
    assert!(matches!(
        frame,
        aeroric_protocol::envelope::Frame::Success(_)
    ));
}
