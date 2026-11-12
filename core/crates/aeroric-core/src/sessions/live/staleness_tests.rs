use std::path::{Path, PathBuf};
use std::process::Command;

use aeroric_protocol::checkpoint::{
    Checkpoint, Effect, Epistemic, EvidenceKind, EvidenceRef, Obligation, ObligationSource,
    ObligationState,
};
use aeroric_protocol::session_live::TurnFileChange;
use aeroric_protocol::turn_ledger::{TurnAction, TurnRecord, TurnState};

use super::*;
use crate::sessions::live::turn_changes::snapshot_tree;

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-stale-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "0\n").unwrap();
    std::fs::write(dir.join("b.txt"), "0\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "base"]);
    dir.canonicalize().unwrap()
}

fn ledger(dir: &Path) -> TurnLedger {
    let ledger = TurnLedger::default();
    ledger.set_dir(dir.with_extension("turns"));
    ledger
}

/// A recorded turn writing `writes`, spanning [at, at + 10].
fn turn(ledger: &TurnLedger, dir: &Path, task: &str, id: &str, at: u64, writes: &[(&str, &str)]) {
    let at = base() + at;
    let (top, before) = snapshot_tree(dir).unwrap();
    for (path, text) in writes {
        std::fs::write(dir.join(path), text).unwrap();
    }
    let (_, after) = snapshot_tree(dir).unwrap();
    ledger.record(
        task,
        TurnRecord {
            turn_id: id.into(),
            started_ms: at,
            ended_ms: at + 10,
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before: None,
            head_after: None,
            files: writes
                .iter()
                .map(|(path, _)| TurnFileChange {
                    path: path.to_string(),
                    additions: Some(1),
                    deletions: Some(1),
                })
                .collect(),
            additions: 1,
            deletions: 1,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        },
    );
}

/// Ledger times near now (older turns are pruned on record).
fn base() -> u64 {
    static BASE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BASE.get_or_init(crate::sessions::live::now_ms)
}

fn effect(turn_id: &str) -> Effect {
    Effect {
        summary: "a.txt".into(),
        kind: "changes".into(),
        turn: 1,
        status: Epistemic::Verified,
        evidence: vec![EvidenceRef {
            kind: EvidenceKind::TurnChanges,
            seq: Some(1),
            turn: Some(1),
            detail: turn_id.into(),
        }],
    }
}

#[test]
fn a_later_turn_makes_an_effect_stale_and_undoing_it_does_not() {
    let dir = repo();
    let ledger = ledger(&dir);
    turn(&ledger, &dir, "t", "u1", 100, &[("a.txt", "1\n")]);
    let mut cp = Checkpoint::new("t");
    cp.effects.push(effect("u1"));
    assert!(stale("t", &cp, &ledger).is_empty());

    turn(&ledger, &dir, "t", "u2", 200, &[("a.txt", "2\n")]);
    let found = stale("t", &cp, &ledger);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].kind.as_str(), found[0].key.as_str()),
        ("effect", "u1")
    );
    assert_eq!(found[0].files[0].path, "a.txt");
    assert_eq!(found[0].files[0].since_turn.as_deref(), Some("u2"));

    ledger.apply("t", "u2", TurnAction::Undo, false).unwrap();
    assert!(stale("t", &cp, &ledger).is_empty());
}

#[test]
fn undoing_the_turn_itself_marks_its_effect() {
    let dir = repo();
    let ledger = ledger(&dir);
    turn(&ledger, &dir, "t", "u1", 100, &[("a.txt", "1\n")]);
    let mut cp = Checkpoint::new("t");
    cp.effects.push(effect("u1"));
    ledger.apply("t", "u1", TurnAction::Undo, false).unwrap();
    let found = stale("t", &cp, &ledger);
    assert_eq!(found[0].files[0].since_turn.as_deref(), Some("u1"));
    ledger.apply("t", "u1", TurnAction::Restore, false).unwrap();
    assert!(stale("t", &cp, &ledger).is_empty());
}

#[test]
fn a_hand_edit_makes_a_passed_criterion_stale() {
    let dir = repo();
    let ledger = ledger(&dir);
    turn(
        &ledger,
        &dir,
        "t",
        "u1",
        100,
        &[("a.txt", "1\n"), ("b.txt", "1\n")],
    );
    let mut cp = Checkpoint::new("t");
    cp.obligations.push(Obligation {
        id: "a1".into(),
        text: "tests pass".into(),
        check: None,
        state: ObligationState::Passed,
        source: ObligationSource::Acceptance,
        passed_ms: Some(base() + 500),
    });
    assert!(stale("t", &cp, &ledger).is_empty());
    std::fs::write(dir.join("b.txt"), "by hand\n").unwrap();
    let found = stale("t", &cp, &ledger);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, "obligation");
    assert_eq!(found[0].files.len(), 1);
    assert_eq!(found[0].files[0].path, "b.txt");
    assert_eq!(found[0].files[0].since_turn, None);
}

#[test]
fn a_read_only_branch_rests_on_the_whole_snapshot() {
    let dir = repo();
    let ledger = ledger(&dir);
    let mut cp = Checkpoint::new("t");
    cp.branches
        .push(aeroric_protocol::checkpoint::BranchResult {
            task_id: "child".into(),
            title: "look".into(),
            summary: "fine".into(),
            files: 0,
            additions: 0,
            deletions: 0,
            returned_ms: 50,
            basis: snapshot(&dir, "child"),
        });
    assert!(stale("t", &cp, &ledger).is_empty());
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    let found = stale("t", &cp, &ledger);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].kind.as_str(), found[0].key.as_str()),
        ("branch", "child")
    );
}
