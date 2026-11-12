use std::process::Command;

use aeroric_protocol::turn_ledger::{TurnAction, TurnRecord, TurnState};

use super::*;
use crate::sessions::live::turn_changes::snapshot_tree;

fn run(dir: &Path, args: &[&str]) -> String {
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
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-ledger-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    run(&dir, &["init", "-q"]);
    std::fs::write(dir.join("src/a.rs"), "old\n").unwrap();
    std::fs::write(dir.join("keep.txt"), "keep\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "base"]);
    dir.canonicalize().unwrap()
}

fn head(dir: &Path) -> Option<String> {
    Some(run(dir, &["rev-parse", "HEAD"]).trim().to_string())
}

/// One turn: edits a.rs, creates b.rs, makes run.sh executable-new.
fn a_turn(ledger: &TurnLedger, dir: &Path, turn: &str, ended_ms: u64) -> TurnRecord {
    let (top, before) = snapshot_tree(dir).unwrap();
    let head_before = head(dir);
    std::fs::write(dir.join("src/a.rs"), "new\n").unwrap();
    std::fs::write(dir.join("src/b.rs"), "fn b() {}\n").unwrap();
    let (_, after) = snapshot_tree(dir).unwrap();
    let record = TurnRecord {
        turn_id: turn.into(),
        started_ms: ended_ms.saturating_sub(10),
        ended_ms,
        top: top.to_string_lossy().into(),
        before,
        after,
        head_before,
        head_after: head(dir),
        files: vec![],
        additions: 2,
        deletions: 1,
        state: TurnState::Pending,
        undone_ms: None,
        undo_group: None,
        state_before_undo: None,
        check: None,
    };
    ledger.record("task-1", record.clone());
    record
}

fn ledger() -> TurnLedger {
    let ledger = TurnLedger::default();
    ledger.set_dir(
        std::env::temp_dir().join(format!("aeroric-ledger-data-{}", uuid::Uuid::new_v4())),
    );
    ledger
}

#[test]
fn undo_puts_the_files_back_and_restore_takes_it_back() {
    let dir = repo();
    let ledger = ledger();
    a_turn(&ledger, &dir, "u1", now_ms());
    let index_before = run(&dir, &["ls-files", "-s"]);
    let plan = ledger.plan("task-1", "u1", TurnAction::Undo, false);
    assert_eq!(plan.blocked, None);
    let mut paths: Vec<_> = plan
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.delete))
        .collect();
    paths.sort();
    assert_eq!(paths, [("src/a.rs", false), ("src/b.rs", true)]);

    ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("src/a.rs")).unwrap(),
        "old\n"
    );
    assert!(!dir.join("src/b.rs").exists());
    assert_eq!(
        run(&dir, &["ls-files", "-s"]),
        index_before,
        "the index is untouched"
    );
    assert_eq!(ledger.get("task-1", "u1").unwrap().state, TurnState::Undone);
    assert!(ledger
        .plan("task-1", "u1", TurnAction::Undo, false)
        .blocked
        .is_some());

    ledger
        .apply("task-1", "u1", TurnAction::Restore, false)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("src/a.rs")).unwrap(),
        "new\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("src/b.rs")).unwrap(),
        "fn b() {}\n"
    );
    assert_eq!(
        ledger.get("task-1", "u1").unwrap().state,
        TurnState::Pending
    );
}

#[test]
fn a_file_changed_after_the_turn_is_skipped_not_overwritten() {
    let dir = repo();
    let ledger = ledger();
    a_turn(&ledger, &dir, "u1", now_ms());
    std::fs::write(dir.join("src/a.rs"), "edited later\n").unwrap();
    let plan = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].path, "src/a.rs");
    assert_eq!(plan.skipped[0].reason, SKIP_CHANGED);
    assert_eq!(
        std::fs::read_to_string(dir.join("src/a.rs")).unwrap(),
        "edited later\n"
    );
    assert!(!dir.join("src/b.rs").exists());
}

#[test]
fn a_commit_in_the_turn_or_a_running_session_blocks_it() {
    let dir = repo();
    let ledger = ledger();
    let mut record = a_turn(&ledger, &dir, "u1", now_ms());
    assert_eq!(
        ledger
            .plan("task-1", "u1", TurnAction::Undo, true)
            .blocked
            .as_deref(),
        Some("这一轮结束后可撤销")
    );
    record.head_after = Some("abcdef0123456".into());
    ledger.record("task-1", record);
    let reason = ledger
        .plan("task-1", "u1", TurnAction::Undo, false)
        .blocked
        .unwrap();
    assert!(reason.contains("abcdef0"), "{reason}");
    assert!(ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("src/a.rs")).unwrap(),
        "new\n"
    );
}

#[test]
fn snapshots_are_pinned_and_old_turns_let_go() {
    let dir = repo();
    let ledger = ledger();
    let now = now_ms();
    a_turn(&ledger, &dir, "old", now - KEEP_MS - 1000);
    std::fs::write(dir.join("src/a.rs"), "old\n").unwrap();
    std::fs::remove_file(dir.join("src/b.rs")).unwrap();
    a_turn(&ledger, &dir, "new", now);
    let refs = run(
        &dir,
        &["for-each-ref", "--format=%(refname)", "refs/aeroric"],
    );
    assert!(
        refs.contains("refs/aeroric/turns/task-1/new/before"),
        "{refs}"
    );
    assert!(!refs.contains("/old/"), "{refs}");
    let kept: Vec<_> = ledger
        .list("task-1")
        .into_iter()
        .map(|r| r.turn_id)
        .collect();
    assert_eq!(kept, ["new"]);
    assert_eq!(
        run(&dir, &["branch", "--list"]).lines().count(),
        1,
        "no branch added"
    );
}

#[test]
fn the_note_names_the_files() {
    let plan = TurnPlan {
        files: vec![PlannedFile {
            path: "a.rs".into(),
            delete: false,
        }],
        ..TurnPlan::default()
    };
    assert!(note(TurnAction::Undo, &plan).contains("a.rs"));
}

/// R2-3a: a turn writing `writes`, spanning [at, at + 10].
fn turn_writing(ledger: &TurnLedger, dir: &Path, id: &str, at: u64, writes: &[(&str, &str)]) {
    let (top, before) = snapshot_tree(dir).unwrap();
    let head_before = head(dir);
    for (path, text) in writes {
        std::fs::write(dir.join(path), text).unwrap();
    }
    let (_, after) = snapshot_tree(dir).unwrap();
    ledger.record(
        "task-1",
        TurnRecord {
            turn_id: id.into(),
            started_ms: at,
            ended_ms: at + 10,
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before,
            head_after: head(dir),
            files: vec![],
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

fn read(dir: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(path)).ok()
}

#[test]
fn undo_from_takes_back_every_later_turn_and_restore_brings_the_batch_back() {
    let dir = repo();
    let ledger = ledger();
    let now = now_ms();
    turn_writing(&ledger, &dir, "u1", now, &[("src/a.rs", "1\n")]);
    turn_writing(
        &ledger,
        &dir,
        "u2",
        now + 100,
        &[("src/a.rs", "2\n"), ("src/n.rs", "n\n")],
    );
    turn_writing(&ledger, &dir, "u3", now + 200, &[("keep.txt", "3\n")]);
    ledger.accept("task-1", "u2").unwrap();

    let plan = ledger.plan("task-1", "u2", TurnAction::UndoFrom, false);
    assert_eq!(plan.blocked, None);
    assert_eq!(plan.turns, vec!["u2", "u3"]);
    let mut paths: Vec<_> = plan.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["keep.txt", "src/a.rs", "src/n.rs"]);
    assert!(plan.files.iter().any(|f| f.path == "src/n.rs" && f.delete));

    ledger
        .apply("task-1", "u2", TurnAction::UndoFrom, false)
        .unwrap();
    assert_eq!(read(&dir, "src/a.rs").as_deref(), Some("1\n"));
    assert_eq!(read(&dir, "src/n.rs"), None);
    assert_eq!(read(&dir, "keep.txt").as_deref(), Some("keep\n"));
    let list = ledger.list("task-1");
    let state = |id: &str| list.iter().find(|r| r.turn_id == id).unwrap().clone();
    assert_eq!(state("u1").state, TurnState::Pending);
    assert_eq!(state("u2").state, TurnState::Undone);
    assert_eq!(state("u3").undo_group.as_deref(), Some("u2"));

    // Taking back either member takes back both; u2 was accepted before.
    let back = ledger.plan("task-1", "u3", TurnAction::Restore, false);
    assert_eq!(back.turns, vec!["u2", "u3"]);
    ledger
        .apply("task-1", "u3", TurnAction::Restore, false)
        .unwrap();
    assert_eq!(read(&dir, "src/a.rs").as_deref(), Some("2\n"));
    assert_eq!(read(&dir, "src/n.rs").as_deref(), Some("n\n"));
    assert_eq!(read(&dir, "keep.txt").as_deref(), Some("3\n"));
    let list = ledger.list("task-1");
    let record = |id: &str| list.iter().find(|r| r.turn_id == id).unwrap().clone();
    assert_eq!(record("u2").state, TurnState::Accepted);
    assert_eq!(record("u3").state, TurnState::Pending);
    assert_eq!(record("u3").undo_group, None);
}

#[test]
fn undo_from_skips_turns_already_undone_and_files_changed_since() {
    let dir = repo();
    let ledger = ledger();
    let now = now_ms();
    turn_writing(&ledger, &dir, "u1", now, &[("src/a.rs", "1\n")]);
    turn_writing(&ledger, &dir, "u2", now + 100, &[("keep.txt", "2\n")]);
    turn_writing(&ledger, &dir, "u3", now + 200, &[("src/b.rs", "3\n")]);
    ledger
        .apply("task-1", "u2", TurnAction::Undo, false)
        .unwrap();
    std::fs::write(dir.join("src/b.rs"), "by hand\n").unwrap();

    let plan = ledger.plan("task-1", "u1", TurnAction::UndoFrom, false);
    assert_eq!(plan.turns, vec!["u1", "u3"]);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, "src/a.rs");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].path, "src/b.rs");
}

#[test]
fn a_commit_anywhere_in_the_batch_blocks_it() {
    let dir = repo();
    let ledger = ledger();
    let now = now_ms();
    turn_writing(&ledger, &dir, "u1", now, &[("src/a.rs", "1\n")]);
    let (top, before) = snapshot_tree(&dir).unwrap();
    let head_before = head(&dir);
    std::fs::write(dir.join("keep.txt"), "2\n").unwrap();
    run(&dir, &["commit", "-qam", "agent"]);
    let (_, after) = snapshot_tree(&dir).unwrap();
    ledger.record(
        "task-1",
        TurnRecord {
            turn_id: "u2".into(),
            started_ms: now + 100,
            ended_ms: now + 110,
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before,
            head_after: head(&dir),
            files: vec![],
            additions: 1,
            deletions: 1,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        },
    );
    let plan = ledger.plan("task-1", "u1", TurnAction::UndoFrom, false);
    let reason = plan.blocked.unwrap();
    assert!(reason.starts_with("其中一轮提交了"), "{reason}");
}

/// R5-1: a finished check stays with its turn.
#[test]
fn a_turns_check_is_kept_with_it() {
    let dir = repo();
    let ledger = ledger();
    a_turn(&ledger, &dir, "turn-1", now_ms());
    let failed = aeroric_protocol::turn_ledger::TurnCheck {
        passed: false,
        summary: "测试 1 项失败".into(),
    };
    ledger.set_check("task-1", "turn-1", failed.clone());
    ledger.set_check("task-1", "no-such-turn", failed.clone());
    let list = ledger.list("task-1");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].check, Some(failed));
}
