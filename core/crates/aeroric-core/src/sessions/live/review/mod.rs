//! B4-1 (2026-10-30): the review loop. After a turn that changed files, an
//! agent of another family reviews the diff in isolation and read-only; its
//! findings go back to the writer, which adopts, rejects (with a reason) or
//! hands each one to the user. Up to [`MAX_ROUNDS`] rounds; a loop that needs
//! the user lands in the attention queue (frontier-roadmap §5.2 form A).
//!
//! [`ReviewLoop`] sits between the host's sink and the checkpoint sink, so it
//! sees every update — the reviewer's and the writer's turns, and the
//! `autoCheck` results it waits for. The turn-change tracker tells it when a
//! turn's diff is known ([`ReviewLoop::turn_settled`]). Starting sessions and
//! sending messages is the host's ([`ReviewHost`]), always off the caller's
//! thread: a sink callback never waits on a session.

pub mod parse;
pub mod prompt;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use aeroric_protocol::checkpoint::ObligationSource;
use aeroric_protocol::review::{
    Decision, ReviewCard, ReviewSettings, ReviewState, ReviewerChoice, Severity, MAX_RAW_CHARS,
    MAX_ROUNDS,
};

pub use super::turn_changes::{TurnDiff, TurnSettled};
use super::update::{SessionUpdate, SessionUpdateKind, TurnEndReason};
use super::{now_ms, DriverRegistry, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// The reviewer the host started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reviewer {
    pub task_id: String,
    pub label: String,
    pub same_family: bool,
}

/// What the loop needs from the host. Called on a thread of its own.
pub trait ReviewHost: Send + Sync {
    /// The task's settings; `None` for a task that is never reviewed (a
    /// reviewer, a side branch, an unknown task).
    fn settings(&self, task_id: &str) -> Option<ReviewSettings>;
    /// Start a read-only reviewer for `parent` with `prompt` as its first message.
    fn start_reviewer(&self, parent: &str, prompt: String) -> Result<Reviewer, String>;
    /// Send `text` to a task as a user message, reopening its session if needed.
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReviewError {
    #[error("这个会话还没有可审查的改动")]
    NoChanges,
    #[error("这个会话正在审查")]
    Busy,
    #[error("这个会话不能被审查")]
    NotReviewable,
    #[error("没有可以发出的审查意见")]
    NothingToSend,
    #[error("没有可以重试的审查")]
    NothingToRetry,
}

/// One loop, as kept on disk (`<data>/reviews/<task>.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Loop {
    card: ReviewCard,
    #[serde(default)]
    diff: Option<TurnDiff>,
    #[serde(default)]
    checks: Option<String>,
    /// The previous round, for the reviewer's next prompt.
    #[serde(default)]
    previous: Option<ReviewCard>,
    /// Waiting for the project's checks on this turn before reviewing.
    #[serde(default)]
    awaiting_checks: Option<String>,
    /// The writer started its answering turn.
    #[serde(default)]
    replying: bool,
    /// The answering turn ended and was read.
    #[serde(default)]
    answered: bool,
    /// What the answering turn changed (`Some(None)`: settled, no change).
    #[serde(default)]
    answer_diff: Option<Option<TurnDiff>>,
    #[serde(default)]
    answer_checking: bool,
    #[serde(default)]
    answer_turn: String,
    /// The reviewer choice this loop's reviewer was picked for. A later loop
    /// reuses the reviewer only while the choice is unchanged (B4-CU-02).
    #[serde(default)]
    chosen: Option<ReviewerChoice>,
}

#[derive(Default)]
struct State {
    loops: HashMap<String, Loop>,
    /// reviewer task → parent task.
    reviewers: HashMap<String, String>,
    /// Each task's last diffed turn, for a review on request.
    last_diff: HashMap<String, (String, TurnDiff)>,
    /// The assistant's text of each task's current turn, by message.
    text: HashMap<String, Vec<(String, String)>>,
    /// The last finished check run per task: (turn, summary).
    checks_done: HashMap<String, (String, String)>,
}

enum Act {
    Start {
        parent: String,
        prompt: String,
    },
    PromptReviewer {
        parent: String,
        reviewer: String,
        prompt: String,
    },
    PromptWriter {
        parent: String,
        text: String,
    },
}

pub struct ReviewLoop {
    inner: Arc<dyn LiveStatusSink>,
    registry: Mutex<Option<Arc<DriverRegistry>>>,
    host: Mutex<Option<Arc<dyn ReviewHost>>>,
    checkpoints: Mutex<Option<Weak<super::checkpoint::CheckpointSink>>>,
    dir: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    me: Mutex<Weak<ReviewLoop>>,
}

impl ReviewLoop {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        let this = Arc::new(Self {
            inner,
            registry: Mutex::new(None),
            host: Mutex::new(None),
            checkpoints: Mutex::new(None),
            dir: Mutex::new(None),
            state: Mutex::default(),
            me: Mutex::new(Weak::new()),
        });
        *this.me.lock() = Arc::downgrade(&this);
        this
    }

    pub fn set_registry(&self, registry: Arc<DriverRegistry>) {
        *self.registry.lock() = Some(registry);
    }

    /// Unset (tests, hosts that opt out): nothing is ever reviewed.
    pub fn set_host(&self, host: Arc<dyn ReviewHost>) {
        *self.host.lock() = Some(host);
    }

    pub fn set_checkpoints(&self, checkpoints: Weak<super::checkpoint::CheckpointSink>) {
        *self.checkpoints.lock() = Some(checkpoints);
    }

    /// Where loops are kept (`<data>/reviews`); unset keeps them in memory.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    /// The task's latest loop. One read from disk after a restart turns a
    /// loop that was mid-round into `interrupted`.
    pub fn card(&self, task_id: &str) -> Option<ReviewCard> {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, task_id);
        state.loops.get(task_id).map(|l| l.card.clone())
    }

    /// Whether `task_id` is a reviewer this host started.
    pub fn parent_of(&self, reviewer: &str) -> Option<String> {
        self.state.lock().reviewers.get(reviewer).cloned()
    }

    /// Loops that need the user: an "ask" answer, past the round limit, or a
    /// settled loop whose blockers were all rejected.
    pub fn attention(&self) -> Vec<(String, ReviewCard)> {
        let state = self.state.lock();
        let mut out: Vec<(String, ReviewCard)> = state
            .loops
            .iter()
            .filter(|(_, l)| needs_user(&l.card))
            .map(|(task, l)| (task.clone(), l.card.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Review the task's last changed turn now, whether or not the loop is on.
    pub fn request(&self, task_id: &str) -> Result<ReviewCard, ReviewError> {
        let host = self.host.lock().clone().ok_or(ReviewError::NotReviewable)?;
        let settings = host.settings(task_id).ok_or(ReviewError::NotReviewable)?;
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, task_id);
        if state
            .loops
            .get(task_id)
            .is_some_and(|l| l.card.state.is_open())
        {
            return Err(ReviewError::Busy);
        }
        let (turn, diff) = state
            .last_diff
            .get(task_id)
            .cloned()
            .ok_or(ReviewError::NoChanges)?;
        let checks = state
            .checks_done
            .get(task_id)
            .filter(|(t, _)| *t == turn)
            .map(|(_, s)| s.clone());
        self.open(&mut state, task_id, &turn, diff, checks, settings.reviewer);
        let act = self.review_act(task_id, &state.loops[task_id]);
        let card = state.loops[task_id].card.clone();
        drop(state);
        self.publish(task_id);
        self.run(act);
        Ok(card)
    }

    /// Send findings held back by a paused loop.
    pub fn send(&self, task_id: &str) -> Result<(), ReviewError> {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, task_id);
        let l = state
            .loops
            .get_mut(task_id)
            .filter(|l| l.card.state == ReviewState::WaitingSend)
            .ok_or(ReviewError::NothingToSend)?;
        let act = to_writer(task_id, l);
        drop(state);
        self.publish(task_id);
        self.run(act);
        Ok(())
    }

    /// Run the current round again after a failure or a restart.
    pub fn retry(&self, task_id: &str) -> Result<(), ReviewError> {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, task_id);
        let l = state
            .loops
            .get_mut(task_id)
            .filter(|l| {
                matches!(l.card.state, ReviewState::Failed | ReviewState::Interrupted)
                    && l.diff.is_some()
            })
            .ok_or(ReviewError::NothingToRetry)?;
        l.card.state = ReviewState::Reviewing;
        l.card.raw = None;
        l.card.findings.clear();
        l.card.summary = None;
        l.replying = false;
        l.answered = false;
        l.answer_diff = None;
        let act = self.review_act(task_id, l);
        drop(state);
        self.publish(task_id);
        self.run(act);
        Ok(())
    }

    /// The turn-change tracker finished `settled.turn_id`: its diff (if any)
    /// is known, and `checking` says whether the project's checks run next.
    pub fn turn_settled(&self, settled: TurnSettled) {
        let task = settled.task_id.clone();
        let mut state = self.state.lock();
        if state.reviewers.contains_key(&task) {
            return;
        }
        if let Some(diff) = &settled.diff {
            state
                .last_diff
                .insert(task.clone(), (settled.turn_id.clone(), diff.clone()));
        }
        self.ensure_loaded(&mut state, &task);
        let mut acts = Vec::new();
        let mut changed = false;
        if let Some(l) = state.loops.get_mut(&task) {
            if l.card.state == ReviewState::WaitingCoder && l.replying {
                l.answer_diff = Some(settled.diff.clone());
                l.answer_checking = settled.checking;
                changed = true;
            } else if l.card.state.is_open() {
                return;
            }
        }
        if changed {
            if let Some(act) = self.resolve(&mut state, &task) {
                acts.push(act);
            }
        } else {
            let settings = self
                .host
                .lock()
                .clone()
                .and_then(|h| h.settings(&task))
                .filter(|s| s.enabled);
            let (Some(diff), Some(settings), TurnEndReason::Completed) =
                (settled.diff, settings, settled.reason)
            else {
                return;
            };
            let done = state
                .checks_done
                .get(&task)
                .filter(|(t, _)| *t == settled.turn_id)
                .map(|(_, s)| s.clone());
            let wait = settled.checking && done.is_none();
            self.open(
                &mut state,
                &task,
                &settled.turn_id,
                diff,
                done,
                settings.reviewer,
            );
            let l = state.loops.get_mut(&task).expect("just opened");
            if wait {
                l.awaiting_checks = Some(settled.turn_id.clone());
            } else {
                acts.push(self.review_act(&task, l));
            }
            changed = true;
        }
        drop(state);
        if changed {
            self.publish(&task);
        }
        for act in acts {
            self.run(act);
        }
    }

    /// A new loop at round 1 for `turn`.
    fn open(
        &self,
        state: &mut State,
        task: &str,
        turn: &str,
        diff: TurnDiff,
        checks: Option<String>,
        choice: ReviewerChoice,
    ) {
        let mut l = Loop {
            card: ReviewCard {
                review_id: format!("rv-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
                round: 1,
                state: ReviewState::Reviewing,
                turn_id: turn.to_string(),
                ..ReviewCard::default()
            },
            diff: Some(diff),
            checks,
            chosen: Some(choice.clone()),
            ..Loop::default()
        };
        // A reviewer of an earlier loop is reused: its own history is not the
        // writer's conversation. A changed choice starts a new reviewer; within
        // one loop the reviewer stays, as it remembers its earlier findings.
        if let Some(old) = state
            .loops
            .get(task)
            .filter(|old| old.chosen.as_ref() == Some(&choice))
        {
            l.card.reviewer_task_id = old.card.reviewer_task_id.clone();
            l.card.reviewer_label = old.card.reviewer_label.clone();
            l.card.same_family = old.card.same_family;
        }
        state.loops.insert(task.to_string(), l);
    }

    /// Start or prompt the reviewer for the loop's current round.
    fn review_act(&self, task: &str, l: &Loop) -> Act {
        let diff = l.diff.clone().unwrap_or_default();
        let (acceptance, constraints) = self.context(task);
        let text = diff_text(&diff);
        let prompt = prompt::reviewer(
            &diff,
            &prompt::Context {
                round: l.card.round,
                acceptance: &acceptance,
                constraints: &constraints,
                checks: l.checks.as_deref(),
                diff_text: text.as_deref(),
                previous: l.previous.as_ref(),
            },
        );
        match &l.card.reviewer_task_id {
            Some(reviewer) => Act::PromptReviewer {
                parent: task.to_string(),
                reviewer: reviewer.clone(),
                prompt,
            },
            None => Act::Start {
                parent: task.to_string(),
                prompt,
            },
        }
    }

    /// The writer's answering turn is read and its diff known: decide.
    fn resolve(&self, state: &mut State, task: &str) -> Option<Act> {
        let l = state.loops.get_mut(task)?;
        if !l.answered {
            return None;
        }
        let diff = l.answer_diff.clone()?;
        let adopted = l
            .card
            .findings
            .iter()
            .filter(|f| f.decision == Some(Decision::Adopt))
            .count();
        if l.card
            .findings
            .iter()
            .any(|f| f.decision == Some(Decision::Ask))
        {
            l.card.state = ReviewState::Escalated;
            return None;
        }
        match diff {
            Some(diff) if adopted > 0 => {
                if l.card.round >= MAX_ROUNDS {
                    l.card.state = ReviewState::Escalated;
                    l.card.raw = Some(format!("已审查 {MAX_ROUNDS} 轮仍有阻断问题，交给你决定"));
                    return None;
                }
                l.previous = Some(l.card.clone());
                l.card.round += 1;
                l.card.state = ReviewState::Reviewing;
                l.card.turn_id = l.answer_turn.clone();
                l.card.findings.clear();
                l.card.summary = None;
                l.card.raw = None;
                l.diff = Some(diff);
                l.replying = false;
                l.answered = false;
                l.answer_diff = None;
                if l.answer_checking {
                    l.awaiting_checks = Some(l.card.turn_id.clone());
                    l.checks = None;
                    return None;
                }
                Some(self.review_act(task, l))
            }
            _ => {
                l.card.state = ReviewState::Settled;
                None
            }
        }
    }

    fn context(&self, task: &str) -> (Vec<String>, Vec<String>) {
        let Some(cp) = self
            .checkpoints
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .and_then(|sink| sink.get(task))
        else {
            return (Vec::new(), Vec::new());
        };
        let acceptance = cp
            .obligations
            .iter()
            .filter(|o| o.source == ObligationSource::Acceptance)
            .map(|o| match &o.check {
                Some(check) => format!("{}（核对：`{check}`）", o.text),
                None => o.text.clone(),
            })
            .collect();
        let constraints = cp.constraints.iter().map(|c| c.text.clone()).collect();
        (acceptance, constraints)
    }

    fn ensure_loaded(&self, state: &mut State, task: &str) {
        if state.loops.contains_key(task) {
            return;
        }
        let Some(dir) = self.dir.lock().clone() else {
            return;
        };
        let Ok(Some(bytes)) = super::checkpoint::store::read_task_file(&dir, task) else {
            return;
        };
        let Ok(mut l) = serde_json::from_slice::<Loop>(&bytes) else {
            return;
        };
        // In memory nothing knew of it: the host restarted mid-round.
        if l.card.state.is_open() {
            l.card.state = ReviewState::Interrupted;
        }
        if let Some(reviewer) = &l.card.reviewer_task_id {
            state.reviewers.insert(reviewer.clone(), task.to_string());
        }
        state.loops.insert(task.to_string(), l);
    }

    /// Append the loop's card to the task's log and save it.
    fn publish(&self, task: &str) {
        let Some(l) = self.state.lock().loops.get(task).cloned() else {
            return;
        };
        if let Some(dir) = self.dir.lock().clone() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&l) {
                if let Err(error) = super::checkpoint::store::write_task_file(&dir, task, &bytes) {
                    eprintln!("review: saving {task} failed: {error}");
                }
            }
        }
        let registry = self.registry.lock().clone();
        if let Some(log) = registry.and_then(|r| r.log(task)) {
            let appended = log.push(SessionUpdateKind::Review { review: l.card }, now_ms());
            self.inner.update(&appended);
        }
    }

    fn run(&self, act: Act) {
        let Some(host) = self.host.lock().clone() else {
            return;
        };
        let me = self.me.lock().clone();
        std::thread::spawn(move || {
            let Some(this) = me.upgrade() else {
                return;
            };
            let (parent, outcome) = match act {
                Act::Start { parent, prompt } => match host.start_reviewer(&parent, prompt) {
                    Ok(reviewer) => {
                        let mut state = this.state.lock();
                        state
                            .reviewers
                            .insert(reviewer.task_id.clone(), parent.clone());
                        if let Some(l) = state.loops.get_mut(&parent) {
                            l.card.reviewer_task_id = Some(reviewer.task_id);
                            l.card.reviewer_label = reviewer.label;
                            l.card.same_family = reviewer.same_family;
                        }
                        (parent, Ok(()))
                    }
                    Err(e) => (parent, Err(e)),
                },
                Act::PromptReviewer {
                    parent,
                    reviewer,
                    prompt,
                } => {
                    this.state
                        .lock()
                        .reviewers
                        .insert(reviewer.clone(), parent.clone());
                    let result = host.prompt(&reviewer, prompt);
                    (parent, result)
                }
                Act::PromptWriter { parent, text } => {
                    let result = host.prompt(&parent, text);
                    (parent, result)
                }
            };
            if let Err(error) = outcome {
                if let Some(l) = this.state.lock().loops.get_mut(&parent) {
                    l.card.state = ReviewState::Failed;
                    l.card.raw = Some(cut_raw(&error));
                }
            }
            this.publish(&parent);
        });
    }

    fn on_update(&self, update: &SessionUpdate) -> Vec<Act> {
        let task = &update.task_id;
        let mut state = self.state.lock();
        match &update.kind {
            SessionUpdateKind::TurnStarted { .. } => {
                state.text.remove(task);
                if let Some(l) = state.loops.get_mut(task) {
                    if l.card.state == ReviewState::WaitingCoder {
                        l.replying = true;
                    }
                }
                Vec::new()
            }
            SessionUpdateKind::MessageChunk {
                message_id,
                role,
                text,
            } if role == "assistant" => {
                if tracks(&state, task) {
                    let messages = state.text.entry(task.clone()).or_default();
                    match messages.iter_mut().find(|(id, _)| id == message_id) {
                        Some((_, body)) => body.push_str(text),
                        None => messages.push((message_id.clone(), text.clone())),
                    }
                }
                Vec::new()
            }
            SessionUpdateKind::MessageCompleted {
                message_id,
                text: Some(text),
            } => {
                if tracks(&state, task) {
                    let messages = state.text.entry(task.clone()).or_default();
                    match messages.iter_mut().find(|(id, _)| id == message_id) {
                        Some((_, body)) => *body = text.clone(),
                        None => messages.push((message_id.clone(), text.clone())),
                    }
                }
                Vec::new()
            }
            SessionUpdateKind::AutoCheck {
                turn_id,
                state: run,
                items,
            } => {
                if run == "running" {
                    return Vec::new();
                }
                let summary = checks_summary(run, items);
                state
                    .checks_done
                    .insert(task.clone(), (turn_id.clone(), summary.clone()));
                let Some(l) = state.loops.get_mut(task) else {
                    return Vec::new();
                };
                if l.awaiting_checks.as_deref() != Some(turn_id.as_str()) {
                    return Vec::new();
                }
                l.awaiting_checks = None;
                l.checks = Some(summary);
                let act = self.review_act(task, l);
                drop(state);
                self.publish(task);
                vec![act]
            }
            SessionUpdateKind::TurnEnded { reason, error, .. } => {
                let text = state
                    .text
                    .remove(task)
                    .map(|m| m.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                if let Some(parent) = state.reviewers.get(task).cloned() {
                    let act =
                        self.reviewed(&mut state, &parent, task, *reason, error.as_deref(), &text);
                    drop(state);
                    self.publish(&parent);
                    return act.into_iter().collect();
                }
                let Some(l) = state.loops.get_mut(task) else {
                    return Vec::new();
                };
                if l.card.state != ReviewState::WaitingCoder || !l.replying || l.answered {
                    return Vec::new();
                }
                let answers = parse::reply(&text);
                for f in &mut l.card.findings {
                    match answers.iter().find(|(id, _, _)| *id == f.id) {
                        Some((_, decision, reason)) => {
                            f.decision = Some(*decision);
                            f.reason = reason.clone();
                        }
                        None => {
                            f.decision = Some(Decision::Ask);
                            f.reason = Some("没有答复".into());
                        }
                    }
                }
                l.answered = true;
                l.answer_turn = turn_of(update);
                let act = self.resolve(&mut state, task);
                drop(state);
                self.publish(task);
                act.into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// The reviewer's turn ended: read its verdict.
    fn reviewed(
        &self,
        state: &mut State,
        parent: &str,
        reviewer: &str,
        reason: TurnEndReason,
        error: Option<&str>,
        text: &str,
    ) -> Option<Act> {
        let paused = self
            .host
            .lock()
            .clone()
            .and_then(|h| h.settings(parent))
            .is_some_and(|s| s.paused);
        let l = state.loops.get_mut(parent)?;
        // A reviewer replaced by a changed choice may still finish a turn.
        if l.card.state != ReviewState::Reviewing
            || l.card
                .reviewer_task_id
                .as_deref()
                .is_some_and(|current| current != reviewer)
        {
            return None;
        }
        if reason != TurnEndReason::Completed {
            l.card.state = ReviewState::Failed;
            l.card.raw = Some(cut_raw(error.unwrap_or("审查会话没有正常结束")));
            return None;
        }
        let Some(verdict) = parse::verdict(text) else {
            l.card.state = ReviewState::Failed;
            l.card.raw = Some(if text.trim().is_empty() {
                "审查者没有给出回答".into()
            } else {
                cut_raw(text)
            });
            return None;
        };
        l.card.summary = verdict.summary;
        l.card.findings = verdict.findings;
        l.card.findings.sort_by_key(|f| f.severity);
        if verdict.pass {
            l.card.state = ReviewState::Passed;
            return None;
        }
        if paused {
            l.card.state = ReviewState::WaitingSend;
            return None;
        }
        Some(to_writer(parent, l))
    }
}

fn to_writer(parent: &str, l: &mut Loop) -> Act {
    l.card.state = ReviewState::WaitingCoder;
    l.replying = false;
    l.answered = false;
    l.answer_diff = None;
    Act::PromptWriter {
        parent: parent.to_string(),
        text: prompt::reply_request(&l.card),
    }
}

fn tracks(state: &State, task: &str) -> bool {
    state.reviewers.contains_key(task)
        || state
            .loops
            .get(task)
            .is_some_and(|l| l.card.state == ReviewState::WaitingCoder)
}

fn turn_of(update: &SessionUpdate) -> String {
    match &update.kind {
        SessionUpdateKind::TurnEnded { turn_id, .. } => turn_id.clone(),
        _ => String::new(),
    }
}

fn needs_user(card: &ReviewCard) -> bool {
    card.state == ReviewState::Escalated
        || (card.state == ReviewState::Settled
            && card
                .findings
                .iter()
                .any(|f| f.severity == Severity::Blocker && f.decision == Some(Decision::Reject)))
}

fn checks_summary(run: &str, items: &[super::update::AutoCheckItem]) -> String {
    let word = match run {
        "passed" => "全部通过",
        "failed" => "有失败",
        "skipped" => "没有可运行的检查",
        _ => "已取消",
    };
    let mut out = word.to_string();
    for item in items {
        out.push_str(&format!("\n- {}：{}", item.label, item.summary));
        for failure in item.failures.iter().take(5) {
            out.push_str(&format!("\n  - {failure}"));
        }
    }
    out
}

fn diff_text(diff: &TurnDiff) -> Option<String> {
    if diff.before.is_empty() {
        return None;
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&diff.top)
        .args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            &diff.before,
            &diff.after,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    (text.len() <= prompt::MAX_INLINE_DIFF).then(|| text.trim_end().to_string())
}

fn cut_raw(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_RAW_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_RAW_CHARS).collect();
    out.push('…');
    out
}

impl LiveStatusSink for ReviewLoop {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.inner
            .status(project_id, task_id, generation, status, reason);
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
        for act in self.on_update(update) {
            self.run(act);
        }
    }
}

#[cfg(test)]
mod tests;
