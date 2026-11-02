//! B5-2 (2026-10-31): one extraction, start to finish — checks, the call,
//! what it wrote, and the 整理记录 row. The host brings the conversation
//! and the call (the analysis model over the user's connection), so this is
//! tested with a fake model.

use std::collections::HashSet;

use aeroric_protocol::memory::{
    HistoryCandidate, HistoryPlan, MemoryDraft, MemoryKind, MemoryRun, MemoryRunState,
    EXTRACT_MAX_CHARS,
};

use super::extract::{self, Transcript};
use super::{evidence, Memory, MemoryError};
use crate::settings::model_detect::ChatOnce;

/// R1-1: how one extraction's writes went. A refusal (forgotten, looks like a
/// key, malformed) is the store doing its job; anything else lost data.
#[derive(Default)]
struct Writes {
    written: u32,
    refused: u32,
    failed: u32,
    first_failure: Option<String>,
}

impl Writes {
    fn note<T>(&mut self, result: Result<T, MemoryError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.written += 1;
                Some(value)
            }
            Err(
                MemoryError::Empty
                | MemoryError::TooLong
                | MemoryError::Secret(_)
                | MemoryError::Forgotten
                | MemoryError::WrongState,
            ) => {
                self.refused += 1;
                None
            }
            Err(error) => {
                self.failed += 1;
                self.first_failure.get_or_insert(error.to_string());
                None
            }
        }
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
        let mut run = MemoryRun {
            id: format!("run-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
            task_id: session.task_id.to_string(),
            project_id: session.project_id.to_string(),
            title: session.title.to_string(),
            at_ms: now_ms,
            state: MemoryRunState::Skipped,
            reason: None,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            facts: 0,
            candidates: 0,
            experiences: 0,
            failed_writes: 0,
        };
        if let Some(reason) = skip_reason(self, &session, &transcript) {
            run.reason = Some(reason);
            self.record(&run, session.task_id, &transcript.hash, now_ms)?;
            return Ok(run);
        }
        let used = self.counted_since(day_start_ms(now_ms));
        if used >= settings.daily_limit {
            return Err(format!("今天已经整理了 {used} 次，到上限了，明天再整理"));
        }
        let kept = self.kept(session.project_id);
        let ids: HashSet<String> = kept.iter().map(|item| item.id.clone()).collect();
        let prompt = extract::prompt(&transcript, &kept);
        run.model = Some(model.clone());
        let reply = match call(extract::SYSTEM, &prompt) {
            Ok(reply) => reply,
            Err(error) => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(format!("调用分析模型失败：{error}"));
                self.record(&run, session.task_id, &transcript.hash, now_ms)?;
                return Ok(run);
            }
        };
        run.input_tokens = reply.input_tokens;
        run.output_tokens = reply.output_tokens;
        run.cost_usd = (reply.input_tokens + reply.output_tokens > 0).then(|| {
            crate::metrics::analytics::estimate_cost_usd(
                &model,
                reply.input_tokens,
                reply.output_tokens,
            )
            .0
        });
        let extracted = match extract::parse(&reply.text, &transcript, &ids) {
            Ok(extracted) => extracted,
            Err(error) => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(error);
                self.record(&run, session.task_id, &transcript.hash, now_ms)?;
                return Ok(run);
            }
        };
        let mut writes = Writes::default();
        if let Some(fact) = &extracted.fact {
            let proof = evidence(
                Some(session.task_id),
                Some("analysis"),
                Some(&model),
                fact,
                now_ms,
            );
            if writes
                .note(self.add_session_fact(session.project_id, fact, proof, now_ms))
                .is_some()
            {
                run.facts = 1;
            }
        }
        let confidence = if session.verified {
            aeroric_protocol::memory::Confidence::Medium
        } else {
            aeroric_protocol::memory::Confidence::Low
        };
        for (title, detail) in &extracted.experiences {
            let proof = evidence(
                Some(session.task_id),
                Some("analysis"),
                Some(&model),
                &transcript.text,
                now_ms,
            );
            if writes
                .note(self.add_experience(
                    session.project_id,
                    title,
                    detail,
                    confidence,
                    proof,
                    now_ms,
                ))
                .is_some()
            {
                run.experiences += 1;
            }
        }
        for candidate in extracted.candidates {
            // About the user: every project; a project's convention: this one.
            let project_id =
                (candidate.kind == MemoryKind::Fact).then(|| session.project_id.to_string());
            let draft = MemoryDraft {
                project_id,
                kind: candidate.kind,
                category: candidate.category,
                text: candidate.text,
                supersedes: candidate.supersedes,
            };
            let proof = evidence(
                Some(session.task_id),
                Some("analysis"),
                Some(&model),
                &candidate.quote,
                now_ms,
            );
            if let Some(item) = writes.note(self.propose(draft, proof, now_ms)) {
                if item.state == aeroric_protocol::memory::MemoryState::Pending {
                    run.candidates += 1;
                }
            }
        }
        let mut notes = Vec::new();
        if writes.failed > 0 {
            notes.push(format!(
                "{} 条没写进去，下次整理时重试：{}",
                writes.failed,
                writes.first_failure.as_deref().unwrap_or("存储出错")
            ));
        }
        if extracted.dropped > 0 {
            notes.push(format!("丢掉 {} 条没有原话依据的候选", extracted.dropped));
        }
        if writes.refused > 0 {
            notes.push(format!(
                "{} 条被记忆规则拦下（已遗忘或疑似密钥）",
                writes.refused
            ));
        }
        run.reason = (!notes.is_empty()).then(|| notes.join("；"));
        run.failed_writes = writes.failed;
        if writes.failed > 0 {
            run.state = if writes.written > 0 {
                MemoryRunState::Partial
            } else {
                MemoryRunState::Failed
            };
            // The transcript stays unmarked so the next trigger writes the
            // rest; `insert` merges what is already there.
            self.add_run(&run).map_err(|e| e.to_string())?;
            return Ok(run);
        }
        run.state = MemoryRunState::Done;
        self.record(&run, session.task_id, &transcript.hash, now_ms)?;
        Ok(run)
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
    #[test]
    fn a_failed_write_is_partial_and_the_next_run_finishes_it() {
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
        assert!(first.reason.as_deref().unwrap().contains("没写进去"));
        assert_eq!(memory.extracted("t1"), None, "stays unmarked to be retried");

        memory.execute_for_test("DROP TRIGGER boom;");
        let second = memory
            .extract_session(session(&messages, "p1", false), None, |_, _| reply(), 2)
            .unwrap();
        assert_eq!(second.state, MemoryRunState::Done);
        assert_eq!(second.experiences, 1);
        assert!(memory.extracted("t1").is_some());
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
        assert_eq!(memory.extracted("t1"), None);
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
        }
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
        let clues: Vec<_> = memory
            .list(Some("p1"), None)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == MemoryKind::SessionFact)
            .collect();
        assert_eq!(clues.len(), 1);
        assert_eq!(clues[0].state, MemoryState::Active);
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
}
