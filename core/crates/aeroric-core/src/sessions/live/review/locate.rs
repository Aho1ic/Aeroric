//! E2 (2026-11-19): a finding points at the right line, and says the same
//! thing once. The reviewer quotes the code it means (`snippet`); the host
//! finds that code in the file as the turn left it and moves the line there
//! when the number drifted, or marks the finding unlocated when the code is
//! nowhere. Findings on the same file, of the same kind, about the same
//! code (or with the same words, when none was quoted) fold into one.
//!
//! The idea is OCR's: comment relocation and the finding identity of its
//! session compare (path, category, whitespace-normalized snippet).

use aeroric_protocol::review::Finding;

/// Whitespace runs collapsed, ends trimmed: re-indenting or re-wrapping does
/// not change the code a finding means.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The 1-based line where `snippet` starts in `content`, the one nearest
/// `near` when it appears more than once. Blank lines are skipped on both
/// sides; a one-line snippet may be part of a line.
pub fn locate(content: &str, snippet: &str, near: Option<u64>) -> Option<u64> {
    let wanted: Vec<String> = snippet
        .lines()
        .map(normalize)
        .filter(|l| !l.is_empty())
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let lines: Vec<(usize, String)> = content
        .lines()
        .enumerate()
        .map(|(i, l)| (i, normalize(l)))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    let mut starts: Vec<usize> = lines
        .windows(wanted.len())
        .filter(|w| w.iter().zip(&wanted).all(|((_, have), want)| have == want))
        .map(|w| w[0].0)
        .collect();
    if starts.is_empty() && wanted.len() == 1 && wanted[0].chars().count() >= 4 {
        starts = lines
            .iter()
            .filter(|(_, l)| l.contains(wanted[0].as_str()))
            .map(|(i, _)| *i)
            .collect();
    }
    let line = |i: &usize| *i as u64 + 1;
    match near {
        Some(near) => starts.iter().min_by_key(|i| line(i).abs_diff(near)),
        None => starts.first(),
    }
    .map(line)
}

/// Place each finding that quoted code in a file `read` can open: move its
/// line to where the code is, or mark it unlocated.
pub fn place(findings: &mut [Finding], read: impl Fn(&str) -> Option<String>) {
    for finding in findings.iter_mut() {
        let (Some(file), Some(snippet)) = (&finding.file, &finding.snippet) else {
            continue;
        };
        let Some(content) = read(file) else {
            continue;
        };
        match locate(&content, snippet, finding.line) {
            Some(line) if finding.line != Some(line) => {
                finding.moved_from = finding.line;
                finding.line = Some(line);
            }
            Some(_) => {}
            None => finding.unlocated = true,
        }
    }
}

/// Fold findings that say the same into the first of them, which keeps the
/// most severe level among them and counts the others in `merged`.
pub fn fold(findings: Vec<Finding>) -> Vec<Finding> {
    let mut kept: Vec<(String, Finding)> = Vec::new();
    for finding in findings {
        let key = identity(&finding);
        match kept.iter_mut().find(|(k, _)| *k == key) {
            Some((_, first)) => {
                first.merged += 1 + finding.merged;
                if finding.severity < first.severity {
                    first.severity = finding.severity;
                }
            }
            None => kept.push((key, finding)),
        }
    }
    kept.into_iter().map(|(_, f)| f).collect()
}

/// File, kind, and the code it means (its words when it quoted none).
fn identity(f: &Finding) -> String {
    let body = f
        .snippet
        .as_deref()
        .map(normalize)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| normalize(&f.text).to_lowercase());
    format!(
        "{}\u{0}{}\u{0}{body}",
        f.file.as_deref().unwrap_or_default(),
        f.category.as_deref().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::review::Severity;

    const FILE: &str = "fn a() {\n    retry();\n}\n\nfn b() {\n    retry();\n    loop {\n        call();\n    }\n}\n";

    fn finding(id: &str, line: Option<u64>, snippet: Option<&str>) -> Finding {
        Finding {
            id: id.into(),
            severity: Severity::Major,
            text: "重试没有上限".into(),
            file: Some("src/a.rs".into()),
            line,
            snippet: snippet.map(str::to_string),
            ..Finding::default()
        }
    }

    #[test]
    fn a_drifted_line_moves_to_its_code_and_missing_code_is_marked() {
        // Re-indented and spread over lines differently: still found.
        assert_eq!(locate(FILE, "loop {\n  call();", None), Some(7));
        // Twice in the file: the one nearest the line the reviewer gave.
        assert_eq!(locate(FILE, "retry();", Some(5)), Some(6));
        assert_eq!(locate(FILE, "retry();", Some(1)), Some(2));
        // Part of a line, when it is one line long enough.
        assert_eq!(locate(FILE, "call()", None), Some(8));
        assert_eq!(locate(FILE, "nowhere();", None), None);

        let mut findings = vec![
            finding("r1", Some(3), Some("loop {")),
            finding("r2", Some(2), Some("retry();")),
            finding("r3", Some(9), Some("gone();")),
            finding("r4", Some(9), None),
        ];
        place(&mut findings, |path| {
            (path == "src/a.rs").then(|| FILE.to_string())
        });
        assert_eq!(
            (findings[0].line, findings[0].moved_from),
            (Some(7), Some(3))
        );
        assert_eq!((findings[1].line, findings[1].moved_from), (Some(2), None));
        assert!(findings[2].unlocated);
        assert_eq!(findings[2].line, Some(9), "the reviewer's line is kept");
        assert!(!findings[3].unlocated, "nothing quoted, nothing to check");
    }

    #[test]
    fn findings_about_the_same_code_fold_into_the_most_severe() {
        let mut blocker = finding("r2", Some(6), Some("retry();  "));
        blocker.severity = Severity::Blocker;
        blocker.text = "换个说法：没有退避".into();
        let mut other_kind = finding("r3", Some(6), Some("retry();"));
        other_kind.category = Some("performance".into());
        let same_words = Finding {
            snippet: None,
            text: "重试没有上限  ".into(),
            ..finding("r5", None, None)
        };
        let folded = fold(vec![
            finding("r1", Some(2), Some("retry();")),
            blocker,
            other_kind,
            finding("r4", None, None),
            same_words,
        ]);
        let ids: Vec<_> = folded.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["r1", "r3", "r4"]);
        assert_eq!(folded[0].merged, 1);
        assert_eq!(folded[0].severity, Severity::Blocker, "the strongest stays");
        assert_eq!(folded[0].text, "重试没有上限", "the first one's words");
        assert_eq!(folded[2].merged, 1, "same words, nothing quoted");
    }
}
