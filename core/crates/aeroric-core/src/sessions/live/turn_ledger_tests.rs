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
        items: Vec::new(),
        incomplete: false,
    };
    ledger.set_check("task-1", "turn-1", failed.clone());
    ledger.set_check("task-1", "no-such-turn", failed.clone());
    let list = ledger.list("task-1");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].check, Some(failed));
}

/// A7: files the user already put back are in place, not "changed since";
/// with every file in place the undo still goes through and only marks it.
#[test]
fn files_put_back_by_hand_are_in_place_not_skipped() {
    let dir = repo();
    let ledger = ledger();
    a_turn(&ledger, &dir, "u1", now_ms());
    std::fs::write(dir.join("src/a.rs"), "old\n").unwrap();
    let plan = ledger.plan("task-1", "u1", TurnAction::Undo, false);
    assert_eq!(plan.in_place, ["src/a.rs"]);
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, "src/b.rs");

    std::fs::remove_file(dir.join("src/b.rs")).unwrap();
    let plan = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert!(plan.files.is_empty() && plan.skipped.is_empty());
    let mut in_place = plan.in_place.clone();
    in_place.sort();
    assert_eq!(in_place, ["src/a.rs", "src/b.rs"]);
    assert_eq!(ledger.get("task-1", "u1").unwrap().state, TurnState::Undone);
    assert!(worth_a_note(&plan));
    assert!(note(TurnAction::Undo, &plan).contains("src/b.rs"));
}

/// A7: a batch shares the classification — a file changed and changed back
/// inside the batch is already in place.
#[test]
fn a_batch_file_changed_back_within_it_is_in_place() {
    let dir = repo();
    let ledger = ledger();
    let now = now_ms();
    turn_writing(&ledger, &dir, "u1", now, &[("src/a.rs", "1\n")]);
    turn_writing(
        &ledger,
        &dir,
        "u2",
        now + 100,
        &[("src/a.rs", "old\n"), ("keep.txt", "2\n")],
    );
    let plan = ledger.plan("task-1", "u1", TurnAction::UndoFrom, false);
    assert_eq!(plan.in_place, ["src/a.rs"]);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, "keep.txt");
    assert!(plan.skipped.is_empty());
}

/// A7: a write goes through a temporary file and a rename — the file keeps
/// its permission bits, a symlink in its place is replaced rather than
/// written through, and no temporary file is left.
#[cfg(unix)]
#[test]
fn a_write_back_keeps_the_mode_and_does_not_write_through_a_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let ledger = ledger();
    a_turn(&ledger, &dir, "u1", now_ms());
    std::fs::set_permissions(dir.join("src/a.rs"), std::fs::Permissions::from_mode(0o640)).unwrap();
    ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(read(&dir, "src/a.rs").as_deref(), Some("old\n"));
    let mode = std::fs::metadata(dir.join("src/a.rs"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);

    // Take it back, then put a symlink where a.rs is, pointing at a file
    // with the same content. B1-2: a link is not the file the turn left
    // (git records the link's own text), so it is skipped — and neither the
    // link nor its target is touched.
    ledger
        .apply("task-1", "u1", TurnAction::Restore, false)
        .unwrap();
    let outside = dir.join("outside.txt");
    std::fs::write(&outside, "new\n").unwrap();
    std::fs::remove_file(dir.join("src/a.rs")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("src/a.rs")).unwrap();
    let plan = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert!(
        plan.skipped
            .iter()
            .any(|f| f.path == "src/a.rs" && f.reason == SKIP_CHANGED),
        "{plan:?}"
    );
    assert_eq!(read(&dir, "outside.txt").as_deref(), Some("new\n"));
    let meta = std::fs::symlink_metadata(dir.join("src/a.rs")).unwrap();
    assert!(meta.file_type().is_symlink());
    let left: Vec<_> = std::fs::read_dir(dir.join("src"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("aeroric-undo"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// A7: a write that fails does not stop the others; the result says file by
/// file what was written and what failed, the turn keeps its state, and
/// trying again finds the written ones in place and finishes.
#[cfg(unix)]
#[test]
fn a_half_done_undo_is_reported_and_finished_by_trying_again() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let ledger = ledger();
    turn_writing(
        &ledger,
        &dir,
        "u1",
        now_ms(),
        &[("src/a.rs", "1\n"), ("keep.txt", "2\n")],
    );
    let src = dir.join("src");
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(src.join(".probe"), "").is_ok() {
        // Running as a user the mode does not stop (root): nothing to show.
        let _ = std::fs::remove_file(src.join(".probe"));
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let plan = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, "keep.txt");
    assert_eq!(plan.failed.len(), 1);
    assert_eq!(plan.failed[0].path, "src/a.rs");
    assert!(!plan.failed[0].reason.is_empty());
    assert_eq!(read(&dir, "keep.txt").as_deref(), Some("keep\n"));
    assert_eq!(
        read(&dir, "src/a.rs").as_deref(),
        Some("1\n"),
        "never half written"
    );
    assert_eq!(
        ledger.get("task-1", "u1").unwrap().state,
        TurnState::Pending
    );
    let told = note(TurnAction::Undo, &plan);
    assert!(
        told.contains("keep.txt") && told.contains("没能改"),
        "{told}"
    );

    let again = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(again.in_place, ["keep.txt"]);
    assert_eq!(again.files.len(), 1);
    assert_eq!(again.files[0].path, "src/a.rs");
    assert!(again.skipped.is_empty() && again.failed.is_empty());
    assert_eq!(read(&dir, "src/a.rs").as_deref(), Some("old\n"));
    assert_eq!(ledger.get("task-1", "u1").unwrap().state, TurnState::Undone);
}

/// B1-2: a folder swapped for a symlink after the turn is never written or
/// deleted through, whether the undo would edit a file or remove one.
#[cfg(unix)]
#[test]
fn undo_never_goes_through_a_folder_swapped_for_a_symlink() {
    let dir = repo();
    let ledger = ledger();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/f.txt"), "before\n").unwrap();
    turn_writing(
        &ledger,
        &dir,
        "u1",
        now_ms(),
        &[("sub/f.txt", "after\n"), ("sub/n.txt", "made\n")],
    );
    // The folder goes elsewhere and a link to it takes its place.
    let outside = std::env::temp_dir().join(format!("aeroric-outside-{}", uuid::Uuid::new_v4()));
    std::fs::rename(dir.join("sub"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("sub")).unwrap();

    let plan = ledger.plan("task-1", "u1", TurnAction::Undo, false);
    assert!(plan.files.is_empty(), "{plan:?}");
    let mut skipped: Vec<_> = plan
        .skipped
        .iter()
        .map(|f| (f.path.as_str(), f.reason.as_str()))
        .collect();
    skipped.sort();
    assert_eq!(
        skipped,
        [("sub/f.txt", SKIP_UNSAFE), ("sub/n.txt", SKIP_UNSAFE)]
    );
    let done = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert!(done.files.is_empty() && done.failed.is_empty(), "{done:?}");
    assert_eq!(
        std::fs::read_to_string(outside.join("f.txt")).unwrap(),
        "after\n"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("n.txt")).unwrap(),
        "made\n"
    );
    // A write that gets past the plan is refused on its own.
    assert_eq!(
        write_back(&dir, "HEAD", "sub/f.txt", true),
        Err(SKIP_UNSAFE.to_string())
    );
    assert!(outside.join("f.txt").exists());
    let _ = std::fs::remove_dir_all(&outside);
}

/// B1-2: a turn that only made a file executable is undone: same content,
/// different mode, so it is not "in place".
#[cfg(unix)]
#[test]
fn a_chmod_only_turn_is_undone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let ledger = ledger();
    let script = dir.join("run.sh");
    std::fs::write(&script, "echo hi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (top, before) = snapshot_tree(&dir).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (_, after) = snapshot_tree(&dir).unwrap();
    ledger.record(
        "task-1",
        TurnRecord {
            turn_id: "u1".into(),
            started_ms: now_ms() - 10,
            ended_ms: now_ms(),
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before: head(&dir),
            head_after: head(&dir),
            files: vec![],
            additions: 0,
            deletions: 0,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        },
    );
    let plan = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(plan.files.len(), 1, "{plan:?}");
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);
}

/// B1-2: a file and a symlink whose text is the file's content have the same
/// blob; the turn that swapped one for the other is still undone, and taken
/// back, with the link restored by a rename.
#[cfg(unix)]
#[test]
fn a_file_swapped_for_a_link_with_the_same_text_is_undone_and_restored() {
    let dir = repo();
    let ledger = ledger();
    let path = dir.join("t");
    std::fs::write(&path, "keep.txt").unwrap();
    let (top, before) = snapshot_tree(&dir).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink("keep.txt", &path).unwrap();
    let (_, after) = snapshot_tree(&dir).unwrap();
    ledger.record(
        "task-1",
        TurnRecord {
            turn_id: "u1".into(),
            started_ms: now_ms() - 10,
            ended_ms: now_ms(),
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before: head(&dir),
            head_after: head(&dir),
            files: vec![],
            additions: 0,
            deletions: 0,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        },
    );
    let undone = ledger
        .apply("task-1", "u1", TurnAction::Undo, false)
        .unwrap();
    assert_eq!(undone.files.len(), 1, "{undone:?}");
    let meta = std::fs::symlink_metadata(&path).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep.txt");

    let restored = ledger
        .apply("task-1", "u1", TurnAction::Restore, false)
        .unwrap();
    assert_eq!(restored.files.len(), 1, "{restored:?}");
    let meta = std::fs::symlink_metadata(&path).unwrap();
    assert!(meta.file_type().is_symlink());
    assert_eq!(std::fs::read_link(&path).unwrap(), Path::new("keep.txt"));
}
