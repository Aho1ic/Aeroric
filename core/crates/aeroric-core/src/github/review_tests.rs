use std::path::{Path, PathBuf};
use std::process::Command;

use aeroric_protocol::github::{
    PullCheck, PullComment, PullFile, PullReview, PullReviewState, PullSnapshot, RepoRef,
};

use super::{checkout, fetch, findings, get, head_ref, list, prompt, remote_for, save};
use crate::github::{snapshot_id, store};

fn sh(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Rig {
    root: PathBuf,
    /// GitHub's side: a bare repository with `refs/pull/7/head`.
    remote: PathBuf,
    /// The user's clone.
    work: PathBuf,
    base: String,
    head: String,
}

fn rig() -> Rig {
    let root = std::env::temp_dir().join(format!("aeroric-pr-{}", uuid::Uuid::new_v4()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("src")).unwrap();
    sh(&root, &["init", "-q", "seed"]);
    std::fs::write(seed.join("src/a.rs"), "fn a() {}\n").unwrap();
    sh(&seed, &["add", "."]);
    sh(&seed, &["commit", "-qm", "base"]);
    let base = sh(&seed, &["rev-parse", "HEAD"]);
    sh(&seed, &["checkout", "-qb", "feature"]);
    std::fs::write(seed.join("src/a.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
    sh(&seed, &["commit", "-qam", "pr"]);
    let head = sh(&seed, &["rev-parse", "HEAD"]);
    sh(&seed, &["checkout", "-q", "main"]);
    sh(&root, &["clone", "-q", "--bare", "seed", "remote.git"]);
    let remote = root.join("remote.git");
    sh(&remote, &["update-ref", "refs/pull/7/head", &head]);
    sh(&remote, &["branch", "-qD", "feature"]);
    sh(&root, &["clone", "-q", "remote.git", "work"]);
    Rig {
        work: root.join("work"),
        remote,
        root,
        base,
        head,
    }
}

fn snapshot(rig: &Rig) -> PullSnapshot {
    PullSnapshot {
        id: snapshot_id(42, 7, &rig.head),
        project_id: "p1".into(),
        repo: RepoRef {
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widget".into(),
            repo_id: 42,
        },
        number: 7,
        title: "加两个函数".into(),
        author: "dev".into(),
        state: "open".into(),
        head_ref: "feature".into(),
        base_ref: "main".into(),
        head_sha: rig.head.clone(),
        base_sha: rig.base.clone(),
        fetched_ms: 1,
        files: vec![PullFile {
            path: "src/a.rs".into(),
            status: "modified".into(),
            additions: 2,
            deletions: 0,
            previous_path: None,
        }],
        checks: vec![PullCheck {
            name: "ci".into(),
            status: "completed".into(),
            conclusion: Some("failure".into()),
            url: String::new(),
        }],
        comments: vec![PullComment {
            author: "lead".into(),
            body: "命名再想想".into(),
            outdated: true,
            ..PullComment::default()
        }],
        ..PullSnapshot::default()
    }
}

/// The head comes into Aeroric's own ref and a detached checkout; the
/// user's branch, HEAD and uncommitted edit stay as they were, and the
/// checkout is not an untracked file of theirs.
#[test]
fn the_head_is_checked_out_beside_the_users_work() {
    let rig = rig();
    let snap = snapshot(&rig);
    std::fs::write(rig.work.join("src/a.rs"), "// 我的改动\n").unwrap();
    let before = sh(&rig.work, &["rev-parse", "HEAD"]);
    let branch = sh(&rig.work, &["branch", "--show-current"]);

    let remote = rig.remote.to_string_lossy().to_string();
    assert_eq!(fetch(&rig.work, &remote, &snap).unwrap(), head_ref(&snap));
    assert_eq!(sh(&rig.work, &["rev-parse", &head_ref(&snap)]), rig.head);
    let at = checkout(&rig.work, &snap).unwrap();
    assert!(at.starts_with(rig.work.join(".aeroric/pr-reviews")));
    assert_eq!(sh(&at, &["rev-parse", "HEAD"]), rig.head);
    assert!(std::fs::read_to_string(at.join("src/a.rs"))
        .unwrap()
        .contains("fn c()"));

    assert_eq!(sh(&rig.work, &["rev-parse", "HEAD"]), before);
    assert_eq!(sh(&rig.work, &["branch", "--show-current"]), branch);
    assert_eq!(
        std::fs::read_to_string(rig.work.join("src/a.rs")).unwrap(),
        "// 我的改动\n"
    );
    assert_eq!(sh(&rig.work, &["status", "--porcelain"]), "M src/a.rs");
    // Again: the same checkout, nothing fetched anew.
    assert_eq!(
        fetch(&rig.work, "/nowhere", &snap).unwrap(),
        head_ref(&snap)
    );
    assert_eq!(checkout(&rig.work, &snap).unwrap(), at);
    std::fs::remove_dir_all(&rig.root).ok();
}

/// Pushed to since the snapshot: refused, and the ref fetched is let go.
#[test]
fn a_pr_pushed_to_since_the_snapshot_is_refused() {
    let rig = rig();
    let snap = snapshot(&rig);
    sh(&rig.remote, &["update-ref", "refs/pull/7/head", &rig.base]);
    let remote = rig.remote.to_string_lossy().to_string();
    let error = fetch(&rig.work, &remote, &snap).unwrap_err();
    assert!(
        error.contains("新提交") && error.contains(&rig.base[..12]),
        "{error}"
    );
    assert!(Command::new("git")
        .arg("-C")
        .arg(&rig.work)
        .args(["rev-parse", "--verify", "-q", &head_ref(&snap)])
        .output()
        .unwrap()
        .stdout
        .is_empty());
    std::fs::remove_dir_all(&rig.root).ok();
}

#[test]
fn the_users_own_remote_is_used_when_it_is_this_repository() {
    let rig = rig();
    let snap = snapshot(&rig);
    sh(
        &rig.work,
        &["remote", "add", "gh", "git@github.com:Acme/Widget.git"],
    );
    assert_eq!(remote_for(&rig.work, &snap.repo), "gh");
    sh(&rig.work, &["remote", "remove", "gh"]);
    assert_eq!(
        remote_for(&rig.work, &snap.repo),
        "https://github.com/acme/widget.git"
    );
    std::fs::remove_dir_all(&rig.root).ok();
}

/// The prompt says what to read and how to answer; the answer's findings
/// keep only the PR's files, drop a line past the end, default an unknown
/// severity; no block is no findings.
#[test]
fn the_reviewer_is_told_the_pr_and_its_findings_are_checked() {
    let rig = rig();
    let snap = snapshot(&rig);
    let text = prompt(&snap);
    for part in [
        "PR #7「加两个函数」",
        "只读",
        "modified src/a.rs +2 -0",
        "ci：failure",
        "（针对旧版本）",
        "```json",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    assert!(text.contains(&format!("git diff {}...HEAD", &rig.base[..12])));

    let remote = rig.remote.to_string_lossy().to_string();
    fetch(&rig.work, &remote, &snap).unwrap();
    let at = checkout(&rig.work, &snap).unwrap();
    let answer = r#"看完了。
```json
{"findings": [
  {"path": "./src/a.rs", "line": 2, "severity": "High", "body": "b 没有测试", "basis": "src/a.rs:2 新增"},
  {"path": "src/a.rs", "line": 99, "severity": "critical", "body": "行号写错的", "basis": ""},
  {"path": "src/other.rs", "line": 1, "severity": "low", "body": "不在 PR 里", "basis": ""},
  {"path": "src/a.rs", "line": "3", "severity": "low", "body": " ", "basis": ""}
]}
```"#;
    let (kept, dropped) = findings(answer, &snap, &at).unwrap();
    assert_eq!(dropped, 2);
    assert_eq!(kept.len(), 2);
    assert_eq!(
        (
            kept[0].path.as_str(),
            kept[0].line,
            kept[0].severity.as_str()
        ),
        ("src/a.rs", Some(2), "high")
    );
    assert_eq!((kept[1].line, kept[1].severity.as_str()), (None, "medium"));
    assert!(findings("没问题。", &snap, &at).is_none());
    assert_eq!(
        findings("{\"findings\": []}", &snap, &at).unwrap().0.len(),
        0
    );
    std::fs::remove_dir_all(&rig.root).ok();
}

/// Reviews are kept per project; one on a superseded snapshot is outdated.
#[test]
fn a_review_of_an_older_head_is_outdated() {
    let data = std::env::temp_dir().join(format!("aeroric-pr-reviews-{}", uuid::Uuid::new_v4()));
    let old = PullSnapshot {
        id: snapshot_id(42, 7, &"a".repeat(40)),
        project_id: "p1".into(),
        number: 7,
        head_sha: "a".repeat(40),
        fetched_ms: 1,
        repo: RepoRef {
            repo_id: 42,
            ..RepoRef::default()
        },
        ..PullSnapshot::default()
    };
    store::save(&data, &old).unwrap();
    let review = PullReview {
        id: "rev-1".into(),
        project_id: "p1".into(),
        snapshot_id: old.id.clone(),
        head_sha: old.head_sha.clone(),
        task_id: "task-1".into(),
        state: PullReviewState::Running,
        created_ms: 2,
        ..PullReview::default()
    };
    save(&data, &review).unwrap();
    assert!(!get(&data, "p1", "rev-1").unwrap().outdated);
    let newer = PullSnapshot {
        id: snapshot_id(42, 7, &"b".repeat(40)),
        head_sha: "b".repeat(40),
        fetched_ms: 3,
        ..old.clone()
    };
    store::save(&data, &newer).unwrap();
    assert!(get(&data, "p1", "rev-1").unwrap().outdated, "针对旧版本");
    assert_eq!(list(&data, "p1").len(), 1);
    assert!(list(&data, "p2").is_empty());
    std::fs::remove_dir_all(&data).ok();
}
