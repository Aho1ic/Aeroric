//! R2-3a (2026-11-05): conclusions whose files changed since they were
//! drawn. A turn's changes, a passed criterion and a side branch's result
//! each rest on the files as they were then; when any of those files is no
//! longer that content (a later turn, the user, an undo), the conclusion
//! may be stale. Only reads: computed when the progress page asks, never
//! stored, and a conclusion without a recorded basis is never judged.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aeroric_protocol::checkpoint::{
    Checkpoint, EvidenceKind, ObligationState, SnapshotRef, StaleConclusion, StaleFile,
};
use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

use super::turn_changes::git;
use super::turn_ledger::TurnLedger;

/// A conclusion's basis: each file and the blob it had (`None`: absent).
struct Basis {
    kind: &'static str,
    key: String,
    files: Vec<(String, Option<String>)>,
    /// Changes after this moment are what made it stale.
    since_ms: u64,
    /// The conclusion's own turn, when undoing it is what changed the files.
    own: Option<TurnRecord>,
}

pub fn stale(task_id: &str, cp: &Checkpoint, ledger: &TurnLedger) -> Vec<StaleConclusion> {
    let mut records = ledger.list(task_id);
    records.sort_by_key(|r| r.started_ms);
    let Some(top) = records.first().map(|r| PathBuf::from(&r.top)) else {
        return branch_only(cp, ledger);
    };
    let mut wanted: Vec<(String, String)> = Vec::new();
    let mut bases: Vec<(Basis, Vec<(String, String)>)> = Vec::new();

    for effect in cp.effects.iter().filter(|e| e.kind == "changes") {
        let Some(id) = effect
            .evidence
            .iter()
            .find(|e| e.kind == EvidenceKind::TurnChanges && !e.detail.is_empty())
            .map(|e| e.detail.clone())
        else {
            continue;
        };
        let Some(record) = records.iter().find(|r| r.turn_id == id) else {
            continue;
        };
        let pairs: Vec<(String, String)> = record
            .files
            .iter()
            .map(|f| (record.after.clone(), f.path.clone()))
            .collect();
        bases.push((
            Basis {
                kind: "effect",
                key: id,
                files: Vec::new(),
                since_ms: record.ended_ms,
                own: Some(record.clone()),
            },
            pairs,
        ));
    }
    for o in &cp.obligations {
        let (ObligationState::Passed, Some(at)) = (o.state, o.passed_ms) else {
            continue;
        };
        let pairs = latest_by_path(&records, at);
        if pairs.is_empty() {
            continue;
        }
        bases.push((
            Basis {
                kind: "obligation",
                key: o.id.clone(),
                files: Vec::new(),
                since_ms: at,
                own: None,
            },
            pairs,
        ));
    }
    for b in &cp.branches {
        let mut child = ledger.list(&b.task_id);
        child.sort_by_key(|r| r.started_ms);
        let pairs = latest_by_path(&child, b.returned_ms);
        if pairs.is_empty() {
            continue;
        }
        bases.push((
            Basis {
                kind: "branch",
                key: b.task_id.clone(),
                files: Vec::new(),
                since_ms: b.returned_ms,
                own: None,
            },
            pairs,
        ));
    }
    for (_, pairs) in &bases {
        wanted.extend(pairs.iter().cloned());
    }
    let blobs = blob_ids(&top, &wanted);
    let paths: Vec<String> = {
        let mut p: Vec<String> = wanted.iter().map(|(_, path)| path.clone()).collect();
        p.sort();
        p.dedup();
        p
    };
    let now = current_ids(&top, &paths);

    let mut out = Vec::new();
    for (mut basis, pairs) in bases {
        basis.files = pairs
            .iter()
            .map(|pair| (pair.1.clone(), blobs.get(pair).cloned().flatten()))
            .collect();
        let files: Vec<StaleFile> = basis
            .files
            .iter()
            .filter(|(path, then)| now.get(path).cloned().flatten() != *then)
            .map(|(path, _)| StaleFile {
                path: path.clone(),
                since_turn: since(&records, &basis, path),
            })
            .collect();
        if !files.is_empty() {
            out.push(StaleConclusion {
                kind: basis.kind.into(),
                key: basis.key,
                files,
            });
        }
    }
    out.extend(branch_only(cp, ledger));
    out
}

/// Branches resting on a snapshot of the parent's files (a read-only
/// branch changed none of its own): stale when any tracked file differs.
fn branch_only(cp: &Checkpoint, ledger: &TurnLedger) -> Vec<StaleConclusion> {
    let mut out = Vec::new();
    for b in &cp.branches {
        let Some(SnapshotRef { top, tree }) = &b.basis else {
            continue;
        };
        if !ledger.list(&b.task_id).is_empty() {
            continue;
        }
        let top = PathBuf::from(top);
        let Some(raw) = git(
            &top,
            &["diff", "--name-only", "-z", "--no-renames", tree],
            None,
        ) else {
            continue;
        };
        let files: Vec<StaleFile> = raw
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| StaleFile {
                path: p.to_string(),
                since_turn: None,
            })
            .collect();
        if !files.is_empty() {
            out.push(StaleConclusion {
                kind: "branch".into(),
                key: b.task_id.clone(),
                files,
            });
        }
    }
    out
}

/// Each path changed by a turn that ended by `at`, with the tree of the
/// last such turn (what the file was then).
fn latest_by_path(records: &[TurnRecord], at: u64) -> Vec<(String, String)> {
    let mut by_path: BTreeMap<String, String> = BTreeMap::new();
    for r in records.iter().filter(|r| r.ended_ms <= at) {
        for f in &r.files {
            by_path.insert(f.path.clone(), r.after.clone());
        }
    }
    by_path
        .into_iter()
        .map(|(path, tree)| (tree, path))
        .collect()
}

/// The first later turn (not undone) that changed `path`; else the
/// conclusion's own turn when it was undone; else none (changed by hand).
fn since(records: &[TurnRecord], basis: &Basis, path: &str) -> Option<String> {
    let later = records.iter().find(|r| {
        r.started_ms >= basis.since_ms
            && r.state != TurnState::Undone
            && basis
                .own
                .as_ref()
                .is_none_or(|own| own.turn_id != r.turn_id)
            && r.files.iter().any(|f| f.path == path)
    });
    if let Some(r) = later {
        return Some(r.turn_id.clone());
    }
    basis
        .own
        .as_ref()
        .filter(|own| own.state == TurnState::Undone)
        .map(|own| own.turn_id.clone())
}

/// `tree:path` → blob id (`None` when the path is not in the tree).
fn blob_ids(top: &Path, pairs: &[(String, String)]) -> HashMap<(String, String), Option<String>> {
    let mut unique: Vec<&(String, String)> = pairs.iter().collect();
    unique.sort();
    unique.dedup();
    let input: String = unique
        .iter()
        .map(|(tree, path)| format!("{tree}:{path}\n"))
        .collect();
    let lines = batch(
        top,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        &input,
    );
    unique
        .into_iter()
        .zip(
            lines
                .iter()
                .map(String::as_str)
                .chain(std::iter::repeat("")),
        )
        .map(|(pair, line)| {
            let id = line
                .strip_suffix(" blob")
                .map(str::to_string)
                .filter(|_| !line.ends_with("missing"));
            (pair.clone(), id)
        })
        .collect()
}

/// Working-tree path → the blob id it would have (`None`: absent).
fn current_ids(top: &Path, paths: &[String]) -> HashMap<String, Option<String>> {
    let present: Vec<&String> = paths
        .iter()
        .filter(|p| std::fs::symlink_metadata(top.join(p)).is_ok())
        .collect();
    let input: String = present.iter().map(|p| format!("{p}\n")).collect();
    let ids = if present.is_empty() {
        Vec::new()
    } else {
        batch(top, &["hash-object", "--stdin-paths"], &input)
    };
    let mut out: HashMap<String, Option<String>> =
        paths.iter().map(|p| (p.clone(), None)).collect();
    for (path, id) in present.into_iter().zip(ids) {
        out.insert(path.clone(), Some(id));
    }
    out
}

/// Run git with `input` on stdin; its output lines.
fn batch(top: &Path, args: &[&str], input: &str) -> Vec<String> {
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// R2-3a: the parent's files when a branch comes back, pinned so `git gc`
/// keeps the tree.
pub fn snapshot(cwd: &Path, branch_task: &str) -> Option<SnapshotRef> {
    let (top, tree) = super::turn_changes::snapshot_tree(cwd)?;
    let name = format!("refs/aeroric/branches/{}", safe(branch_task));
    let _ = git(&top, &["update-ref", &name, &tree], None);
    Some(SnapshotRef {
        top: top.to_string_lossy().to_string(),
        tree,
    })
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "staleness_tests.rs"]
mod tests;
