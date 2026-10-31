//! B5-2 (2026-10-31): turning a finished session into one session clue and
//! a few pending candidates (frontier-roadmap §6.2; Q-M9–Q-M15). Only the
//! conversation's words go out — no tool output — cut to the latest
//! `EXTRACT_MAX_CHARS`, secrets masked. A candidate's quote must be found in
//! the conversation or it is dropped (A08: no citation, no candidate).

use std::collections::HashSet;

use aeroric_protocol::memory::{
    ExperienceDetail, MemoryCategory, MemoryItem, MemoryKind, EXTRACT_MAX_CANDIDATES,
    EXTRACT_MAX_CHARS, EXTRACT_MAX_EXPERIENCES, MAX_TEXT_CHARS,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::normalize;

/// The conversation as sent: `用户：…` / `Agent：…` blocks, newest kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub user_turns: usize,
    /// sha256 of the whole conversation (before cutting), first 16 hex.
    pub hash: String,
}

pub fn transcript(messages: &[(String, String)], home: Option<&std::path::Path>) -> Transcript {
    let mut whole = String::new();
    let mut user_turns = 0;
    for (role, text) in messages {
        let who = if role == "user" {
            user_turns += 1;
            "用户"
        } else {
            "Agent"
        };
        whole.push_str(&format!("{who}：{}\n\n", text.trim()));
    }
    let digest = Sha256::digest(whole.as_bytes());
    let hash = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let count = whole.chars().count();
    let kept: String = if count > EXTRACT_MAX_CHARS {
        whole.chars().skip(count - EXTRACT_MAX_CHARS).collect()
    } else {
        whole
    };
    let text = crate::maintenance::diagnostic_report::redact(&kept, home, &[]);
    Transcript {
        text,
        user_turns,
        hash,
    }
}

pub const SYSTEM: &str = "你是 Aeroric 的记忆整理员。你读一段已经结束的编码会话，只输出一个 JSON 对象，不要任何别的文字。\
不确定就少写；不要记录密钥、临时状态、只对这次对话有用的细节。";

/// The user message: the rules, what is already kept, and the conversation.
pub fn prompt(transcript: &Transcript, existing: &[MemoryItem]) -> String {
    let mut kept = String::new();
    for item in existing.iter().take(40) {
        let kind = if item.kind == MemoryKind::Profile {
            "关于用户"
        } else {
            "项目"
        };
        kept.push_str(&format!("- [{}]（{kind}）{}\n", item.id, item.text));
    }
    if kept.is_empty() {
        kept.push_str("（暂无）\n");
    }
    format!(
        "请整理下面这段会话，输出 JSON：\n\
{{\"sessionFact\": {{\"task\": \"这次要做什么，一句话\", \"approach\": \"怎么做的\", \"result\": \"结果如何、还剩什么\", \"files\": [\"涉及的主要文件\"]}},\n \
\"candidates\": [{{\"kind\": \"profile 或 fact\", \"category\": \"kind 为 profile 时：language/interaction/preference/constraint\", \
\"text\": \"值得长期记住的一句话\", \"quote\": \"支撑它的用户原话，逐字摘录\", \"supersedes\": \"它取代的已有记忆 id，可省略\"}}],\n \
\"experiences\": [{{\"title\": \"一句话说清是什么问题的什么解法\", \"problem\": \"遇到的问题与症状\", \"solution\": \"可复用的解法\", \
\"searchKeys\": [\"检索关键词\"], \"envs\": [\"适用环境，如 macOS、Rust 1.8x、某依赖版本\"]}}]}}\n\n\
规则：\n\
1. candidates 只写用户明确表达过的偏好、习惯、限制，或项目的约定与决定；最多 {EXTRACT_MAX_CANDIDATES} 条，没有就写空数组。\n\
2. quote 必须逐字来自下面会话里“用户：”说的话。\n\
3. 已经记住的不要重复；与已有记忆矛盾时，用 supersedes 指出被取代的那条。\n\
4. experiences 只在会话确实解决了问题时写，最多 {EXTRACT_MAX_EXPERIENCES} 条，写成下次遇到同样问题能直接用的解法；没解决就写空数组。\n\n\
已经记住的：\n{kept}\n会话：\n{}",
        transcript.text
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReply {
    #[serde(default)]
    session_fact: Option<RawFact>,
    #[serde(default)]
    candidates: Vec<RawCandidate>,
    #[serde(default)]
    experiences: Vec<RawExperience>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExperience {
    #[serde(default)]
    title: String,
    #[serde(default)]
    problem: String,
    #[serde(default)]
    solution: String,
    #[serde(default)]
    search_keys: Vec<String>,
    #[serde(default)]
    envs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFact {
    #[serde(default)]
    task: String,
    #[serde(default)]
    approach: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCandidate {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    supersedes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: MemoryKind,
    pub category: Option<MemoryCategory>,
    pub text: String,
    pub quote: String,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extracted {
    /// The session clue's text (`任务｜做法｜结果｜文件`).
    pub fact: Option<String>,
    pub candidates: Vec<Candidate>,
    /// Candidates dropped (no quote in the conversation, bad shape).
    pub dropped: usize,
    /// B5-3a: `(title, detail)`.
    pub experiences: Vec<(String, ExperienceDetail)>,
}

fn cut(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// The model's reply, checked against the conversation and the kept ids.
pub fn parse(
    reply: &str,
    transcript: &Transcript,
    ids: &HashSet<String>,
) -> Result<Extracted, String> {
    let start = reply.find('{').ok_or("模型没有返回 JSON")?;
    let end = reply.rfind('}').ok_or("模型没有返回 JSON")?;
    if end < start {
        return Err("模型没有返回 JSON".into());
    }
    let raw: RawReply = serde_json::from_str(&reply[start..=end])
        .map_err(|e| format!("模型返回的 JSON 读不懂：{e}"))?;
    let mut out = Extracted::default();
    if let Some(f) = raw.session_fact.filter(|f| !f.task.trim().is_empty()) {
        let mut parts = vec![format!("任务：{}", f.task.trim())];
        if !f.approach.trim().is_empty() {
            parts.push(format!("做法：{}", f.approach.trim()));
        }
        if !f.result.trim().is_empty() {
            parts.push(format!("结果：{}", f.result.trim()));
        }
        let files: Vec<&str> = f
            .files
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .take(8)
            .collect();
        if !files.is_empty() {
            parts.push(format!("文件：{}", files.join("、")));
        }
        out.fact = Some(cut(&parts.join("｜"), MAX_TEXT_CHARS));
    }
    let said = normalize(&transcript.text);
    for c in raw.candidates {
        if out.candidates.len() == EXTRACT_MAX_CANDIDATES {
            out.dropped += 1;
            continue;
        }
        let kind = match c.kind.as_str() {
            "profile" => MemoryKind::Profile,
            "fact" => MemoryKind::Fact,
            _ => {
                out.dropped += 1;
                continue;
            }
        };
        let quote = normalize(&c.quote);
        if c.text.trim().is_empty() || quote.chars().count() < 2 || !said.contains(&quote) {
            out.dropped += 1;
            continue;
        }
        let category = match (kind, c.category.as_deref()) {
            (MemoryKind::Profile, Some("language")) => Some(MemoryCategory::Language),
            (MemoryKind::Profile, Some("interaction")) => Some(MemoryCategory::Interaction),
            (MemoryKind::Profile, Some("constraint")) => Some(MemoryCategory::Constraint),
            (MemoryKind::Profile, _) => Some(MemoryCategory::Preference),
            _ => None,
        };
        out.candidates.push(Candidate {
            kind,
            category,
            text: cut(&c.text, MAX_TEXT_CHARS),
            quote: cut(&c.quote, 200),
            supersedes: c.supersedes.filter(|id| ids.contains(id)),
        });
    }
    let list = |values: Vec<String>, max: usize| -> Vec<String> {
        values
            .iter()
            .map(|v| cut(v, 60))
            .filter(|v| !v.is_empty())
            .take(max)
            .collect()
    };
    for e in raw.experiences {
        if out.experiences.len() == EXTRACT_MAX_EXPERIENCES
            || e.title.trim().is_empty()
            || e.solution.trim().is_empty()
        {
            continue;
        }
        out.experiences.push((
            cut(&e.title, 200),
            ExperienceDetail {
                problem: cut(&e.problem, MAX_TEXT_CHARS),
                solution: cut(&e.solution, MAX_TEXT_CHARS),
                search_keys: list(e.search_keys, 8),
                envs: list(e.envs, 6),
            },
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said() -> Transcript {
        transcript(
            &[
                (
                    "user".into(),
                    "以后回复都用简体中文。帮我修 SSH 超时".into(),
                ),
                (
                    "assistant".into(),
                    "好的，我改用 connect_timeout。key sk-ant-abcdefgh123456".into(),
                ),
                ("user".into(), "发布前先跑 native:check sessions".into()),
            ],
            None,
        )
    }

    #[test]
    fn the_transcript_keeps_words_masks_secrets_and_counts_the_user() {
        let t = said();
        assert_eq!(t.user_turns, 2);
        assert!(t.text.starts_with("用户：以后回复"));
        assert!(!t.text.contains("sk-ant-abcdefgh123456"), "{}", t.text);
        assert_eq!(t.hash.len(), 16);
        let long = transcript(
            &[("user".into(), "字".repeat(EXTRACT_MAX_CHARS + 50))],
            None,
        );
        assert_eq!(long.text.chars().count(), EXTRACT_MAX_CHARS);
        assert!(prompt(&t, &[]).contains("（暂无）"));
    }

    #[test]
    fn a_reply_is_checked_against_the_conversation() {
        let t = said();
        let ids: HashSet<String> = ["mem-old".to_string()].into();
        let reply = r#"好的：```json
{"sessionFact": {"task": "修 SSH 超时", "approach": "改用 connect_timeout", "result": "已修", "files": ["ssh.rs"]},
 "candidates": [
   {"kind": "profile", "category": "language", "text": "回复用简体中文", "quote": "以后回复都用简体中文", "supersedes": "mem-old"},
   {"kind": "fact", "text": "发布前跑 native:check", "quote": "发布前先跑 native:check sessions", "supersedes": "mem-made-up"},
   {"kind": "fact", "text": "编的", "quote": "用户从没说过这句"},
   {"kind": "habit", "text": "坏类型", "quote": "帮我修"}
 ]}
```"#;
        let out = parse(reply, &t, &ids).unwrap();
        assert_eq!(
            out.fact.as_deref(),
            Some("任务：修 SSH 超时｜做法：改用 connect_timeout｜结果：已修｜文件：ssh.rs")
        );
        assert_eq!(out.candidates.len(), 2);
        assert_eq!(out.candidates[0].category, Some(MemoryCategory::Language));
        assert_eq!(out.candidates[0].supersedes.as_deref(), Some("mem-old"));
        assert_eq!(out.candidates[1].supersedes, None, "an id it was not given");
        assert_eq!(out.dropped, 2);
        assert!(out.experiences.is_empty());
        let with = r#"{"experiences": [
            {"title": "SSH 超时", "problem": "卡住", "solution": "connect_timeout", "searchKeys": ["ssh","a","b","c","d","e","f","g","h"], "envs": ["macOS"]},
            {"title": "", "solution": "没标题的丢掉"},
            {"title": "没解法的丢掉", "solution": " "},
            {"title": "第二条", "solution": "x"},
            {"title": "第三条超出上限", "solution": "y"}]}"#;
        let out = parse(with, &t, &ids).unwrap();
        assert_eq!(out.experiences.len(), 2);
        assert_eq!(out.experiences[0].1.search_keys.len(), 8);
        assert!(prompt(&t, &[]).contains("experiences"));
        assert!(parse("没有 JSON", &t, &ids).is_err());
        assert!(parse("{坏的}", &t, &ids).is_err());
    }
}
