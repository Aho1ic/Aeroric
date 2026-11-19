//! E3 (2026-11-19): a round against the one before. Each finding of this
//! round is new or says again one of the last round's (the same code, or the
//! same words on the same file); each adopted finding of the last round that
//! was not said again is resolved only when its file was in front of the
//! reviewer this round, and unchecked otherwise.
//!
//! From round 2 on, a round reviews what the writer's answer changed and
//! nothing else, so a file the answer left alone is never looked at again:
//! its findings are "not re-reviewed", never "resolved". The idea is OCR's
//! session compare (OCR #854: re-reviewing the whole range every push).

use aeroric_protocol::review::{
    CoverageState, Decision, FileCoverage, Finding, ReviewCard, ReviewCoverage, RoundCompare,
};

use super::{coverage, locate};

/// Mark this round's `findings` that say a previous one again, and sort the
/// rest of the previous round's adopted findings into resolved and unchecked.
pub fn compare(
    previous: &ReviewCard,
    findings: &mut [Finding],
    covered: Option<&ReviewCoverage>,
) -> RoundCompare {
    let mut out = RoundCompare {
        previous_round: previous.round,
        ..RoundCompare::default()
    };
    for old in &previous.findings {
        let key = locate::identity(old);
        let again = findings
            .iter_mut()
            .find(|f| f.repeats.is_none() && (locate::identity(f) == key || same_words(old, f)));
        if let Some(f) = again {
            f.repeats = Some(old.id.clone());
            continue;
        }
        // Rejected and let go: the writer's answer stands, nothing was fixed.
        if old.decision != Some(Decision::Adopt) {
            continue;
        }
        let kept = Finding {
            repeats: None,
            ..old.clone()
        };
        if rechecked(old, covered) {
            out.resolved.push(kept);
        } else {
            out.unchecked.push(kept);
        }
    }
    out
}

/// The same file and the same words, whitespace and case aside.
fn same_words(a: &Finding, b: &Finding) -> bool {
    a.file == b.file && words(&a.text) == words(&b.text)
}

fn words(text: &str) -> String {
    locate::normalize(text).to_lowercase()
}

/// Its file was reviewed or seen this round; without a file, every file in
/// scope was.
fn rechecked(old: &Finding, covered: Option<&ReviewCoverage>) -> bool {
    let Some(covered) = covered else {
        return false;
    };
    match &old.file {
        Some(file) => covered.files.iter().any(|f| {
            coverage::same_path(file, &f.path)
                && matches!(f.state, FileCoverage::Reviewed | FileCoverage::Seen)
        }),
        None => covered.state == CoverageState::Complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::review::{CoveredFile, Severity};

    fn finding(id: &str, file: Option<&str>, snippet: Option<&str>, text: &str) -> Finding {
        Finding {
            id: id.into(),
            severity: Severity::Blocker,
            text: text.into(),
            file: file.map(str::to_string),
            snippet: snippet.map(str::to_string),
            decision: Some(Decision::Adopt),
            ..Finding::default()
        }
    }

    fn covered(state: CoverageState, files: &[(&str, FileCoverage)]) -> ReviewCoverage {
        ReviewCoverage {
            state,
            files: files
                .iter()
                .map(|(path, state)| CoveredFile {
                    path: (*path).into(),
                    state: *state,
                    reason: None,
                })
                .collect(),
        }
    }

    #[test]
    fn each_old_finding_persists_resolves_or_goes_unchecked() {
        let mut rejected = finding("r5", Some("src/a.rs"), None, "命名");
        rejected.decision = Some(Decision::Reject);
        let previous = ReviewCard {
            round: 1,
            findings: vec![
                finding("r1", Some("src/a.rs"), Some("retry();"), "重试没有上限"),
                finding("r2", Some("src/a.rs"), Some("sleep(0);"), "忙等"),
                finding("r3", Some("src/b.rs"), Some("unwrap()"), "会崩"),
                finding("r4", None, None, "缺测试"),
                rejected,
            ],
            ..ReviewCard::default()
        };
        let mut now = vec![
            // r1 again, other words, re-indented code.
            finding("r1", Some("src/a.rs"), Some("  retry();"), "还是没有上限"),
            // r3 again in the same words, nothing quoted.
            finding("r2", Some("src/b.rs"), None, "会崩 "),
            finding("r3", Some("src/a.rs"), Some("log();"), "新问题"),
        ];
        // a.rs reviewed this round; b.rs was not changed by the answer.
        let cov = covered(
            CoverageState::Complete,
            &[("src/a.rs", FileCoverage::Reviewed)],
        );
        let cmp = compare(&previous, &mut now, Some(&cov));
        let repeats: Vec<_> = now.iter().map(|f| f.repeats.as_deref()).collect();
        assert_eq!(repeats, [Some("r1"), Some("r3"), None]);
        let ids = |list: &[Finding]| list.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
        // r2 sat in a file reviewed again; r4 had no file and the round was
        // complete. The rejected r5 is neither.
        assert_eq!(ids(&cmp.resolved), ["r2", "r4"]);
        assert!(cmp.unchecked.is_empty());
        assert_eq!(cmp.previous_round, 1);

        // b.rs's finding not said again, and b.rs never in front of the
        // reviewer: not re-reviewed, never resolved.
        let mut quiet = vec![];
        let partial = covered(
            CoverageState::Partial,
            &[
                ("src/a.rs", FileCoverage::Reviewed),
                ("src/b.rs", FileCoverage::Missed),
            ],
        );
        let cmp = compare(&previous, &mut quiet, Some(&partial));
        assert_eq!(ids(&cmp.resolved), ["r1", "r2"]);
        assert_eq!(ids(&cmp.unchecked), ["r3", "r4"]);
        let cmp = compare(&previous, &mut quiet, None);
        assert_eq!(cmp.unchecked.len(), 4, "no coverage, nothing rechecked");
    }
}
