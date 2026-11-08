use std::process::Command;
use std::sync::Arc;

use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

use super::*;

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
    assert!(out.status.success(), "git {args:?}");
}

fn lines(edit: impl Fn(usize) -> String) -> String {
    (1..=30).map(|i| format!("{}\n", edit(i))).collect()
}

/// A repo whose one turn edits lines 3 and 25 of a.txt: two hunks.
fn rig() -> (PathBuf, ChangeReview, Arc<TurnLedger>) {
    let dir = std::env::temp_dir().join(format!("aeroric-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), lines(|i| format!("line {i}"))).unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "base"]);
    let dir = dir.canonicalize().unwrap();
    let ledger = Arc::new(TurnLedger::default());
    ledger.set_dir(dir.with_extension("turns"));
    let (top, before) = snapshot_tree(&dir).unwrap();
    std::fs::write(
        dir.join("a.txt"),
        lines(|i| match i {
            3 => "line three".into(),
            25 => "line twenty-five".into(),
            _ => format!("line {i}"),
        }),
    )
    .unwrap();
    let (_, after) = snapshot_tree(&dir).unwrap();
    ledger.record(
        "t",
        TurnRecord {
            turn_id: "u1".into(),
            started_ms: 1,
            ended_ms: now_ms(),
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before: None,
            head_after: None,
            files: vec![],
            additions: 2,
            deletions: 2,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
        },
    );
    let review = ChangeReview::new(Arc::clone(&ledger));
    review.set_dir(dir.with_extension("review"));
    (dir, review, ledger)
}

fn hunks(review: &ChangeReview) -> Vec<ReviewHunk> {
    review.diff("t", ChangeScope::Turn, Some("u1")).files[0]
        .hunks
        .clone()
}

#[test]
fn a_turn_reads_as_files_and_hunks_with_line_numbers() {
    let (_, review, _) = rig();
    let diff = review.diff("t", ChangeScope::Turn, Some("u1"));
    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(
        (file.path.as_str(), file.additions, file.deletions),
        ("a.txt", 2, 2)
    );
    assert_eq!(file.hunks.len(), 2);
    let added = file.hunks[0].lines.iter().find(|l| l.kind == "+").unwrap();
    assert_eq!((added.text.as_str(), added.new), ("line three", Some(3)));
    assert!(file.hunks.iter().all(|h| h.state == HunkState::Pending));
}

#[test]
fn rejecting_one_hunk_reverts_only_it_and_a_moved_hunk_is_stale() {
    let (dir, review, _) = rig();
    let [first, second] = <[ReviewHunk; 2]>::try_from(hunks(&review)).unwrap();
    let verdict = review
        .reject("t", ChangeScope::Turn, Some("u1"), &first.id)
        .unwrap();
    assert_eq!((verdict.state, verdict.line), (HunkState::Rejected, 3));
    let text = std::fs::read_to_string(dir.join("a.txt")).unwrap();
    assert!(
        text.contains("line 3\n") && text.contains("line twenty-five\n"),
        "{text}"
    );
    assert_eq!(review.rejected("t", "u1"), 1);

    let moved = text.replace("line twenty-five", "edited by hand");
    std::fs::write(dir.join("a.txt"), moved).unwrap();
    let verdict = review
        .reject("t", ChangeScope::Turn, Some("u1"), &second.id)
        .unwrap();
    assert_eq!(verdict.state, HunkState::Stale);
    assert!(std::fs::read_to_string(dir.join("a.txt"))
        .unwrap()
        .contains("edited by hand"));
    assert!(reject_note(&verdict).contains("a.txt"));
}

#[test]
fn accepting_every_hunk_accepts_the_turn() {
    let (_, review, ledger) = rig();
    for hunk in hunks(&review) {
        review
            .accept("t", ChangeScope::Turn, Some("u1"), &hunk.id)
            .unwrap();
    }
    assert!(hunks(&review)
        .iter()
        .all(|h| h.state == HunkState::Accepted));
    assert_eq!(ledger.get("t", "u1").unwrap().state, TurnState::Accepted);
}

#[test]
fn comments_wait_and_go_once() {
    let (_, review, _) = rig();
    let first = hunks(&review)[0].id.clone();
    review
        .comment("t", ChangeScope::Turn, Some("u1"), &first, " 换个名字 ")
        .unwrap();
    assert_eq!(review.diff("t", ChangeScope::Turn, Some("u1")).unsent, 1);
    let refused = review.take_comments("t", |_| Err::<(), _>("队列写不进".to_string()));
    assert!(refused.unwrap().is_err());
    assert!(!hunks(&review)[0].comment_sent, "没排上队就还算未发送");
    let message = review.take_comments("t", Ok).unwrap().unwrap();
    assert!(message.contains("- a.txt:3 — 换个名字"), "{message}");
    assert!(review.take_comments("t", Ok).is_none());
    assert!(hunks(&review)[0].comment_sent);
}

#[test]
fn since_seen_starts_where_the_user_last_looked() {
    let (dir, review, _) = rig();
    assert_eq!(review.diff("t", ChangeScope::Session, None).files.len(), 1);
    review.mark_seen("t").unwrap();
    let seen = review.diff("t", ChangeScope::Seen, None);
    assert!(seen.files.is_empty() && seen.seen_ms.is_some());
    std::fs::write(dir.join("b.txt"), "new\n").unwrap();
    let seen = review.diff("t", ChangeScope::Seen, None);
    assert_eq!(
        seen.files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["b.txt"]
    );
    assert_eq!(review.diff("t", ChangeScope::Session, None).files.len(), 2);
}
