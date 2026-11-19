//! E1 (2026-11-19): which of a round's changed files the reviewer accounted
//! for. The host picks the files in scope (leaving out secrets and binaries,
//! each with its reason); the reviewer says per file whether it reviewed or
//! skipped it; a file it did not mention still counts as seen when it was in
//! the prompt or the reviewer opened it. The round's state follows from the
//! files alone, never from the verdict.

use aeroric_protocol::review::{CoverageState, CoveredFile, FileCoverage, ReviewCoverage};
use serde::{Deserialize, Serialize};

use super::TurnDiff;

pub const SECRET: &str = "可能含有凭据，没有发给审查者";
pub const BINARY: &str = "二进制文件";
pub const NO_REASON: &str = "审查者没有写原因";
/// A pass that rests on no file at all.
pub const NOT_REVIEWED: &str = "审查者没有看任何改动的文件，这一轮不能算通过";

/// What a round put in front of the reviewer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// In scope, in diff order.
    pub selected: Vec<String>,
    /// Left out: (path, why).
    pub excluded: Vec<(String, String)>,
    /// The selected files' diff was in the prompt.
    pub inlined: bool,
    /// E4: the ids of the review rules the prompt carried.
    #[serde(default)]
    pub rules: Vec<String>,
}

/// What the reviewer said about one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub path: String,
    pub reviewed: bool,
    pub reason: Option<String>,
}

/// The round's scope: every changed file but secrets and binaries.
pub fn plan(diff: &TurnDiff) -> Plan {
    let mut plan = Plan::default();
    for file in &diff.files {
        let why = if secret(&file.path) {
            Some(SECRET)
        } else if file.additions.is_none() && file.deletions.is_none() {
            Some(BINARY)
        } else {
            None
        };
        match why {
            Some(why) => plan.excluded.push((file.path.clone(), why.to_string())),
            None => plan.selected.push(file.path.clone()),
        }
    }
    plan
}

/// A file that may hold credentials: `.env` (not its templates), private
/// keys, certificates and login files.
pub fn secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if name == ".env" || name.starts_with(".env.") {
        return ![".example", ".sample", ".template", ".dist"]
            .iter()
            .any(|t| name.ends_with(t));
    }
    matches!(
        name.as_str(),
        "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" | ".netrc" | ".pgpass" | ".git-credentials"
    ) || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Each file's end, and the round's state. `opened`: what the reviewer's
/// tool calls named (commands, paths), searched for each file's path.
pub fn judge(plan: &Plan, claims: Option<&[Claim]>, opened: &[String]) -> ReviewCoverage {
    let mut files = Vec::new();
    for path in &plan.selected {
        let claim = claims.and_then(|c| c.iter().find(|c| same_path(&c.path, path)));
        let (state, reason) = match claim {
            Some(c) if c.reviewed => (FileCoverage::Reviewed, None),
            Some(c) => (
                FileCoverage::Skipped,
                Some(c.reason.clone().unwrap_or_else(|| NO_REASON.to_string())),
            ),
            None if plan.inlined || opened.iter().any(|o| o.contains(path.as_str())) => {
                (FileCoverage::Seen, None)
            }
            None => (FileCoverage::Missed, None),
        };
        files.push(CoveredFile {
            path: path.clone(),
            state,
            reason,
        });
    }
    let looked = files
        .iter()
        .filter(|f| matches!(f.state, FileCoverage::Reviewed | FileCoverage::Seen))
        .count();
    let state = if files.is_empty() {
        CoverageState::Empty
    } else if looked == 0 {
        CoverageState::Unreviewed
    } else if looked == files.len() {
        CoverageState::Complete
    } else {
        CoverageState::Partial
    };
    files.extend(plan.excluded.iter().map(|(path, why)| CoveredFile {
        path: path.clone(),
        state: FileCoverage::Excluded,
        reason: Some(why.clone()),
    }));
    ReviewCoverage { state, files }
}

/// The reviewer may write `./src/a.rs`, `a/src/a.rs` or the path from the
/// repository's parent: the same file when one ends with the other at a
/// path boundary.
pub(super) fn same_path(said: &str, path: &str) -> bool {
    let said = said.trim().trim_start_matches("./");
    said == path
        || said
            .strip_suffix(path)
            .is_some_and(|head| head.ends_with('/'))
        || path
            .strip_suffix(said)
            .is_some_and(|head| head.ends_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::session_live::TurnFileChange;

    fn change(path: &str, lines: Option<u64>) -> TurnFileChange {
        TurnFileChange {
            path: path.into(),
            additions: lines,
            deletions: lines,
        }
    }

    fn diff(files: Vec<TurnFileChange>) -> TurnDiff {
        TurnDiff {
            files,
            ..TurnDiff::default()
        }
    }

    fn claim(path: &str, reviewed: bool, reason: Option<&str>) -> Claim {
        Claim {
            path: path.into(),
            reviewed,
            reason: reason.map(str::to_string),
        }
    }

    #[test]
    fn secrets_and_binaries_are_left_out_with_their_reasons() {
        let plan = plan(&diff(vec![
            change("src/a.rs", Some(1)),
            change("config/.env", Some(1)),
            change(".env.example", Some(1)),
            change("deploy/tls.pem", Some(1)),
            change("logo.png", None),
        ]));
        assert_eq!(plan.selected, ["src/a.rs", ".env.example"]);
        assert_eq!(
            plan.excluded,
            [
                ("config/.env".to_string(), SECRET.to_string()),
                ("deploy/tls.pem".to_string(), SECRET.to_string()),
                ("logo.png".to_string(), BINARY.to_string())
            ]
        );
        assert!(secret("home/.ssh/id_ed25519"));
        assert!(!secret("src/keys.rs"));
    }

    #[test]
    fn each_file_ends_one_way_and_the_state_follows_from_the_files() {
        let mut plan = Plan {
            selected: vec!["src/a.rs".into(), "src/b.rs".into(), "src/c.rs".into()],
            excluded: vec![("x/.env".into(), SECRET.into())],
            inlined: false,
            rules: Vec::new(),
        };
        let claims = [
            claim("./src/a.rs", true, None),
            claim("src/b.rs", false, Some("生成的代码")),
        ];
        let opened = ["git diff abc def -- src/c.rs".to_string()];
        let cov = judge(&plan, Some(&claims), &opened);
        let states: Vec<_> = cov.files.iter().map(|f| f.state).collect();
        assert_eq!(
            states,
            [
                FileCoverage::Reviewed,
                FileCoverage::Skipped,
                FileCoverage::Seen,
                FileCoverage::Excluded
            ]
        );
        assert_eq!(cov.files[1].reason.as_deref(), Some("生成的代码"));
        assert_eq!(cov.state, CoverageState::Partial, "b was skipped");

        // Nothing said, nothing opened, nothing inlined: not reviewed at all.
        let cov = judge(&plan, None, &[]);
        assert_eq!(cov.state, CoverageState::Unreviewed);
        assert!(cov.files[..3]
            .iter()
            .all(|f| f.state == FileCoverage::Missed));

        // The diff was in the prompt: every file at least seen.
        plan.inlined = true;
        let cov = judge(&plan, None, &[]);
        assert_eq!(cov.state, CoverageState::Complete);
        assert!(cov.files[..3].iter().all(|f| f.state == FileCoverage::Seen));

        // A skip without a reason still says so.
        let cov = judge(&plan, Some(&[claim("src/a.rs", false, None)]), &[]);
        assert_eq!(cov.files[0].reason.as_deref(), Some(NO_REASON));
    }

    #[test]
    fn only_excluded_files_leave_nothing_in_scope() {
        let plan = plan(&diff(vec![change(".env", Some(1))]));
        assert_eq!(judge(&plan, None, &[]).state, CoverageState::Empty);
        assert!(same_path("repo/src/a.rs", "src/a.rs"));
        assert!(!same_path("xsrc/a.rs", "src/a.rs"));
    }
}
