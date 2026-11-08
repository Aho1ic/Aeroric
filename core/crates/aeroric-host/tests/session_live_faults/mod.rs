//! R1-4 (2026-11-04): fault injection on the fake agents. Whatever breaks,
//! a message is never shown as delivered without the agent's answer, and a
//! dead session never holds the queue behind it.

use super::*;

async fn deliveries(host: &Host) -> Vec<Value> {
    host.call("delivery.list", json!({"taskId": "task-1"}))
        .await
        .unwrap()["deliveries"]
        .as_array()
        .unwrap()
        .clone()
}

async fn until_delivery(host: &Host, text: &str, state: &str) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let list = deliveries(host).await;
        if let Some(found) = list
            .iter()
            .find(|d| d["text"] == text && d["state"] == state)
        {
            return found.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {text} to be {state}: {list:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Codex's first turn, through to its end.
async fn codex_first_turn(host: &mut Host) -> Peer {
    host.start_task("hi");
    let mut peer = tokio::time::timeout(WAIT, host.peers.recv())
        .await
        .unwrap()
        .unwrap();
    let init = peer.expect("initialize").await;
    peer.reply(&init, json!({}));
    peer.expect("initialized").await;
    let start = peer.expect("thread/start").await;
    peer.reply(
        &start,
        json!({"thread": {"id": "th-1", "path": "/tmp/aeroric-no-such-rollout.jsonl"}}),
    );
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );
    // "done", not "not running": before the driver has seen the turn start,
    // the record is not running and turnActive is false either, and the
    // next message was steered into the turn (C 批, a flake on a busy Mac).
    host.until("done", |h| h.record()["status"] == "done").await;
    host.until_state("turn over", |s| s["turnActive"] == false)
        .await;
    peer
}

/// Codex dies after taking the message, before it answers `turn/start`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_dying_before_it_answers_leaves_the_message_unknown() {
    let mut host = Host::start(true).await;
    let mut peer = codex_first_turn(&mut host).await;
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "第二条"}),
    )
    .await
    .unwrap();
    peer.expect("turn/start").await;
    drop(peer);
    let lost = until_delivery(&host, "第二条", "unknown").await;
    assert_ne!(lost["evidence"], "protocol", "{lost}");
    host.until("failed", |h| h.record()["status"] == "failed")
        .await;
}

/// Codex takes the message and never answers at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_never_answering_times_out_to_unknown() {
    let mut host = Host::start(true).await;
    host.structured
        .deliveries()
        .set_send_timeout(Duration::from_millis(300));
    let mut peer = codex_first_turn(&mut host).await;
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "没回音"}),
    )
    .await
    .unwrap();
    peer.expect("turn/start").await;
    let lost = until_delivery(&host, "没回音", "unknown").await;
    assert!(lost["reason"].is_string(), "{lost}");
    drop(peer);
}

/// Claude dies after the message is written, before its echo.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claude_dying_before_its_echo_leaves_the_message_unknown() {
    let mut host = Host::start(true).await;
    host.start_agent_task("claude", "你好");
    let (_, mut peer) = tokio::time::timeout(WAIT, host.claude_peers.recv())
        .await
        .unwrap()
        .unwrap();
    peer.claude_turn("cs-1", "你好").await;
    host.until("idle", |h| h.record()["status"] != "running")
        .await;
    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "第二条"}),
    )
    .await
    .unwrap();
    let user = peer.next().await;
    assert_eq!(user["message"]["content"], "第二条", "{user}");
    drop(peer);
    let lost = until_delivery(&host, "第二条", "unknown").await;
    assert_ne!(lost["evidence"], "echo", "{lost}");
}
