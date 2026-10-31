//! B5-2 (2026-10-31): one extraction, start to finish — checks, the call,
//! what it wrote, and the 整理记录 row. The host brings the conversation
//! and the call (the analysis model over the user's connection), so this is
//! tested with a fake model.

use std::collections::HashSet;

use aeroric_protocol::memory::{MemoryDraft, MemoryKind, MemoryRun, MemoryRunState};

use super::extract::{self, Transcript};
use super::{evidence, Memory};
use crate::settings::model_detect::ChatOnce;

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

impl Memory {
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
        if let Some(fact) = &extracted.fact {
            let proof = evidence(
                Some(session.task_id),
                Some("analysis"),
                Some(&model),
                fact,
                now_ms,
            );
            if self
                .add_session_fact(session.project_id, fact, proof, now_ms)
                .is_ok()
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
            if self
                .add_experience(session.project_id, title, detail, confidence, proof, now_ms)
                .is_ok()
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
            if let Ok(item) = self.propose(draft, proof, now_ms) {
                if item.state == aeroric_protocol::memory::MemoryState::Pending {
                    run.candidates += 1;
                }
            }
        }
        if extracted.dropped > 0 {
            run.reason = Some(format!("丢掉 {} 条没有原话依据的候选", extracted.dropped));
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
        self.add_run(run).map_err(|e| e.to_string())?;
        self.mark_extracted(task_id, hash, now_ms)
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
