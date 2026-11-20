//! B5-2 (2026-10-31): one extraction, start to finish — checks, the call,
//! what it wrote, and the 整理记录 row. The host brings the conversation
//! and the call (the analysis model over the user's connection), so this is
//! tested with a fake model.

use std::collections::HashSet;

use aeroric_protocol::memory::{
    HistoryCandidate, HistoryPlan, MemoryDraft, MemoryItem, MemoryKind, MemoryRun, MemoryRunKind,
    MemoryRunState, RunOutcome, RunOutcomeKind, RunOutcomeSubject, EXTRACT_MAX_CHARS,
};

use super::extract::{self, Transcript};
use super::runs::{PendingWrite, Retry};
use super::{evidence, Memory, MemoryError};
use crate::settings::model_detect::ChatOnce;

/// R1-1: how one extraction's writes went. A refusal (forgotten, looks like a
/// key, malformed) is the store doing its job; anything else lost data.
/// C3: each write leaves an outcome.
#[derive(Default)]
struct Writes {
    written: u32,
    refused: u32,
    failed: u32,
    canceled: u32,
    first_failure: Option<String>,
    outcomes: Vec<RunOutcome>,
    /// C3 second step: the failed writes, whole, by their outcome's place.
    retry: Vec<(usize, PendingWrite)>,
}

impl Writes {
    fn note(
        &mut self,
        subject: RunOutcomeSubject,
        sig: String,
        result: Result<MemoryItem, MemoryError>,
    ) -> Option<MemoryItem> {
        let (outcome, item_id, reason, item) = match result {
            Ok(item) => {
                self.written += 1;
                // The same words already kept gain this evidence instead.
                let kind = if item.evidence.len() > 1 {
                    RunOutcomeKind::Merged
                } else {
                    RunOutcomeKind::Written
                };
                (kind, Some(item.id.clone()), None, Some(item))
            }
            Err(
                error @ (MemoryError::Empty
                | MemoryError::TooLong
                | MemoryError::Secret(_)
                | MemoryError::Forgotten
                | MemoryError::WrongState),
            ) => {
                self.refused += 1;
                (RunOutcomeKind::Refused, None, Some(error.to_string()), None)
            }
            Err(error) => {
                self.failed += 1;
                self.first_failure.get_or_insert(error.to_string());
                (RunOutcomeKind::Failed, None, Some(error.to_string()), None)
            }
        };
        self.outcomes.push(RunOutcome {
            subject,
            outcome,
            sig,
            item_id,
            reason,
        });
        item
    }

    /// Not written: the user canceled first.
    fn cancel(&mut self, subject: RunOutcomeSubject, sig: String) {
        self.canceled += 1;
        self.outcomes.push(RunOutcome {
            subject,
            outcome: RunOutcomeKind::Canceled,
            sig,
            item_id: None,
            reason: None,
        });
    }
}

/// What the host knows about the session being extracted.
pub struct Session<'a> {
    pub task_id: &'a str,
    pub project_id: &'a str,
    pub title: &'a str,
    /// The conversation's text blocks, `(role, text)`.
    pub messages: &'a [(String, String)],
    /// The user asked (会话菜单 / 重试): one user message is enough, and an
    /// already extracted conversation is done again.
    pub forced: bool,
    /// B5-3a: its acceptance criteria all passed (experiences start at 中).
    pub verified: bool,
    /// C3: the `Queued` row the automatic pass made for it, if any.
    pub run_id: Option<&'a str>,
}

/// Why nothing was sent, as the 整理记录 row says it.
fn skip_reason(memory: &Memory, session: &Session<'_>, transcript: &Transcript) -> Option<String> {
    if !memory.project_enabled(session.project_id) {
        return Some("项目没有开启记忆".into());
    }
    if session.messages.is_empty() {
        return Some("读不到这个会话的对话".into());
    }
    let needed = if session.forced { 1 } else { 2 };
    if transcript.user_turns < needed {
        return Some(format!("你只说了 {} 句，不整理", transcript.user_turns));
    }
    None
}

/// The local day's start, for the daily limit.
pub fn day_start_ms(now_ms: u64) -> u64 {
    use chrono::{Local, TimeZone};
    let Some(now) = Local.timestamp_millis_opt(now_ms as i64).single() else {
        return now_ms - now_ms % 86_400_000;
    };
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map(|midnight| midnight.timestamp_millis() as u64)
        .unwrap_or(now_ms - now_ms % 86_400_000)
}

/// Q4: an imported session as 整理历史会话 plans it — its file size stands
/// in for its text, which is not read to plan.
pub struct HistorySource {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub last_at_ms: u64,
    pub bytes: u64,
}

/// The instructions and what is kept, sent with every extraction.
const PROMPT_TOKENS: u64 = 1_500;
/// A reply is a clue and a few candidates.
const REPLY_TOKENS: u64 = 400;

/// Rough tokens for one session: a transcript file is mostly JSON around the
/// text (about a quarter is text), the text is cut at `EXTRACT_MAX_CHARS`,
/// and a character is about half a token.
pub fn history_tokens(bytes: u64) -> (u64, u64) {
    let chars = (bytes / 4).min(EXTRACT_MAX_CHARS as u64);
    (chars / 2 + PROMPT_TOKENS, REPLY_TOKENS)
}

impl Memory {
    /// Q4: what extracting these sessions would cost; already extracted
    /// ones are listed as done and left out of the sums.
    pub fn history_plan(
        &self,
        project_id: &str,
        sources: &[HistorySource],
        now_ms: u64,
    ) -> HistoryPlan {
        let settings = self.settings();
        let model = settings
            .model
            .clone()
            .filter(|_| settings.connection_id.is_some());
        let mut plan = HistoryPlan {
            enabled: self.project_enabled(project_id),
            remaining_today: settings
                .daily_limit
                .saturating_sub(self.counted_since(day_start_ms(now_ms))),
            model: model.clone(),
            ..HistoryPlan::default()
        };
        for source in sources {
            let done = self.extracted(&source.id).is_some();
            if !done {
                let (input, output) = history_tokens(source.bytes);
                plan.pending += 1;
                plan.input_tokens += input;
                plan.output_tokens += output;
            }
            plan.sessions.push(HistoryCandidate {
                id: source.id.clone(),
                title: source.title.clone(),
                agent: source.agent.clone(),
                last_at_ms: source.last_at_ms,
                done,
            });
        }
        plan.sessions
            .sort_by(|a, b| b.last_at_ms.cmp(&a.last_at_ms));
        plan.cost_usd = model.map(|model| {
            crate::metrics::analytics::estimate_cost_usd(
                &model,
                plan.input_tokens,
                plan.output_tokens,
            )
            .0
        });
        plan
    }

    /// Extract one session. `Err` means nothing was done and nothing is
    /// recorded (no model chosen, the day's limit reached, already done);
    /// otherwise the row it recorded.
    pub fn extract_session(
        &self,
        session: Session<'_>,
        home: Option<&std::path::Path>,
        call: impl FnOnce(&str, &str) -> Result<ChatOnce, String>,
        now_ms: u64,
    ) -> Result<MemoryRun, String> {
        let settings = self.settings();
        let model = settings
            .model
            .clone()
            .filter(|_| settings.connection_id.is_some())
            .ok_or("还没有选分析模型：在记忆页的“整理设置”里选一个")?;
        let transcript = extract::transcript(session.messages, home);
        if !session.forced
            && self.extracted(session.task_id).as_deref() == Some(transcript.hash.as_str())
        {
            return Err("这份对话已经整理过".into());
        }
        let mut run = super::runs::new_run(
            MemoryRunKind::Extract,
            session.task_id,
            session.project_id,
            session.title,
            MemoryRunState::Skipped,
            now_ms,
        );
        if let Some(id) = session.run_id {
            run.id = id.to_string();
        }
        if let Some(reason) = skip_reason(self, &session, &transcript) {
            run.reason = Some(reason);
            self.record(&run, session.task_id, &transcript.hash, now_ms)?;
            return Ok(run);
        }
        let used = self.counted_since(day_start_ms(now_ms));
        if used >= settings.daily_limit {
            return Err(format!("今天已经整理了 {used} 次，到上限了，明天再整理"));
        }
        // C3: from here the row says 进行中 until the run ends.
        self.begin_run(&mut run).map_err(|e| e.to_string())?;
        let result = self.extract_running(&session, &transcript, &model, &mut run, call, now_ms);
        self.end_run(&run.id);
        if let Err(error) = result {
            // The row must not stay 进行中.
            run.state = MemoryRunState::Failed;
            run.reason = Some(error.clone());
            let _ = self.add_run(&run);
            return Err(error);
        }
        Ok(run)
    }

    /// The run once its row says `Running`: the call, the writes, the row.
    fn extract_running(
        &self,
        session: &Session<'_>,
        transcript: &Transcript,
        model: &str,
        run: &mut MemoryRun,
        call: impl FnOnce(&str, &str) -> Result<ChatOnce, String>,
        now_ms: u64,
    ) -> Result<(), String> {
        let kept = self.kept(session.project_id);
        let ids: HashSet<String> = kept.iter().map(|item| item.id.clone()).collect();
        let prompt = extract::prompt(transcript, &kept);
        run.model = Some(model.to_string());
        // C2: what is deleted, archived or replaced while the model works
        // is not written back.
        let epoch = self.epoch();
        let reply = match call(extract::SYSTEM, &prompt) {
            Ok(reply) => reply,
            Err(error) => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(format!("调用分析模型失败：{error}"));
                return self.record(run, session.task_id, &transcript.hash, now_ms);
            }
        };
        run.input_tokens = reply.input_tokens;
        run.output_tokens = reply.output_tokens;
        run.cost_usd = (reply.input_tokens + reply.output_tokens > 0).then(|| {
            crate::metrics::analytics::estimate_cost_usd(
                model,
                reply.input_tokens,
                reply.output_tokens,
            )
            .0
        });
        let mut extracted = match extract::parse(&reply.text, transcript, &ids) {
            Ok(extracted) => extracted,
            Err(error) => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(error);
                return self.record(run, session.task_id, &transcript.hash, now_ms);
            }
        };
        let withdrawn = self
            .drop_withdrawn(epoch, &mut extracted)
            .map_err(|e| e.to_string())?;
        let mut writes = Writes::default();
        for (subject, text) in &withdrawn {
            writes.outcomes.push(RunOutcome {
                subject: *subject,
                outcome: RunOutcomeKind::Withdrawn,
                sig: self.sig(text),
                item_id: None,
                reason: None,
            });
        }
        let run_id = run.id.clone();
        let canceled = || self.cancel_requested(&run_id);
        let confidence = if session.verified {
            aeroric_protocol::memory::Confidence::Medium
        } else {
            aeroric_protocol::memory::Confidence::Low
        };
        let analysis = |excerpt: &str| {
            evidence(
                Some(session.task_id),
                Some("analysis"),
                Some(model),
                excerpt,
                now_ms,
            )
        };
        // C3 second step: every write whole first, so one the store fails
        // can be made again later without the model.
        let mut todo: Vec<PendingWrite> = Vec::new();
        if let Some(fact) = &extracted.fact {
            todo.push(PendingWrite::Fact {
                project_id: session.project_id.to_string(),
                text: fact.to_string(),
                evidence: analysis(fact),
            });
        }
        for (title, detail) in &extracted.experiences {
            todo.push(PendingWrite::Experience {
                project_id: session.project_id.to_string(),
                title: title.clone(),
                detail: detail.clone(),
                confidence,
                evidence: analysis(&transcript.text),
            });
        }
        for candidate in extracted.candidates.drain(..) {
            // About the user: every project; a project's convention: this one.
            let project_id =
                (candidate.kind == MemoryKind::Fact).then(|| session.project_id.to_string());
            let mut proof = analysis(&candidate.quote);
            // C1: the message the user said it in, when one message holds it.
            proof.location = super::locate::locate(session.messages, &candidate.quote);
            todo.push(PendingWrite::Candidate {
                draft: MemoryDraft {
                    project_id,
                    kind: candidate.kind,
                    category: candidate.category,
                    text: candidate.text,
                    supersedes: candidate.supersedes,
                },
                evidence: proof,
            });
        }
        for write in todo {
            let sig = self.sig(write.text());
            if canceled() {
                writes.cancel(write.subject(), sig);
                continue;
            }
            self.write_one(run, &mut writes, write, sig, now_ms);
        }
        let mut notes = Vec::new();
        let was_canceled = writes.canceled > 0 || canceled();
        if was_canceled {
            notes.push(if writes.written > 0 {
                format!(
                    "已取消：取消前写入的 {} 条保留，没有撤回；其余 {} 条没有写入",
                    writes.written, writes.canceled
                )
            } else {
                "已取消：模型的回复没有写入".into()
            });
        }
        // C3 second step: the failed writes wait, whole, to be made again
        // without the model (not when the user canceled).
        let saved = writes.failed > 0
            && !was_canceled
            && self
                .save_retry(
                    &run.id,
                    &Retry {
                        epoch,
                        writes: std::mem::take(&mut writes.retry),
                    },
                    now_ms,
                )
                .is_ok();
        if writes.failed > 0 {
            notes.push(format!(
                "{} 条没写进去，{}：{}",
                writes.failed,
                if saved {
                    "会自动重试，不再调用模型"
                } else {
                    "下次整理时重试"
                },
                writes.first_failure.as_deref().unwrap_or("存储出错")
            ));
        }
        if extracted.dropped > 0 {
            notes.push(format!("丢掉 {} 条没有原话依据的候选", extracted.dropped));
        }
        if !withdrawn.is_empty() {
            notes.push(format!(
                "整理期间有 {} 条相同的记忆被删除、归档或取代，没有写回",
                withdrawn.len()
            ));
        }
        if writes.refused > 0 {
            notes.push(format!(
                "{} 条被记忆规则拦下（已遗忘或疑似密钥）",
                writes.refused
            ));
        }
        run.reason = (!notes.is_empty()).then(|| notes.join("；"));
        run.failed_writes = writes.failed;
        run.outcomes = std::mem::take(&mut writes.outcomes);
        if was_canceled {
            // The session is marked: the automatic pass leaves it; the
            // user can still 整理这次会话.
            run.state = MemoryRunState::Canceled;
            return self.record(run, session.task_id, &transcript.hash, now_ms);
        }
        if writes.failed > 0 {
            run.state = if writes.written > 0 {
                MemoryRunState::Partial
            } else {
                MemoryRunState::Failed
            };
            if saved {
                // The session is done with the model; the retry writes
                // the rest.
                return self.record(run, session.task_id, &transcript.hash, now_ms);
            }
            // The transcript stays unmarked so the next trigger writes the
            // rest; `insert` merges what is already there.
            return self.add_run(run).map_err(|e| e.to_string());
        }
        run.state = MemoryRunState::Done;
        self.record(run, session.task_id, &transcript.hash, now_ms)
    }

    /// One write and its outcome; a failed one is kept to be tried again.
    fn write_one(
        &self,
        run: &mut MemoryRun,
        writes: &mut Writes,
        write: PendingWrite,
        sig: String,
        now_ms: u64,
    ) {
        let subject = write.subject();
        let index = writes.outcomes.len();
        if let Some(item) = writes.note(subject, sig, write.apply(self, now_ms)) {
            match subject {
                RunOutcomeSubject::Fact => run.facts = 1,
                RunOutcomeSubject::Experience => run.experiences += 1,
                RunOutcomeSubject::Candidate => {
                    if item.state == aeroric_protocol::memory::MemoryState::Pending {
                        run.candidates += 1;
                    }
                }
            }
        }
        if writes.outcomes[index].outcome == RunOutcomeKind::Failed {
            writes.retry.push((index, write));
        }
    }

    /// C3 second step: make the writes a run could not, without asking the
    /// model again. `Ok(None)` when none wait (a failed call, or kept past
    /// the retention): the caller extracts the session again instead.
    pub fn retry_run(&self, run_id: &str, now_ms: u64) -> Result<Option<MemoryRun>, MemoryError> {
        let mut run = self.run(run_id).ok_or(MemoryError::UnknownItem)?;
        if !matches!(run.state, MemoryRunState::Partial | MemoryRunState::Failed) {
            return Err(MemoryError::WrongState);
        }
        let Some(retry) = self.load_retry(run_id)? else {
            return Ok(None);
        };
        let texts: Vec<&str> = retry.writes.iter().map(|(_, w)| w.text()).collect();
        let gone = self.withdrawn_since(retry.epoch, &texts)?;
        let waiting: HashSet<usize> = retry.writes.iter().map(|(index, _)| *index).collect();
        // A failed write no longer waiting was forgotten meanwhile.
        for (index, outcome) in run.outcomes.iter_mut().enumerate() {
            if outcome.outcome == RunOutcomeKind::Failed && !waiting.contains(&index) {
                outcome.outcome = RunOutcomeKind::Refused;
                outcome.reason = Some(MemoryError::Forgotten.to_string());
            }
        }
        let mut writes = Writes::default();
        let mut withdrawn = 0;
        for ((index, write), gone) in retry.writes.into_iter().zip(gone) {
            let sig = run
                .outcomes
                .get(index)
                .map(|o| o.sig.clone())
                .unwrap_or_else(|| self.sig(write.text()));
            let outcome = if gone {
                withdrawn += 1;
                RunOutcome {
                    subject: write.subject(),
                    outcome: RunOutcomeKind::Withdrawn,
                    sig,
                    item_id: None,
                    reason: None,
                }
            } else {
                let waiting = writes.retry.len();
                self.write_one(&mut run, &mut writes, write, sig, now_ms);
                let outcome = writes.outcomes.pop().expect("one outcome per write");
                // Failed again: it keeps its place in the run's outcomes.
                if writes.retry.len() > waiting {
                    if let Some((_, write)) = writes.retry.pop() {
                        writes.retry.push((index, write));
                    }
                }
                outcome
            };
            match run.outcomes.get_mut(index) {
                Some(slot) => *slot = outcome,
                None => run.outcomes.push(outcome),
            }
        }
        let left = writes.retry;
        let mut notes: Vec<String> = run
            .reason
            .as_deref()
            .unwrap_or_default()
            .split('；')
            .filter(|note| !note.is_empty() && !note.contains("没写进去"))
            .map(str::to_string)
            .collect();
        if writes.written > 0 {
            notes.push(format!("重试补写了 {} 条", writes.written));
        }
        if withdrawn > 0 {
            notes.push(format!(
                "重试前有 {withdrawn} 条相同的记忆被删除、归档或取代，没有写"
            ));
        }
        if !left.is_empty() {
            notes.push(format!(
                "{} 条还是没写进去，会再自动重试：{}",
                left.len(),
                writes.first_failure.as_deref().unwrap_or("存储出错")
            ));
        }
        run.reason = (!notes.is_empty()).then(|| notes.join("；"));
        run.failed_writes = left.len() as u32;
        run.state = if left.is_empty() {
            MemoryRunState::Done
        } else if run
            .outcomes
            .iter()
            .any(|o| matches!(o.outcome, RunOutcomeKind::Written | RunOutcomeKind::Merged))
        {
            MemoryRunState::Partial
        } else {
            MemoryRunState::Failed
        };
        if left.is_empty() {
            self.drop_retry(run_id)?;
        } else {
            self.save_retry(
                run_id,
                &Retry {
                    epoch: retry.epoch,
                    writes: left,
                },
                now_ms,
            )?;
        }
        self.add_run(&run)?;
        Ok(Some(run))
    }

    /// C3: a text's signature as a run outcome carries it (the keyed one
    /// forgetting keeps, shortened).
    fn sig(&self, text: &str) -> String {
        self.with(|store| Ok(store.signature(text)))
            .unwrap_or_default()
            .chars()
            .take(16)
            .collect()
    }

    /// C2: candidates and experiences whose words were withdrawn after
    /// `epoch` leave `extracted`; C3: which they were.
    fn drop_withdrawn(
        &self,
        epoch: u64,
        extracted: &mut extract::Extracted,
    ) -> Result<Vec<(RunOutcomeSubject, String)>, MemoryError> {
        let texts: Vec<&str> = extracted
            .candidates
            .iter()
            .map(|c| c.text.as_str())
            .chain(
                extracted
                    .experiences
                    .iter()
                    .map(|(title, _)| title.as_str()),
            )
            .collect();
        let gone = self.withdrawn_since(epoch, &texts)?;
        let (candidates, experiences) = gone.split_at(extracted.candidates.len());
        let mut dropped = Vec::new();
        let mut flags = candidates.iter();
        extracted.candidates.retain(|c| {
            let gone = flags.next().copied().unwrap_or(false);
            if gone {
                dropped.push((RunOutcomeSubject::Candidate, c.text.clone()));
            }
            !gone
        });
        let mut flags = experiences.iter();
        extracted.experiences.retain(|(title, _)| {
            let gone = flags.next().copied().unwrap_or(false);
            if gone {
                dropped.push((RunOutcomeSubject::Experience, title.clone()));
            }
            !gone
        });
        Ok(dropped)
    }

    fn record(
        &self,
        run: &MemoryRun,
        task_id: &str,
        hash: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        self.finish_run(run, task_id, hash, now_ms)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use aeroric_protocol::memory::{MemoryKind, MemoryRunState, MemorySettings, MemoryState};

    use super::super::{evidence, Memory};
    use super::Session;
    use crate::settings::model_detect::ChatOnce;

    fn rig() -> Memory {
        let dir = std::env::temp_dir().join(format!("aeroric-extract-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(dir).unwrap();
        memory.set_project_enabled("p1", true).unwrap();
        memory
            .set_settings(&MemorySettings {
                auto: true,
                connection_id: Some("c1".into()),
                model: Some("deepseek-v4.1-flash".into()),
                ..MemorySettings::default()
            })
            .unwrap();
        memory
    }

    fn source(id: &str, bytes: u64, at: u64) -> super::HistorySource {
        super::HistorySource {
            id: id.into(),
            title: id.into(),
            agent: "claude".into(),
            last_at_ms: at,
            bytes,
        }
    }

    /// Q4: the plan sums what is not done yet, newest first, and says how
    /// many runs are left today.
    #[test]
    fn a_history_plan_counts_only_what_is_not_extracted() {
        let memory = rig();
        let now = 1_800_000_000_000;
        let talk = talk();
        memory
            .extract_session(
                session(&talk, "p1", false),
                None,
                |_, _| {
                    Ok(ChatOnce {
                        text: REPLY.into(),
                        input_tokens: 10,
                        output_tokens: 5,
                    })
                },
                now,
            )
            .unwrap();
        let plan = memory.history_plan(
            "p1",
            &[source("t1", 4_000, 1), source("old", 400_000, 2)],
            now,
        );
        assert!(plan.enabled);
        assert_eq!(plan.pending, 1);
        assert_eq!(plan.sessions[0].id, "old");
        assert!(plan.sessions[1].done);
        // 400 KB is cut at the extraction cap.
        assert_eq!(
            plan.input_tokens,
            aeroric_protocol::memory::EXTRACT_MAX_CHARS as u64 / 2 + 1_500
        );
        assert_eq!(plan.output_tokens, 400);
        assert!(plan.cost_usd.is_some());
        assert_eq!(
            plan.remaining_today,
            MemorySettings::default().daily_limit - 1
        );
        assert!(!memory.history_plan("p2", &[], now).enabled);
    }

    /// R1-1: a store that loses one kind of write reports it and retries.
    /// C3 second step: the retry makes only that write, without the model,
    /// and the run's row becomes whole.
    #[test]
    fn a_failed_write_is_partial_and_the_retry_writes_only_it() {
        let memory = rig();
        let messages = talk();
        let reply = || {
            Ok(ChatOnce {
                text: REPLY.into(),
                input_tokens: 1,
                output_tokens: 1,
            })
        };
        memory.execute_for_test(
            "CREATE TRIGGER boom BEFORE INSERT ON items WHEN NEW.kind = 'experience'
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        );
        let first = memory
            .extract_session(session(&messages, "p1", false), None, |_, _| reply(), 1)
            .unwrap();
        assert_eq!(first.state, MemoryRunState::Partial);
        assert_eq!(
            (first.facts, first.experiences, first.failed_writes),
            (1, 0, 1)
        );
        assert!(first
            .reason
            .as_deref()
            .unwrap()
            .contains("1 条没写进去，会自动重试，不再调用模型："));
        assert_eq!(
            first.outcomes[1].outcome,
            aeroric_protocol::memory::RunOutcomeKind::Failed
        );
        assert!(
            memory.extracted("t1").is_some(),
            "the model is not asked again"
        );
        assert_eq!(memory.retryable(2), vec![first.id.clone()]);

        // Still failing: it waits again, in its place.
        let again = memory.retry_run(&first.id, 2).unwrap().unwrap();
        assert_eq!(again.state, MemoryRunState::Partial);
        assert!(again
            .reason
            .as_deref()
            .unwrap()
            .contains("1 条还是没写进去，会再自动重试"));
        assert_eq!(memory.retryable(2), vec![first.id.clone()]);

        memory.execute_for_test("DROP TRIGGER boom;");
        let before =
            memory.list(None, None).unwrap().len() + memory.list(Some("p1"), None).unwrap().len();
        let done = memory.retry_run(&first.id, 3).unwrap().unwrap();
        assert_eq!(done.id, first.id, "the same row");
        assert_eq!(done.state, MemoryRunState::Done);
        assert_eq!(
            (done.facts, done.experiences, done.failed_writes),
            (1, 1, 0)
        );
        assert_eq!(
            done.outcomes[1].outcome,
            aeroric_protocol::memory::RunOutcomeKind::Written
        );
        assert_eq!(done.outcomes[1].sig, first.outcomes[1].sig);
        assert!(done.reason.as_deref().unwrap().contains("重试补写了 1 条"));
        assert!(!done.reason.as_deref().unwrap().contains("没写进去"));
        let after =
            memory.list(None, None).unwrap().len() + memory.list(Some("p1"), None).unwrap().len();
        assert_eq!(
            after,
            before + 1,
            "only the experience, nothing written twice"
        );
        assert!(memory.retryable(4).is_empty());
        assert_eq!(memory.runs(10).unwrap(), vec![done.clone()]);
        assert!(matches!(
            memory.retry_run(&first.id, 5),
            Err(super::super::MemoryError::WrongState)
        ));
    }

    /// C3 second step: a saved write whose words were forgotten meanwhile
    /// leaves the disk at once; one withdrawn meanwhile is not written.
    #[test]
    fn a_waiting_write_follows_forgetting_and_withdrawal() {
        use aeroric_protocol::memory::RunOutcomeKind;
        let memory = rig();
        let messages = talk();
        memory.execute_for_test(
            "CREATE TRIGGER boom BEFORE INSERT ON items WHEN NEW.state = 'pending'
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        );
        let run = memory
            .extract_session(session(&messages, "p1", false), None, talk_reply, 1)
            .unwrap();
        assert_eq!(run.failed_writes, 2, "both candidates");
        memory.execute_for_test("DROP TRIGGER boom;");
        // One is said by the user and forgotten; the other is said and
        // archived.
        let draft = |text: &str, kind| aeroric_protocol::memory::MemoryDraft {
            project_id: (kind == MemoryKind::Fact).then(|| "p1".to_string()),
            kind,
            category: None,
            text: text.into(),
            supersedes: None,
        };
        let said = memory
            .remember(
                draft("回复用简体中文", MemoryKind::Profile),
                evidence(None, None, None, "/记住", 2),
                2,
            )
            .unwrap();
        memory.forget(&said.id, 2).unwrap();
        let body = memory.load_retry(&run.id).unwrap().unwrap();
        assert_eq!(body.writes.len(), 1, "the forgotten words left the disk");
        let other = memory
            .remember(
                draft("发布前跑 native:check sessions", MemoryKind::Fact),
                evidence(None, None, None, "/记住", 3),
                3,
            )
            .unwrap();
        memory.archive(&other.id, true, 3).unwrap();
        let done = memory.retry_run(&run.id, 4).unwrap().unwrap();
        assert_eq!(done.state, MemoryRunState::Done);
        assert_eq!(done.outcomes[3].outcome, RunOutcomeKind::Withdrawn);
        assert_eq!(
            done.outcomes[2].outcome,
            RunOutcomeKind::Refused,
            "forgotten, not retried"
        );
        assert!(memory.attention().unwrap().is_empty(), "nothing came back");
    }

    /// R1-1: nothing reached the store: failed, not done.
    #[test]
    fn every_write_failing_is_a_failed_run() {
        let memory = rig();
        let messages = talk();
        memory.execute_for_test(
            "CREATE TRIGGER boom BEFORE INSERT ON items
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        );
        let run = memory
            .extract_session(
                session(&messages, "p1", false),
                None,
                |_, _| {
                    Ok(ChatOnce {
                        text: REPLY.into(),
                        input_tokens: 1,
                        output_tokens: 1,
                    })
                },
                1,
            )
            .unwrap();
        assert_eq!(run.state, MemoryRunState::Failed);
        assert!(run.failed_writes > 0);
        // C3 second step: every write waits whole; the model is not asked
        // again.
        assert!(memory.extracted("t1").is_some());
        assert_eq!(
            memory.load_retry(&run.id).unwrap().unwrap().writes.len(),
            run.failed_writes as usize
        );
    }

    fn talk() -> Vec<(String, String)> {
        vec![
            (
                "user".into(),
                "以后回复都用简体中文。帮我修 SSH 超时，token sk-ant-abcdefgh123456".into(),
            ),
            (
                "assistant".into(),
                "改用 connect_timeout，测试通过。".into(),
            ),
            ("user".into(), "发布前先跑 native:check sessions".into()),
        ]
    }

    const REPLY: &str = r#"{"sessionFact": {"task": "修 SSH 超时", "approach": "改用 connect_timeout", "result": "测试通过", "files": ["ssh.rs"]},
      "experiences": [{"title": "SSH 超时改用 connect_timeout", "problem": "卡住", "solution": "connect_timeout", "searchKeys": ["ssh"], "envs": ["macOS"]}],
      "candidates": [{"kind": "profile", "category": "language", "text": "回复用简体中文", "quote": "以后回复都用简体中文"},
                     {"kind": "fact", "text": "发布前跑 native:check sessions", "quote": "发布前先跑 native:check sessions"},
                     {"kind": "fact", "text": "编的", "quote": "从来没说过"}]}"#;

    fn session<'a>(
        messages: &'a [(String, String)],
        project: &'a str,
        forced: bool,
    ) -> Session<'a> {
        Session {
            task_id: "t1",
            project_id: project,
            title: "修超时",
            messages,
            forced,
            verified: true,
            run_id: None,
        }
    }

    /// C2: what the user archives or replaces while the model works is not
    /// written back as a new candidate; the run says how many it held back.
    #[test]
    fn what_is_withdrawn_during_a_run_is_not_written_back() {
        let memory = rig();
        let talk = talk();
        let kept = memory
            .remember(
                aeroric_protocol::memory::MemoryDraft {
                    project_id: None,
                    kind: MemoryKind::Profile,
                    category: None,
                    text: "回复用简体中文".into(),
                    supersedes: None,
                },
                evidence(None, None, None, "/记住 回复用简体中文", 1),
                1,
            )
            .unwrap();
        let run = memory
            .extract_session(
                session(&talk, "p1", false),
                None,
                |_, _| {
                    // The user archives it while the model reads the session.
                    memory.archive(&kept.id, true, 2).unwrap();
                    Ok(ChatOnce {
                        text: REPLY.into(),
                        input_tokens: 10,
                        output_tokens: 10,
                    })
                },
                3,
            )
            .unwrap();
        assert_eq!(run.candidates, 1, "only the other candidate");
        assert!(run.outcomes.iter().any(|o| o.outcome
            == aeroric_protocol::memory::RunOutcomeKind::Withdrawn
            && o.subject == aeroric_protocol::memory::RunOutcomeSubject::Candidate));
        assert!(
            run.reason
                .as_deref()
                .unwrap()
                .contains("整理期间有 1 条相同的记忆被删除、归档或取代，没有写回"),
            "{:?}",
            run.reason
        );
        let same: Vec<_> = memory
            .list(None, None)
            .unwrap()
            .into_iter()
            .filter(|i| i.text == "回复用简体中文")
            .collect();
        assert_eq!(same.len(), 1, "no new pending copy");
        assert_eq!(same[0].state, MemoryState::Archived);
        // A later run, started after, may propose it again: the user may
        // have said it again.
        let again = memory
            .extract_session(
                session(&talk, "p1", true),
                None,
                |_, _| {
                    Ok(ChatOnce {
                        text: REPLY.into(),
                        input_tokens: 10,
                        output_tokens: 10,
                    })
                },
                4,
            )
            .unwrap();
        assert_eq!(again.candidates, 2);
    }

    #[test]
    fn a_finished_session_becomes_a_clue_and_pending_candidates() {
        let memory = rig();
        let talk = talk();
        let mut sent = String::new();
        let run = memory
            .extract_session(
                session(&talk, "p1", false),
                None,
                |system, user| {
                    sent = format!("{system}\n{user}");
                    Ok(ChatOnce {
                        text: REPLY.into(),
                        input_tokens: 4000,
                        output_tokens: 300,
                    })
                },
                1_000,
            )
            .unwrap();
        assert!(
            !sent.contains("sk-ant-abcdefgh123456"),
            "secrets are masked"
        );
        assert!(sent.contains("用户：以后回复都用简体中文"));
        assert_eq!(run.state, MemoryRunState::Done);
        assert_eq!((run.facts, run.candidates, run.experiences), (1, 2, 1));
        let learned: Vec<_> = memory
            .list(Some("p1"), None)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == MemoryKind::Experience)
            .collect();
        assert_eq!(learned.len(), 1);
        assert_eq!(
            learned[0].confidence,
            Some(aeroric_protocol::memory::Confidence::Medium),
            "the session verified its criteria"
        );
        assert_eq!(run.reason.as_deref(), Some("丢掉 1 条没有原话依据的候选"));
        assert!(run.cost_usd.unwrap() > 0.0);
        let pending = memory.attention().unwrap();
        assert_eq!(pending.len(), 2);
        let profile = pending
            .iter()
            .find(|i| i.kind == MemoryKind::Profile)
            .unwrap();
        assert_eq!(profile.project_id, None, "about the user: global");
        assert_eq!(profile.evidence[0].excerpt, "以后回复都用简体中文");
        // C1: the place it was said is kept, and read back from the store.
        let place = profile.evidence[0].location.clone().unwrap();
        assert_eq!(
            (
                place.message_index,
                place.role.as_str(),
                place.start,
                place.end
            ),
            (0, "user", 0, 10)
        );
        assert_eq!(
            place.message_hash,
            super::super::locate::message_hash(&talk[0].1)
        );
        let fact = pending.iter().find(|i| i.kind == MemoryKind::Fact).unwrap();
        assert_eq!(fact.evidence[0].location.as_ref().unwrap().message_index, 2);
        let clues: Vec<_> = memory
            .list(Some("p1"), None)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == MemoryKind::SessionFact)
            .collect();
        assert_eq!(clues.len(), 1);
        assert_eq!(clues[0].state, MemoryState::Active);
        assert_eq!(
            clues[0].evidence[0].location, None,
            "a summary has no place"
        );
        // Once only, unless asked again.
        let again = memory.extract_session(
            session(&talk, "p1", false),
            None,
            |_, _| unreachable!(),
            2_000,
        );
        assert_eq!(again.unwrap_err(), "这份对话已经整理过");
        assert_eq!(memory.runs(10).unwrap().len(), 1);
    }

    #[test]
    fn skips_failures_and_the_daily_limit_are_recorded_honestly() {
        let memory = rig();
        let talk = talk();
        let skipped = memory
            .extract_session(session(&talk, "p2", false), None, |_, _| unreachable!(), 1)
            .unwrap();
        assert_eq!(skipped.state, MemoryRunState::Skipped);
        assert_eq!(skipped.reason.as_deref(), Some("项目没有开启记忆"));
        let one = vec![("user".into(), "就一句".into())];
        let short = memory
            .extract_session(session(&one, "p1", false), None, |_, _| unreachable!(), 2)
            .unwrap();
        assert_eq!(short.reason.as_deref(), Some("你只说了 1 句，不整理"));
        let failed = memory
            .extract_session(
                session(&talk, "p1", true),
                None,
                |_, _| Err("HTTP 500".into()),
                3,
            )
            .unwrap();
        assert_eq!(failed.state, MemoryRunState::Failed);
        let bad = memory
            .extract_session(
                session(&talk, "p1", true),
                None,
                |_, _| {
                    Ok(ChatOnce {
                        text: "不是 JSON".into(),
                        input_tokens: 1,
                        output_tokens: 1,
                    })
                },
                4,
            )
            .unwrap();
        assert_eq!(bad.reason.as_deref(), Some("模型没有返回 JSON"));
        let mut settings = memory.settings();
        settings.daily_limit = 2;
        memory.set_settings(&settings).unwrap();
        let full =
            memory.extract_session(session(&talk, "p1", true), None, |_, _| unreachable!(), 5);
        assert!(full.unwrap_err().contains("到上限"));
        settings.model = None;
        memory.set_settings(&settings).unwrap();
        assert!(memory
            .extract_session(session(&talk, "p1", true), None, |_, _| unreachable!(), 6)
            .unwrap_err()
            .contains("还没有选分析模型"));
        let _ = evidence(None, None, None, "", 0);
    }

    /// CU F1 (2026-10-31): extracting a session again replaces its clue.
    #[test]
    fn extracting_again_replaces_the_session_clue() {
        let memory = rig();
        let talk = talk();
        let reply = |result: &'static str| {
            move |_: &str, _: &str| {
                Ok(ChatOnce {
                    text: format!(
                        r#"{{"sessionFact": {{"task": "修 SSH 超时", "result": "{result}"}}}}"#
                    ),
                    input_tokens: 1,
                    output_tokens: 1,
                })
            }
        };
        memory
            .extract_session(session(&talk, "p1", true), None, reply("两轮：已修"), 1)
            .unwrap();
        memory
            .extract_session(
                session(&talk, "p1", true),
                None,
                reply("三轮：已修并定下约定"),
                2,
            )
            .unwrap();
        let clues: Vec<_> = memory
            .list(Some("p1"), None)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == MemoryKind::SessionFact)
            .collect();
        let active: Vec<_> = clues
            .iter()
            .filter(|i| i.state == MemoryState::Active)
            .collect();
        assert_eq!(active.len(), 1);
        assert!(active[0].text.contains("三轮"));
        let old = clues
            .iter()
            .find(|i| i.state == MemoryState::Superseded)
            .unwrap();
        assert_eq!(old.superseded_by.as_deref(), Some(active[0].id.as_str()));
    }

    fn talk_reply(_: &str, _: &str) -> Result<ChatOnce, String> {
        Ok(ChatOnce {
            text: REPLY.into(),
            input_tokens: 10,
            output_tokens: 5,
        })
    }

    /// C3: while the model works the row says 进行中; canceling then
    /// writes nothing, says so, keeps what the call cost, and marks the
    /// session so the automatic pass leaves it.
    #[test]
    fn a_run_is_running_while_the_model_works_and_cancel_stops_its_writes() {
        use aeroric_protocol::memory::{RunOutcomeKind, RunOutcomeSubject};
        let memory = rig();
        let talk = talk();
        let mut seen = None;
        let run = memory
            .extract_session(
                session(&talk, "p1", false),
                None,
                |system, user| {
                    let row = memory.runs(1).unwrap().remove(0);
                    assert_eq!(row.state, MemoryRunState::Running);
                    let asked = memory.cancel_run(&row.id, 2).unwrap();
                    assert_eq!(asked.state, MemoryRunState::Running, "it stops itself");
                    seen = Some(row.id);
                    talk_reply(system, user)
                },
                1,
            )
            .unwrap();
        assert_eq!(Some(run.id.clone()), seen);
        assert_eq!(run.state, MemoryRunState::Canceled);
        assert_eq!(
            run.reason.as_deref().unwrap().split('；').next(),
            Some("已取消：模型的回复没有写入")
        );
        assert_eq!((run.facts, run.candidates, run.experiences), (0, 0, 0));
        assert_eq!((run.input_tokens, run.output_tokens), (10, 5));
        let subjects: Vec<_> = run
            .outcomes
            .iter()
            .map(|o| (o.subject, o.outcome))
            .collect();
        assert_eq!(
            subjects,
            vec![
                (RunOutcomeSubject::Fact, RunOutcomeKind::Canceled),
                (RunOutcomeSubject::Experience, RunOutcomeKind::Canceled),
                (RunOutcomeSubject::Candidate, RunOutcomeKind::Canceled),
                (RunOutcomeSubject::Candidate, RunOutcomeKind::Canceled),
            ]
        );
        assert!(memory.list(None, None).unwrap().is_empty());
        assert!(memory.list(Some("p1"), None).unwrap().is_empty());
        assert!(memory.extracted("t1").is_some(), "the pass leaves it");
        assert_eq!(memory.counted_since(0), 1, "the call was made");
        assert_eq!(memory.runs(10).unwrap(), vec![run.clone()]);
        // An ended run cannot be canceled; the request is spent.
        assert!(matches!(
            memory.cancel_run(&run.id, 3),
            Err(super::super::MemoryError::WrongState)
        ));
        assert!(!memory.cancel_requested(&run.id));
        assert!(matches!(
            memory.cancel_run("run-nope", 3),
            Err(super::super::MemoryError::UnknownItem)
        ));
        // The user can still extract it by hand.
        let again = memory
            .extract_session(session(&talk, "p1", true), None, talk_reply, 4)
            .unwrap();
        assert_eq!(again.state, MemoryRunState::Done);
    }

    /// C3: every proposal says where it went — new, joined to the same
    /// words already kept, or held back by a rule — carrying no text.
    #[test]
    fn each_proposal_says_where_it_went() {
        use aeroric_protocol::memory::{MemoryDraft, RunOutcomeKind, RunOutcomeSubject};
        let memory = rig();
        let talk = talk();
        let draft = |text: &str, kind| MemoryDraft {
            project_id: (kind == MemoryKind::Fact).then(|| "p1".to_string()),
            kind,
            category: None,
            text: text.into(),
            supersedes: None,
        };
        let kept = memory
            .remember(
                draft("回复用简体中文", MemoryKind::Profile),
                evidence(None, None, None, "/记住 回复用简体中文", 1),
                1,
            )
            .unwrap();
        let gone = memory
            .remember(
                draft("发布前跑 native:check sessions", MemoryKind::Fact),
                evidence(None, None, None, "/记住", 1),
                1,
            )
            .unwrap();
        memory.forget(&gone.id, 1).unwrap();
        let run = memory
            .extract_session(session(&talk, "p1", false), None, talk_reply, 2)
            .unwrap();
        assert_eq!(run.state, MemoryRunState::Done);
        let got: Vec<_> = run
            .outcomes
            .iter()
            .map(|o| (o.subject, o.outcome, o.item_id.is_some()))
            .collect();
        assert_eq!(
            got,
            vec![
                (RunOutcomeSubject::Fact, RunOutcomeKind::Written, true),
                (RunOutcomeSubject::Experience, RunOutcomeKind::Written, true),
                (RunOutcomeSubject::Candidate, RunOutcomeKind::Merged, true),
                (RunOutcomeSubject::Candidate, RunOutcomeKind::Refused, false),
            ]
        );
        assert_eq!(run.outcomes[2].item_id.as_deref(), Some(kept.id.as_str()));
        assert!(run.outcomes[3].reason.is_some());
        for outcome in &run.outcomes {
            assert_eq!(outcome.sig.chars().count(), 16);
        }
        let body = serde_json::to_string(&run).unwrap();
        assert!(!body.contains("native:check"), "no text in the outcomes");
        assert_eq!(memory.runs(1).unwrap()[0].outcomes, run.outcomes);
    }

    /// C3: the automatic pass's queued row shows at once, uses up nothing,
    /// and canceling it marks the session so the pass leaves it.
    #[test]
    fn a_queued_run_is_shown_and_canceled_at_once() {
        let memory = rig();
        let id = memory.queue_run("t1", "p1", "修超时", 1).unwrap();
        assert!(memory.queued(&id));
        assert_eq!(memory.runs(10).unwrap()[0].state, MemoryRunState::Queued);
        assert_eq!(memory.counted_since(0), 0);
        let run = memory.cancel_run(&id, 2).unwrap();
        assert_eq!(run.state, MemoryRunState::Canceled);
        assert_eq!(
            run.reason.as_deref(),
            Some(super::super::runs::CANCELED_QUEUED)
        );
        assert!(!memory.queued(&id));
        assert_eq!(memory.extracted("t1").as_deref(), Some(""));
        assert_eq!(memory.counted_since(0), 0, "no model was asked");
        // A queued row the pass then runs becomes that run's row.
        let next = memory.queue_run("t2", "p1", "另一个", 3).unwrap();
        let talk = talk();
        let mut s = session(&talk, "p1", false);
        s.task_id = "t2";
        s.run_id = Some(&next);
        let ran = memory.extract_session(s, None, talk_reply, 4).unwrap();
        assert_eq!(ran.id, next);
        assert_eq!(ran.state, MemoryRunState::Done);
        assert_eq!(memory.runs(10).unwrap().len(), 2);
        // One the pass does not get to leaves the log.
        let left = memory.queue_run("t3", "p1", "第三个", 5).unwrap();
        memory.unqueue(&left);
        assert!(memory.run(&left).is_none());
        memory.unqueue(&next);
        assert!(memory.run(&next).is_some(), "only a queued row leaves");
    }

    /// C3: what was going when Aeroric stopped is failed and can be done
    /// again, not lost; what was only queued leaves the log.
    #[test]
    fn what_was_going_when_aeroric_stopped_fails_and_can_be_retried() {
        let dir = std::env::temp_dir().join(format!("aeroric-runs-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(dir.clone()).unwrap();
        memory.set_project_enabled("p1", true).unwrap();
        memory
            .set_settings(&MemorySettings {
                auto: true,
                connection_id: Some("c1".into()),
                model: Some("deepseek-v4.1-flash".into()),
                ..MemorySettings::default()
            })
            .unwrap();
        let queued = memory.queue_run("t9", "p1", "排着", 1).unwrap();
        let mut running = super::super::runs::new_run(
            aeroric_protocol::memory::MemoryRunKind::Extract,
            "t1",
            "p1",
            "修超时",
            MemoryRunState::Running,
            2,
        );
        running.model = Some("deepseek-v4.1-flash".into());
        memory.add_run(&running).unwrap();

        let reopened = Memory::default();
        reopened.set_dir(dir).unwrap();
        assert!(reopened.run(&queued).is_none());
        let failed = reopened.run(&running.id).unwrap();
        assert_eq!(failed.state, MemoryRunState::Failed);
        assert_eq!(
            failed.reason.as_deref(),
            Some(super::super::runs::INTERRUPTED)
        );
        assert_eq!(reopened.extracted("t1"), None, "the pass takes it up again");
        let talk = talk();
        let again = reopened
            .extract_session(session(&talk, "p1", false), None, talk_reply, 3)
            .unwrap();
        assert_eq!(again.state, MemoryRunState::Done);
    }
}
