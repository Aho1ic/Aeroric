//! 2026-11-12 discussion (round table): 2–5 member sessions answer one topic
//! read-only. Round 1 each answers alone; every later round each sees what
//! the others said since and ends on a stance line; the moderator sums up
//! after the last round, or as soon as everyone keeps their stance from
//! round 2 on (frontier-roadmap Q-R7; antumi's group chat).
//!
//! 2026-11-13 second batch: the user can speak in the discussion (members
//! hear it next round; once paused or summed up it starts another round),
//! ask one member by name (it answers alone, the round does not move on),
//! ask for another round, and start it anonymous (members call each other
//! 成员 A / B).
//!
//! 2026-11-13 third batch, draft review ([`Format::Review`]): the moderator
//! is the author and writes version 1; every other member reviews it alone
//! and ends on "结论：通过" or "结论：需修改"; the author sees every review
//! and writes the next version with notes on what changed. Odd rounds are
//! the author's, even rounds the reviewers'; round r is version ⌈r/2⌉. The
//! latest version is final once everyone passes it or the revisions run
//! out ([`after_review_round`]).
//!
//! 2026-11-13 fourth batch, research ([`Format::Research`]): the moderator
//! splits the question (a "分工：" line, then "- 名字：要查清的事" per
//! member); each member gets only its part and the user's words, researches
//! alone; the moderator then writes the report. A discussion also keeps the
//! user's requirements (`requirements`), sent with every member prompt.
//!
//! 2026-11-14 relay ([`Format::Relay`], the roadmap's ring experiment): the
//! members speak one at a time in order, each hearing only the post just
//! before its own and what the user said; `max_rounds` counts laps and
//! `round` counts turns (turn r is member (r−1) mod n). No stance lines;
//! after the last lap the moderator sums up with every post. The round
//! table already is the mesh (each round everyone hears everyone).
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

use super::update::{PermissionSendState, SessionUpdate, SessionUpdateKind, TurnEndReason};
use super::{now_ms, Decision, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

pub const MIN_MEMBERS: usize = 2;
pub const MAX_MEMBERS: usize = 5;
pub const MAX_ROUNDS: u32 = 5;
pub const DEFAULT_ROUNDS: u32 = 3;
pub const MAX_REVISIONS: u32 = 3;
pub const DEFAULT_REVISIONS: u32 = 2;

fn default_revisions() -> u32 {
    DEFAULT_REVISIONS
}

/// How the members work together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// Everyone answers each round; the moderator sums up.
    #[default]
    Roundtable,
    /// The moderator writes, the others review, until it is final.
    Review,
    /// The moderator splits the question, each member researches its part
    /// alone, the moderator writes the report.
    Research,
    /// One at a time in order, each hearing only the one before.
    Relay,
}

/// What a draft-review post is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Draft,
    Review,
    Revise,
    Final,
    /// Research: the moderator's split, then each member's part.
    Split,
    Research,
}

/// A reviewer's call on a version; on the final post, whether everyone
/// passed the last version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Pass,
    Revise,
}

/// The version round `round` of a draft review writes or reviews.
pub fn version_of(round: u32) -> u32 {
    round.div_ceil(2)
}

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
    /// The user spoke; `member` means nothing here.
    User,
    /// A member answering the user who asked it by name.
    Reply,
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
    /// A user post: the members it asks by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<usize>,
    /// Draft review only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

impl Post {
    fn new(round: u32, member: usize, kind: PostKind) -> Self {
        Post {
            round,
            member,
            kind,
            state: PostState::Speaking,
            text: String::new(),
            stance: None,
            error: None,
            waiting: None,
            started_ms: now_ms(),
            ended_ms: None,
            mentions: Vec::new(),
            stage: None,
            version: None,
            verdict: None,
        }
    }

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
    /// Members call each other 成员 A / B, not by agent and model. Set at
    /// the start only, so earlier posts keep their names.
    #[serde(default)]
    pub anonymous: bool,
    /// Members asked by name while still talking: asked once they finish.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_replies: Vec<usize>,
    #[serde(default)]
    pub format: Format,
    /// Draft review: how many times the author may revise.
    #[serde(default = "default_revisions")]
    pub max_revisions: u32,
    /// What the user asks of every post ("观点要具体……"). `None` (saved
    /// before 2026-11-13, or not given) is the default; empty asks nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<String>,
    /// Research: each member's part, by member index; the moderator's is
    /// empty. Filled from the moderator's split.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<String>,
}

impl Discussion {
    /// What the discussion's task record shows in the sidebar.
    pub fn task_status(&self) -> TaskStatus {
        let talking = || {
            if self.posts.iter().any(|p| p.state == PostState::Waiting) {
                TaskStatus::InputRequired
            } else {
                TaskStatus::Running
            }
        };
        match self.state {
            DiscussionState::Running | DiscussionState::Pausing | DiscussionState::Summarizing => {
                talking()
            }
            // A member asked by name is still answering.
            DiscussionState::Paused | DiscussionState::Done if self.in_flight() => talking(),
            DiscussionState::Paused => TaskStatus::Interrupted,
            DiscussionState::Done => TaskStatus::Done,
            DiscussionState::Stopped => TaskStatus::Cancelled,
        }
    }

    /// Who speaks in `round`: everyone at a round table; in a draft review
    /// the author in odd rounds and the reviewers in even ones.
    pub fn speakers(&self, round: u32) -> Vec<usize> {
        let all = 0..self.members.len();
        match self.format {
            Format::Roundtable => all.collect(),
            Format::Review if round % 2 == 1 => vec![self.moderator],
            Format::Review => all.filter(|m| *m != self.moderator).collect(),
            Format::Research if round == 1 => vec![self.moderator],
            Format::Research => all.filter(|m| *m != self.moderator).collect(),
            Format::Relay => vec![(round.max(1) as usize - 1) % self.members.len()],
        }
    }

    /// Relay: which lap turn `round` is in.
    pub fn lap(&self, round: u32) -> u32 {
        (round.max(1) - 1) / self.members.len().max(1) as u32 + 1
    }

    /// Members are working alone: a draft review's reviewers, a research's
    /// members, a relay's runners. Asking one by name would tell it the
    /// others'.
    pub fn working_alone(&self) -> bool {
        match self.format {
            Format::Roundtable => false,
            Format::Review => self.round.is_multiple_of(2),
            Format::Research => self.round == 2,
            Format::Relay => true,
        }
    }

    /// The author's latest finished version.
    fn latest_draft(&self) -> Option<&Post> {
        self.posts.iter().rev().find(|p| {
            p.member == self.moderator
                && p.kind == PostKind::Answer
                && p.state == PostState::Done
                && p.version.is_some()
        })
    }

    /// `member`'s finished post of `round`, the latest when retried.
    fn said(&self, round: u32, member: usize) -> Option<&Post> {
        self.posts.iter().rev().find(|p| {
            p.round == round
                && p.member == member
                && p.kind == PostKind::Answer
                && p.state == PostState::Done
        })
    }

    /// Every reviewer passed the author's latest version.
    pub fn latest_version_passed(&self) -> bool {
        let Some(version) = self.latest_draft().and_then(|p| p.version) else {
            return false;
        };
        let round = version * 2;
        self.speakers(round).iter().all(|m| {
            self.said(round, *m)
                .is_some_and(|p| p.verdict == Some(Verdict::Pass))
        })
    }

    /// What members call member `i`: its label, or 成员 A / B when anonymous.
    pub fn name(&self, i: usize) -> String {
        if self.anonymous {
            format!("成员 {}", char::from(b'A' + (i % 26) as u8))
        } else {
            self.members[i].label.clone()
        }
    }

    fn in_flight(&self) -> bool {
        self.posts.iter().any(Post::in_flight)
    }

    /// The posts `member` has not been sent yet, by the others or the
    /// user, from before `round` (`None`: every round so far).
    pub fn unseen(&self, member: usize, round: Option<u32>) -> Vec<usize> {
        self.posts
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                // A reviewer hears the author and the user, never the other
                // reviewers, so each review is its own; a researcher hears
                // only the user (its part comes with its prompt).
                let reviewer = self.format == Format::Review && member != self.moderator;
                let researcher = self.format == Format::Research && member != self.moderator;
                // A relay's runner hears only the turn just before its own;
                // a reply or the summary hears everyone, as at a round table.
                let runner = self.format == Format::Relay && round.is_some();
                let heard = match p.kind {
                    PostKind::User => true,
                    PostKind::Answer | PostKind::Reply if runner => {
                        p.kind == PostKind::Answer
                            && p.member != member
                            && round.is_some_and(|r| p.round + 1 == r)
                    }
                    PostKind::Answer | PostKind::Reply if researcher => false,
                    PostKind::Answer | PostKind::Reply if reviewer => p.member == self.moderator,
                    PostKind::Answer | PostKind::Reply => p.member != member,
                    PostKind::Summary => false,
                };
                heard
                    && p.state == PostState::Done
                    && round.is_none_or(|r| p.round < r)
                    && !self.members[member].seen.contains(i)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The summary post of the summing up under way: a later round's summing
    /// up (after another round) is a new one.
    fn current_summary(&self) -> Option<&Post> {
        self.posts
            .iter()
            .rev()
            .find(|p| p.kind == PostKind::Summary && p.round == self.round)
    }
}

/// What comes after a round has been said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Round(u32),
    Summarize,
    Pause(String),
    /// Draft review: the latest version is final; whether everyone passed it.
    Finalize(bool),
}

/// The step after round `discussion.round`, every answer of it in.
pub fn after_round(discussion: &Discussion) -> Next {
    match discussion.format {
        Format::Review => return after_review_round(discussion),
        Format::Research => return after_research_round(discussion),
        Format::Relay => return after_relay_turn(discussion),
        Format::Roundtable => {}
    }
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
    // The user spoke to everyone during this round: the members answer it
    // next round, even past the round limit or when everyone kept their
    // stance. Asking by name was answered already.
    if discussion
        .posts
        .iter()
        .any(|p| p.kind == PostKind::User && p.round == round && p.mentions.is_empty())
    {
        return Next::Round(round + 1);
    }
    if round >= discussion.max_rounds {
        return Next::Summarize;
    }
    if round >= 2 && said.iter().all(|p| p.stance == Some(Stance::Keep)) {
        return Next::Summarize;
    }
    Next::Round(round + 1)
}

/// Relay: the next turn, or the summary once the last lap is run. A turn
/// that failed pauses (carrying on asks the same runner again); the user
/// speaking during a turn makes sure the next one runs.
pub fn after_relay_turn(d: &Discussion) -> Next {
    let round = d.round;
    let done = d
        .posts
        .iter()
        .any(|p| p.round == round && p.kind == PostKind::Answer && p.state == PostState::Done);
    if !done {
        let runner = d
            .speakers(round)
            .first()
            .map(|m| d.name(*m))
            .unwrap_or_default();
        return Next::Pause(format!("第 {} 圈「{runner}」没发言成功", d.lap(round)));
    }
    let user = d
        .posts
        .iter()
        .any(|p| p.kind == PostKind::User && p.round == round && p.mentions.is_empty());
    if user || round < d.max_rounds * d.members.len() as u32 {
        Next::Round(round + 1)
    } else {
        Next::Summarize
    }
}

/// Draft review: after the author's round the reviewers review; after the
/// reviews the version is final if all passed or no revision is left, else
/// the author revises. A missing draft or review pauses; a version nobody
/// finished reviewing never counts as passed.
pub fn after_review_round(d: &Discussion) -> Next {
    let round = d.round;
    let version = version_of(round);
    if round % 2 == 1 {
        return if d.said(round, d.moderator).is_some() {
            Next::Round(round + 1)
        } else {
            Next::Pause(format!("作者没写出第 {version} 版"))
        };
    }
    let reviewers = d.speakers(round);
    let missing = reviewers
        .iter()
        .filter(|m| d.said(round, **m).is_none())
        .count();
    if missing > 0 {
        return Next::Pause(format!("第 {version} 版还有 {missing} 位评审没评完"));
    }
    if reviewers.iter().all(|m| {
        d.said(round, *m)
            .is_some_and(|p| p.verdict == Some(Verdict::Pass))
    }) {
        return Next::Finalize(true);
    }
    if version > d.max_revisions {
        return Next::Finalize(false);
    }
    Next::Round(round + 1)
}

/// Research: the split (checked when it lands: a split missing someone
/// fails) leads to the research; when every member has its part done the
/// moderator writes the report. A missing split or part pauses.
pub fn after_research_round(d: &Discussion) -> Next {
    if d.round == 1 {
        return if d.said(1, d.moderator).is_some() {
            Next::Round(2)
        } else {
            Next::Pause("主持人没拆好分工".into())
        };
    }
    let missing = d
        .speakers(d.round)
        .iter()
        .filter(|m| d.said(d.round, **m).is_none())
        .count();
    if missing > 0 {
        return Next::Pause(format!("还有 {missing} 位成员没查完"));
    }
    Next::Summarize
}

/// The parts in a moderator's split: after a line with "分工", lines like
/// "- 名字：要查清的事". Names are matched case-insensitively against each
/// member's `names`. Returns each found member's part.
pub fn parse_assignments(text: &str, names: &[(usize, Vec<String>)]) -> HashMap<usize, String> {
    let mut found = HashMap::new();
    let mut started = false;
    for line in text.lines() {
        let line = line.trim();
        if !started {
            started = line.contains("分工");
            // "分工：" may carry the first part on the same line.
            if !started {
                continue;
            }
        }
        let item = line
            .trim_start_matches(|c: char| matches!(c, '-' | '*' | '•' | '·') || c.is_ascii_digit())
            .trim_start_matches(['.', '、', ')', '）'])
            .trim();
        let Some(at) = item.find(['：', ':']) else {
            continue;
        };
        let name = item[..at]
            .trim()
            .trim_matches(|c| matches!(c, '*' | '「' | '」' | '`' | '"'))
            .trim()
            .to_lowercase();
        let part = item[at..]
            .trim_start_matches(['：', ':'])
            .trim()
            .to_string();
        if part.is_empty() {
            continue;
        }
        if let Some((member, _)) = names
            .iter()
            .find(|(_, ns)| ns.iter().any(|n| n.to_lowercase() == name))
        {
            found.entry(*member).or_insert(part);
        }
    }
    found
}

/// A review's last lines: "结论：通过" or "结论：需修改". Unreadable counts
/// as needing changes, so an unclear review never passes a version.
pub fn parse_verdict(text: &str) -> Verdict {
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()).take(3) {
        let line = line
            .trim()
            .trim_matches(|c| matches!(c, '*' | '_' | '`' | '>'));
        let Some(rest) = line.find("结论").map(|at| &line[at + "结论".len()..]) else {
            continue;
        };
        let rest = rest.trim_start_matches([':', '：', ' ', '*']);
        if rest.starts_with("通过") {
            return Verdict::Pass;
        }
        if rest.starts_with("需修改") || rest.starts_with("需要修改") || rest.starts_with("不通过")
        {
            return Verdict::Revise;
        }
    }
    Verdict::Revise
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

/// The first name a member goes by in a split.
fn names_of(names: &[(usize, Vec<String>)], member: usize) -> String {
    names
        .iter()
        .find(|(m, _)| *m == member)
        .and_then(|(_, ns)| ns.first().cloned())
        .unwrap_or_default()
}

/// Messages to send once the lock is let go.
type Outbox = Vec<(String, String)>;

pub struct Discussions {
    inner: Arc<dyn LiveStatusSink>,
    host: Mutex<Option<Arc<dyn DiscussionHost>>>,
    dir: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    me: Mutex<Weak<Self>>,
    /// B5-3: one save at a time, each of the state as it is when it writes,
    /// so an older snapshot never lands after a newer one.
    writes: Mutex<()>,
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// B5-3: through the shared atomic writer (unique temp name, synced, 0600).
fn save(dir: &Path, discussion: &Discussion) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(discussion).map_err(|e| e.to_string())?;
    crate::storage::atomic::write_atomic(
        &file(dir, &discussion.id),
        &bytes,
        crate::storage::atomic::FileMode::Private,
    )
    .map_err(|e| e.to_string())
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
        discussion.pending_replies.clear();
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
            writes: Mutex::default(),
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
        if discussion.format == Format::Review
            && !(1..=MAX_REVISIONS).contains(&discussion.max_revisions)
        {
            return Err(DiscussionError::Invalid(format!(
                "修订次数要在 1 到 {MAX_REVISIONS} 之间"
            )));
        }
        let now = now_ms();
        discussion.round = 1;
        discussion.state = DiscussionState::Running;
        let (stage, version) = match discussion.format {
            Format::Roundtable | Format::Relay => (None, None),
            Format::Review => (Some(Stage::Draft), Some(1)),
            Format::Research => (Some(Stage::Split), None),
        };
        discussion.posts = discussion
            .speakers(1)
            .into_iter()
            .map(|member| Post {
                started_ms: now,
                stage,
                version,
                ..Post::new(1, member, PostKind::Answer)
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
            DiscussionState::Paused if d.in_flight() => Err(DiscussionError::Invalid(
                "被点名的成员还在回答，等它说完再继续".into(),
            )),
            DiscussionState::Paused => {
                d.note = None;
                let summary_failed = d
                    .current_summary()
                    .is_some_and(|p| p.state == PostState::Failed);
                if summary_failed {
                    return Ok(Self::start_summary(d));
                }
                let round = d.round;
                let lost: Vec<usize> = d
                    .speakers(round)
                    .into_iter()
                    .filter(|m| d.said(round, *m).is_none())
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
            // Already summing up (the round ended early on its own just
            // before the click): nothing to do, and not an error.
            if d.state == DiscussionState::Summarizing {
                return Ok(Vec::new());
            }
            if !matches!(
                d.state,
                DiscussionState::Running | DiscussionState::Pausing | DiscussionState::Paused
            ) {
                return Err(DiscussionError::Invalid("讨论已经结束".into()));
            }
            if d.format == Format::Review {
                // 现在定稿: whoever is writing finishes first.
                if d.latest_draft().is_none() {
                    return Err(DiscussionError::Invalid("作者还没写出任何一版".into()));
                }
                d.note = None;
                d.state = DiscussionState::Summarizing;
                d.pending_replies.clear();
                return Ok(if d.in_flight() {
                    Vec::new()
                } else {
                    Self::finalize(d)
                });
            }
            if !d.posts.iter().any(|p| p.state == PostState::Done) {
                return Err(DiscussionError::Invalid(
                    "还没有人发言，没有可总结的".into(),
                ));
            }
            if d.format == Format::Research
                && !d
                    .posts
                    .iter()
                    .any(|p| p.stage == Some(Stage::Research) && p.state == PostState::Done)
            {
                return Err(DiscussionError::Invalid(
                    "还没有成员查完，没有可汇总的".into(),
                ));
            }
            d.note = None;
            d.state = DiscussionState::Summarizing;
            d.pending_replies.clear();
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
            d.pending_replies.clear();
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

    /// The user speaks. Naming members (`mentions`) asks only them: each
    /// answers alone, now or once it finishes what it is saying, and the
    /// round does not move on. Otherwise every member hears it next round;
    /// once paused or summed up, that round starts now and is summed up
    /// again.
    pub fn say(
        &self,
        id: &str,
        text: &str,
        mentions: &[usize],
    ) -> Result<Discussion, DiscussionError> {
        let text = text.trim().to_string();
        self.act(id, |d| {
            if text.is_empty() {
                return Err(DiscussionError::Invalid("发言不能为空".into()));
            }
            if mentions.iter().any(|m| *m >= d.members.len()) {
                return Err(DiscussionError::Invalid("点名的不是成员".into()));
            }
            match d.state {
                DiscussionState::Stopped => {
                    return Err(DiscussionError::Invalid("讨论已经停止".into()))
                }
                DiscussionState::Summarizing => {
                    return Err(DiscussionError::Invalid(
                        "主持人正在总结，等总结写完再发言".into(),
                    ))
                }
                DiscussionState::Paused | DiscussionState::Done
                    if mentions.is_empty() && d.in_flight() =>
                {
                    return Err(DiscussionError::Invalid(
                        "被点名的成员还在回答，等它说完再发言".into(),
                    ))
                }
                _ => {}
            }
            if !mentions.is_empty()
                && d.working_alone()
                && matches!(d.state, DiscussionState::Running | DiscussionState::Pausing)
            {
                return Err(DiscussionError::Invalid(if d.format == Format::Research {
                    "调研进行中不能点名：被点名的成员会看到别人的结果".into()
                } else if d.format == Format::Relay {
                    "接力进行中不能点名：被点名的成员会看到全部发言".into()
                } else {
                    "评审进行中不能点名：被点名的评审会看到别人的意见".into()
                }));
            }
            if mentions.is_empty()
                && d.format == Format::Research
                && d.state == DiscussionState::Done
            {
                return Err(DiscussionError::Invalid(
                    "调研已出报告：可以 @成员 追问，或用报告卡转去讨论、起草".into(),
                ));
            }
            let mut mentions = mentions.to_vec();
            mentions.sort_unstable();
            mentions.dedup();
            let now = now_ms();
            d.posts.push(Post {
                state: PostState::Done,
                text: text.clone(),
                ended_ms: Some(now),
                started_ms: now,
                mentions: mentions.clone(),
                ..Post::new(d.round, 0, PostKind::User)
            });
            if !mentions.is_empty() {
                let mut outbox = Vec::new();
                for m in mentions {
                    let talking = d.posts.iter().any(|p| p.member == m && p.in_flight());
                    if talking {
                        if !d.pending_replies.contains(&m) {
                            d.pending_replies.push(m);
                        }
                    } else {
                        outbox.extend(Self::reply(d, m));
                    }
                }
                return Ok(outbox);
            }
            Ok(match d.state {
                // Research has no further round: the report will hear it.
                DiscussionState::Paused if d.format == Format::Research => Vec::new(),
                DiscussionState::Paused | DiscussionState::Done => Self::new_round(d),
                // Running or pausing: heard next round.
                _ => Vec::new(),
            })
        })
    }

    /// One more round once paused or summed up, then a fresh summary.
    pub fn another_round(&self, id: &str) -> Result<Discussion, DiscussionError> {
        self.act(id, |d| match d.state {
            _ if d.format == Format::Research => Err(DiscussionError::Invalid(
                "分工调研没有再来一轮：用报告卡转去讨论或起草".into(),
            )),
            DiscussionState::Paused | DiscussionState::Done if d.in_flight() => Err(
                DiscussionError::Invalid("被点名的成员还在回答，等它说完".into()),
            ),
            DiscussionState::Paused | DiscussionState::Done => {
                if !d.posts.iter().any(|p| p.state == PostState::Done) {
                    return Err(DiscussionError::Invalid("还没有人发言".into()));
                }
                Ok(Self::new_round(d))
            }
            _ => Err(DiscussionError::Invalid(
                "讨论暂停或总结之后才能再来一轮".into(),
            )),
        })
    }

    fn host(&self) -> Option<Arc<dyn DiscussionHost>> {
        self.host.lock().clone()
    }

    fn persist(&self, discussion: &Discussion) {
        if let Some(dir) = self.dir.lock().clone() {
            if let Err(error) = save(&dir, discussion) {
                eprintln!("discussion: saving {} failed: {error}", discussion.id);
            }
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
        let discussion = {
            // B5-3: read and write under one lock, newest state last.
            let _write = self.writes.lock();
            let Some(discussion) = self.get(id) else {
                return;
            };
            self.persist(&discussion);
            discussion
        };
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
        let (text, stage) = match d.format {
            Format::Roundtable if round == 1 => (prompt::opening(d, member), None),
            Format::Roundtable => (prompt::round(d, &unseen), None),
            Format::Review if round == 1 => (prompt::opening(d, member), Some(Stage::Draft)),
            Format::Review if round.is_multiple_of(2) => {
                (prompt::review(d, member, &unseen), Some(Stage::Review))
            }
            Format::Review => (prompt::revise(d, &unseen), Some(Stage::Revise)),
            Format::Research if round == 1 => (prompt::opening(d, member), Some(Stage::Split)),
            Format::Research => (prompt::research(d, member, &unseen), Some(Stage::Research)),
            Format::Relay if round == 1 => (prompt::opening(d, member), None),
            Format::Relay => (prompt::relay(d, member, &unseen), None),
        };
        d.members[member].seen.extend(unseen);
        d.posts.push(Post {
            stage,
            version: (d.format == Format::Review).then(|| version_of(round)),
            ..Post::new(round, member, PostKind::Answer)
        });
        (d.members[member].task_id.clone(), text)
    }

    /// Member `member` answers the user's latest post that named it.
    fn reply(d: &mut Discussion, member: usize) -> Option<(String, String)> {
        let question = d
            .posts
            .iter()
            .rposition(|p| p.kind == PostKind::User && p.mentions.contains(&member))?;
        let heard: Vec<usize> = d
            .unseen(member, None)
            .into_iter()
            .filter(|i| *i != question)
            .collect();
        let text = prompt::reply(d, question, &heard);
        d.members[member].seen.extend(heard);
        d.members[member].seen.push(question);
        d.posts.push(Post::new(d.round, member, PostKind::Reply));
        Some((d.members[member].task_id.clone(), text))
    }

    /// Everyone answers round `round + 1`, the last before a new summary.
    /// In a draft review: the author writes the next version (再改一版),
    /// reviewed once more before it is final.
    fn new_round(d: &mut Discussion) -> Outbox {
        match d.format {
            Format::Roundtable => {
                d.round += 1;
                d.max_rounds = d.round;
            }
            Format::Review => {
                d.round += if d.round.is_multiple_of(2) { 1 } else { 2 };
                d.max_revisions = version_of(d.round) - 1;
            }
            // Never asked for (another_round and say refuse it).
            Format::Research => return Vec::new(),
            // One more lap, from the next runner.
            Format::Relay => {
                d.round += 1;
                let n = d.members.len() as u32;
                d.max_rounds = (d.round + n - 1).div_ceil(n);
            }
        }
        d.state = DiscussionState::Running;
        d.note = None;
        let round = d.round;
        d.speakers(round)
            .into_iter()
            .map(|m| Self::ask(d, m, round))
            .collect()
    }

    /// Draft review: the author's latest version is final.
    fn finalize(d: &mut Discussion) -> Outbox {
        let passed = d.latest_version_passed();
        let Some((text, version)) = d.latest_draft().map(|p| (p.text.clone(), p.version)) else {
            d.state = DiscussionState::Paused;
            d.note = Some("作者还没写出任何一版".into());
            return Vec::new();
        };
        let now = now_ms();
        d.posts.push(Post {
            state: PostState::Done,
            text,
            stage: Some(Stage::Final),
            version,
            verdict: Some(if passed {
                Verdict::Pass
            } else {
                Verdict::Revise
            }),
            started_ms: now,
            ended_ms: Some(now),
            ..Post::new(d.round, d.moderator, PostKind::Summary)
        });
        d.state = DiscussionState::Done;
        d.note = None;
        Vec::new()
    }

    fn start_summary(d: &mut Discussion) -> Outbox {
        d.state = DiscussionState::Summarizing;
        let m = d.moderator;
        let unseen = d.unseen(m, None);
        let text = if d.format == Format::Research {
            prompt::report(d, &unseen)
        } else {
            prompt::summary(d, &unseen)
        };
        d.members[m].seen.extend(unseen);
        d.posts.push(Post::new(d.round, m, PostKind::Summary));
        vec![(d.members[m].task_id.clone(), text)]
    }

    /// Research: read the moderator's split once it lands. A split that
    /// leaves a member out fails, so carrying on asks for it again.
    fn check_split(d: &mut Discussion) {
        if d.format != Format::Research || d.round != 1 {
            return;
        }
        let names: Vec<(usize, Vec<String>)> = (0..d.members.len())
            .filter(|m| *m != d.moderator)
            .map(|m| (m, vec![d.name(m), d.members[m].label.clone()]))
            .collect();
        let Some(post) =
            d.posts.iter_mut().rev().find(|p| {
                p.round == 1 && p.stage == Some(Stage::Split) && p.state == PostState::Done
            })
        else {
            return;
        };
        let parts = parse_assignments(&post.text, &names);
        let missing: Vec<usize> = names
            .iter()
            .map(|(m, _)| *m)
            .filter(|m| !parts.contains_key(m))
            .collect();
        if !missing.is_empty() {
            let who: Vec<String> = missing
                .iter()
                .map(|m| format!("「{}」", names_of(&names, *m)))
                .collect();
            post.state = PostState::Failed;
            post.error = Some(format!("分工里漏了{}", who.join("、")));
            return;
        }
        d.assignments = (0..d.members.len())
            .map(|m| parts.get(&m).cloned().unwrap_or_default())
            .collect();
    }

    /// The round is said: the next round, the summary, or a pause.
    fn next(d: &mut Discussion) -> Outbox {
        Self::check_split(d);
        match after_round(d) {
            Next::Round(round) => {
                d.round = round;
                d.speakers(round)
                    .into_iter()
                    .map(|m| Self::ask(d, m, round))
                    .collect()
            }
            Next::Summarize => Self::start_summary(d),
            Next::Finalize(_) => Self::finalize(d),
            Next::Pause(reason) => {
                // Why the split failed says more than "没拆好".
                let why = d
                    .posts
                    .iter()
                    .rev()
                    .find(|p| p.stage == Some(Stage::Split) && p.state == PostState::Failed)
                    .and_then(|p| p.error.clone())
                    .filter(|_| d.format == Format::Research && d.round == 1);
                d.state = DiscussionState::Paused;
                d.note = Some(why.map(|w| format!("{reason}：{w}")).unwrap_or(reason));
                Vec::new()
            }
        }
    }

    /// After a post settled: what the discussion does now.
    fn advance(d: &mut Discussion) -> Outbox {
        if d.in_flight() {
            return Vec::new();
        }
        match d.state {
            DiscussionState::Summarizing if d.format == Format::Review => Self::finalize(d),
            DiscussionState::Summarizing => match d.current_summary().map(|p| p.state) {
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
            let relay = d.format == Format::Relay;
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
                post.stance = (post.kind == PostKind::Answer
                    && post.round >= 2
                    && post.stage.is_none()
                    && !relay)
                    .then(|| parse_stance(&text));
                post.verdict = (post.stage == Some(Stage::Review)).then(|| parse_verdict(&text));
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
            // Asked by name while talking: it answers the user now.
            let mut outbox = Vec::new();
            if let Some(at) = d.pending_replies.iter().position(|m| *m == member) {
                d.pending_replies.remove(at);
                if !stopped {
                    outbox.extend(Self::reply(d, member));
                }
            }
            outbox.extend(Self::advance(d));
            outbox
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
            // B1-1: a member is read-only (`plan`), so the host's
            // `ReadOnlyGuard` denies every request it makes — leaving plan
            // mode included; the member waits only until that lands.
            SessionUpdateKind::PermissionRequest { summary, .. } => {
                let (task, summary) = (task.clone(), summary.clone());
                self.spawn(move |this| this.waiting(&task, Some(summary)));
            }
            SessionUpdateKind::Question { title, .. } => {
                let (task, title) = (task.clone(), title.clone());
                self.spawn(move |this| this.waiting(&task, Some(title)));
            }
            // A2: an answer that did not get through (`failed`) leaves the
            // member waiting; `ended` settles the request like a resolution.
            SessionUpdateKind::PermissionResolved { .. }
            | SessionUpdateKind::PermissionSendFailed {
                state: PermissionSendState::Ended,
                ..
            } => {
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
