//! 2026-11-12 discussion (round table): 2–5 member sessions answer one topic
//! read-only. Round 1 each answers alone; every later round each sees what
//! the others said since and ends on a stance line; the moderator sums up
//! after the last round, or as soon as everyone keeps their stance from
//! round 2 on (frontier-roadmap Q-R7; antumi's group chat).
//!
//! [`Discussions`] sits on the session update chain: it collects each
//! member's last message of a turn and, when the turn ends, records it as
//! the member's post and decides the next step. Posts are written to disk as
//! they land; a turn cut off by a restart is marked failed and the
//! discussion pauses until the user carries on. Starting members, sending
//! them messages and the discussion's own task record are the host's
//! ([`DiscussionHost`]).

pub mod prompt;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::update::{SessionUpdate, SessionUpdateKind, TurnEndReason};
use super::{now_ms, Decision, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

pub const MIN_MEMBERS: usize = 2;
pub const MAX_MEMBERS: usize = 5;
pub const MAX_ROUNDS: u32 = 5;
pub const DEFAULT_ROUNDS: u32 = 3;

const RESTARTED: &str = "App 重启时这一轮被中断";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub task_id: String,
    pub label: String,
    pub configuration_id: String,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The member's angle ("反对方", "关注性能").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// Indexes of the posts already sent to this member.
    #[serde(default)]
    pub seen: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stance {
    Keep,
    Changed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostKind {
    Answer,
    Summary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostState {
    Speaking,
    /// The member waits on the user (an approval, a question).
    Waiting,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub round: u32,
    pub member: usize,
    pub kind: PostKind,
    pub state: PostState,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stance: Option<Stance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the member waits on, while `Waiting`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
}

impl Post {
    fn in_flight(&self) -> bool {
        matches!(self.state, PostState::Speaking | PostState::Waiting)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscussionState {
    Running,
    /// Stops once this round is said.
    Pausing,
    Paused,
    /// The moderator is (about to be) summing up.
    Summarizing,
    Done,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discussion {
    /// The discussion's own task record.
    pub id: String,
    pub project_id: String,
    pub topic: String,
    pub members: Vec<Member>,
    pub moderator: usize,
    pub max_rounds: u32,
    pub round: u32,
    pub state: DiscussionState,
    #[serde(default)]
    pub posts: Vec<Post>,
    /// Why it paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_ms: u64,
}

impl Discussion {
    /// What the discussion's task record shows in the sidebar.
    pub fn task_status(&self) -> TaskStatus {
        match self.state {
            DiscussionState::Running | DiscussionState::Pausing | DiscussionState::Summarizing => {
                if self.posts.iter().any(|p| p.state == PostState::Waiting) {
                    TaskStatus::InputRequired
                } else {
                    TaskStatus::Running
                }
            }
            DiscussionState::Paused => TaskStatus::Interrupted,
            DiscussionState::Done => TaskStatus::Done,
            DiscussionState::Stopped => TaskStatus::Cancelled,
        }
    }

    fn in_flight(&self) -> bool {
        self.posts.iter().any(Post::in_flight)
    }

    /// The posts `member` has not been sent yet, by the others, from before
    /// `round` (`None`: every round, for the summary).
    pub fn unseen(&self, member: usize, round: Option<u32>) -> Vec<usize> {
        self.posts
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                p.member != member
                    && p.kind == PostKind::Answer
                    && p.state == PostState::Done
                    && round.is_none_or(|r| p.round < r)
                    && !self.members[member].seen.contains(i)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// What comes after a round has been said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Round(u32),
    Summarize,
    Pause(String),
}

/// The step after round `discussion.round`, every answer of it in.
pub fn after_round(discussion: &Discussion) -> Next {
    let round = discussion.round;
    let answers: Vec<&Post> = discussion
        .posts
        .iter()
        .filter(|p| p.round == round && p.kind == PostKind::Answer)
        .collect();
    let said: Vec<&&Post> = answers
        .iter()
        .filter(|p| p.state == PostState::Done)
        .collect();
    if said.is_empty() {
        return Next::Pause(format!("第 {round} 轮没有人发言成功"));
    }
    if round >= discussion.max_rounds {
        return Next::Summarize;
    }
    if round >= 2 && said.iter().all(|p| p.stance == Some(Stance::Keep)) {
        return Next::Summarize;
    }
    Next::Round(round + 1)
}

/// The stance a round-2+ answer ends on: its last lines, "立场：维持" or
/// "立场：已修改". Unreadable counts as changed, so the talk goes on.
pub fn parse_stance(text: &str) -> Stance {
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()).take(3) {
        let line = line
            .trim()
            .trim_matches(|c| matches!(c, '*' | '_' | '`' | '>'));
        let Some(rest) = line.find("立场").map(|at| &line[at + "立场".len()..]) else {
            continue;
        };
        let rest = rest.trim_start_matches([':', '：', ' ', '*']);
        if rest.starts_with("维持") {
            return Stance::Keep;
        }
        if rest.starts_with("已修改") || rest.starts_with("修改") {
            return Stance::Changed;
        }
    }
    Stance::Changed
}

/// What a post's turn cost, from its member's transcript.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostUsage {
    pub tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// The post's turn among its member's turns: the first to start once the
/// message was queued (a little slack for clocks), before the post ended.
pub fn usage(
    post: &Post,
    turns: &[crate::sessions::transcript_stats::TurnStat],
) -> Option<PostUsage> {
    const SLACK_MS: u64 = 5_000;
    let turn = turns.iter().find(|t| {
        t.started_at + SLACK_MS >= post.started_ms
            && post.ended_ms.is_none_or(|end| t.started_at <= end)
    })?;
    Some(PostUsage {
        tokens: turn.tokens,
        cost_usd: turn.cost_usd,
        duration_ms: turn.ended_at.map(|end| end.saturating_sub(turn.started_at)),
    })
}

/// Claude asks to leave plan mode with its plan; a discussion wants prose.
fn is_plan_exit(kind: &str) -> bool {
    kind == "ExitPlanMode"
}

#[derive(Debug, PartialEq, Eq)]
pub enum DiscussionError {
    NotFound,
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for DiscussionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscussionError::NotFound => write!(f, "讨论不存在"),
            DiscussionError::Invalid(m) | DiscussionError::Io(m) => write!(f, "{m}"),
        }
    }
}

/// What a discussion needs from the host. Called off the sink's thread.
pub trait DiscussionHost: Send + Sync {
    /// Send `text` to a member as a user message.
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String>;
    fn interrupt(&self, task_id: &str);
    fn respond(&self, task_id: &str, request_id: &str, decision: Decision);
    /// The discussion changed: its task record follows its state.
    fn changed(&self, discussion: &Discussion);
}

#[derive(Default)]
struct State {
    by_id: HashMap<String, Discussion>,
    /// Member task → (discussion, member index).
    member_of: HashMap<String, (String, usize)>,
    /// The messages of a member's current turn, in order.
    text: HashMap<String, Vec<(String, String)>>,
}

/// Messages to send once the lock is let go.
type Outbox = Vec<(String, String)>;

pub struct Discussions {
    inner: Arc<dyn LiveStatusSink>,
    host: Mutex<Option<Arc<dyn DiscussionHost>>>,
    dir: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    me: Mutex<Weak<Self>>,
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn save(dir: &Path, discussion: &Discussion) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(discussion).map_err(|e| e.to_string())?;
    let path = file(dir, &discussion.id);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Every saved discussion; one cut off by a restart is paused, its turns
/// in flight marked failed.
pub fn load_all(dir: &Path) -> Vec<Discussion> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(mut discussion) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Discussion>(&b).ok())
        else {
            continue;
        };
        let mut cut = false;
        for post in discussion.posts.iter_mut().filter(|p| p.in_flight()) {
            post.state = PostState::Failed;
            post.error = Some(RESTARTED.into());
            post.waiting = None;
            cut = true;
        }
        if matches!(
            discussion.state,
            DiscussionState::Running | DiscussionState::Pausing | DiscussionState::Summarizing
        ) {
            discussion.state = DiscussionState::Paused;
            discussion.note = Some(if cut {
                format!("{RESTARTED}，点继续接着讨论")
            } else {
                "App 重启，讨论已暂停，点继续接着讨论".into()
            });
        }
        out.push(discussion);
    }
    out.sort_by_key(|d| d.created_ms);
    out
}

impl Discussions {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        let this = Arc::new(Self {
            inner,
            host: Mutex::default(),
            dir: Mutex::default(),
            state: Mutex::default(),
            me: Mutex::default(),
        });
        *this.me.lock() = Arc::downgrade(&this);
        this
    }

    pub fn set_host(&self, host: Arc<dyn DiscussionHost>) {
        *self.host.lock() = Some(host);
    }

    /// Where discussions are kept; loads the saved ones.
    pub fn set_dir(&self, dir: PathBuf) {
        let loaded = load_all(&dir);
        let mut state = self.state.lock();
        for discussion in loaded {
            for (i, member) in discussion.members.iter().enumerate() {
                state
                    .member_of
                    .insert(member.task_id.clone(), (discussion.id.clone(), i));
            }
            let _ = save(&dir, &discussion);
            state.by_id.insert(discussion.id.clone(), discussion);
        }
        drop(state);
        *self.dir.lock() = Some(dir);
    }

    pub fn get(&self, id: &str) -> Option<Discussion> {
        self.state.lock().by_id.get(id).cloned()
    }

    /// The discussion `task_id` speaks in, and its place there.
    pub fn member_of(&self, task_id: &str) -> Option<(String, usize)> {
        self.state.lock().member_of.get(task_id).cloned()
    }

    pub fn remove(&self, id: &str) {
        let mut state = self.state.lock();
        if let Some(discussion) = state.by_id.remove(id) {
            for member in &discussion.members {
                state.member_of.remove(&member.task_id);
            }
        }
        drop(state);
        if let Some(dir) = self.dir.lock().clone() {
            let _ = std::fs::remove_file(file(&dir, id));
        }
    }

    /// Takes a new discussion whose members are about to be started with
    /// [`prompt::opening`]: round 1 is under way.
    pub fn begin(&self, mut discussion: Discussion) -> Result<Discussion, DiscussionError> {
        let n = discussion.members.len();
        if !(MIN_MEMBERS..=MAX_MEMBERS).contains(&n) {
            return Err(DiscussionError::Invalid(format!(
                "讨论要 {MIN_MEMBERS} 到 {MAX_MEMBERS} 个成员"
            )));
        }
        if !(1..=MAX_ROUNDS).contains(&discussion.max_rounds) {
            return Err(DiscussionError::Invalid(format!(
                "轮数要在 1 到 {MAX_ROUNDS} 之间"
            )));
        }
        if discussion.moderator >= n {
            return Err(DiscussionError::Invalid("主持人不是成员".into()));
        }
        if discussion.topic.trim().is_empty() {
            return Err(DiscussionError::Invalid("题目不能为空".into()));
        }
        let now = now_ms();
        discussion.round = 1;
        discussion.state = DiscussionState::Running;
        discussion.posts = (0..n)
            .map(|member| Post {
                round: 1,
                member,
                kind: PostKind::Answer,
                state: PostState::Speaking,
                text: String::new(),
                stance: None,
                error: None,
                waiting: None,
                started_ms: now,
                ended_ms: None,
            })
            .collect();
        let mut state = self.state.lock();
        for (i, member) in discussion.members.iter().enumerate() {
            state
                .member_of
                .insert(member.task_id.clone(), (discussion.id.clone(), i));
        }
        state
            .by_id
            .insert(discussion.id.clone(), discussion.clone());
        drop(state);
        self.persist(&discussion);
        // The discussion's record shows it running from the start.
        if let Some(host) = self.host() {
            host.changed(&discussion);
        }
        Ok(discussion)
    }

    /// A member could not be started (or sent its message).
    pub fn member_failed(&self, id: &str, member: usize, reason: &str) {
        let outbox = {
            let mut state = self.state.lock();
            let Some(discussion) = state.by_id.get_mut(id) else {
                return;
            };
            let Some(post) = discussion
                .posts
                .iter_mut()
                .rev()
                .find(|p| p.member == member && p.in_flight())
            else {
                return;
            };
            post.state = PostState::Failed;
            post.error = Some(reason.to_string());
            post.waiting = None;
            post.ended_ms = Some(now_ms());
            Self::advance(discussion)
        };
        self.after_change(id, outbox);
    }

    /// Stop once the round under way is said.
    pub fn pause(&self, id: &str) -> Result<Discussion, DiscussionError> {
        self.act(id, |d| match d.state {
            DiscussionState::Running if d.in_flight() => {
                d.state = DiscussionState::Pausing;
                Ok(Vec::new())
            }
            DiscussionState::Running => {
                d.state = DiscussionState::Paused;
                Ok(Vec::new())
            }
            _ => Err(DiscussionError::Invalid("讨论不在进行中".into())),
        })
    }

    /// Carry on: retry the members the round lost, or take the next step.
    pub fn resume(&self, id: &str) -> Result<Discussion, DiscussionError> {
        self.act(id, |d| match d.state {
            DiscussionState::Pausing => {
                d.state = DiscussionState::Running;
                Ok(Vec::new())
            }
            DiscussionState::Paused => {
                d.note = None;
                let summary_failed = d
                    .posts
                    .last()
                    .is_some_and(|p| p.kind == PostKind::Summary && p.state == PostState::Failed);
                if summary_failed {
                    return Ok(Self::start_summary(d));
                }
                let round = d.round;
                let lost: Vec<usize> = (0..d.members.len())
                    .filter(|m| {
                        !d.posts.iter().any(|p| {
                            p.round == round
                                && p.member == *m
                                && p.kind == PostKind::Answer
                                && p.state == PostState::Done
                        })
                    })
                    .collect();
                d.state = DiscussionState::Running;
                if lost.is_empty() {
                    return Ok(Self::next(d));
                }
                Ok(lost.into_iter().map(|m| Self::ask(d, m, round)).collect())
            }
            _ => Err(DiscussionError::Invalid("讨论没有暂停".into())),
        })
    }

    /// Sum up now: members still talking are cut off first.
    pub fn summarize_now(&self, id: &str) -> Result<Discussion, DiscussionError> {
        let mut cut = Vec::new();
        let result = self.act(id, |d| {
            if !matches!(
                d.state,
                DiscussionState::Running | DiscussionState::Pausing | DiscussionState::Paused
            ) {
                return Err(DiscussionError::Invalid("讨论已经结束".into()));
            }
            if !d.posts.iter().any(|p| p.state == PostState::Done) {
                return Err(DiscussionError::Invalid(
                    "还没有人发言，没有可总结的".into(),
                ));
            }
            d.note = None;
            d.state = DiscussionState::Summarizing;
            cut = d
                .posts
                .iter()
                .filter(|p| p.in_flight())
                .map(|p| d.members[p.member].task_id.clone())
                .collect();
            if cut.is_empty() {
                Ok(Self::start_summary(d))
            } else {
                Ok(Vec::new())
            }
        });
        if let Some(host) = self.host() {
            for task in cut {
                host.interrupt(&task);
            }
        }
        result
    }

    /// End it; members still talking are cut off.
    pub fn stop(&self, id: &str) -> Result<Discussion, DiscussionError> {
        let mut cut = Vec::new();
        let result = self.act(id, |d| {
            if matches!(d.state, DiscussionState::Done | DiscussionState::Stopped) {
                return Err(DiscussionError::Invalid("讨论已经结束".into()));
            }
            d.state = DiscussionState::Stopped;
            d.note = None;
            cut = d
                .posts
                .iter()
                .filter(|p| p.in_flight())
                .map(|p| d.members[p.member].task_id.clone())
                .collect();
            Ok(Vec::new())
        });
        if let Some(host) = self.host() {
            for task in cut {
                host.interrupt(&task);
            }
        }
        result
    }

    fn host(&self) -> Option<Arc<dyn DiscussionHost>> {
        self.host.lock().clone()
    }

    fn persist(&self, discussion: &Discussion) {
        if let Some(dir) = self.dir.lock().clone() {
            let _ = save(&dir, discussion);
        }
    }

    fn act(
        &self,
        id: &str,
        f: impl FnOnce(&mut Discussion) -> Result<Outbox, DiscussionError>,
    ) -> Result<Discussion, DiscussionError> {
        let (discussion, outbox) = {
            let mut state = self.state.lock();
            let discussion = state.by_id.get_mut(id).ok_or(DiscussionError::NotFound)?;
            let outbox = f(discussion)?;
            (discussion.clone(), outbox)
        };
        self.after_change(id, outbox);
        Ok(self.get(id).unwrap_or(discussion))
    }

    /// Save, tell the host, and send what is due — off the lock.
    fn after_change(&self, id: &str, outbox: Outbox) {
        let Some(discussion) = self.get(id) else {
            return;
        };
        self.persist(&discussion);
        let Some(host) = self.host() else {
            return;
        };
        host.changed(&discussion);
        for (task, text) in outbox {
            if let Err(error) = host.prompt(&task, text) {
                if let Some((_, member)) = self.member_of(&task) {
                    self.member_failed(id, member, &format!("消息没发出去：{error}"));
                }
            }
        }
    }

    /// A new post for `member` in `round`, and its message.
    fn ask(d: &mut Discussion, member: usize, round: u32) -> (String, String) {
        let unseen = if round == 1 {
            Vec::new()
        } else {
            d.unseen(member, Some(round))
        };
        let text = if round == 1 {
            prompt::opening(d, member)
        } else {
            prompt::round(d, &unseen)
        };
        d.members[member].seen.extend(unseen);
        d.posts.push(Post {
            round,
            member,
            kind: PostKind::Answer,
            state: PostState::Speaking,
            text: String::new(),
            stance: None,
            error: None,
            waiting: None,
            started_ms: now_ms(),
            ended_ms: None,
        });
        (d.members[member].task_id.clone(), text)
    }

    fn start_summary(d: &mut Discussion) -> Outbox {
        d.state = DiscussionState::Summarizing;
        let m = d.moderator;
        let unseen = d.unseen(m, None);
        let text = prompt::summary(d, &unseen);
        d.members[m].seen.extend(unseen);
        d.posts.push(Post {
            round: d.round,
            member: m,
            kind: PostKind::Summary,
            state: PostState::Speaking,
            text: String::new(),
            stance: None,
            error: None,
            waiting: None,
            started_ms: now_ms(),
            ended_ms: None,
        });
        vec![(d.members[m].task_id.clone(), text)]
    }

    /// The round is said: the next round, the summary, or a pause.
    fn next(d: &mut Discussion) -> Outbox {
        match after_round(d) {
            Next::Round(round) => {
                d.round = round;
                (0..d.members.len())
                    .map(|m| Self::ask(d, m, round))
                    .collect()
            }
            Next::Summarize => Self::start_summary(d),
            Next::Pause(reason) => {
                d.state = DiscussionState::Paused;
                d.note = Some(reason);
                Vec::new()
            }
        }
    }

    /// After a post settled: what the discussion does now.
    fn advance(d: &mut Discussion) -> Outbox {
        if d.in_flight() {
            return Vec::new();
        }
        let last_summary = d
            .posts
            .last()
            .filter(|p| p.kind == PostKind::Summary)
            .map(|p| p.state);
        match d.state {
            DiscussionState::Summarizing => match last_summary {
                Some(PostState::Done) => {
                    d.state = DiscussionState::Done;
                    Vec::new()
                }
                Some(_) => {
                    d.state = DiscussionState::Paused;
                    d.note = Some("主持人没能写出总结".into());
                    Vec::new()
                }
                None => Self::start_summary(d),
            },
            DiscussionState::Pausing => {
                d.state = DiscussionState::Paused;
                Vec::new()
            }
            DiscussionState::Running => Self::next(d),
            _ => Vec::new(),
        }
    }

    /// A member's turn ended: its post is said (or failed).
    fn settle(&self, task: &str, reason: TurnEndReason, error: Option<String>) {
        let text = self
            .state
            .lock()
            .text
            .remove(task)
            .and_then(|m| {
                m.into_iter()
                    .rev()
                    .map(|(_, t)| t)
                    .find(|t| !t.trim().is_empty())
            })
            .unwrap_or_default();
        let Some((id, member)) = self.member_of(task) else {
            return;
        };
        let outbox = {
            let mut state = self.state.lock();
            let Some(d) = state.by_id.get_mut(&id) else {
                return;
            };
            let stopped = d.state == DiscussionState::Stopped;
            let summarizing = d.state == DiscussionState::Summarizing;
            // A turn the user started in the member's own session has no post.
            let Some(post) = d
                .posts
                .iter_mut()
                .rev()
                .find(|p| p.member == member && p.in_flight())
            else {
                return;
            };
            post.ended_ms = Some(now_ms());
            post.waiting = None;
            if reason == TurnEndReason::Completed && !text.trim().is_empty() {
                post.state = PostState::Done;
                post.stance =
                    (post.kind == PostKind::Answer && post.round >= 2).then(|| parse_stance(&text));
                post.text = text;
            } else {
                post.state = PostState::Failed;
                post.text = text;
                post.error = Some(match (reason, error) {
                    (_, Some(error)) => error,
                    (TurnEndReason::Interrupted, None) if stopped => "讨论已停止".into(),
                    (TurnEndReason::Interrupted, None) if summarizing => "被立即总结打断".into(),
                    (TurnEndReason::Interrupted, None) => "被打断".into(),
                    (TurnEndReason::Completed, None) => "没有回复".into(),
                    (_, None) => "这一轮没有正常结束".into(),
                });
            }
            Self::advance(d)
        };
        self.after_change(&id, outbox);
    }

    fn waiting(&self, task: &str, what: Option<String>) {
        let Some((id, member)) = self.member_of(task) else {
            return;
        };
        let changed = {
            let mut state = self.state.lock();
            let Some(post) = state.by_id.get_mut(&id).and_then(|d| {
                d.posts
                    .iter_mut()
                    .rev()
                    .find(|p| p.member == member && p.in_flight())
            }) else {
                return;
            };
            let next = if what.is_some() {
                PostState::Waiting
            } else {
                PostState::Speaking
            };
            let changed = post.state != next;
            post.state = next;
            post.waiting = what;
            changed
        };
        if changed {
            self.after_change(&id, Vec::new());
        }
    }

    fn spawn(&self, f: impl FnOnce(Arc<Self>) + Send + 'static) {
        let me = self.me.lock().clone();
        std::thread::spawn(move || {
            if let Some(this) = me.upgrade() {
                f(this);
            }
        });
    }

    /// 目检 2026-11-12: a member whose session failed to start never ends a
    /// turn; its post fails with the session's reason instead of waiting.
    fn on_status(&self, task: &str, status: TaskStatus, reason: Option<String>) {
        let end = match status {
            TaskStatus::Failed => TurnEndReason::Failed,
            TaskStatus::Cancelled | TaskStatus::Interrupted => TurnEndReason::Interrupted,
            _ => return,
        };
        if !self.state.lock().member_of.contains_key(task) {
            return;
        }
        let task = task.to_string();
        let error = reason
            .or_else(|| (end == TurnEndReason::Failed).then(|| "成员会话没能运行".to_string()));
        self.spawn(move |this| this.settle(&task, end, error));
    }

    fn on_update(&self, update: &SessionUpdate) {
        let task = &update.task_id;
        if !self.state.lock().member_of.contains_key(task) {
            return;
        }
        match &update.kind {
            SessionUpdateKind::TurnStarted { .. } => {
                self.state.lock().text.remove(task);
            }
            SessionUpdateKind::MessageChunk {
                message_id,
                role,
                text,
            } if role == "assistant" => {
                let mut state = self.state.lock();
                let messages = state.text.entry(task.clone()).or_default();
                match messages.iter_mut().find(|(id, _)| id == message_id) {
                    Some((_, body)) => body.push_str(text),
                    None => messages.push((message_id.clone(), text.clone())),
                }
            }
            SessionUpdateKind::MessageCompleted {
                message_id,
                text: Some(text),
            } => {
                let mut state = self.state.lock();
                let messages = state.text.entry(task.clone()).or_default();
                match messages.iter_mut().find(|(id, _)| id == message_id) {
                    Some((_, body)) => *body = text.clone(),
                    None => messages.push((message_id.clone(), text.clone())),
                }
            }
            SessionUpdateKind::PermissionRequest {
                id, kind, summary, ..
            } => {
                let (task, request) = (task.clone(), id.clone());
                if is_plan_exit(kind) {
                    self.spawn(move |this| {
                        if let Some(host) = this.host() {
                            host.respond(
                                &task,
                                &request,
                                Decision::Deny {
                                    reason: Some(prompt::NO_PLAN.into()),
                                },
                            );
                        }
                    });
                    return;
                }
                let summary = summary.clone();
                self.spawn(move |this| this.waiting(&task, Some(summary)));
            }
            SessionUpdateKind::Question { title, .. } => {
                let (task, title) = (task.clone(), title.clone());
                self.spawn(move |this| this.waiting(&task, Some(title)));
            }
            SessionUpdateKind::PermissionResolved { .. } => {
                let task = task.clone();
                self.spawn(move |this| this.waiting(&task, None));
            }
            SessionUpdateKind::TurnEnded { reason, error, .. } => {
                let (task, reason, error) = (task.clone(), *reason, error.clone());
                self.spawn(move |this| this.settle(&task, reason, error));
            }
            _ => {}
        }
    }

    /// Tests: a member's session status on the caller's thread.
    #[cfg(test)]
    fn status_now(&self, task: &str, status: TaskStatus, reason: Option<&str>) {
        let end = match status {
            TaskStatus::Failed => TurnEndReason::Failed,
            _ => TurnEndReason::Interrupted,
        };
        self.settle(task, end, reason.map(str::to_string));
    }

    /// Tests: settle a turn on the caller's thread.
    #[cfg(test)]
    fn settle_now(&self, task: &str, reason: TurnEndReason, text: &str) {
        self.state
            .lock()
            .text
            .insert(task.to_string(), vec![("m".to_string(), text.to_string())]);
        self.settle(task, reason, None);
    }
}

impl LiveStatusSink for Discussions {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.inner
            .status(project_id, task_id, generation, status, reason.clone());
        self.on_status(task_id, status, reason);
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        self.inner
            .session(project_id, task_id, family, agent, session_id, session_path);
    }

    fn update(&self, update: &SessionUpdate) {
        self.inner.update(update);
        self.on_update(update);
    }
}

#[cfg(test)]
mod tests;
