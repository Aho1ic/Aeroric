//! Turning a raw PTY stream into text that is safe to paste into another agent.
//!
//! Two jobs, and they are not the same one:
//!
//! [`strip_terminal_control_sequences`] is the *safety* rule. A handoff prompt is typed
//! into another agent's live PTY, so an escape sequence that survives the copy is
//! executable control input in someone else's terminal — the transcript may contain
//! anything the agent printed, including things a hostile repo made it print.
//!
//! [`sanitize_terminal_history_for_handoff`] is the *legibility* rule. An agent TUI
//! redraws its spinner by moving the cursor, not by writing newlines, so the raw history
//! is one enormous line of overlapping frames ("WWo•Wor•WorkWorki"). Splitting on the
//! cursor-movement finals first is what makes the frames lines at all; dropping the
//! spinner fragments and the status bar is what makes them worth reading.
//!
//! Both are ports of `src/sessionHandoff.ts`, byte-for-byte on behaviour.

use std::collections::{HashSet, VecDeque};

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
const C1_CSI: char = '\u{9b}';

/// CSI parameter / intermediate / final byte classes, per ECMA-48 as the client's regex
/// reads them.
fn is_csi_parameter(c: char) -> bool {
    ('\u{30}'..='\u{3f}').contains(&c)
}
fn is_csi_intermediate(c: char) -> bool {
    ('\u{20}'..='\u{2f}').contains(&c)
}
fn is_csi_final(c: char) -> bool {
    ('\u{40}'..='\u{7e}').contains(&c)
}
fn is_esc_sequence_tail(c: char) -> bool {
    // `[@-Z\\-_]`: '@'..'Z' plus the range '\'..'_'.
    ('\u{40}'..='\u{5a}').contains(&c) || ('\u{5c}'..='\u{5f}').contains(&c)
}
fn is_esc_designator(c: char) -> bool {
    // `[0-2A-Z]`
    ('\u{30}'..='\u{32}').contains(&c) || c.is_ascii_uppercase()
}
fn is_c0_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{08}' | '\u{0b}' | '\u{0c}' | '\u{0e}'..='\u{1f}' | '\u{7f}')
}
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200d}' | '\u{feff}')
}

/// Drop OSC sequences: `ESC ]` up to the first BEL or `ESC \`. An unterminated OSC
/// matches nothing and is left alone — inventing a terminator would delete text.
fn drop_osc(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == ESC && index + 1 < chars.len() && chars[index + 1] == ']' {
            let mut cursor = index + 2;
            let mut terminated = None;
            while cursor < chars.len() {
                if chars[cursor] == BEL {
                    terminated = Some(cursor + 1);
                    break;
                }
                if chars[cursor] == ESC && cursor + 1 < chars.len() && chars[cursor + 1] == '\\' {
                    terminated = Some(cursor + 2);
                    break;
                }
                cursor += 1;
            }
            match terminated {
                Some(next) => {
                    index = next;
                    continue;
                }
                None => {
                    out.push(chars[index]);
                    index += 1;
                }
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Drop `ESC [` … final / C1 CSI sequences. `split` keeps the final byte as `\n` for the
/// cursor-moving ones, which is what makes redraw frames into lines.
fn rewrite_csi(chars: &[char], split: bool) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        let introducer_len =
            if chars[index] == ESC && index + 1 < chars.len() && chars[index + 1] == '[' {
                Some(2)
            } else if chars[index] == C1_CSI {
                Some(1)
            } else {
                None
            };
        let Some(introducer_len) = introducer_len else {
            out.push(chars[index]);
            index += 1;
            continue;
        };
        // `[0-?]*[ -/]*[@-~]` — the three classes in order, and a final byte must exist.
        let mut cursor = index + introducer_len;
        while cursor < chars.len() && is_csi_parameter(chars[cursor]) {
            cursor += 1;
        }
        while cursor < chars.len() && is_csi_intermediate(chars[cursor]) {
            cursor += 1;
        }
        if cursor < chars.len() && is_csi_final(chars[cursor]) {
            if split && LINE_BREAKING_CSI_FINALS.contains(&chars[cursor]) {
                out.push('\n');
            }
            index = cursor + 1;
            continue;
        }
        // No final byte: the regex matches nothing here, so the bytes stay.
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Cursor-moving and erasing finals. An agent TUI repaints its spinner with these instead
/// of writing `\r` or `\n`; without a split every frame concatenates.
const LINE_BREAKING_CSI_FINALS: &[char] = &[
    'A', 'B', 'E', 'F', 'H', 'f', 'd', 'J', 'K', 'L', 'M', 'S', 'T',
];

/// Drop `ESC`-initiated non-CSI sequences.
fn drop_esc_sequences(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != ESC {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Alt 1: `[()][0-2A-Z]`
        if index + 2 < chars.len()
            && matches!(chars[index + 1], '(' | ')')
            && is_esc_designator(chars[index + 2])
        {
            index += 3;
            continue;
        }
        // Alt 2: `[ -/]*[@-Z\\-_]`
        let mut cursor = index + 1;
        while cursor < chars.len() && is_csi_intermediate(chars[cursor]) {
            cursor += 1;
        }
        if cursor < chars.len() && is_esc_sequence_tail(chars[cursor]) {
            index = cursor + 1;
            continue;
        }
        // A lone ESC is not a control sequence and is not in the C0 class either: it stays.
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn drop_classes(chars: &[char], predicate: impl Fn(char) -> bool) -> Vec<char> {
    chars.iter().copied().filter(|c| !predicate(*c)).collect()
}

/// `\r\n` → `\n`, then every remaining `\r` means "this row was overwritten": keep only
/// what came after the last one. That is what a terminal would have shown.
fn normalize_carriage_returns(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| match line.rfind('\r') {
            Some(at) => &line[at + '\r'.len_utf8()..],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `stripTerminalControlSequences`.
pub fn strip_terminal_control_sequences(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let chars = drop_osc(&chars);
    let chars = rewrite_csi(&chars, false);
    let chars = drop_esc_sequences(&chars);
    let chars = drop_classes(&chars, is_c0_control);
    let chars = drop_classes(&chars, is_zero_width);
    normalize_carriage_returns(&chars.into_iter().collect::<String>())
}

const SPINNER_LABELS: &[&str] = &[
    "Working",
    "Reconnecting",
    "Exploring",
    "Explored",
    "Thinking",
    "Waiting",
    "Running",
    "Ran",
];

/// Every contiguous slice of every spinner label: a redraw frame can start or end mid
/// word, so "orkin" has to be recognised as noise on its own.
fn spinner_fragments() -> HashSet<String> {
    let mut set = HashSet::new();
    for label in SPINNER_LABELS {
        let chars: Vec<char> = label.chars().collect();
        for start in 0..chars.len() {
            for end in (start + 1)..=chars.len() {
                set.insert(chars[start..end].iter().collect());
            }
        }
    }
    set
}

const STATUS_BAR_MARKERS: &[&str] = &["esc to interrupt", "esc to view transcript"];

/// Redraw frames repeat the same rows many times, but not back to back, so a
/// previous-line comparison cannot collapse them. The window is bounded so a long session
/// cannot hold every line it ever printed.
const DEDUPE_WINDOW_LINES: usize = 400;

/// `sanitizeTerminalHistoryForHandoff`.
pub fn sanitize_terminal_history_for_handoff(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let chars = drop_osc(&chars);
    let chars = rewrite_csi(&chars, true);
    let chars = drop_esc_sequences(&chars);
    let chars = drop_classes(&chars, is_c0_control);
    let chars = drop_classes(&chars, is_zero_width);
    let cleaned = normalize_carriage_returns(&chars.into_iter().collect::<String>());

    let fragments = spinner_fragments();
    let mut lines = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: VecDeque<String> = VecDeque::new();

    for raw in cleaned.split('\n') {
        let line = raw.trim_end_matches([' ', '\t']).to_string();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Terminal art and redraw fragments carry no words and no numbers.
        if !trimmed
            .chars()
            .any(|c| c.is_ascii_alphanumeric() || ('\u{3400}'..='\u{9fff}').contains(&c))
        {
            continue;
        }
        if STATUS_BAR_MARKERS
            .iter()
            .any(|marker| trimmed.contains(marker))
        {
            continue;
        }
        if is_spinner_frame_with(&fragments, trimmed) {
            continue;
        }
        let key = trimmed.to_string();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        order.push_back(key);
        if order.len() > DEDUPE_WINDOW_LINES {
            if let Some(evicted) = order.pop_front() {
                seen.remove(&evicted);
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn is_spinner_frame_with(fragments: &HashSet<String>, line: &str) -> bool {
    let mut words = 0usize;
    let mut buffer = String::new();
    for c in line.chars().chain(std::iter::once('\0')) {
        if c.is_ascii_alphabetic() {
            buffer.push(c);
            continue;
        }
        if buffer.is_empty() {
            continue;
        }
        words += 1;
        if !fragments
            .iter()
            .any(|fragment| fragment.eq_ignore_ascii_case(&buffer))
        {
            return false;
        }
        buffer.clear();
    }
    words > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_survives_untouched() {
        let text = "cargo test --locked\n10 passed\n";
        assert_eq!(strip_terminal_control_sequences(text), text);
    }

    #[test]
    fn an_injected_control_sequence_is_not_passed_on() {
        // The reason this function exists: a transcript line that clears the screen or
        // sets a window title must arrive as text, not as an instruction.
        assert_eq!(
            strip_terminal_control_sequences("hi\u{1b}[2J\u{1b}]0;pwn\u{7}bye"),
            "hibye"
        );
        assert_eq!(
            strip_terminal_control_sequences("\u{1b}(Bnormal"),
            "normal",
            "charset designation"
        );
        assert_eq!(
            strip_terminal_control_sequences("\u{9b}31mred"),
            "red",
            "C1 introducer"
        );
        assert_eq!(
            strip_terminal_control_sequences("a\u{7}b\u{0}c"),
            "abc",
            "C0 controls"
        );
        assert_eq!(
            strip_terminal_control_sequences("x\u{200b}y\u{feff}z"),
            "xyz",
            "zero-width"
        );
    }

    #[test]
    fn an_unterminated_osc_loses_only_the_introducer_it_can_be_read_as() {
        // `ESC ]` with no terminator is not an OSC sequence to the client's regex, so
        // nothing is deleted there. What remains is that `ESC` matched the *ordinary*
        // escape-sequence rule one line later, because `]` sits inside `[@-Z\\-_]`. The
        // answer is "a0:title" and it was measured from `src/sessionHandoff.ts`, not
        // reasoned out — a rule this accidental is exactly what a rewrite gets wrong.
        assert_eq!(
            strip_terminal_control_sequences("a\u{1b}]0:title"),
            "a0:title"
        );
        // Deleting to end-of-input would eat the rest of the transcript.
        assert!(!strip_terminal_control_sequences("a\u{1b}]0:title").is_empty());
    }

    #[test]
    fn a_carriage_return_means_the_row_was_overwritten() {
        assert_eq!(
            strip_terminal_control_sequences("first\rsecond\r\nnext"),
            "second\nnext"
        );
        assert_eq!(
            strip_terminal_control_sequences("keep\r\nplain"),
            "keep\nplain"
        );
    }

    #[test]
    fn redraw_frames_become_lines_and_then_disappear() {
        let mut raw = String::from("W");
        for label in ["Wo", "Wor", "Work", "Worki", "Working"] {
            raw.push_str("\u{1b}[1A\r");
            raw.push_str(label);
        }
        let clean = sanitize_terminal_history_for_handoff(&raw);
        assert!(
            !clean.contains("Working"),
            "spinner fragments are noise: {clean:?}"
        );
        assert!(
            !clean.contains("WWo"),
            "frames must not concatenate: {clean:?}"
        );
    }

    #[test]
    fn a_status_bar_line_is_dropped_but_its_surrounding_content_is_not() {
        let raw = "the real answer\u{1b}[2K\nesc to interrupt\nanother real line\n";
        assert_eq!(
            sanitize_terminal_history_for_handoff(raw),
            "the real answer\nanother real line"
        );
    }

    #[test]
    fn repeated_frames_collapse_within_the_window_and_reappear_after_it() {
        let mut raw = String::new();
        raw.push_str("milestone one\n");
        for index in 0..(DEDUPE_WINDOW_LINES - 1) {
            raw.push_str(&format!("noise line {index}\n"));
        }
        raw.push_str("milestone one\n");
        let clean = sanitize_terminal_history_for_handoff(&raw);
        let occurrences = clean.matches("milestone one").count();
        assert_eq!(occurrences, 1, "still inside the dedupe window");

        let mut raw = String::new();
        raw.push_str("milestone one\n");
        for index in 0..(DEDUPE_WINDOW_LINES + 2) {
            raw.push_str(&format!("noise line {index}\n"));
        }
        raw.push_str("milestone one\n");
        let clean = sanitize_terminal_history_for_handoff(&raw);
        assert_eq!(
            clean.matches("milestone one").count(),
            2,
            "the window is bounded, so an old line is forgotten rather than held forever"
        );
    }

    #[test]
    fn cjk_and_numeric_rows_are_never_spinner_noise() {
        let raw = "Working\n编译完成\n42\n";
        let clean = sanitize_terminal_history_for_handoff(raw);
        assert!(clean.contains("编译完成"), "{clean:?}");
        assert!(clean.contains("42"), "{clean:?}");
        assert!(!clean.contains("Working"), "{clean:?}");
    }

    #[test]
    fn a_real_word_that_happens_to_start_like_a_label_survives() {
        // " Ran" is a spinner label, "Ran across the road" is not: one unknown word is
        // enough to keep the line.
        let fragments = spinner_fragments();
        assert!(is_spinner_frame_with(&fragments, "Ran"));
        assert!(!is_spinner_frame_with(&fragments, "Ran across the road"));
        assert!(
            !is_spinner_frame_with(&fragments, "!!!"),
            "no latin words at all is not a frame"
        );
    }

    #[test]
    fn trailing_whitespace_is_trimmed_but_leading_indent_is_not() {
        let raw = "  code  \t\nnext\n";
        assert_eq!(sanitize_terminal_history_for_handoff(raw), "  code\nnext");
    }
}
