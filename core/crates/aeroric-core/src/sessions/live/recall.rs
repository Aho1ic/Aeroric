//! H4 (2026-11-21): when the user sends a message, look in the project's
//! memory and Wiki locally (no model) and, only when something clearly
//! matches, put a few pointers in front of the message in the "Aeroric 附加
//! 的上下文" envelope. The decision is made here, on the host, from the
//! message alone, so every harness gets the same one.
//!
//! Off unless the user turns it on: a wrong injection pollutes every turn.
//! In between, `candidate` (the default) only records what would have been
//! attached, for H8 to judge.

use std::path::PathBuf;

use aeroric_protocol::memory::MemoryItem;
use aeroric_protocol::wiki::WikiPageSummary;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A message shorter than this ("继续", "好的") is not looked up.
pub const MIN_MESSAGE_CHARS: usize = 6;
/// A memory matches when it shares this many of the message's keywords (a
/// long identifier, such as `dbx-ref.txt`, counts as two).
pub const MIN_SHARED: usize = 2;
/// An ASCII keyword this long is specific enough to count twice.
const SPECIFIC_CHARS: usize = 6;
pub const MAX_MEMORIES: usize = 3;
pub const MAX_PAGES: usize = 1;
/// A memory's text is cut to this many characters in the pointer.
const MEMORY_CHARS: usize = 160;

/// Words too common to say what a message is about.
const COMMON: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "this",
    "that",
    "from",
    "what",
    "how",
    "why",
    "can",
    "you",
    "please",
    "help",
    "我们",
    "你们",
    "他们",
    "这个",
    "那个",
    "一下",
    "怎么",
    "什么",
    "为什么",
    "可以",
    "帮我",
    "看看",
    "现在",
    "然后",
    "还是",
    "就是",
    "一个",
    "没有",
    "不是",
    "是不",
    "这里",
    "那里",
    "如果",
    "因为",
    "所以",
    "但是",
    "需要",
    "应该",
    "已经",
    "问题",
];

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// The message's keywords: identifier-like ASCII words of three or more
/// characters, and each pair of adjacent CJK characters; common ones left out.
pub fn keywords(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |word: String| {
        if !COMMON.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
    };
    let lower = message.to_lowercase();
    let mut ascii = String::new();
    let mut cjk: Vec<char> = Vec::new();
    for c in lower.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || (!ascii.is_empty() && matches!(c, '-' | '_' | '.' | '/')) {
            ascii.push(c);
        } else {
            let word = ascii.trim_end_matches(['-', '_', '.', '/']).to_string();
            if word.chars().count() >= 3 {
                push(word);
            }
            ascii.clear();
        }
        if is_cjk(c) {
            cjk.push(c);
        } else {
            for pair in cjk.windows(2) {
                push(pair.iter().collect());
            }
            cjk.clear();
        }
    }
    out
}

/// How much of `keywords` `text` shares (a long ASCII one counts twice).
fn shared(keywords: &[String], text: &str) -> usize {
    let text = text.to_lowercase();
    keywords
        .iter()
        .filter(|k| text.contains(k.as_str()))
        .map(|k| {
            if k.is_ascii() && k.chars().count() >= SPECIFIC_CHARS {
                2
            } else {
                1
            }
        })
        .sum()
}

/// The full-text query that finds the candidates (any keyword).
pub fn query(message: &str) -> String {
    keywords(message).join(" ")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecallMode {
    Off,
    /// Decide and record, attach nothing.
    #[default]
    Candidate,
    On,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallHit {
    /// `memory` or `wiki`.
    pub kind: String,
    /// A memory id or a page slug.
    pub id: String,
    pub text: String,
}

/// One message's decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallDecision {
    pub at_ms: u64,
    pub mode: RecallMode,
    pub hits: Vec<RecallHit>,
    /// The hits went in front of the message.
    pub attached: bool,
    /// Why nothing was looked up, when nothing was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// What matched `message`: memories sharing at least [`MIN_SHARED`] of its
/// keywords, most shared first (at most [`MAX_MEMORIES`]), and Wiki pages
/// whose title the message names ([`MAX_PAGES`]). The same inputs give the
/// same hits whatever the harness.
pub fn hits(message: &str, memories: &[MemoryItem], pages: &[WikiPageSummary]) -> Vec<RecallHit> {
    let words = keywords(message);
    let mut scored: Vec<(usize, &MemoryItem)> = memories
        .iter()
        .map(|item| (shared(&words, &item.text), item))
        .filter(|(n, _)| *n >= MIN_SHARED)
        .collect();
    // Stable: equal scores keep the order they were found in.
    scored.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    let mut out: Vec<RecallHit> = scored
        .into_iter()
        .take(MAX_MEMORIES)
        .map(|(_, item)| RecallHit {
            kind: "memory".into(),
            id: item.id.clone(),
            text: item
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(MEMORY_CHARS)
                .collect(),
        })
        .collect();
    let lower = message.to_lowercase();
    out.extend(
        pages
            .iter()
            .filter(|p| p.slug != "index" && p.title.chars().count() >= 2)
            .filter(|p| lower.contains(&p.title.to_lowercase()))
            .take(MAX_PAGES)
            .map(|p| RecallHit {
                kind: "wiki".into(),
                id: p.slug.clone(),
                text: p.title.clone(),
            }),
    );
    out
}

/// Why `message` is not looked up at all, if it is not.
pub fn skip(message: &str) -> Option<&'static str> {
    (message.trim().chars().count() < MIN_MESSAGE_CHARS).then_some("消息太短")
}

/// What goes in the envelope.
pub fn block(hits: &[RecallHit]) -> String {
    let mut out = String::from(
        "# 可能相关的记忆和 Wiki\n\nAeroric 按这条消息在本地找到的，不一定相关；用不上就忽略。\n",
    );
    for hit in hits {
        out.push_str(&match hit.kind.as_str() {
            "wiki" => format!(
                "- Wiki 页「{}」（{}）：要细节用 read_wiki 读全文\n",
                hit.text, hit.id
            ),
            _ => format!("- 记忆 {}：{}\n", hit.id, hit.text),
        });
    }
    out
}

/// Why the envelope is there, as it says on it.
pub const WHY: &str = "Aeroric 按这条消息自动找到的相关记忆和 Wiki";

/// The mode (`<data>/recall/mode.json`) and each task's decisions
/// (`<data>/recall/<task>.jsonl`).
#[derive(Default)]
pub struct RecallLog {
    dir: Mutex<Option<PathBuf>>,
}

impl RecallLog {
    pub fn set_dir(&self, data_dir: PathBuf) {
        *self.dir.lock() = Some(data_dir.join("recall"));
    }

    fn dir(&self) -> Option<PathBuf> {
        self.dir.lock().clone()
    }

    pub fn mode(&self) -> RecallMode {
        self.dir()
            .and_then(|dir| std::fs::read_to_string(dir.join("mode.json")).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn set_mode(&self, mode: RecallMode) -> Result<(), String> {
        let dir = self.dir().ok_or("没有数据目录")?;
        crate::settings::ensure_private_dir(&dir)?;
        let text = serde_json::to_string(&mode).map_err(|e| e.to_string())?;
        crate::settings::atomic_write_private(&dir.join("mode.json"), &text)
    }

    pub fn note(&self, task_id: &str, decision: &RecallDecision) {
        use std::io::Write;
        let Some(dir) = self.dir() else { return };
        if crate::settings::ensure_private_dir(&dir).is_err() {
            return;
        }
        let safe: String = task_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let Ok(line) = serde_json::to_string(decision) else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{safe}.jsonl")))
        {
            let _ = writeln!(file, "{line}");
        }
    }

    pub fn decisions(&self, task_id: &str) -> Vec<RecallDecision> {
        let safe: String = task_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir()
            .and_then(|dir| std::fs::read_to_string(dir.join(format!("{safe}.jsonl"))).ok())
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "recall_tests.rs"]
mod tests;
