use super::*;
use std::time::{Duration, Instant};

use aeroric_protocol::review::{ReviewerChoice, Severity};
use aeroric_protocol::session_live::AutoCheckItem;

struct Quiet;
impl LiveStatusSink for Quiet {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

#[derive(Default)]
struct FakeHost {
    settings: Mutex<HashMap<String, ReviewSettings>>,
    /// (task, text) of every message sent, starts included.
    sent: Mutex<Vec<(String, String)>>,
    fail_start: Mutex<bool>,
    starts: Mutex<u32>,
}

impl ReviewHost for FakeHost {
    fn settings(&self, task_id: &str) -> Option<ReviewSettings> {
        self.settings.lock().get(task_id).cloned()
    }
    fn start_reviewer(&self, parent: &str, prompt: String) -> Result<Reviewer, String> {
        if *self.fail_start.lock() {
            return Err("没有可用的审查者".into());
        }
        let id = {
            let mut starts = self.starts.lock();
            *starts += 1;
            format!("rev-{starts}")
        };
        self.sent.lock().push((id.clone(), prompt));
        Ok(Reviewer {
            task_id: id,
            label: format!("Claude Code · sonnet（审 {parent}）"),
            same_family: false,
        })
    }
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        self.sent.lock().push((task_id.into(), text));
        Ok(())
    }
}

struct Rig {
    host: Arc<FakeHost>,
    reviews: Arc<ReviewLoop>,
    seq: Mutex<u64>,
}

fn rig(enabled: bool, paused: bool) -> Rig {
    let host = Arc::new(FakeHost::default());
    host.settings.lock().insert(
        "t1".into(),
        ReviewSettings {
            enabled,
            reviewer: ReviewerChoice::Auto,
            paused,
        },
    );
    let reviews = ReviewLoop::new(Arc::new(Quiet));
    reviews.set_host(Arc::clone(&host) as Arc<dyn ReviewHost>);
    Rig {
        host,
        reviews,
        seq: Mutex::new(0),
    }
}

fn diff(files: &[&str]) -> TurnDiff {
    TurnDiff {
        top: "/w".into(),
        before: String::new(),
        after: String::new(),
        files: files
            .iter()
            .map(|p| super::super::update::TurnFileChange {
                path: (*p).into(),
                additions: Some(1),
                deletions: Some(0),
            })
            .collect(),
        additions: files.len() as u64,
        deletions: 0,
    }
}

impl Rig {
    fn push(&self, task: &str, kind: SessionUpdateKind) {
        let mut seq = self.seq.lock();
        *seq += 1;
        self.reviews.update(&SessionUpdate {
            task_id: task.into(),
            seq: *seq,
            at_ms: 0,
            kind,
        });
    }
    /// One whole turn of `task` that says `text`.
    fn turn(&self, task: &str, turn: &str, text: &str) {
        self.push(
            task,
            SessionUpdateKind::TurnStarted {
                turn_id: turn.into(),
            },
        );
        self.push(
            task,
            SessionUpdateKind::MessageCompleted {
                message_id: format!("m-{turn}"),
                text: Some(text.into()),
            },
        );
        self.push(
            task,
            SessionUpdateKind::TurnEnded {
                turn_id: turn.into(),
                reason: TurnEndReason::Completed,
                error: None,
                raw_status: None,
            },
        );
    }
    fn settled(&self, task: &str, turn: &str, changed: Option<TurnDiff>, checking: bool) {
        self.reviews.turn_settled(TurnSettled {
            task_id: task.into(),
            turn_id: turn.into(),
            reason: TurnEndReason::Completed,
            error: None,
            diff: changed,
            checking,
        });
    }
    fn state(&self) -> ReviewState {
        self.reviews.card("t1").map(|c| c.state).unwrap_or_default()
    }
    fn wait_sent(&self, count: usize) -> Vec<(String, String)> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let sent = self.host.sent.lock().clone();
            if sent.len() >= count {
                return sent;
            }
            assert!(
                Instant::now() < deadline,
                "only {} sent: {sent:?}",
                sent.len()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn wait_reviewer(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.reviews.parent_of("rev-1").is_none() {
            assert!(Instant::now() < deadline, "the reviewer never started");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

const BLOCKER: &str = "```json\n{\"verdict\":\"changes\",\"summary\":\"缺上限\",\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"src/a.rs\",\"line\":3,\"text\":\"重试没有上限\"},{\"id\":\"r2\",\"severity\":\"minor\",\"text\":\"命名\"}]}\n```";
/// E1: a pass that says which files it reviewed.
const PASS: &str = "```json\n{\"verdict\":\"pass\",\"files\":[{\"path\":\"src/a.rs\",\"status\":\"reviewed\"},{\"path\":\"x\",\"status\":\"reviewed\"}],\"findings\":[]}\n```";
/// A pass that says nothing about the files.
const BARE_PASS: &str = "```json\n{\"verdict\":\"pass\",\"findings\":[]}\n```";

fn answer(r1: &str, r2: &str) -> String {
    format!("```json\n{{\"responses\":[{{\"id\":\"r1\",\"decision\":\"{r1}\",\"reason\":\"理由\"}},{{\"id\":\"r2\",\"decision\":\"{r2}\"}}]}}\n```")
}

/// Up to the writer having been sent round `n`'s findings.
fn to_writer(rig: &Rig) {
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", BLOCKER);
    let sent = rig.wait_sent(2);
    assert_eq!(sent[1].0, "t1");
    assert_eq!(rig.state(), ReviewState::WaitingCoder);
}

#[test]
fn a_clean_review_passes_and_the_reviewer_never_sees_the_conversation() {
    let rig = rig(true, false);
    rig.turn("t1", "u1", "我的私下推理：先改 a.rs");
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    let sent = rig.wait_sent(1);
    assert_eq!(sent[0].0, "rev-1");
    assert!(sent[0].1.starts_with(prompt::REVIEW_MARKER));
    assert!(sent[0].1.contains("src/a.rs"));
    assert!(!sent[0].1.contains("私下推理"));
    rig.wait_reviewer();
    assert_eq!(rig.state(), ReviewState::Reviewing);
    rig.turn("rev-1", "v1", PASS);
    assert_eq!(rig.state(), ReviewState::Passed);
    assert_eq!(rig.wait_sent(1).len(), 1, "a pass sends the writer nothing");
}

#[test]
fn adopted_findings_with_changes_start_the_next_round_with_the_same_reviewer() {
    let rig = rig(true, false);
    to_writer(&rig);
    let request = &rig.wait_sent(2)[1].1;
    assert!(request.starts_with(prompt::REPLY_MARKER));
    assert!(request.contains("[r1 · 阻断] src/a.rs:3 — 重试没有上限"));
    rig.turn("t1", "u2", &answer("adopt", "reject"));
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(card.findings[0].decision, Some(Decision::Adopt));
    assert_eq!(card.findings[1].decision, Some(Decision::Reject));
    assert_eq!(
        rig.state(),
        ReviewState::WaitingCoder,
        "waits for the turn's diff"
    );
    rig.settled("t1", "u2", Some(diff(&["src/a.rs"])), false);
    let sent = rig.wait_sent(3);
    assert_eq!(sent[2].0, "rev-1");
    assert!(sent[2].1.contains("第 2 轮"));
    assert!(sent[2].1.contains("上一轮你的意见与对方的答复"));
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(
        (card.round, card.state, card.turn_id.as_str()),
        (2, ReviewState::Reviewing, "u2")
    );
    assert!(card.findings.is_empty());
}

#[test]
fn rejecting_every_blocker_settles_and_asks_the_user_to_look() {
    let rig = rig(true, false);
    to_writer(&rig);
    rig.turn("t1", "u2", &answer("reject", "reject"));
    rig.settled("t1", "u2", None, false);
    assert_eq!(rig.state(), ReviewState::Settled);
    let attention = rig.reviews.attention();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].0, "t1");
}

#[test]
fn an_ask_or_a_missing_answer_escalates() {
    let rig = rig(true, false);
    to_writer(&rig);
    rig.turn("t1", "u2", "改好了，没有 JSON");
    rig.settled("t1", "u2", Some(diff(&["src/a.rs"])), false);
    assert_eq!(rig.state(), ReviewState::Escalated);
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(card.findings[0].reason.as_deref(), Some("没有答复"));
    assert_eq!(rig.reviews.attention().len(), 1);
}

#[test]
fn adopting_without_changing_anything_settles() {
    let rig = rig(true, false);
    to_writer(&rig);
    // The tracker can finish before the turn's end reaches the loop.
    rig.push(
        "t1",
        SessionUpdateKind::TurnStarted {
            turn_id: "u2".into(),
        },
    );
    rig.push(
        "t1",
        SessionUpdateKind::MessageCompleted {
            message_id: "m-u2".into(),
            text: Some(answer("adopt", "adopt")),
        },
    );
    rig.settled("t1", "u2", None, false);
    assert_eq!(
        rig.state(),
        ReviewState::WaitingCoder,
        "the answer is not in yet"
    );
    rig.push(
        "t1",
        SessionUpdateKind::TurnEnded {
            turn_id: "u2".into(),
            reason: TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    );
    assert_eq!(rig.state(), ReviewState::Settled);
}

#[test]
fn the_third_round_is_the_last() {
    let rig = rig(true, false);
    to_writer(&rig);
    for round in 2..=3 {
        let turn = format!("u{round}");
        rig.turn("t1", &turn, &answer("adopt", "adopt"));
        rig.settled("t1", &turn, Some(diff(&["src/a.rs"])), false);
        let sent = rig.wait_sent(round as usize * 2 - 1);
        assert!(sent.last().unwrap().1.contains(&format!("第 {round} 轮")));
        rig.turn("rev-1", &format!("v{round}"), BLOCKER);
        rig.wait_sent(round as usize * 2);
    }
    rig.turn("t1", "u4", &answer("adopt", "adopt"));
    rig.settled("t1", "u4", Some(diff(&["src/a.rs"])), false);
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!((card.round, card.state), (3, ReviewState::Escalated));
    assert!(card.raw.unwrap().contains("3 轮"));
}

#[test]
fn a_paused_loop_holds_the_findings_until_sent() {
    let rig = rig(true, true);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", BLOCKER);
    assert_eq!(rig.state(), ReviewState::WaitingSend);
    assert_eq!(rig.host.sent.lock().len(), 1);
    rig.reviews.send("t1").unwrap();
    assert_eq!(rig.state(), ReviewState::WaitingCoder);
    assert_eq!(rig.wait_sent(2)[1].0, "t1");
    assert_eq!(rig.reviews.send("t1"), Err(ReviewError::NothingToSend));
}

#[test]
fn off_reviews_nothing_but_a_request_reviews_the_last_change() {
    let rig = rig(false, false);
    assert_eq!(
        rig.reviews.request("t1").unwrap_err(),
        ReviewError::NoChanges
    );
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.host.sent.lock().is_empty());
    assert!(rig.reviews.card("t1").is_none());
    let card = rig.reviews.request("t1").unwrap();
    assert_eq!(
        (card.turn_id.as_str(), card.state),
        ("u1", ReviewState::Reviewing)
    );
    assert_eq!(rig.reviews.request("t1").unwrap_err(), ReviewError::Busy);
    assert_eq!(rig.wait_sent(1)[0].0, "rev-1");
    assert_eq!(
        rig.reviews.request("other").unwrap_err(),
        ReviewError::NotReviewable
    );
}

/// B4-CU-02: a changed reviewer choice starts a new reviewer for the next
/// loop; an unchanged one reuses the old reviewer.
#[test]
fn a_changed_reviewer_choice_starts_a_new_reviewer() {
    let rig = rig(false, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    rig.reviews.request("t1").unwrap();
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", PASS);
    assert_eq!(rig.state(), ReviewState::Passed);

    rig.reviews.request("t1").unwrap();
    assert_eq!(rig.wait_sent(2)[1].0, "rev-1");
    rig.turn("rev-1", "v2", PASS);
    assert_eq!(rig.state(), ReviewState::Passed);

    rig.host.settings.lock().get_mut("t1").unwrap().reviewer = ReviewerChoice::Named {
        agent: "codex".into(),
        agent_config_id: None,
        model: None,
    };
    rig.reviews.request("t1").unwrap();
    assert_eq!(rig.wait_sent(3)[2].0, "rev-2");
    let deadline = Instant::now() + Duration::from_secs(3);
    while rig.reviews.card("t1").and_then(|c| c.reviewer_task_id) != Some("rev-2".into()) {
        assert!(
            Instant::now() < deadline,
            "the new reviewer never took the loop"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // The replaced reviewer finishing late is not this loop's verdict.
    rig.turn("rev-1", "v3", BLOCKER);
    assert_eq!(rig.state(), ReviewState::Reviewing);
    rig.turn("rev-2", "w1", PASS);
    assert_eq!(rig.state(), ReviewState::Passed);
}

#[test]
fn a_turn_with_checks_waits_for_them_in_either_order() {
    let rig = rig(true, false);
    let item = AutoCheckItem {
        kind: "tests".into(),
        profile: "rust".into(),
        label: "cargo test".into(),
        state: "failed".into(),
        summary: "3 项中 1 项失败".into(),
        failures: vec!["retry_caps".into()],
        output_tail: String::new(),
        exit_code: None,
        issues: None,
        duration_ms: None,
        optional: false,
        truncated: false,
    };
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), true);
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.host.sent.lock().is_empty(), "waits for the checks");
    assert_eq!(rig.state(), ReviewState::Reviewing);
    rig.push(
        "t1",
        SessionUpdateKind::AutoCheck {
            turn_id: "u1".into(),
            state: "failed".into(),
            trigger: None,
            note: None,
            input_revision: None,
            items: vec![item.clone()],
        },
    );
    let sent = rig.wait_sent(1);
    assert!(sent[0].1.contains("有失败") && sent[0].1.contains("retry_caps"));

    let early = self::rig(true, false);
    early.push(
        "t1",
        SessionUpdateKind::AutoCheck {
            turn_id: "u1".into(),
            state: "passed".into(),
            trigger: None,
            note: None,
            input_revision: None,
            items: vec![],
        },
    );
    early.settled("t1", "u1", Some(diff(&["src/a.rs"])), true);
    assert!(early.wait_sent(1)[0].1.contains("全部通过"));
}

#[test]
fn reviewers_branches_and_unchanged_turns_are_never_reviewed() {
    let rig = rig(true, false);
    rig.settled("t1", "u0", None, false);
    rig.settled("branch", "b1", Some(diff(&["x"])), false);
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.host.sent.lock().is_empty());
    to_writer(&rig);
    rig.settled("rev-1", "v9", Some(diff(&["x"])), false);
    assert!(rig.reviews.card("rev-1").is_none());
}

#[test]
fn an_unreadable_verdict_fails_with_the_text_and_can_be_retried() {
    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", "我觉得挺好的");
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(card.state, ReviewState::Failed);
    assert_eq!(card.raw.as_deref(), Some("我觉得挺好的"));
    rig.reviews.retry("t1").unwrap();
    assert_eq!(rig.state(), ReviewState::Reviewing);
    assert_eq!(rig.wait_sent(2)[1].0, "rev-1");
    rig.turn("rev-1", "v2", BLOCKER);
    assert_eq!(
        rig.reviews.card("t1").unwrap().findings[0].severity,
        Severity::Blocker
    );
}

#[test]
fn a_reviewer_that_cannot_start_fails_the_round() {
    let rig = rig(true, false);
    *rig.host.fail_start.lock() = true;
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while rig.state() != ReviewState::Failed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        rig.reviews.card("t1").unwrap().raw.as_deref(),
        Some("没有可用的审查者")
    );
}

#[test]
fn a_loop_left_mid_round_reads_back_as_interrupted() {
    let dir = super::super::checkpoint::store::tests::Scratch::new();
    let rig = rig(true, false);
    rig.reviews.set_dir(dir.path().to_path_buf());
    rig.settled("t1", "u1", Some(diff(&["src/a.rs"])), false);
    rig.wait_reviewer();
    // The reviewer is known in memory a moment before the card is saved.
    let file = dir.path().join("t1.json");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !std::fs::read_to_string(&file).is_ok_and(|s| s.contains("rev-1")) {
        assert!(Instant::now() < deadline, "the card was never saved");
        std::thread::sleep(Duration::from_millis(5));
    }
    let again = ReviewLoop::new(Arc::new(Quiet));
    again.set_dir(dir.path().to_path_buf());
    let card = again.card("t1").unwrap();
    assert_eq!(card.state, ReviewState::Interrupted);
    assert_eq!(again.parent_of("rev-1").as_deref(), Some("t1"));
}

/// C 批 (2026-11-08): the reviewer is told what in the diff may weaken its
/// checks, to judge each; a diff without any says nothing about it.
#[test]
fn gate_flags_reach_the_reviewer_to_judge() {
    let flags = vec!["a.rs:3 加了 `#[ignore`，跳过了测试".to_string()];
    let context = |gates| prompt::Context {
        round: 1,
        acceptance: &[],
        constraints: &[],
        checks: None,
        gates,
        diff_text: Some("diff"),
        previous: None,
        selected: &[],
        excluded: &[],
        rules: "",
    };
    let text = prompt::reviewer(&diff(&["a.rs"]), &context(&flags));
    assert!(text.contains("## 门禁完整性"), "{text}");
    assert!(text.contains("- a.rs:3 加了 `#[ignore`，跳过了测试"));
    let clean = prompt::reviewer(&diff(&["a.rs"]), &context(&[]));
    assert!(!clean.contains("门禁完整性"));
}

/// B5: the second round repeats the first (the same blocking issue,
/// reworded but at the same place) and the answer leaves the files exactly
/// as before: the loop stops instead of using up its rounds.
#[test]
fn a_round_that_repeats_the_last_one_stops_the_loop() {
    const REWORDED: &str = "```json\n{\"verdict\":\"changes\",\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"src/a.rs\",\"line\":3,\"text\":\"重试次数仍然没有上限！\"},{\"id\":\"r2\",\"severity\":\"minor\",\"text\":\"另一个命名\"}]}\n```";
    let tree = |after: &str| TurnDiff {
        after: after.into(),
        ..diff(&["src/a.rs"])
    };
    let rig = rig(true, false);
    to_writer(&rig);
    rig.turn("t1", "u2", &answer("adopt", "adopt"));
    rig.settled("t1", "u2", Some(tree("tree-1")), false);
    rig.wait_sent(3);
    rig.turn("rev-1", "v2", REWORDED);
    rig.wait_sent(4);
    rig.turn("t1", "u3", &answer("adopt", "adopt"));
    rig.settled("t1", "u3", Some(tree("tree-1")), false);
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!((card.round, card.state), (2, ReviewState::Escalated));
    assert_eq!(card.raw.as_deref(), Some(super::NO_PROGRESS));
}

/// B5: the same issues while the files keep changing is an author still
/// trying: the loop goes on.
#[test]
fn the_same_issues_with_new_changes_go_on() {
    let tree = |after: &str| TurnDiff {
        after: after.into(),
        ..diff(&["src/a.rs"])
    };
    let rig = rig(true, false);
    to_writer(&rig);
    rig.turn("t1", "u2", &answer("adopt", "adopt"));
    rig.settled("t1", "u2", Some(tree("tree-1")), false);
    rig.wait_sent(3);
    rig.turn("rev-1", "v2", BLOCKER);
    rig.wait_sent(4);
    rig.turn("t1", "u3", &answer("adopt", "adopt"));
    rig.settled("t1", "u3", Some(tree("tree-2")), false);
    let sent = rig.wait_sent(5);
    assert!(sent.last().unwrap().1.contains("第 3 轮"));
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!((card.round, card.state), (3, ReviewState::Reviewing));
}

/// E1: a reviewer that says "pass" but neither had the diff in its prompt,
/// listed a file nor opened one: the round rests on nothing.
#[test]
fn a_pass_that_rests_on_no_file_is_not_a_pass() {
    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs", "src/b.rs"])), false);
    rig.wait_sent(1);
    rig.turn("rev-1", "v1", BARE_PASS);
    assert_eq!(rig.state(), ReviewState::Escalated);
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(card.raw.as_deref(), Some(coverage::NOT_REVIEWED));
    let cov = card.coverage.expect("coverage");
    assert_eq!(cov.state, CoverageState::Unreviewed);
    assert!(cov
        .files
        .iter()
        .all(|f| f.state == aeroric_protocol::review::FileCoverage::Missed));
}

/// E1: a file the reviewer opened is seen even when it did not list it;
/// one it never opened stays missed, and the pass says so.
#[test]
fn a_file_the_reviewer_opened_counts_as_seen() {
    use aeroric_protocol::review::FileCoverage;
    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs", "src/b.rs"])), false);
    let prompt = rig.wait_sent(1)[0].1.clone();
    assert!(
        prompt.contains("要审查的文件：\n- src/a.rs\n- src/b.rs"),
        "{prompt}"
    );
    rig.push(
        "rev-1",
        SessionUpdateKind::TurnStarted {
            turn_id: "v1".into(),
        },
    );
    rig.push(
        "rev-1",
        SessionUpdateKind::ToolCall {
            id: "c1".into(),
            kind: aeroric_protocol::session_live::ToolKind::Command,
            title: "git diff".into(),
            input: serde_json::json!({"command": "git -C /w diff a b -- src/a.rs"}),
        },
    );
    rig.push(
        "rev-1",
        SessionUpdateKind::MessageCompleted {
            message_id: "m-v1".into(),
            text: Some(BARE_PASS.into()),
        },
    );
    rig.push(
        "rev-1",
        SessionUpdateKind::TurnEnded {
            turn_id: "v1".into(),
            reason: TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    );
    assert_eq!(rig.state(), ReviewState::Passed);
    let cov = rig.reviews.card("t1").unwrap().coverage.unwrap();
    assert_eq!(cov.state, CoverageState::Partial);
    let states: Vec<_> = cov.files.iter().map(|f| f.state).collect();
    assert_eq!(states, [FileCoverage::Seen, FileCoverage::Missed]);
}

/// E1: a secret is named as left out and never put in the prompt; the
/// reviewer's own per-file account is kept.
#[test]
fn a_secret_is_left_out_of_the_review_and_the_files_are_accounted_for() {
    use aeroric_protocol::review::FileCoverage;
    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs", "config/.env"])), false);
    let prompt = rig.wait_sent(1)[0].1.clone();
    assert!(
        prompt.contains("不在审查范围、不要打开的文件：\n- config/.env（可能含有凭据"),
        "{prompt}"
    );
    rig.turn("rev-1", "v1", PASS);
    assert_eq!(rig.state(), ReviewState::Passed);
    let cov = rig.reviews.card("t1").unwrap().coverage.unwrap();
    assert_eq!(cov.state, CoverageState::Complete);
    let ends: Vec<_> = cov
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.state))
        .collect();
    assert_eq!(
        ends,
        [
            ("src/a.rs", FileCoverage::Reviewed),
            ("config/.env", FileCoverage::Excluded)
        ]
    );
}

/// E1: the inlined diff leaves the secret's lines out, in a real repository.
#[test]
fn the_inlined_diff_never_carries_a_secret() {
    let dir = std::env::temp_dir().join(format!("aeroric-e1-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    let before = git(&["write-tree"]);
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.join("config/.env"), "TOKEN=sk-very-secret\n").unwrap();
    git(&["add", "-A"]);
    let after = git(&["write-tree"]);
    let mut changed = diff(&["a.rs", "config/.env"]);
    changed.top = dir.clone();
    changed.before = before;
    changed.after = after;
    let text = diff_text(&changed, &["config/.env"]).unwrap();
    assert!(text.contains("fn a() {}"), "{text}");
    assert!(!text.contains("sk-very-secret"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

/// E2: a finding with the wrong line and a `./` path lands on its code in
/// the file the turn left; the same finding said twice is kept once, at the
/// stronger level.
#[test]
fn findings_land_on_their_code_and_say_the_same_thing_once() {
    let dir = std::env::temp_dir().join(format!("aeroric-e2-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    let before = git(&["write-tree"]);
    std::fs::write(
        dir.join("src/a.rs"),
        "fn a() {\n    // 说明\n    loop {\n        retry();\n    }\n}\n",
    )
    .unwrap();
    git(&["add", "-A"]);
    let after = git(&["write-tree"]);
    let mut changed = diff(&["src/a.rs"]);
    changed.top = dir.clone();
    changed.before = before;
    changed.after = after;

    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(changed), false);
    rig.wait_sent(1);
    const TWICE: &str = "```json\n{\"verdict\":\"changes\",\"files\":[{\"path\":\"src/a.rs\",\"status\":\"reviewed\"}],\"findings\":[{\"id\":\"r1\",\"severity\":\"minor\",\"category\":\"bug\",\"file\":\"./src/a.rs\",\"line\":1,\"snippet\":\"  retry();\",\"text\":\"重试没有上限\"},{\"id\":\"r2\",\"severity\":\"blocker\",\"category\":\"bug\",\"file\":\"src/a.rs\",\"line\":4,\"snippet\":\"retry();\",\"text\":\"换个说法：会一直重试\"}]}\n```";
    rig.turn("rev-1", "v1", TWICE);
    let card = rig.reviews.card("t1").unwrap();
    assert_eq!(card.findings.len(), 1, "{:?}", card.findings);
    let f = &card.findings[0];
    assert_eq!(f.id, "r1");
    assert_eq!(f.file.as_deref(), Some("src/a.rs"));
    assert_eq!((f.line, f.moved_from), (Some(4), Some(1)));
    assert_eq!(f.severity, Severity::Blocker);
    assert_eq!(f.merged, 1);
    assert_eq!(f.category.as_deref(), Some("bug"));
    // The writer gets the quoted code with the finding.
    let sent = rig.wait_sent(2);
    assert!(sent[1].1.contains("retry();"), "{}", sent[1].1);
    std::fs::remove_dir_all(&dir).ok();
}

/// E3: round 2 against round 1. The answer changed only a.rs: the finding
/// on a.rs said again persists, the other one on a.rs is resolved, and the
/// one on b.rs, which nobody looked at again, is not re-reviewed.
#[test]
fn the_second_round_says_what_persists_what_is_resolved_and_what_went_unchecked() {
    const ROUND1: &str = "```json\n{\"verdict\":\"changes\",\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"src/a.rs\",\"line\":3,\"text\":\"重试没有上限\"},{\"id\":\"r2\",\"severity\":\"major\",\"file\":\"src/b.rs\",\"text\":\"会崩\"},{\"id\":\"r3\",\"severity\":\"minor\",\"file\":\"src/a.rs\",\"text\":\"忙等\"}]}\n```";
    const ADOPT_ALL: &str = "```json\n{\"responses\":[{\"id\":\"r1\",\"decision\":\"adopt\"},{\"id\":\"r2\",\"decision\":\"adopt\"},{\"id\":\"r3\",\"decision\":\"adopt\"}]}\n```";
    const ROUND2: &str = "```json\n{\"verdict\":\"changes\",\"files\":[{\"path\":\"src/a.rs\",\"status\":\"reviewed\"}],\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"src/a.rs\",\"line\":5,\"text\":\"重试没有上限\"},{\"id\":\"r2\",\"severity\":\"minor\",\"file\":\"src/a.rs\",\"text\":\"日志太多\"}]}\n```";
    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(diff(&["src/a.rs", "src/b.rs"])), false);
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", ROUND1);
    rig.wait_sent(2);
    assert!(
        rig.reviews.card("t1").unwrap().compare.is_none(),
        "round 1 has nothing before it"
    );
    rig.turn("t1", "u2", ADOPT_ALL);
    rig.settled("t1", "u2", Some(diff(&["src/a.rs"])), false);
    let sent = rig.wait_sent(3);
    assert!(sent[2].1.contains("用原来的 file 和 snippet 再报一次"));
    rig.turn("rev-1", "v2", ROUND2);
    let sent = rig.wait_sent(4);

    let card = rig.reviews.card("t1").unwrap();
    assert_eq!((card.round, card.state), (2, ReviewState::WaitingCoder));
    let repeats: Vec<_> = card
        .findings
        .iter()
        .map(|f| (f.id.as_str(), f.repeats.as_deref()))
        .collect();
    assert_eq!(repeats, [("r1", Some("r1")), ("r2", None)]);
    let cmp = card.compare.expect("round 2 compares");
    assert_eq!(cmp.previous_round, 1);
    let ids = |list: &[aeroric_protocol::review::Finding]| {
        list.iter().map(|f| f.id.clone()).collect::<Vec<_>>()
    };
    assert_eq!(ids(&cmp.resolved), ["r3"]);
    assert_eq!(ids(&cmp.unchecked), ["r2"], "b.rs was not looked at again");
    assert!(sent[3].1.contains("上一轮也提过，还没改好"));
}

/// E4: the project's rules reach the reviewer for the files they match, as
/// the project had them before the turn, even when the turn rewrote the
/// rules file; a finding keeps only a rule the prompt carried.
#[test]
fn project_rules_reach_the_reviewer_as_they_were_before_the_turn() {
    let dir = std::env::temp_dir().join(format!("aeroric-e4-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join(".aeroric")).unwrap();
    std::fs::create_dir_all(dir.join("src/billing")).unwrap();
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    std::fs::write(
        dir.join(rules::FILE),
        r#"{"rules": [{"id": "money", "paths": ["src/billing/**"], "severity": "blocker", "text": "金额一律用整数分"}]}"#,
    )
    .unwrap();
    git(&["add", "-A"]);
    let before = git(&["write-tree"]);
    // The turn empties the rules and adds billing code.
    std::fs::write(dir.join(rules::FILE), r#"{"rules": []}"#).unwrap();
    std::fs::write(dir.join("src/billing/fee.rs"), "let fee = 0.1 * price;\n").unwrap();
    git(&["add", "-A"]);
    let after = git(&["write-tree"]);
    let mut changed = diff(&[rules::FILE, "src/billing/fee.rs"]);
    changed.top = dir.clone();
    changed.before = before;
    changed.after = after;

    let rig = rig(true, false);
    rig.settled("t1", "u1", Some(changed), false);
    let prompt = rig.wait_sent(1)[0].1.clone();
    assert!(
        prompt.contains(
            "### money（违反时按「阻断」报）\n适用于：src/billing/fee.rs\n金额一律用整数分"
        ),
        "{prompt}"
    );
    assert!(
        prompt.contains("### rust（违反时按「重要」报）"),
        "the built-in rule too"
    );
    assert!(prompt.contains("这一轮改了规则文件"));
    assert!(
        !prompt.contains("### python"),
        "only rules that match a file"
    );

    const CITED: &str = "```json\n{\"verdict\":\"changes\",\"findings\":[{\"id\":\"r1\",\"severity\":\"blocker\",\"file\":\"src/billing/fee.rs\",\"rule\":\"money\",\"text\":\"用了浮点\"},{\"id\":\"r2\",\"severity\":\"minor\",\"file\":\"src/billing/fee.rs\",\"rule\":\"made-up\",\"text\":\"命名\"}]}\n```";
    rig.wait_reviewer();
    rig.turn("rev-1", "v1", CITED);
    let card = rig.reviews.card("t1").unwrap();
    let cited: Vec<_> = card.findings.iter().map(|f| f.rule.as_deref()).collect();
    assert_eq!(cited, [Some("money"), None]);
    std::fs::remove_dir_all(&dir).ok();
}
