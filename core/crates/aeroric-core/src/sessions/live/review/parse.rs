//! Reading the reviewer's verdict and the writer's answers out of a turn's
//! text: the last ```json block, else the outermost `{…}`. Lenient about
//! spelling (阻断 / blocker, 采纳 / adopt), strict about having something.

use aeroric_protocol::review::{Decision, Finding, Severity, MAX_TEXT_CHARS};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct Verdict {
    pub pass: bool,
    pub summary: Option<String>,
    pub findings: Vec<Finding>,
}

#[derive(Deserialize)]
struct RawVerdict {
    #[serde(default)]
    verdict: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    findings: Vec<RawFinding>,
}

#[derive(Deserialize)]
struct RawFinding {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<Value>,
}

/// The reviewer's verdict; `None` when the text holds none.
pub fn verdict(text: &str) -> Option<Verdict> {
    let raw: RawVerdict = serde_json::from_str(candidate(text)?).ok()?;
    let mut findings = Vec::new();
    for (i, f) in raw.findings.into_iter().enumerate() {
        let Some(body) = f
            .text
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let id = match f.id {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(Value::Number(n)) => format!("r{n}"),
            _ => format!("r{}", i + 1),
        };
        findings.push(Finding {
            id,
            severity: severity(f.severity.as_deref()),
            text: cut(&body),
            file: f
                .file
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            line: match f.line {
                Some(Value::Number(n)) => n.as_u64(),
                Some(Value::String(s)) => s.trim().parse().ok(),
                _ => None,
            },
            decision: None,
            reason: None,
        });
    }
    let said = raw
        .verdict
        .as_deref()
        .map(|v| v.trim().to_ascii_lowercase());
    if said.is_none() && findings.is_empty() {
        return None;
    }
    let blocking = findings.iter().any(|f| f.severity == Severity::Blocker);
    Some(Verdict {
        // Q-B7: no blocker is a pass, whatever the reviewer called it.
        pass: !blocking,
        summary: raw.summary.map(|s| cut(s.trim())).filter(|s| !s.is_empty()),
        findings,
    })
}

#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    responses: Vec<RawResponse>,
}

#[derive(Deserialize)]
struct RawResponse {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// The writer's answers as `(finding id, decision, reason)`; empty when the
/// text holds none (every finding then counts as unanswered).
pub fn reply(text: &str) -> Vec<(String, Decision, Option<String>)> {
    let Some(raw) = candidate(text).and_then(|c| serde_json::from_str::<RawReply>(c).ok()) else {
        return Vec::new();
    };
    raw.responses
        .into_iter()
        .filter_map(|r| {
            let id = match r.id? {
                Value::String(s) => s.trim().to_string(),
                Value::Number(n) => format!("r{n}"),
                _ => return None,
            };
            let reason = r.reason.map(|s| cut(s.trim())).filter(|s| !s.is_empty());
            Some((id, decision(r.decision.as_deref()), reason))
        })
        .collect()
}

fn severity(raw: Option<&str>) -> Severity {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("blocker" | "blocking" | "critical" | "阻断") => Severity::Blocker,
        Some("major" | "important" | "重要") => Severity::Major,
        _ => Severity::Minor,
    }
}

fn decision(raw: Option<&str>) -> Decision {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("adopt" | "accept" | "accepted" | "fixed" | "采纳") => Decision::Adopt,
        Some("reject" | "rejected" | "decline" | "驳回") => Decision::Reject,
        _ => Decision::Ask,
    }
}

fn candidate(text: &str) -> Option<&str> {
    if let Some(start) = text.rfind("```json") {
        let body = &text[start + "```json".len()..];
        if let Some(end) = body.find("```") {
            return Some(body[..end].trim());
        }
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

pub(crate) fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_TEXT_CHARS).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_json_block_is_the_verdict() {
        let text = "先看了一下。\n```json\n{\"verdict\":\"pass\"}\n```\n复查后：\n```json\n{\"verdict\":\"changes\",\"summary\":\"缺上限\",\"findings\":[{\"id\":\"r1\",\"severity\":\"阻断\",\"file\":\"src/a.rs\",\"line\":\"12\",\"text\":\"重试没有上限\"},{\"severity\":\"minor\",\"text\":\"命名\"},{\"text\":\"\"}]}\n```";
        let v = verdict(text).unwrap();
        assert!(!v.pass);
        assert_eq!(v.summary.as_deref(), Some("缺上限"));
        assert_eq!(v.findings.len(), 2);
        assert_eq!(v.findings[0].severity, Severity::Blocker);
        assert_eq!(v.findings[0].line, Some(12));
        assert_eq!(v.findings[1].id, "r2");
    }

    #[test]
    fn no_blocker_is_a_pass_and_no_json_is_nothing() {
        let v = verdict("```json\n{\"verdict\":\"changes\",\"findings\":[{\"severity\":\"major\",\"text\":\"缺测试\"}]}\n```").unwrap();
        assert!(v.pass);
        assert!(verdict("看起来不错").is_none());
        assert!(verdict("{\"summary\": \"只有总评\"}").is_none());
    }

    #[test]
    fn answers_read_loosely_and_an_unknown_word_is_ask() {
        let r = reply("改好了。\n```json\n{\"responses\":[{\"id\":\"r1\",\"decision\":\"采纳\"},{\"id\":\"r2\",\"decision\":\"reject\",\"reason\":\"已有集成测试\"},{\"id\":3,\"decision\":\"maybe\"}]}\n```");
        assert_eq!(
            r,
            vec![
                ("r1".into(), Decision::Adopt, None),
                ("r2".into(), Decision::Reject, Some("已有集成测试".into())),
                ("r3".into(), Decision::Ask, None),
            ]
        );
        assert!(reply("没有 JSON").is_empty());
    }
}
