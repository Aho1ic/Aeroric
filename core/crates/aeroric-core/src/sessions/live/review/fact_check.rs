//! E2 (2026-11-26): the fact check. Before a blocking round's findings go to
//! the writer, the analysis model reads them against the diff and may take a
//! finding out on two grounds only (OCR's rule): the code it quotes is not in
//! the change, or a line of the diff refutes it word for word. Leaving a
//! wrong finding costs the writer a few seconds; taking out a right one
//! silently loses a real problem — so every removal the model proposes must
//! also pass a check here, and some findings are never taken out:
//!
//! - the five protected topics (memory safety, concurrency, declaration
//!   consistency, behaviour or compatibility change, unused parameters),
//!   recognised by the model's label or by the finding's own words;
//! - `security` findings;
//! - a finding the previous round raised too (E3 `repeats`).
//!
//! Off unless the task's review settings turn it on (`factCheck`).

use serde::Deserialize;

use aeroric_protocol::review::{FilterGround, FilteredFinding, Finding};

use super::locate::normalize;
use super::parse::candidate;

/// The answer budget: one short line per finding.
pub const MAX_TOKENS: u32 = 1500;
/// The diff the model sees, at most.
pub const MAX_DIFF_CHARS: usize = 60_000;
/// A quoted line shorter than this (whitespace collapsed) refutes nothing:
/// `}` or `None` is everywhere.
const MIN_QUOTE_CHARS: usize = 4;
/// A quote that is only part of a diff line must be at least this long.
const MIN_PART_CHARS: usize = 8;

pub const SYSTEM: &str = "你是代码审查意见的事实核查员。你只判断每条意见是否被 diff 本身直接驳倒，不评价意见好坏，不补充新问题。\n\
口径：留下一条错的意见只花作者几秒；删掉一条对的意见会让真问题悄悄消失。拿不准就 keep。\n\
只有两种情况可以不保留：\n\
- not_in_diff：意见引用的代码在这次的 diff 里根本找不到（不在新增、删除或上下文的任何一行）。\n\
- refuted：diff 里有一行，字面上直接说明意见说错了，不需要任何推理链。line 字段必须从 diff 里逐字照抄那一行（不含行首的 + 或空格）。\n\
需要看 diff 以外的代码、需要推理、或者只是你觉得意见不重要的，一律 keep。\n\
每条意见还要标 topic：memory_safety（内存安全）、concurrency（并发）、declaration（声明一致性）、compatibility（行为或兼容性变化）、unused_parameter（未使用的参数）、other（其他）。\n\
只回答一个 JSON 对象，不要别的文字：{\"results\":[{\"id\":\"…\",\"verdict\":\"keep|not_in_diff|refuted\",\"topic\":\"…\",\"line\":\"…\"}]}";

/// The user message: the findings, then the diff.
pub fn prompt(findings: &[Finding], diff: &str) -> String {
    let mut out = String::from("## 审查意见\n\n");
    for f in findings {
        out.push_str(&format!("- id: {}\n", f.id));
        if let Some(file) = &f.file {
            match f.line {
                Some(line) => out.push_str(&format!("  位置: {file}:{line}\n")),
                None => out.push_str(&format!("  位置: {file}\n")),
            }
        }
        out.push_str(&format!("  意见: {}\n", f.text));
        if let Some(snippet) = &f.snippet {
            out.push_str("  引用的代码:\n");
            for line in snippet.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out.push_str("\n## diff\n\n```diff\n");
    if diff.chars().count() > MAX_DIFF_CHARS {
        out.extend(diff.chars().take(MAX_DIFF_CHARS));
        out.push_str("\n…（diff 太长，后面的省略了）\n");
    } else {
        out.push_str(diff);
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    NotInDiff,
    Refuted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    pub id: String,
    pub verdict: Verdict,
    pub topic: String,
    pub line: Option<String>,
}

#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    results: Vec<RawJudgement>,
}

#[derive(Deserialize)]
struct RawJudgement {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    verdict: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    line: Option<String>,
}

/// The model's judgements; `None` when the answer holds no readable JSON.
/// A verdict it does not know is `keep`.
pub fn parse(reply: &str) -> Option<Vec<Judgement>> {
    let raw: RawReply = serde_json::from_str(candidate(reply)?).ok()?;
    Some(
        raw.results
            .into_iter()
            .filter_map(|r| {
                let id = match r.id? {
                    serde_json::Value::String(s) => s.trim().to_string(),
                    serde_json::Value::Number(n) => format!("r{n}"),
                    _ => return None,
                };
                let verdict = match r.verdict.as_deref().map(str::trim) {
                    Some("not_in_diff") => Verdict::NotInDiff,
                    Some("refuted") => Verdict::Refuted,
                    _ => Verdict::Keep,
                };
                Some(Judgement {
                    id,
                    verdict,
                    topic: r.topic.unwrap_or_default().trim().to_lowercase(),
                    line: r.line.filter(|l| !l.trim().is_empty()),
                })
            })
            .collect(),
    )
}

/// What the check left.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub kept: Vec<Finding>,
    pub filtered: Vec<FilteredFinding>,
    /// Removals the model proposed that a guard turned down.
    pub guarded: u32,
}

/// Apply the model's judgements to `findings`, keeping every finding a
/// guard protects or a removal the diff does not bear out.
pub fn apply(findings: Vec<Finding>, judgements: &[Judgement], diff: &str) -> Outcome {
    let diff = Diff::read(diff);
    let mut out = Outcome::default();
    for finding in findings {
        let Some(judgement) = judgements.iter().find(|j| j.id == finding.id) else {
            out.kept.push(finding);
            continue;
        };
        let removal = match judgement.verdict {
            Verdict::Keep => None,
            Verdict::NotInDiff => not_in_diff(&finding, &diff),
            Verdict::Refuted => refuted(&finding, judgement.line.as_deref(), &diff),
        };
        let wanted = judgement.verdict != Verdict::Keep;
        match removal {
            Some((ground, evidence)) if !protected(&finding, &judgement.topic) => {
                out.filtered.push(FilteredFinding {
                    finding,
                    ground,
                    evidence,
                })
            }
            _ => {
                if wanted {
                    out.guarded += 1;
                }
                out.kept.push(finding);
            }
        }
    }
    out
}

/// Never taken out, whatever the model says.
pub fn protected(finding: &Finding, topic: &str) -> bool {
    topic != "other"
        || finding.category.as_deref() == Some("security")
        || finding.repeats.is_some()
        || protected_words(&finding.text)
}

/// The protected topics, from the finding's own words.
pub fn protected_words(text: &str) -> bool {
    const PARTS: &[&str] = &[
        // concurrency
        "concurren",
        "race",
        "deadlock",
        "mutex",
        "thread",
        "atomic",
        "并发",
        "竞态",
        "竞争",
        "死锁",
        "线程",
        "原子",
        "加锁",
        "锁住",
        "持锁",
        "上锁",
        // memory safety
        "memory",
        "unsafe",
        "use-after",
        "use after",
        "dangling",
        "overflow",
        "out of bounds",
        "out-of-bounds",
        "leak",
        "null pointer",
        "内存",
        "越界",
        "溢出",
        "泄漏",
        "悬垂",
        "野指针",
        "空指针",
        // declaration consistency
        "signature",
        "declar",
        "签名",
        "声明",
        // behaviour or compatibility change
        "compat",
        "breaking",
        "backward",
        "behavior change",
        "behaviour change",
        "兼容",
        "破坏性",
        "行为变化",
        "行为改变",
        "行为变了",
        // unused parameters
        "unused",
        "未使用",
        "没有用到",
        "没用到",
        "没有被使用",
        "参数没有用",
    ];
    let lower = text.to_lowercase();
    PARTS.iter().any(|p| lower.contains(p))
        || ["lock", "locks", "locked", "locking", "unlock", "unlocked"]
            .iter()
            .any(|w| has_word(&lower, w))
}

/// `word` standing alone (not inside `block` or `clock`).
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The quoted code is in the change nowhere: not in the file as the turn
/// left it (E2 marked it unlocated) and on no line of the diff.
fn not_in_diff(finding: &Finding, diff: &Diff) -> Option<(FilterGround, String)> {
    let snippet = finding.snippet.as_deref()?;
    if !finding.unlocated {
        return None;
    }
    let wanted: Vec<String> = snippet
        .lines()
        .map(normalize)
        .filter(|l| !l.is_empty())
        .collect();
    if wanted.is_empty() {
        return None;
    }
    // Either way round: the snippet inside a diff line, or a long enough
    // diff line inside the snippet (the reviewer quoted more than changed).
    let anywhere = wanted.iter().any(|w| {
        diff.lines.iter().any(|line| {
            line.text.contains(w.as_str())
                || (line.text.chars().count() >= MIN_PART_CHARS && w.contains(line.text.as_str()))
        })
    });
    (!anywhere).then(|| {
        (
            FilterGround::NotInDiff,
            "引用的代码不在这次的改动里".to_string(),
        )
    })
}

/// The quoted line is on the after side of the finding's file in the diff.
fn refuted(finding: &Finding, quote: Option<&str>, diff: &Diff) -> Option<(FilterGround, String)> {
    let quote = normalize(quote?);
    let quote = quote
        .strip_prefix("+ ")
        .or_else(|| quote.strip_prefix('+'))
        .unwrap_or(&quote)
        .trim()
        .to_string();
    let length = quote.chars().count();
    if length < MIN_QUOTE_CHARS {
        return None;
    }
    let found = diff.lines.iter().find(|line| {
        line.after_side
            && finding.file.as_deref().is_none_or(|file| {
                super::coverage::same_path(file, &line.file)
                    || super::coverage::same_path(&line.file, file)
            })
            && (line.text == quote
                || length >= MIN_PART_CHARS && line.text.contains(quote.as_str()))
    })?;
    Some((FilterGround::Refuted, found.raw.clone()))
}

struct DiffLine {
    file: String,
    /// Whitespace collapsed, without its `+`, `-` or ` `.
    text: String,
    /// As in the diff, without the marker.
    raw: String,
    /// Added or context: what the file says after the turn.
    after_side: bool,
}

struct Diff {
    lines: Vec<DiffLine>,
}

impl Diff {
    fn read(diff: &str) -> Self {
        let mut lines = Vec::new();
        let mut file = String::new();
        let mut in_hunk = false;
        for line in diff.lines() {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                file = rest
                    .rsplit_once(" b/")
                    .map(|(_, b)| b.to_string())
                    .unwrap_or_default();
                in_hunk = false;
                continue;
            }
            if line.starts_with("@@") {
                in_hunk = true;
                continue;
            }
            if !in_hunk {
                if let Some(path) = line.strip_prefix("+++ b/") {
                    file = path.to_string();
                }
                continue;
            }
            let (marker, body) = match line.chars().next() {
                Some(c @ ('+' | '-' | ' ')) => (c, &line[1..]),
                _ => continue,
            };
            let text = normalize(body);
            if text.is_empty() {
                continue;
            }
            lines.push(DiffLine {
                file: file.clone(),
                text,
                raw: body.trim().to_string(),
                after_side: marker != '-',
            });
        }
        Self { lines }
    }
}

#[cfg(test)]
#[path = "fact_check_tests.rs"]
mod tests;
