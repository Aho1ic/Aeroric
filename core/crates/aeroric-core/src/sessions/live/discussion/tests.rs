use super::*;

struct Quiet;
impl LiveStatusSink for Quiet {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

#[derive(Default)]
struct FakeHost {
    /// (task, text) of every message sent.
    sent: Mutex<Vec<(String, String)>>,
    interrupted: Mutex<Vec<String>>,
    fail_prompt: Mutex<bool>,
}

impl DiscussionHost for FakeHost {
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        if *self.fail_prompt.lock() {
            return Err("会话没打开".into());
        }
        self.sent.lock().push((task_id.to_string(), text));
        Ok(())
    }
    fn interrupt(&self, task_id: &str) {
        self.interrupted.lock().push(task_id.to_string());
    }
    fn respond(&self, _: &str, _: &str, _: Decision) {}
    fn changed(&self, _: &Discussion) {}
}

impl FakeHost {
    fn take(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.sent.lock())
    }
}

fn member(task: &str, label: &str) -> Member {
    Member {
        task_id: task.into(),
        label: label.into(),
        configuration_id: format!("cfg-{task}"),
        agent: "codex".into(),
        model: None,
        angle: None,
        seen: Vec::new(),
    }
}

fn discussion(rounds: u32) -> Discussion {
    Discussion {
        id: "d1".into(),
        project_id: "p".into(),
        topic: "数据增强放在哪".into(),
        members: vec![member("a", "Claude"), member("b", "Codex")],
        moderator: 0,
        max_rounds: rounds,
        round: 0,
        state: DiscussionState::Running,
        posts: Vec::new(),
        note: None,
        created_ms: 1,
    }
}

fn dir() -> PathBuf {
    std::env::temp_dir().join(format!("aeroric-discussion-{}", uuid::Uuid::new_v4()))
}

fn setup(rounds: u32) -> (Arc<Discussions>, Arc<FakeHost>, PathBuf) {
    let discussions = Discussions::new(Arc::new(Quiet));
    let host = Arc::new(FakeHost::default());
    discussions.set_host(host.clone());
    let dir = dir();
    discussions.set_dir(dir.clone());
    discussions.begin(discussion(rounds)).unwrap();
    (discussions, host, dir)
}

#[test]
fn stances_are_read_from_the_last_lines() {
    assert_eq!(parse_stance("我同意。\n\n立场：维持"), Stance::Keep);
    assert_eq!(parse_stance("……\n**立场：已修改**\n"), Stance::Changed);
    assert_eq!(parse_stance("……\n立场: 维持"), Stance::Keep);
    assert_eq!(parse_stance("没有写立场"), Stance::Changed);
}

#[test]
fn a_round_is_followed_by_the_next_the_summary_or_a_pause() {
    let mut d = discussion(3);
    d.round = 1;
    let post = |round, member, state, stance| Post {
        round,
        member,
        kind: PostKind::Answer,
        state,
        text: "x".into(),
        stance,
        error: None,
        waiting: None,
        started_ms: 0,
        ended_ms: None,
    };
    d.posts = vec![
        post(1, 0, PostState::Failed, None),
        post(1, 1, PostState::Failed, None),
    ];
    assert!(matches!(after_round(&d), Next::Pause(_)));
    d.posts[1].state = PostState::Done;
    assert_eq!(after_round(&d), Next::Round(2));
    // Round 2: everyone keeps their stance.
    d.round = 2;
    d.posts
        .push(post(2, 0, PostState::Done, Some(Stance::Keep)));
    d.posts
        .push(post(2, 1, PostState::Done, Some(Stance::Changed)));
    assert_eq!(after_round(&d), Next::Round(3));
    d.posts[3].stance = Some(Stance::Keep);
    assert_eq!(after_round(&d), Next::Summarize);
    d.round = 3;
    d.posts
        .push(post(3, 0, PostState::Done, Some(Stance::Changed)));
    assert_eq!(after_round(&d), Next::Summarize);
}

#[test]
fn members_hear_only_what_the_others_said_since() {
    let (discussions, host, dir) = setup(3);
    discussions.settle_now("a", TurnEndReason::Completed, "A 的第一轮看法");
    assert!(host.take().is_empty(), "round 2 waits for every member");
    discussions.settle_now("b", TurnEndReason::Completed, "B 的第一轮看法");
    let sent = host.take();
    assert_eq!(sent.len(), 2);
    let to = |task: &str| sent.iter().find(|(t, _)| t == task).unwrap().1.clone();
    assert!(to("a").contains("B 的第一轮看法") && !to("a").contains("A 的第一轮看法"));
    assert!(to("b").contains("A 的第一轮看法") && !to("b").contains("B 的第一轮看法"));
    assert!(to("a").contains("<discussion-post from=\"Codex\" round=\"1\">"));
    assert!(to("a").contains("立场：维持"));

    // Round 2: both keep their stance, so the moderator sums up early.
    discussions.settle_now("a", TurnEndReason::Completed, "还是这样\n立场：维持");
    discussions.settle_now("b", TurnEndReason::Completed, "同意\n立场：维持");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "a");
    assert!(sent[0].1.contains("同意") && !sent[0].1.contains("还是这样"));
    assert!(sent[0].1.contains("共识"));
    discussions.settle_now("a", TurnEndReason::Completed, "共识：……");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    assert_eq!(d.task_status(), TaskStatus::Done);
    assert_eq!(d.posts.last().unwrap().kind, PostKind::Summary);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_last_round_ends_in_a_summary() {
    let (discussions, host, dir) = setup(1);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("<discussion-post from=\"Codex\""));
    assert_eq!(
        discussions.get("d1").unwrap().state,
        DiscussionState::Summarizing
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_round_goes_on_without_a_failed_member_and_pauses_when_nobody_spoke() {
    let (discussions, host, dir) = setup(3);
    discussions.settle_now("a", TurnEndReason::Failed, "");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    assert_eq!(host.take().len(), 2, "round 2 goes on without A's answer");
    discussions.settle_now("a", TurnEndReason::Failed, "");
    discussions.settle_now("b", TurnEndReason::Interrupted, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert!(d.note.unwrap().contains("第 2 轮"));
    assert!(host.take().is_empty());

    // Carrying on asks both again for round 2.
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|(_, t)| t.starts_with("第 2 轮")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pausing_waits_for_the_round_then_resuming_starts_the_next() {
    let (discussions, host, dir) = setup(3);
    let d = discussions.pause("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Pausing);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    assert_eq!(
        discussions.get("d1").unwrap().state,
        DiscussionState::Paused
    );
    assert!(host.take().is_empty());
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 2);
    assert_eq!(discussions.get("d1").unwrap().round, 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn summing_up_now_cuts_off_whoever_is_still_talking() {
    let (discussions, host, dir) = setup(3);
    assert!(discussions.summarize_now("d1").is_err(), "nothing said yet");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    discussions.summarize_now("d1").unwrap();
    assert_eq!(*host.interrupted.lock(), vec!["a".to_string()]);
    assert!(host.take().is_empty(), "the summary waits for A to stop");
    discussions.settle_now("a", TurnEndReason::Interrupted, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.posts[0].error.as_deref(), Some("被立即总结打断"));
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("B"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stopping_ends_it_without_another_word() {
    let (discussions, host, dir) = setup(3);
    discussions.stop("d1").unwrap();
    assert_eq!(host.interrupted.lock().len(), 2);
    discussions.settle_now("a", TurnEndReason::Interrupted, "");
    discussions.settle_now("b", TurnEndReason::Interrupted, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Stopped);
    assert_eq!(d.posts[1].error.as_deref(), Some("讨论已停止"));
    assert!(host.take().is_empty());
    assert!(discussions.resume("d1").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_message_that_cannot_be_sent_fails_the_post() {
    let (discussions, host, dir) = setup(3);
    *host.fail_prompt.lock() = true;
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert!(d.posts[2].error.as_deref().unwrap().contains("没发出去"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_restart_marks_the_turns_in_flight_failed_and_pauses() {
    let (discussions, _host, dir) = setup(3);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    drop(discussions);
    let again = Discussions::new(Arc::new(Quiet));
    let host = Arc::new(FakeHost::default());
    again.set_host(host.clone());
    again.set_dir(dir.clone());
    let d = again.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert_eq!(d.posts[1].state, PostState::Failed);
    assert_eq!(d.posts[0].state, PostState::Done);
    assert_eq!(again.member_of("b"), Some(("d1".into(), 1)));
    // Carrying on asks only B again, for round 1.
    again.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "b");
    assert!(sent[0].1.contains("第 1 轮") && !sent[0].1.contains("<discussion-post"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_discussion_is_checked() {
    let discussions = Discussions::new(Arc::new(Quiet));
    let mut d = discussion(3);
    d.members.truncate(1);
    assert!(discussions.begin(d).is_err());
    let mut d = discussion(6);
    d.id = "d2".into();
    assert!(discussions.begin(d).is_err());
    let mut d = discussion(3);
    d.moderator = 2;
    assert!(discussions.begin(d).is_err());
}

#[test]
fn a_member_that_never_started_fails_its_post_with_the_reason() {
    let (discussions, host, dir) = setup(3);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.status_now("b", TaskStatus::Failed, Some("Claude 登录已失效"));
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.posts[1].state, PostState::Failed);
    assert_eq!(d.posts[1].error.as_deref(), Some("Claude 登录已失效"));
    // The round goes on with A's answer alone.
    assert_eq!(d.round, 2);
    assert_eq!(host.take().len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}
