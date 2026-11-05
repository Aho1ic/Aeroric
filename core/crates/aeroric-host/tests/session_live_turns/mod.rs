//! R2-1 (2026-11-04): a turn that changed files is recorded, undone, and
//! the agent hears about it with its next message.

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git");
    assert!(out.status.success(), "{args:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_undone_turn_restores_its_files_and_tells_the_agent() {
    let mut host = Host::start(true).await;
    let workspace = host._root.join("workspace");
    std::fs::write(workspace.join("a.txt"), "old\n").unwrap();
    git(&workspace, &["init", "-q"]);
    git(&workspace, &["add", "-A"]);
    git(&workspace, &["commit", "-q", "-m", "base"]);

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
        json!({"thread": {"id": "th-1", "path": "/tmp/aeroric-no-rollout.jsonl"}}),
    );
    let turn = peer.expect("turn/start").await;
    peer.reply(&turn, json!({"turn": {"id": "u1"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u1"}}),
    );
    // Let the turn-start snapshot land before the agent "writes".
    tokio::time::sleep(Duration::from_millis(400)).await;
    std::fs::write(workspace.join("a.txt"), "new\n").unwrap();
    std::fs::write(workspace.join("b.txt"), "made\n").unwrap();
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u1", "status": "completed"}}),
    );

    let deadline = tokio::time::Instant::now() + WAIT;
    let turns = loop {
        let got = host
            .call("turns.list", json!({"taskId": "task-1"}))
            .await
            .unwrap();
        if got["turns"].as_array().is_some_and(|t| !t.is_empty()) {
            break got;
        }
        assert!(tokio::time::Instant::now() < deadline, "no turn recorded");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(turns["turns"][0]["state"], "pending", "{turns}");
    host.until_state("turn over", |s| s["turnActive"] == false)
        .await;

    let plan = host
        .call(
            "turns.plan",
            json!({"taskId": "task-1", "turnId": "u1", "action": "undo"}),
        )
        .await
        .unwrap();
    assert_eq!(plan["plan"]["files"].as_array().unwrap().len(), 2, "{plan}");
    let done = host
        .call(
            "turns.apply",
            json!({"taskId": "task-1", "turnId": "u1", "action": "undo"}),
        )
        .await
        .unwrap();
    assert_eq!(done["turn"]["state"], "undone", "{done}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("a.txt")).unwrap(),
        "old\n"
    );
    assert!(!workspace.join("b.txt").exists());

    // R2-2: the restored turn is reviewed hunk by hunk.
    host.call(
        "turns.apply",
        json!({"taskId": "task-1", "turnId": "u1", "action": "restore"}),
    )
    .await
    .unwrap();
    let diff = host
        .call(
            "changes.diff",
            json!({"taskId": "task-1", "scope": "turn", "turnId": "u1"}),
        )
        .await
        .unwrap();
    let files = diff["diff"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "{diff}");
    let a = files.iter().find(|f| f["path"] == "a.txt").unwrap();
    let hunk = a["hunks"][0]["id"].as_str().unwrap().to_string();
    host.call(
        "changes.comment",
        json!({"taskId": "task-1", "scope": "turn", "turnId": "u1", "hunkId": hunk, "text": "保留旧值"}),
    )
    .await
    .unwrap();
    let rejected = host
        .call(
            "changes.reject",
            json!({"taskId": "task-1", "scope": "turn", "turnId": "u1", "hunkId": hunk}),
        )
        .await
        .unwrap();
    assert_eq!(rejected["state"], "rejected", "{rejected}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("a.txt")).unwrap(),
        "old\n"
    );
    let turns = host
        .call("turns.list", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert_eq!(turns["turns"][0]["rejected"], 1, "{turns}");

    host.call(
        "session.prompt",
        json!({"taskId": "task-1", "text": "继续"}),
    )
    .await
    .unwrap();
    let next = peer.expect("turn/start").await;
    let text = next["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.starts_with("[Aeroric 状态提醒]"), "{text}");
    assert!(
        text.contains("用户撤销了一轮改动") && text.contains("a.txt"),
        "{text}"
    );
    assert!(text.ends_with("继续"), "{text}");
    assert!(text.contains("用户拒绝了 a.txt"), "{text}");
    peer.reply(&next, json!({"turn": {"id": "u2"}}));
    peer.notify(
        "turn/started",
        json!({"threadId": "th-1", "turn": {"id": "u2"}}),
    );
    peer.notify(
        "turn/completed",
        json!({"threadId": "th-1", "turn": {"id": "u2", "status": "completed"}}),
    );
    host.until_state("turn over", |s| s["turnActive"] == false)
        .await;
    let sent = host
        .call("changes.sendComments", json!({"taskId": "task-1"}))
        .await
        .unwrap();
    assert!(sent["deliveryId"].is_string(), "{sent}");
    let comments = peer.expect("turn/start").await;
    let text = comments["params"]["input"][0]["text"].as_str().unwrap();
    assert!(text.contains("a.txt:1 — 保留旧值"), "{text}");
}
