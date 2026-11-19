//! C1 (2026-11-19): where a memory's words came from, and what became of
//! that place. At extraction the words (already checked to be in the
//! conversation) are matched against each message with the same comparison
//! the check uses; the model's own say-so about where is not trusted. A
//! lookup later reads the session again and says whether the message is
//! still there unchanged, changed with the words still found, changed with
//! them gone, or the session gone.

use aeroric_protocol::memory::{
    Evidence, EvidenceLocation, EvidenceLookup, EvidenceMessage, EvidenceStatus,
};
use sha2::{Digest, Sha256};

use super::{fold, normalize};

/// A long message is shown as this many chars around the words.
pub const WINDOW_CHARS: usize = 4_000;
/// Chars kept before the words in a window.
const WINDOW_LEAD: usize = 1_500;

/// sha256 of a message's text, first 16 hex: its version.
pub fn message_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Where `words` are in `text`, as char offsets (end exclusive), compared
/// the way extraction checks a quote: whitespace, width and case aside.
pub fn find_span(text: &str, words: &str) -> Option<(usize, usize)> {
    let needle = normalize(words);
    let wanted = needle.chars().count();
    if wanted < 2 {
        return None;
    }
    let mut hay = String::new();
    let mut origin = Vec::new();
    for (index, c) in text.chars().enumerate() {
        for folded in fold(c) {
            hay.push(folded);
            origin.push(index);
        }
    }
    let at = hay.find(&needle)?;
    let first = hay[..at].chars().count();
    Some((origin[first], origin[first + wanted - 1] + 1))
}

/// The message `words` came from: the user's messages first (a quote is the
/// user's), then the rest. `None` when no single message holds them.
pub fn locate(messages: &[(String, String)], words: &str) -> Option<EvidenceLocation> {
    let user = messages
        .iter()
        .enumerate()
        .filter(|(_, (r, _))| r == "user");
    let rest = messages
        .iter()
        .enumerate()
        .filter(|(_, (r, _))| r != "user");
    user.chain(rest).find_map(|(index, (role, text))| {
        let (start, end) = find_span(text, words)?;
        Some(EvidenceLocation {
            message_index: index as u32,
            role: role.clone(),
            start: start as u32,
            end: end as u32,
            message_hash: message_hash(text),
        })
    })
}

/// What became of `evidence`'s source, given the session's messages now
/// (`None`: the session is gone; empty: its history is).
pub fn lookup(evidence: &Evidence, messages: Option<&[(String, String)]>) -> EvidenceLookup {
    let answer = |status, message| EvidenceLookup {
        status,
        task_id: evidence.task_id.clone(),
        excerpt: evidence.excerpt.clone(),
        message,
    };
    let Some(place) = &evidence.location else {
        return answer(EvidenceStatus::Unrecorded, None);
    };
    let messages = match messages {
        Some(messages) if !messages.is_empty() => messages,
        _ => return answer(EvidenceStatus::Gone, None),
    };
    if let Some((role, text)) = messages.get(place.message_index as usize) {
        if *role == place.role && message_hash(text) == place.message_hash {
            let span = (place.start as usize, place.end as usize);
            return answer(
                EvidenceStatus::Located,
                Some(window(place.message_index, role, text, span)),
            );
        }
    }
    match locate(messages, &evidence.excerpt) {
        Some(found) => {
            let (role, text) = &messages[found.message_index as usize];
            let span = (found.start as usize, found.end as usize);
            answer(
                EvidenceStatus::Moved,
                Some(window(found.message_index, role, text, span)),
            )
        }
        None => answer(EvidenceStatus::Changed, None),
    }
}

/// The message, or a window of it around the words.
fn window(index: u32, role: &str, text: &str, (start, end): (usize, usize)) -> EvidenceMessage {
    let count = text.chars().count();
    let (from, to) = if count <= WINDOW_CHARS {
        (0, count)
    } else {
        let from = start.saturating_sub(WINDOW_LEAD).min(count - WINDOW_CHARS);
        (from, (from + WINDOW_CHARS).max(end).min(count))
    };
    EvidenceMessage {
        message_index: index,
        role: role.to_string(),
        text: text.chars().skip(from).take(to - from).collect(),
        start: (start - from) as u32,
        end: (end.min(to) - from) as u32,
        cut_before: from > 0,
        cut_after: to < count,
    }
}

#[cfg(test)]
#[path = "locate_tests.rs"]
mod tests;
