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
            },
        );
    }
    fn settled(&self, task: &str, turn: &str, changed: Option<TurnDiff>, checking: bool) {
        self.reviews.turn_settled(TurnSettled {
            task_id: task.into(),
            turn_id: turn.into(),
            reason: TurnEndReason::Completed,
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
const PASS: &str = "```json\n{\"verdict\":\"pass\",\"findings\":[]}\n```";

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
