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
        anonymous: false,
        pending_replies: Vec::new(),
        format: Format::Roundtable,
        max_revisions: DEFAULT_REVISIONS,
        requirements: None,
        assignments: Vec::new(),
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
        state,
        text: "x".into(),
        stance,
        ..Post::new(round, member, PostKind::Answer)
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
    // A second click while it is already summing up does nothing quietly.
    discussions.summarize_now("d1").unwrap();
    assert_eq!(host.interrupted.lock().len(), 1);
    assert!(host.take().is_empty());
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

fn setup_with(d: Discussion) -> (Arc<Discussions>, Arc<FakeHost>, PathBuf) {
    let discussions = Discussions::new(Arc::new(Quiet));
    let host = Arc::new(FakeHost::default());
    discussions.set_host(host.clone());
    let dir = dir();
    discussions.set_dir(dir.clone());
    discussions.begin(d).unwrap();
    (discussions, host, dir)
}

#[test]
fn the_user_is_heard_next_round_even_past_the_limit() {
    let (discussions, host, dir) = setup(1);
    discussions.say("d1", "别忘了显存只有 24G", &[]).unwrap();
    assert!(host.take().is_empty(), "nobody is interrupted");
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    // One round was the limit, but the user spoke: round 2 answers it.
    let sent = host.take();
    assert_eq!(sent.len(), 2);
    assert!(sent
        .iter()
        .all(|(_, t)| t.contains("<user-post round=\"1\">\n别忘了显存只有 24G")));
    assert!(sent.iter().all(|(_, t)| t.contains("请一并回应")));
    assert_eq!(discussions.get("d1").unwrap().round, 2);
    discussions.settle_now("a", TurnEndReason::Completed, "A2\n立场：已修改");
    discussions.settle_now("b", TurnEndReason::Completed, "B2\n立场：已修改");
    let sent = host.take();
    assert_eq!(sent.len(), 1, "then the summary");
    assert!(
        sent[0].1.contains("共识") && !sent[0].1.contains("<user-post"),
        "the moderator saw it already"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn speaking_after_the_summary_starts_a_round_and_a_new_summary() {
    let (discussions, host, dir) = setup(1);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    host.take();
    discussions.settle_now("a", TurnEndReason::Completed, "总结一");
    assert_eq!(discussions.get("d1").unwrap().state, DiscussionState::Done);
    assert!(discussions.say("d1", "   ", &[]).is_err());

    let d = discussions.say("d1", "如果只有一张卡呢", &[]).unwrap();
    assert_eq!(
        (d.state, d.round, d.max_rounds),
        (DiscussionState::Running, 2, 2)
    );
    let sent = host.take();
    assert_eq!(sent.len(), 2);
    assert!(sent
        .iter()
        .all(|(_, t)| t.starts_with("第 2 轮") && t.contains("如果只有一张卡呢")));
    discussions.settle_now("a", TurnEndReason::Completed, "A2\n立场：已修改");
    discussions.settle_now("b", TurnEndReason::Completed, "B2\n立场：已修改");
    assert_eq!(host.take().len(), 1, "summed up again");
    discussions.settle_now("a", TurnEndReason::Completed, "总结二");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    let summaries: Vec<&str> = d
        .posts
        .iter()
        .filter(|p| p.kind == PostKind::Summary)
        .map(|p| p.text.as_str())
        .collect();
    assert_eq!(summaries, ["总结一", "总结二"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn another_round_follows_a_pause_or_a_summary_only() {
    let (discussions, host, dir) = setup(3);
    assert!(discussions.another_round("d1").is_err(), "still running");
    discussions.pause("d1").unwrap();
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    let d = discussions.another_round("d1").unwrap();
    assert_eq!(
        (d.state, d.round, d.max_rounds),
        (DiscussionState::Running, 2, 2)
    );
    assert_eq!(host.take().len(), 2);
    discussions.settle_now("a", TurnEndReason::Completed, "A2");
    discussions.settle_now("b", TurnEndReason::Completed, "B2");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("共识"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_member_asked_by_name_answers_alone_once_it_has_finished() {
    let (discussions, host, dir) = setup(3);
    assert!(discussions.say("d1", "@Codex ?", &[5]).is_err());
    // B is still on round 1: the question waits for it.
    discussions
        .say("d1", "@Codex 为什么选 mosaic", &[1])
        .unwrap();
    assert!(host.take().is_empty());
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "b");
    assert!(sent[0].1.contains("用户点名问你") && sent[0].1.contains("为什么选 mosaic"));
    assert!(
        !sent[0].1.contains("立场：维持"),
        "a reply has no stance line"
    );
    // A finishes round 1, but round 2 waits for B's reply.
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    assert!(host.take().is_empty());
    assert_eq!(discussions.get("d1").unwrap().round, 1);
    discussions.settle_now("b", TurnEndReason::Completed, "因为小目标多");
    let d = discussions.get("d1").unwrap();
    let reply = d.posts.iter().find(|p| p.kind == PostKind::Reply).unwrap();
    assert_eq!((reply.member, reply.round, reply.stance), (1, 1, None));
    // Round 2: A hears B's answer, its reply and the user; B hears A only.
    let sent = host.take();
    let to = |task: &str| sent.iter().find(|(t, _)| t == task).unwrap().1.clone();
    assert!(to("a").contains("因为小目标多") && to("a").contains("为什么选 mosaic"));
    assert!(to("b").contains("A") && !to("b").contains("为什么选 mosaic"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn asking_by_name_after_the_summary_leaves_it_summed_up() {
    let (discussions, host, dir) = setup(1);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    discussions.settle_now("a", TurnEndReason::Completed, "总结");
    host.take();
    let d = discussions.say("d1", "@Claude 展开说说", &[0]).unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    assert_eq!(
        d.task_status(),
        TaskStatus::Running,
        "the reply is under way"
    );
    assert_eq!(host.take().len(), 1);
    assert!(discussions.another_round("d1").is_err());
    assert!(discussions.say("d1", "大家怎么看", &[]).is_err());
    discussions.settle_now("a", TurnEndReason::Completed, "展开如下");
    let d = discussions.get("d1").unwrap();
    assert_eq!(
        (d.state, d.task_status()),
        (DiscussionState::Done, TaskStatus::Done)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn speaking_is_refused_while_summing_up_or_after_a_stop() {
    let (discussions, _host, dir) = setup(1);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    assert!(discussions.say("d1", "等等", &[]).is_err());
    discussions.stop("d1").unwrap();
    assert!(discussions.say("d1", "等等", &[]).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_anonymous_discussion_never_says_who_is_who() {
    let mut d = discussion(3);
    d.anonymous = true;
    let opening = prompt::opening(&d, 0);
    assert!(opening.contains("你是「成员 A」") && opening.contains("「成员 B」"));
    assert!(!opening.contains("Claude") && !opening.contains("Codex"));
    assert!(opening.contains("匿名"));
    let (discussions, host, dir) = setup_with(d);
    discussions.settle_now("a", TurnEndReason::Completed, "A");
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    let sent = host.take();
    assert!(sent
        .iter()
        .all(|(_, t)| t.contains("from=\"成员 ") && !t.contains("Codex") && !t.contains("Claude")));
    let _ = std::fs::remove_dir_all(dir);
}

/// A draft review: Claude writes, Codex and Kimi review.
fn review(revisions: u32) -> Discussion {
    let mut d = discussion(3);
    d.members.push(member("c", "Kimi"));
    d.format = Format::Review;
    d.max_revisions = revisions;
    d
}

#[test]
fn verdicts_are_read_from_the_last_lines() {
    assert_eq!(parse_verdict("挺好\n\n结论：通过"), Verdict::Pass);
    assert_eq!(parse_verdict("……\n**结论：需修改**"), Verdict::Revise);
    assert_eq!(parse_verdict("结论: 通过\n"), Verdict::Pass);
    assert_eq!(parse_verdict("没写结论"), Verdict::Revise);
    assert_eq!(version_of(1), 1);
    assert_eq!(version_of(2), 1);
    assert_eq!(version_of(3), 2);
}

#[test]
fn a_draft_is_reviewed_alone_revised_and_final_once_everyone_passes() {
    let (discussions, host, dir) = setup_with(review(2));
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.posts.len(), 1, "only the author writes first");
    assert_eq!(
        (d.posts[0].stage, d.posts[0].version),
        (Some(Stage::Draft), Some(1))
    );
    assert!(prompt::opening(&d, 0).contains("请写出完整的第 1 版"));
    assert!(prompt::opening(&d, 1).contains("是评审「Codex」"));

    discussions.settle_now("a", TurnEndReason::Completed, "第一版正文");
    let sent = host.take();
    let to: Vec<&str> = sent.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(to, ["b", "c"]);
    assert!(sent
        .iter()
        .all(|(_, t)| t.contains("第一版正文") && t.contains("结论：通过")));

    discussions.settle_now("b", TurnEndReason::Completed, "可以\n结论：通过");
    discussions.settle_now("c", TurnEndReason::Completed, "缺少对比实验\n结论：需修改");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "a");
    assert!(sent[0].1.contains("缺少对比实验") && sent[0].1.contains("可以"));
    assert!(sent[0].1.contains("请写出完整的第 2 版") && sent[0].1.contains("修订说明"));

    discussions.settle_now(
        "a",
        TurnEndReason::Completed,
        "第二版正文\n修订说明：加了对比",
    );
    let sent = host.take();
    let to_b = &sent.iter().find(|(t, _)| t == "b").unwrap().1;
    assert!(to_b.contains("第二版正文"));
    assert!(
        !to_b.contains("缺少对比实验"),
        "a reviewer never sees another's review"
    );

    discussions.settle_now("b", TurnEndReason::Completed, "结论：通过");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    assert!(host.take().is_empty(), "final without another turn");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    let last = d.posts.last().unwrap();
    assert_eq!(
        (last.kind, last.stage, last.version, last.verdict),
        (
            PostKind::Summary,
            Some(Stage::Final),
            Some(2),
            Some(Verdict::Pass)
        )
    );
    assert!(last.text.starts_with("第二版正文"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_latest_version_is_final_when_the_revisions_run_out() {
    let (discussions, host, dir) = setup_with(review(1));
    discussions.settle_now("a", TurnEndReason::Completed, "v1");
    discussions.settle_now("b", TurnEndReason::Completed, "结论：需修改");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    discussions.settle_now("a", TurnEndReason::Completed, "v2");
    discussions.settle_now("b", TurnEndReason::Completed, "还是不行");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    let last = d.posts.last().unwrap();
    assert_eq!(
        (last.version, last.verdict),
        (Some(2), Some(Verdict::Revise))
    );
    assert_eq!(
        host.take().len(),
        5,
        "two reviews twice and one revision were asked"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_review_pauses_and_carrying_on_asks_only_that_reviewer() {
    let (discussions, host, dir) = setup_with(review(2));
    discussions.settle_now("a", TurnEndReason::Failed, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert_eq!(d.note.as_deref(), Some("作者没写出第 1 版"));
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("请写出完整的第 1 版"));

    discussions.settle_now("a", TurnEndReason::Completed, "v1");
    host.take();
    discussions.settle_now("b", TurnEndReason::Failed, "");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    let d = discussions.get("d1").unwrap();
    assert_eq!(
        d.state,
        DiscussionState::Paused,
        "an unfinished review never passes"
    );
    assert!(d.note.unwrap().contains("1 位评审没评完"));
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "b");
    discussions.settle_now("b", TurnEndReason::Completed, "结论：通过");
    assert_eq!(discussions.get("d1").unwrap().state, DiscussionState::Done);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn finishing_now_waits_for_whoever_is_writing() {
    let (discussions, host, dir) = setup_with(review(2));
    assert!(discussions.summarize_now("d1").is_err(), "no version yet");
    discussions.settle_now("a", TurnEndReason::Completed, "v1");
    host.take();
    discussions.summarize_now("d1").unwrap();
    assert!(host.interrupted.lock().is_empty(), "nobody is cut off");
    discussions.settle_now("b", TurnEndReason::Completed, "结论：通过");
    assert_eq!(
        discussions.get("d1").unwrap().state,
        DiscussionState::Summarizing
    );
    discussions.settle_now("c", TurnEndReason::Completed, "结论：需修改");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    assert_eq!(d.posts.last().unwrap().verdict, Some(Verdict::Revise));
    assert!(host.take().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_more_version_after_the_final_is_reviewed_once_then_final() {
    let (discussions, host, dir) = setup_with(review(1));
    discussions.settle_now("a", TurnEndReason::Completed, "v1");
    discussions.settle_now("b", TurnEndReason::Completed, "结论：通过");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    assert_eq!(discussions.get("d1").unwrap().state, DiscussionState::Done);
    host.take();
    discussions.say("d1", "再压缩到一页", &[]).unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "a");
    assert!(sent[0].1.contains("再压缩到一页") && sent[0].1.contains("第 2 版"));
    discussions.settle_now("a", TurnEndReason::Completed, "v2");
    let sent = host.take();
    assert!(
        sent.iter().all(|(_, t)| t.contains("再压缩到一页")),
        "reviewers hear the user too"
    );
    discussions.settle_now("b", TurnEndReason::Completed, "结论：需修改");
    discussions.settle_now("c", TurnEndReason::Completed, "结论：通过");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Done);
    let finals: Vec<Option<u32>> = d
        .posts
        .iter()
        .filter(|p| p.stage == Some(Stage::Final))
        .map(|p| p.version)
        .collect();
    assert_eq!(finals, [Some(1), Some(2)]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn nobody_is_asked_by_name_while_the_reviewers_are_at_work() {
    let (discussions, _host, dir) = setup_with(review(2));
    discussions.settle_now("a", TurnEndReason::Completed, "v1");
    assert!(discussions.say("d1", "@Codex 怎么看", &[1]).is_err());
    assert!(discussions.say("d1", "注意篇幅", &[]).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

/// A research: Claude splits, Codex and Kimi research.
fn research() -> Discussion {
    let mut d = discussion(3);
    d.members.push(member("c", "Kimi"));
    d.format = Format::Research;
    d
}

const SPLIT: &str = "先拆一下。\n分工：\n- Codex：查数据加载为什么慢\n- kimi：查 GPU 利用率";

#[test]
fn a_split_is_read_with_its_bullets_and_names() {
    let names = vec![
        (1, vec!["Codex".to_string()]),
        (2, vec!["成员 C".to_string(), "Kimi".to_string()]),
    ];
    let parts = parse_assignments(
        "分工：\n1. **Codex**：读 loader\n* 「成员 C」: 看日志\n- 别人：无关",
        &names,
    );
    assert_eq!(parts.get(&1).map(String::as_str), Some("读 loader"));
    assert_eq!(parts.get(&2).map(String::as_str), Some("看日志"));
    assert_eq!(parts.len(), 2);
    assert!(parse_assignments("- Codex：查数据加载", &names).is_empty());
}

#[test]
fn each_member_researches_its_own_part_then_the_moderator_reports() {
    let (discussions, host, dir) = setup_with(research());
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.posts.len(), 1);
    assert_eq!(d.posts[0].stage, Some(Stage::Split));
    assert!(prompt::opening(&d, 0).contains("「分工：」"));

    discussions.settle_now("a", TurnEndReason::Completed, SPLIT);
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.assignments[1], "查数据加载为什么慢");
    assert_eq!(d.round, 2);
    let sent = host.take();
    let to = |task: &str| sent.iter().find(|(t, _)| t == task).unwrap().1.clone();
    assert!(to("b").contains("查数据加载为什么慢") && !to("b").contains("GPU"));
    assert!(to("c").contains("查 GPU 利用率") && !to("c").contains("数据加载"));

    discussions.settle_now("b", TurnEndReason::Completed, "B：解码在主线程");
    discussions.settle_now("c", TurnEndReason::Completed, "C：利用率 40%");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "a");
    assert!(
        sent[0].1.contains("调研报告")
            && sent[0].1.contains("解码在主线程")
            && sent[0].1.contains("利用率 40%")
    );
    discussions.settle_now("a", TurnEndReason::Completed, "报告");
    assert_eq!(discussions.get("d1").unwrap().state, DiscussionState::Done);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_split_that_leaves_someone_out_is_asked_again() {
    let (discussions, host, dir) = setup_with(research());
    discussions.settle_now("a", TurnEndReason::Completed, "分工：\n- Codex：查数据加载");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert!(d.note.unwrap().contains("漏了「Kimi」"));
    assert!(host.take().is_empty());
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("「分工：」"));
    discussions.settle_now("a", TurnEndReason::Completed, SPLIT);
    assert_eq!(host.take().len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_member_who_did_not_finish_pauses_and_is_asked_again_alone() {
    let (discussions, host, dir) = setup_with(research());
    discussions.settle_now("a", TurnEndReason::Completed, SPLIT);
    host.take();
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    discussions.settle_now("c", TurnEndReason::Failed, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert_eq!(d.note.as_deref(), Some("还有 1 位成员没查完"));
    discussions.resume("d1").unwrap();
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "c");
    assert!(sent[0].1.contains("查 GPU 利用率"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reporting_now_cuts_off_whoever_is_still_researching() {
    let (discussions, host, dir) = setup_with(research());
    discussions.settle_now("a", TurnEndReason::Completed, SPLIT);
    host.take();
    assert!(
        discussions.summarize_now("d1").is_err(),
        "nothing researched yet"
    );
    discussions.settle_now("b", TurnEndReason::Completed, "B 的结果");
    discussions.summarize_now("d1").unwrap();
    assert_eq!(*host.interrupted.lock(), vec!["c".to_string()]);
    discussions.settle_now("c", TurnEndReason::Interrupted, "");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("调研报告") && sent[0].1.contains("B 的结果"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn during_research_nobody_is_asked_by_name_and_the_report_hears_the_user() {
    let (discussions, host, dir) = setup_with(research());
    discussions.settle_now("a", TurnEndReason::Completed, SPLIT);
    host.take();
    assert!(discussions.say("d1", "@Codex 进度？", &[1]).is_err());
    discussions
        .say("d1", "重点看 dataloader 的 num_workers", &[])
        .unwrap();
    assert!(
        host.take().is_empty(),
        "members already at work are not interrupted"
    );
    assert!(discussions.another_round("d1").is_err());
    discussions.settle_now("b", TurnEndReason::Completed, "B");
    discussions.settle_now("c", TurnEndReason::Completed, "C");
    let sent = host.take();
    assert!(sent[0].1.contains("num_workers"));
    discussions.settle_now("a", TurnEndReason::Completed, "报告");
    assert!(
        discussions.say("d1", "再查查", &[]).is_err(),
        "after the report only by name"
    );
    discussions.say("d1", "@Kimi 40% 怎么测的", &[2]).unwrap();
    assert_eq!(host.take().len(), 1);
    assert!(discussions.another_round("d1").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_requirements_go_with_every_prompt_unless_cleared() {
    let mut d = discussion(3);
    assert!(prompt::opening(&d, 0).contains(prompt::DEFAULT_REQUIREMENTS));
    d.requirements = Some("先给结论，每次 300 字以内".into());
    let opening = prompt::opening(&d, 0);
    assert!(opening.contains("讨论要求（用户定的）：先给结论，每次 300 字以内"));
    assert!(!opening.contains("600 字"));
    assert!(prompt::summary(&d, &[]).contains("300 字"));
    d.requirements = Some("  ".into());
    assert!(!prompt::opening(&d, 0).contains("讨论要求"));
    assert!(
        prompt::opening(&d, 0).contains("不要修改任何文件"),
        "read-only stays"
    );
    // A draft review's author writes without the length limit.
    let mut r = review(2);
    r.requirements = Some("每次 300 字以内".into());
    assert!(!prompt::opening(&r, 0).contains("300 字"));
}

/// A relay of three: Claude, Codex, Kimi, two laps.
fn relay(laps: u32) -> Discussion {
    let mut d = discussion(laps);
    d.members.push(member("c", "Kimi"));
    d.format = Format::Relay;
    d
}

#[test]
fn a_relay_runs_one_at_a_time_and_each_hears_only_the_one_before() {
    let d = relay(2);
    assert_eq!(d.speakers(1), vec![0]);
    assert_eq!(d.speakers(5), vec![1]);
    assert_eq!((d.lap(3), d.lap(4)), (1, 2));
    assert!(prompt::opening(&d, 0).contains("你是第一棒"));
    assert!(
        !prompt::opening(&d, 1).contains("<topic>"),
        "waits for its turn"
    );

    let (discussions, host, dir) = setup_with(relay(2));
    discussions.settle_now("a", TurnEndReason::Completed, "A 第一棒");
    let sent = host.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "b");
    assert!(sent[0].1.contains("A 第一棒") && sent[0].1.contains("<topic>"));
    discussions.settle_now("b", TurnEndReason::Completed, "B 第一棒");
    let sent = host.take();
    assert_eq!(sent[0].0, "c");
    assert!(sent[0].1.contains("B 第一棒") && !sent[0].1.contains("A 第一棒"));
    discussions.settle_now("c", TurnEndReason::Completed, "C 第一棒");
    // Lap 2: A hears only C, and is not told the topic again.
    let sent = host.take();
    assert_eq!(sent[0].0, "a");
    assert!(sent[0].1.contains("第 2 圈") && sent[0].1.contains("C 第一棒"));
    assert!(!sent[0].1.contains("B 第一棒") && !sent[0].1.contains("<topic>"));
    assert!(!sent[0].1.contains("立场"));
    discussions.settle_now("a", TurnEndReason::Completed, "A 第二棒\n立场：维持");
    assert_eq!(discussions.get("d1").unwrap().posts[3].stance, None);
    discussions.settle_now("b", TurnEndReason::Completed, "B 第二棒");
    host.take();
    discussions.settle_now("c", TurnEndReason::Completed, "C 第二棒");
    // Six turns: the moderator sums up with everything.
    let sent = host.take();
    assert_eq!(sent[0].0, "a");
    assert!(sent[0].1.contains("总结") && sent[0].1.contains("B 第一棒"));
    assert_eq!(
        discussions.get("d1").unwrap().state,
        DiscussionState::Summarizing
    );
    discussions.settle_now("a", TurnEndReason::Completed, "总结");
    assert_eq!(discussions.get("d1").unwrap().state, DiscussionState::Done);
    // Another round is one more lap, from the next runner.
    discussions.another_round("d1").unwrap();
    let d = discussions.get("d1").unwrap();
    assert_eq!((d.round, d.max_rounds), (7, 3));
    assert_eq!(host.take()[0].0, "a");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_relay_turn_pauses_and_nobody_is_asked_by_name_on_the_way() {
    let (discussions, host, dir) = setup_with(relay(1));
    assert!(discussions.say("d1", "@Codex 怎么看", &[1]).is_err());
    assert!(discussions.say("d1", "注意成本", &[]).is_ok());
    discussions.settle_now("a", TurnEndReason::Completed, "A 第一棒");
    // The user's words reach the next runner with the turn before.
    let sent = host.take();
    assert!(sent[0].1.contains("注意成本") && sent[0].1.contains("A 第一棒"));
    discussions.settle_now("b", TurnEndReason::Failed, "");
    let d = discussions.get("d1").unwrap();
    assert_eq!(d.state, DiscussionState::Paused);
    assert_eq!(d.note.as_deref(), Some("第 1 圈「Codex」没发言成功"));
    let _ = std::fs::remove_dir_all(dir);
}
