//! H7 third step (2026-11-21): 对照原文纠错. In a dream, each profile item
//! and project fact is set beside the user's messages of the scope's recent
//! sessions that ended after it was written and share its words (no model);
//! the analysis model says which of those messages overturn it, quoting the
//! user; core finds the quote in that very message (C1's `find_span`), and
//! the item gets a replacement candidate — pending, superseding it, its
//! evidence the user's words and where they are. Accepting it is the
//! ordinary confirm; nothing changes until then.

use std::collections::HashSet;

use aeroric_protocol::memory::{
    Evidence, EvidenceLocation, MemoryItem, MemoryKind, MemoryState, MAX_TEXT_CHARS,
};
use serde::Deserialize;

use super::locate::{find_span, message_hash};
use super::{normalize, secrets};

/// Sessions the host hands a dream: the scope's most recently ended ones.
pub const RECENT_SESSIONS: usize = 4;
/// Pairs one dream asks about (each item at most once).
pub const MAX_PAIRS: usize = 8;
/// Character pairs an item and a message must share to be set side by side.
const MIN_SHARED: usize = 2;
/// What of a message the model is shown.
const MESSAGE_CHARS: usize = 600;
/// Pairs asked about once are not asked again; the newest this many kept.
const CHECKED_KEEP: usize = 500;
pub(super) const CHECKED_KEY: &str = "contradict:checked";

pub const SYSTEM: &str = "你在核对一个用户的长期记忆和用户之后说的话。下面每一对给出一条记忆，和用户在这条记忆写下之后某次会话里说的一句原话。\
逐对判断这句话是否推翻了这条记忆：用户改了主意、纠正了它、或说明它不对。只是相关、补充、或另说一件事都不算；拿不准就不算。\n\
对推翻的那几对，给出 pair（序号）、quote（从那句原话里逐字摘出推翻它的部分）、text（改正后的记忆，一句话，沿用原记忆的说法）、reason（一句话理由）。\n\
只输出一个 JSON：{\"corrections\": [{\"pair\": 1, \"quote\": \"…\", \"text\": \"…\", \"reason\": \"…\"}]}。都没有推翻就输出 {\"corrections\": []}。";

/// One session's text messages, as extraction reads them.
#[derive(Debug, Clone)]
pub struct Source {
    pub task_id: String,
    /// When it last ended (completed or archived).
    pub ended_ms: u64,
    /// `(role, text)`; the user's are `"user"`.
    pub messages: Vec<(String, String)>,
}

/// An item set beside one user message.
#[derive(Debug, Clone)]
pub struct Pair {
    pub item: MemoryItem,
    /// Index into the sources.
    pub source: usize,
    /// Index into that source's messages.
    pub message: usize,
}

/// A replacement the model gave and core checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    /// Index into the pairs.
    pub pair: usize,
    pub quote: String,
    pub text: String,
    pub reason: String,
    pub location: EvidenceLocation,
}

/// What "asked about once" is keyed by: the item and the message's words.
pub fn key(item_id: &str, message: &str) -> String {
    format!("{item_id}|{}", message_hash(message))
}

fn shape(text: &str) -> HashSet<(char, char)> {
    let chars: Vec<char> = normalize(text).chars().collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// The pairs worth asking about, most shared first, at most `MAX_PAIRS`:
/// an active profile item or fact, unpinned or not, beside the user message
/// of a session that ended after it was written which shares the most of its
/// words. Left out: an item already waiting for a replacement (`waiting`),
/// the message the item came from, pairs asked about before (`checked`).
pub fn pairs(
    items: &[MemoryItem],
    sources: &[Source],
    checked: &HashSet<String>,
    waiting: &HashSet<String>,
) -> Vec<Pair> {
    let mut out: Vec<(usize, u64, Pair)> = Vec::new();
    for item in items {
        if !matches!(item.kind, MemoryKind::Profile | MemoryKind::Fact)
            || item.state != MemoryState::Active
            || waiting.contains(&item.id)
        {
            continue;
        }
        let own: HashSet<&str> = item
            .evidence
            .iter()
            .filter_map(|e| e.location.as_ref().map(|l| l.message_hash.as_str()))
            .collect();
        let mine = shape(&item.text);
        let mut best: Option<(usize, u64, usize, usize)> = None;
        for (s, source) in sources.iter().enumerate() {
            if source.ended_ms <= item.created_ms {
                continue;
            }
            for (m, (role, text)) in source.messages.iter().enumerate() {
                if role != "user" || own.contains(message_hash(text).as_str()) {
                    continue;
                }
                if checked.contains(&key(&item.id, text)) {
                    continue;
                }
                let shared = mine.intersection(&shape(text)).count();
                if shared < MIN_SHARED {
                    continue;
                }
                let better = best.is_none_or(|(n, at, _, _)| (shared, source.ended_ms) > (n, at));
                if better {
                    best = Some((shared, source.ended_ms, s, m));
                }
            }
        }
        if let Some((shared, at, source, message)) = best {
            out.push((
                shared,
                at,
                Pair {
                    item: item.clone(),
                    source,
                    message,
                },
            ));
        }
    }
    out.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    out.truncate(MAX_PAIRS);
    out.into_iter().map(|(_, _, p)| p).collect()
}

fn kind_word(kind: MemoryKind) -> &'static str {
    if kind == MemoryKind::Profile {
        "关于用户"
    } else {
        "项目约定"
    }
}

fn message_of<'a>(pair: &Pair, sources: &'a [Source]) -> &'a str {
    &sources[pair.source].messages[pair.message].1
}

pub fn prompt(pairs: &[Pair], sources: &[Source]) -> String {
    let mut out = String::new();
    for (n, pair) in pairs.iter().enumerate() {
        let message = message_of(pair, sources);
        let mut shown: String = message.chars().take(MESSAGE_CHARS).collect();
        if message.chars().count() > MESSAGE_CHARS {
            shown.push('…');
        }
        out.push_str(&format!(
            "第 {} 对\n记忆（{}）：{}\n用户之后说：{}\n\n",
            n + 1,
            kind_word(pair.item.kind),
            pair.item.text,
            shown
        ));
    }
    out
}

#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    corrections: Vec<RawCorrection>,
}

#[derive(Deserialize)]
struct RawCorrection {
    #[serde(default)]
    pair: usize,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    reason: String,
}

/// The model's reply, checked against the pairs it was sent: the quote must
/// be the user's words in that message (at least two characters), the text
/// new, not too long and not a secret; what does not fit is dropped and
/// counted.
pub fn parse(
    reply: &str,
    pairs: &[Pair],
    sources: &[Source],
) -> Result<(Vec<Correction>, u32), String> {
    let raw: RawReply = serde_json::from_str(super::dream::json_object(reply)?)
        .map_err(|e| format!("模型返回的 JSON 读不懂：{e}"))?;
    let mut out: Vec<Correction> = Vec::new();
    let mut dropped = 0;
    for c in raw.corrections {
        let Some(index) = c.pair.checked_sub(1).filter(|i| *i < pairs.len()) else {
            dropped += 1;
            continue;
        };
        let pair = &pairs[index];
        let message = message_of(pair, sources);
        let text = c.text.trim().to_string();
        let span = (normalize(&c.quote).chars().count() >= 2)
            .then(|| find_span(message, &c.quote))
            .flatten();
        let fits = !text.is_empty()
            && text.chars().count() <= MAX_TEXT_CHARS
            && normalize(&text) != normalize(&pair.item.text)
            && secrets::looks_secret(&text).is_none()
            && !out.iter().any(|o| o.pair == index);
        let Some((start, end)) = span.filter(|_| fits) else {
            dropped += 1;
            continue;
        };
        out.push(Correction {
            pair: index,
            quote: message.chars().skip(start).take(end - start).collect(),
            text,
            reason: c.reason.trim().chars().take(200).collect(),
            location: EvidenceLocation {
                message_index: pair.message as u32,
                role: "user".into(),
                start: start as u32,
                end: end as u32,
                message_hash: message_hash(message),
            },
        });
    }
    Ok((out, dropped))
}

/// The evidence a correction carries: the user's words, where they are.
pub fn evidence(
    correction: &Correction,
    source: &Source,
    model: Option<&str>,
    now_ms: u64,
) -> Evidence {
    let mut proof = super::evidence(
        Some(&source.task_id),
        Some("analysis"),
        model,
        &correction.quote,
        now_ms,
    );
    proof.location = Some(correction.location.clone());
    proof
}

/// The checked set after `asked` were asked about: the newest kept.
pub(super) fn remember_checked(old: Option<String>, asked: &[String]) -> String {
    let mut list: Vec<String> = old
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    for key in asked {
        if !list.contains(key) {
            list.push(key.clone());
        }
    }
    let skip = list.len().saturating_sub(CHECKED_KEEP);
    serde_json::to_string(&list[skip..]).unwrap_or_else(|_| "[]".into())
}

pub(super) fn read_checked(value: Option<String>) -> HashSet<String> {
    value
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "contradict_tests.rs"]
mod tests;
