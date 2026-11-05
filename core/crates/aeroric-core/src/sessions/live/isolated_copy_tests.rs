use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use aeroric_protocol::change_review::{ChangeScope, HunkState};

use super::*;
use crate::sessions::live::change_review::ChangeReview;
use crate::sessions::live::turn_ledger::TurnLedger;

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
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-copy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(dir.join("c.txt"), "c\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "base"]);
    dir.canonicalize().unwrap()
}

fn review(dir: &Path) -> (ChangeReview, PathBuf) {
    let copies = dir.with_extension("copies");
    let review = ChangeReview::new(Arc::new(TurnLedger::default()));
    review.set_dir(dir.with_extension("review"));
    review.set_copies_dir(copies.clone());
    (review, copies)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn a_copy_carries_uncommitted_work_and_the_original_never_sees_it() {
    let dir = repo();
    std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
    let (_, copies) = review(&dir);
    let (record, cwd) = create(&copies, &dir, "task-1").unwrap();
    let copy = PathBuf::from(&record.path);
    assert_eq!(cwd, copy);
    assert_eq!(read(&copy.join("a.txt")), "one\nTWO\nthree\n");
    assert_eq!(read(&copy.join("new.txt")), "fresh\n");
    let status = run(&dir, &["status", "--porcelain"]);
    assert!(!status.contains(".aeroric"), "{status}");
    assert_eq!(load(&copies, "task-1").unwrap().base, record.base);
    // A second exclude line is never added.
    create(&copies, &dir, "task-2").unwrap();
    let exclude = read(&dir.join(".git/info/exclude"));
    assert_eq!(exclude.matches("/.aeroric/").count(), 1, "{exclude}");
    remove(&copies, "task-1").unwrap();
    assert!(!copy.exists());
    assert!(load(&copies, "task-1").is_none());
}

#[test]
fn a_hunk_is_brought_back_to_the_original_working_tree_only() {
    let dir = repo();
    let (review, copies) = review(&dir);
    let (record, _) = create(&copies, &dir, "t").unwrap();
    let copy = PathBuf::from(&record.path);
    std::fs::write(copy.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let diff = review.diff("t", ChangeScope::Origin, None);
    assert_eq!(diff.files.len(), 1, "{diff:?}");
    assert_eq!(review.origin_pending("t"), Some(1));
    let hunk = diff.files[0].hunks[0].id.clone();
    let verdict = review
        .accept("t", ChangeScope::Origin, None, &hunk)
        .unwrap();
    assert_eq!(verdict.state, HunkState::Applied);
    assert_eq!(read(&dir.join("a.txt")), "one\nTWO\nthree\n");
    assert_eq!(
        run(&dir, &["diff", "--cached", "--name-only"]),
        "",
        "index untouched"
    );
    assert_eq!(review.origin_pending("t"), Some(0));
    assert!(load(&copies, "t").unwrap().cleared_ms.is_some());
}

#[test]
fn a_moved_original_is_a_conflict_and_resolving_merges_it_into_the_copy() {
    let dir = repo();
    let (review, copies) = review(&dir);
    let (record, _) = create(&copies, &dir, "t").unwrap();
    let copy = PathBuf::from(&record.path);
    std::fs::write(copy.join("a.txt"), "one\nagent\nthree\n").unwrap();
    std::fs::write(dir.join("a.txt"), "one\nuser\nthree\n").unwrap();
    std::fs::write(dir.join("c.txt"), "c by user\n").unwrap();

    let (applied, conflicts) = review.bring_all("t").unwrap();
    assert_eq!((applied, conflicts), (0, 1));
    assert_eq!(
        read(&dir.join("a.txt")),
        "one\nuser\nthree\n",
        "original untouched"
    );

    let resolved = resolve(&copies, "t").unwrap();
    assert_eq!(resolved.conflicted, vec!["a.txt".to_string()]);
    assert!(resolved.synced.contains(&"c.txt".to_string()));
    assert_eq!(
        read(&copy.join("c.txt")),
        "c by user\n",
        "the user's own edit came in"
    );
    let merged = read(&copy.join("a.txt"));
    assert!(
        merged.contains("<<<<<<< 副本") && merged.contains("user") && merged.contains("agent"),
        "{merged}"
    );
    let text = prompt(&load(&copies, "t").unwrap(), &resolved);
    assert!(
        text.contains("a.txt") && text.contains("冲突标记"),
        "{text}"
    );

    // The agent settles it; only its own work is left to bring back.
    std::fs::write(copy.join("a.txt"), "one\nuser and agent\nthree\n").unwrap();
    let diff = review.diff("t", ChangeScope::Origin, None);
    let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt"]);
    let (applied, conflicts) = review.bring_all("t").unwrap();
    assert_eq!((applied, conflicts), (1, 0));
    assert_eq!(read(&dir.join("a.txt")), "one\nuser and agent\nthree\n");
}

#[test]
fn a_repository_without_a_commit_cannot_have_a_copy() {
    let dir = std::env::temp_dir().join(format!("aeroric-copy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q"]);
    assert!(check(&dir).unwrap_err().contains("没有任何提交"));
}
