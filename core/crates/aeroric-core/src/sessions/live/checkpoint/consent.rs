//! R1-3b (2026-11-04): what the user plainly agreed to — or refused — about
//! committing, pushing and releasing, read from their own messages by rule.
//! It goes into the handoff brief as reference only and grants nothing.
//!
//! Agreement is read narrowly (short imperative clauses only), refusal
//! broadly: a missed refusal costs more than a missed agreement.

use aeroric_protocol::checkpoint::{Consent, ConsentKind, Stance};

/// The quote kept per record.
const MAX_QUOTE_CHARS: usize = 60;
/// A clause longer than this (punctuation and spaces aside) is a description, not an instruction.
const MAX_AGREE_CHARS: usize = 12;
/// Refusals kept per kind.
const MAX_REFUSALS: usize = 5;

const KEYWORDS: &[(ConsentKind, &[&str])] = &[
    (ConsentKind::Commit, &["提交", "commit"]),
    (ConsentKind::Push, &["推送", "push"]),
    (
        ConsentKind::Release,
        &["发布", "上线", "release", "publish"],
    ),
];
/// Before the keyword, these make the clause a refusal.
const REFUSALS: &[&str] = &[
    "不要", "别", "先别", "不用", "暂不", "不许", "不准", "不", "勿", "don't", "dont", "do not",
    "never", "no",
];
/// Anywhere in the clause, these keep it from reading as agreement.
const HEDGES: &[&str] = &[
    "不", "别", "没", "勿", "未", "don't", "dont", "not", "never", "no",
];
/// A clause with these describes the action rather than asking for it.
const DESCRIPTIVE: &[&str] = &["前", "后", "以后", "的时候", "的"];
const QUESTIONS: &[&str] = &["要不要", "是否", "能不能", "可不可以", "可以吗"];
const SPLIT: &[char] = &[
    '。', '，', ',', '；', ';', '！', '!', '？', '?', '\n', '、', '.', '：', ':',
];

/// The user's own words in a "user" message, or `None` when Aeroric or the
/// agent put it there (a status reminder's own lines are cut off the front).
pub fn typed_text(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let text = match text.strip_prefix("[Aeroric 状态提醒]") {
        Some(rest) => rest.split_once("\n\n").map(|(_, typed)| typed)?,
        None => text,
    };
    let typed = text.trim();
    let injected = typed.starts_with("[Aeroric")
        || typed.starts_with("# 交接清单")
        || crate::sessions::codex::is_injected_context(typed);
    (!typed.is_empty() && !injected).then_some(typed)
}

/// Each kind the message speaks to, with the stance its last clause on it takes.
pub fn scan(text: &str) -> Vec<(ConsentKind, Stance)> {
    let mut found: Vec<(ConsentKind, Stance)> = Vec::new();
    for (clause, question) in clauses(text) {
        let lower = clause.to_lowercase();
        if question || QUESTIONS.iter().any(|q| lower.contains(q)) {
            continue;
        }
        for (kind, words) in KEYWORDS {
            let Some(at) = words.iter().filter_map(|w| find_word(&lower, w)).min() else {
                continue;
            };
            let stance = if REFUSALS
                .iter()
                .any(|r| find_word(&lower[..at], r).is_some())
            {
                Some(Stance::No)
            } else if agrees(&lower) {
                Some(Stance::Yes)
            } else {
                None
            };
            if let Some(stance) = stance {
                found.retain(|(k, _)| k != kind);
                found.push((*kind, stance));
            }
        }
    }
    found
}

/// The message as quoted in a record.
pub fn quote(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_QUOTE_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(MAX_QUOTE_CHARS).collect();
    cut.push('…');
    cut
}

/// Add a record unless the same words were already noted this turn (the
/// sender and the session's own echo both report a message). Keeps, per
/// kind, the latest agreement and the refusals after it.
pub fn record(list: &mut Vec<Consent>, new: Consent) -> bool {
    if list.iter().any(|c| {
        c.kind == new.kind && c.stance == new.stance && c.turn == new.turn && c.quote == new.quote
    }) {
        return false;
    }
    let kind = new.kind;
    list.push(new);
    let last_yes = list
        .iter()
        .rposition(|c| c.kind == kind && c.stance == Stance::Yes);
    let mut refusals_left = MAX_REFUSALS;
    let mut keep: Vec<bool> = vec![true; list.len()];
    for (i, c) in list.iter().enumerate().rev() {
        if c.kind != kind {
            continue;
        }
        keep[i] = match (c.stance, last_yes) {
            (Stance::Yes, Some(yes)) => i == yes,
            (Stance::No, Some(yes)) if i < yes => false,
            (Stance::No, _) if refusals_left > 0 => {
                refusals_left -= 1;
                true
            }
            _ => false,
        };
    }
    let mut keep = keep.into_iter();
    list.retain(|_| keep.next().unwrap_or(true));
    true
}

/// Clauses with whether each one asked a question.
fn clauses(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if SPLIT.contains(&ch) {
            let question = matches!(ch, '?' | '？');
            push_clause(&mut out, &mut current, question);
        } else {
            current.push(ch);
        }
    }
    push_clause(&mut out, &mut current, false);
    out
}

fn push_clause(out: &mut Vec<(String, bool)>, current: &mut String, question: bool) {
    let clause = current.trim().to_string();
    current.clear();
    if clause.is_empty() {
        return;
    }
    let question =
        question || clause.ends_with('吗') || clause.ends_with('么') || clause.ends_with('呢');
    out.push((clause, question));
}

fn agrees(clause: &str) -> bool {
    let length = clause
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation())
        .count();
    length <= MAX_AGREE_CHARS
        && !HEDGES.iter().any(|h| find_word(clause, h).is_some())
        && !DESCRIPTIVE.iter().any(|d| clause.contains(d))
}

/// Where `word` starts in `hay`; an ASCII word must stand alone ("no" is not in "now").
fn find_word(hay: &str, word: &str) -> Option<usize> {
    if !word.is_ascii() {
        return hay.find(word);
    }
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(found) = hay[from..].find(word) {
        let start = from + found;
        let end = start + word.len();
        let before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before && after {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stances(text: &str) -> Vec<(ConsentKind, Stance)> {
        scan(text)
    }

    #[test]
    fn short_imperatives_agree() {
        use ConsentKind::*;
        assert_eq!(stances("目检通过，提交"), vec![(Commit, Stance::Yes)]);
        assert_eq!(stances("推送"), vec![(Push, Stance::Yes)]);
        assert_eq!(stances("可以发布了"), vec![(Release, Stance::Yes)]);
        assert_eq!(stances("ok, push it"), vec![(Push, Stance::Yes)]);
    }

    #[test]
    fn descriptions_and_questions_do_not() {
        for text in [
            "提交前先跑测试",
            "要不要推送？",
            "可以提交吗",
            "这次提交的内容有点多，你帮我把测试也补上",
            "提交不了",
            "I committed it yesterday",
            "now we can talk",
        ] {
            assert!(stances(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn refusals_are_read_broadly() {
        use ConsentKind::*;
        assert_eq!(stances("先别推送，等我看完"), vec![(Push, Stance::No)]);
        assert_eq!(
            stances("不要提交到 main 上，我还要再看看这几个文件的改动"),
            vec![(Commit, Stance::No)]
        );
        assert_eq!(stances("don't push yet"), vec![(Push, Stance::No)]);
        assert_eq!(
            stances("提交，先别推送"),
            vec![(Commit, Stance::Yes), (Push, Stance::No)]
        );
    }

    #[test]
    fn injected_text_is_not_the_users() {
        assert_eq!(
            typed_text("<environment_context>提交</environment_context>"),
            None
        );
        assert_eq!(typed_text("[Aeroric 委派] 提交"), None);
        assert_eq!(
            typed_text("[Aeroric 状态提醒]\n- 未完成：x\n\n推送"),
            Some("推送")
        );
    }

    fn consent(kind: ConsentKind, stance: Stance, quote: &str, turn: u64) -> Consent {
        Consent {
            kind,
            stance,
            quote: quote.into(),
            turn,
            seq: None,
            at_ms: turn,
        }
    }

    #[test]
    fn the_latest_agreement_and_later_refusals_are_kept() {
        use ConsentKind::*;
        let mut list = Vec::new();
        assert!(record(&mut list, consent(Push, Stance::No, "别推送", 1)));
        assert!(record(&mut list, consent(Push, Stance::Yes, "推送", 2)));
        assert!(record(&mut list, consent(Commit, Stance::Yes, "提交", 3)));
        assert!(!record(&mut list, consent(Commit, Stance::Yes, "提交", 3)));
        assert!(record(&mut list, consent(Push, Stance::Yes, "推送吧", 4)));
        assert!(record(&mut list, consent(Push, Stance::No, "先别推送", 5)));
        let kept: Vec<_> = list.iter().map(|c| (c.kind, c.stance, c.turn)).collect();
        assert_eq!(
            kept,
            vec![
                (Commit, Stance::Yes, 3),
                (Push, Stance::Yes, 4),
                (Push, Stance::No, 5)
            ]
        );
    }
}
