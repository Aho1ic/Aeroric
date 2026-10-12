//! 事件重同步（验收「并发 100 个 mutation 的 seq 顺序与 snapshot 一致」
//! 「重连去重、snapshot watermark 缺口」）。第一片的原子边界在这里过真机：
//! 8 线程并发 append 经 RPC `events.since` 读回，序与集合都必须精确。

mod host_harness;

use host_harness::Harness;
use serde_json::json;

#[tokio::test]
async fn concurrent_appends_replay_in_order_to_a_late_watermark() {
    let harness = Harness::start("replay").await;
    let log = arc_clone(&harness.services.log);
    let mut workers = Vec::new();
    for worker in 0..8u64 {
        let log = arc_clone(&log);
        workers.push(tokio::spawn(async move {
            for i in 0..13u64 {
                log.append("task-status", json!({ "worker": worker, "i": i }));
            }
        }));
    }
    for worker in workers {
        worker.await.expect("worker");
    }

    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "c").await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "events.since", json!({"after":"1"})),
    )
    .await;
    let aeroric_protocol::envelope::Frame::Success(success) = frame else {
        panic!("expected success");
    };
    assert_eq!(success.result["latestSeq"], 104);
    assert_eq!(success.result["reset"], false);
    let events = success.result["events"].as_array().expect("array");
    assert_eq!(events.len(), 103, "after=1 ⇒ 2..=104 全部补发");
    let seqs: Vec<u64> = events
        .iter()
        .map(|event| event["seq"].as_u64().expect("numeric seq"))
        .collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "补发序乱: {seqs:?}"
    );
    assert_eq!(seqs.first(), Some(&2));
    assert_eq!(seqs.last(), Some(&104));
}

#[tokio::test]
async fn evicted_watermark_gets_reset_and_the_client_lifts_to_latest() {
    let harness = Harness::start("replay").await;
    for i in 0..(aeroric_core::events::log::MAX_ENTRIES + 20) {
        harness
            .services
            .log
            .append("task-status", json!({ "i": i }));
    }
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "c").await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "events.since", json!({"after":"5"})),
    )
    .await;
    let aeroric_protocol::envelope::Frame::Success(success) = frame else {
        panic!("expected success");
    };
    assert_eq!(
        success.result["reset"], true,
        "被挤穿的 watermark 必须 reset"
    );
    assert_eq!(success.result["events"].as_array().expect("array").len(), 0);
    // 客户端侧合并语义（core snapshot 的集成形状）：reset ⇒ 全量刷新位＋
    // watermark 抬到 latest，不把洞当增量。
    let decision = aeroric_core::events::snapshot::SnapshotDecision::at(5).apply_backfill(
        &[],
        success.result["latestSeq"].as_u64().expect("latest"),
        true,
    );
    assert!(decision.needs_full_refresh);
    assert_eq!(
        decision.watermark,
        success.result["latestSeq"].as_u64().expect("latest")
    );
}

#[tokio::test]
async fn reconnect_backfill_then_live_frames_never_double_apply() {
    let harness = Harness::start("replay").await;
    harness.services.log.append("task-status", json!({"a":1}));
    harness.services.log.append("task-status", json!({"a":2}));
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "c").await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "events.since", json!({"after":"0"})),
    )
    .await;
    let aeroric_protocol::envelope::Frame::Success(success) = frame else {
        panic!("expected success");
    };
    assert_eq!(
        success.result["events"].as_array().expect("array").len(),
        2,
        "切片一百零四语义:after=0 自举补发环内全部(推送型消费者靠 apply_backfill 兜底)"
    );
    assert_eq!(success.result["reset"], false);
    // 切片一百零四语义:after=0 自举补发环内全部——推送型消费者把这一批
    // 经 apply_backfill 应用后,watermark 抬到 latest;随后重连窗口里的
    // 重复直播帧(seq<=watermark)必须被 SnapshotDecision 丢弃。
    let batch: Vec<aeroric_core::events::log::LoggedEvent> = success.result["events"]
        .as_array()
        .expect("array")
        .iter()
        .map(|event| aeroric_core::events::log::LoggedEvent {
            seq: event["seq"].as_u64().expect("seq"),
            event: event["event"].as_str().expect("event").to_string(),
            data: event["data"].clone(),
        })
        .collect();
    let latest = success.result["latestSeq"].as_u64().expect("latest");
    let mut decision = aeroric_core::events::snapshot::SnapshotDecision::at(0)
        .apply_backfill(&batch, latest, false);
    assert_eq!(decision.watermark, 2, "自举把 watermark 抬到 latest");
    assert!(!decision.apply_live(1), "自举已含的重复帧必须丢");
    assert!(!decision.apply_live(2), "自举已含的重复帧必须丢");
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "3",
            "events.since",
            json!({"after": decision.watermark.to_string()}),
        ),
    )
    .await;
    let aeroric_protocol::envelope::Frame::Success(success) = frame else {
        panic!("expected success");
    };
    assert_eq!(success.result["events"].as_array().expect("array").len(), 0);
    assert!(!decision.apply_live(2), "重连窗口的重复帧必须丢");
    assert!(decision.apply_live(3));
}

fn arc_clone<T>(arc: &std::sync::Arc<T>) -> std::sync::Arc<T> {
    std::sync::Arc::clone(arc)
}
