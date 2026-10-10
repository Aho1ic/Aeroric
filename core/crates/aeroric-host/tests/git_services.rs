//! P8-05 slice 64: Git query services over authenticated UDS.
//! Uses synthetic temporary git repositories.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempRepo {
    root: PathBuf,
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn new_repo(label: &str) -> TempRepo {
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{label}-{seq}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create repo dir");
    let init_out = std::process::Command::new("git")
        .arg("init")
        .arg("-b")
        .arg("main")
        .current_dir(&path)
        .output()
        .expect("git init");
    assert!(init_out.status.success(), "git init failed");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&path)
        .output()
        .expect("git config user.name");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&path)
        .output()
        .expect("git config user.email");
    TempRepo { root: path }
}

fn commit_file(repo: &TempRepo, path: &str, content: &str, message: &str) {
    let full = repo.root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&full, content).expect("write file");
    let add_out = std::process::Command::new("git")
        .args(["add", path])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(add_out.status.success());
    let commit_out = std::process::Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(&repo.root)
        .output()
        .expect("git commit");
    assert!(commit_out.status.success());
}

/// Run git in the fixture repo and return trimmed stdout (end-state assertions
/// read real git state, not the reply payload).
fn git_output(repo: &TempRepo, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(&repo.root)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {:?}",
        out.stderr
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request(id, method, params)).await
}

fn success_result(reply: Frame) -> Value {
    match reply {
        Frame::Success(success) => success.result,
        Frame::Failure(failure) => panic!("expected success, got failure: {failure:?}"),
        other => panic!("expected success frame, got {other:?}"),
    }
}

#[tokio::test]
async fn git_status_queries_repository_status_and_validates_params() {
    let harness = Harness::start("git-status").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-status-test");
    commit_file(&repo, "tracked.txt", "initial content\n", "initial");

    // Add an untracked file and modify the tracked file
    std::fs::write(repo.root.join("untracked.txt"), "untracked\n").expect("write untracked");
    std::fs::write(repo.root.join("tracked.txt"), "modified content\n").expect("write modified");

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "status-1",
        "git.git_status",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let result = success_result(reply);
    let changes = result.as_array().expect("result should be array");
    assert_eq!(changes.len(), 2);

    let paths: Vec<&str> = changes.iter().filter_map(|c| c["path"].as_str()).collect();
    assert!(paths.contains(&"tracked.txt"));
    assert!(paths.contains(&"untracked.txt"));

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "status-bad",
        "git.git_status",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "status-remote",
        "git.git_status",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_list_branches_queries_branches_and_validates_params() {
    let harness = Harness::start("git-branches").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-branches-test");
    commit_file(&repo, "tracked.txt", "initial content\n", "initial");

    // Create a new branch
    let branch_out = std::process::Command::new("git")
        .args(["branch", "feature"])
        .current_dir(&repo.root)
        .output()
        .expect("git branch feature");
    assert!(branch_out.status.success());

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "branches-1",
        "git.git_list_branches",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let result = success_result(reply);
    let branches = result.as_array().expect("result should be array");
    assert_eq!(branches.len(), 2);

    let main_branch = branches
        .iter()
        .find(|b| b["name"] == "main")
        .expect("main branch");
    assert_eq!(main_branch["current"], true);
    let feat_branch = branches
        .iter()
        .find(|b| b["name"] == "feature")
        .expect("feature branch");
    assert_eq!(feat_branch["current"], false);

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "branches-bad",
        "git.git_list_branches",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "branches-remote",
        "git.git_list_branches",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_log_queries_commit_history_and_validates_params() {
    let harness = Harness::start("git-log").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-log-test");
    commit_file(&repo, "file1.txt", "line 1\n", "first commit message");
    commit_file(&repo, "file2.txt", "line 2\n", "second commit message");

    // Success call: full log
    let reply = query(
        &mut reader,
        &mut writer,
        "log-1",
        "git.git_log",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "limit": 50
        }),
    )
    .await;
    let result = success_result(reply);
    let commits = result.as_array().expect("result should be array");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["message"], "second commit message");
    assert_eq!(commits[1]["message"], "first commit message");

    // Search filter call
    let reply_search = query(
        &mut reader,
        &mut writer,
        "log-search",
        "git.git_log",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "limit": 50,
            "search": "first"
        }),
    )
    .await;
    let result_search = success_result(reply_search);
    let search_commits = result_search.as_array().expect("result should be array");
    assert_eq!(search_commits.len(), 1);
    assert_eq!(search_commits[0]["message"], "first commit message");

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "log-bad",
        "git.git_log",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "log-remote",
        "git.git_log",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_show_diff_queries_commit_diff_and_validates_params() {
    let harness = Harness::start("git-show-diff").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-diff-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");
    commit_file(
        &repo,
        "file1.txt",
        "hello world\nadded line\n",
        "modify file",
    );

    let rev_out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo.root)
        .output()
        .expect("git rev-parse HEAD");
    let head_hash = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "diff-1",
        "git.git_show_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": head_hash
        }),
    )
    .await;
    let result = success_result(reply);
    let diff_text = result.as_str().expect("result should be string");
    assert!(diff_text.contains("+added line"));

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "diff-bad",
        "git.git_show_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "diff-remote",
        "git.git_show_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": head_hash,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_stage_stages_file_and_validates_params() {
    let harness = Harness::start("git-stage").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stage-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Modify file1.txt
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "stage-1",
        "git.git_stage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "file1.txt"
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, Value::Null);

    // Verify file is staged
    let status_out = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repo.root)
        .output()
        .expect("git status");
    let status_str = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status_str.starts_with("M  file1.txt"),
        "status was: {status_str}"
    );

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "stage-bad",
        "git.git_stage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stage-remote",
        "git.git_stage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "file1.txt",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_unstage_unstages_file_and_validates_params() {
    let harness = Harness::start("git-unstage").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-unstage-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Modify file1.txt and stage it
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    let stage_out = std::process::Command::new("git")
        .args(["add", "file1.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(stage_out.status.success());

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "unstage-1",
        "git.git_unstage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "file1.txt"
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, Value::Null);

    // Verify file is unstaged
    let status_out = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repo.root)
        .output()
        .expect("git status");
    let status_str = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status_str.starts_with(" M file1.txt"),
        "status was: {status_str}"
    );

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "unstage-bad",
        "git.git_unstage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "unstage-remote",
        "git.git_unstage",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "file1.txt",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_commit_creates_commit_and_validates_params() {
    let harness = Harness::start("git-commit").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-commit-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Modify file1.txt and stage it
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    let stage_out = std::process::Command::new("git")
        .args(["add", "file1.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(stage_out.status.success());

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "commit-1",
        "git.git_commit",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "message": "test commit via host"
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, Value::Null);

    // Verify commit was created
    let log_out = std::process::Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(&repo.root)
        .output()
        .expect("git log");
    let msg = String::from_utf8_lossy(&log_out.stdout);
    assert_eq!(msg.trim(), "test commit via host");

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "commit-bad",
        "git.git_commit",
        json!({
            "projectPath": repo.root.to_string_lossy(),
        }),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "commit-remote",
        "git.git_commit",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "message": "remote message",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_stage_all_stages_everything_and_validates_params() {
    let harness = Harness::start("git-stage-all").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stage-all-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Untracked + modified files must both end up staged by `git add -A`.
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    std::fs::write(repo.root.join("file2.txt"), "brand new\n").expect("new file");

    let reply = query(
        &mut reader,
        &mut writer,
        "stage-all-1",
        "git.git_stage_all",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, Value::Null);

    let staged_out = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&repo.root)
        .output()
        .expect("git diff --cached");
    let staged = String::from_utf8_lossy(&staged_out.stdout);
    assert!(
        staged.contains("file1.txt"),
        "modified file staged: {staged}"
    );
    assert!(
        staged.contains("file2.txt"),
        "untracked file staged: {staged}"
    );

    // Control: without includeUntracked the untracked file stays in the worktree.
    let repo2 = new_repo("aeroric-git-stash-push-tracked-only");
    commit_file(&repo2, "file1.txt", "hello world\n", "initial");
    std::fs::write(repo2.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    std::fs::write(repo2.root.join("file2.txt"), "brand new\n").expect("new file");
    let reply_tracked = query(
        &mut reader,
        &mut writer,
        "stash-push-tracked",
        "git.git_stash_push",
        json!({ "projectPath": repo2.root.to_string_lossy() }),
    )
    .await;
    assert!(success_result(reply_tracked).is_string());
    let status2 = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo2.root)
        .output()
        .expect("git status");
    let status2 = String::from_utf8_lossy(&status2.stdout);
    assert!(
        status2.contains("?? file2.txt"),
        "untracked file must survive without includeUntracked: {status2}"
    );

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "stage-all-bad",
        "git.git_stage_all",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stage-all-remote",
        "git.git_stage_all",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_unstage_all_clears_the_index_and_validates_params() {
    let harness = Harness::start("git-unstage-all").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-unstage-all-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Stage a modification, then clear the whole index.
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    let add_out = std::process::Command::new("git")
        .args(["add", "file1.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(add_out.status.success());

    let reply = query(
        &mut reader,
        &mut writer,
        "unstage-all-1",
        "git.git_unstage_all",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result, Value::Null);

    // Nothing staged, change still present in the worktree.
    let staged_out = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&repo.root)
        .output()
        .expect("git diff --cached");
    assert!(
        String::from_utf8_lossy(&staged_out.stdout)
            .trim()
            .is_empty(),
        "index must be empty after unstage-all"
    );
    let status_out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo.root)
        .output()
        .expect("git status");
    let status = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status.contains(" M file1.txt"),
        "worktree change must survive: {status}"
    );

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "unstage-all-bad",
        "git.git_unstage_all",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "unstage-all-remote",
        "git.git_unstage_all",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_conflict_files_lists_unmerged_paths_and_validates_params() {
    let harness = Harness::start("git-conflict-files").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-conflict-files-test");
    commit_file(&repo, "a.txt", "base\n", "base");
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo.root)
            .output()
            .expect("git")
    };
    assert!(run(&["checkout", "-b", "b"]).status.success());
    commit_file(&repo, "a.txt", "from-b\n", "b change");
    assert!(run(&["checkout", "main"]).status.success());
    commit_file(&repo, "a.txt", "from-main\n", "main change");
    // Diverging merge leaves a.txt unmerged.
    let _ = run(&["merge", "b"]);

    let reply = query(
        &mut reader,
        &mut writer,
        "conflict-files-1",
        "git.git_conflict_files",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let result = success_result(reply);
    let conflicts = result.as_array().expect("conflict list");
    assert_eq!(conflicts.len(), 1, "{result}");
    assert_eq!(conflicts[0]["path"], "a.txt");

    // Cross-check against the legacy argv itself: the host now answers from `git status`
    // (core::conflict_files), while the legacy command used `git diff --diff-filter=U -z`.
    // The two must agree on the raw path bytes, not merely on "one entry".
    let legacy = std::process::Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U", "-z"])
        .current_dir(&repo.root)
        .output()
        .expect("git diff --diff-filter=U");
    let legacy_paths: Vec<String> = legacy
        .stdout
        .split(|b| *b == 0)
        .filter(|e| !e.is_empty())
        .map(|e| String::from_utf8_lossy(e).into_owned())
        .collect();
    let host_paths: Vec<String> = conflicts
        .iter()
        .map(|c| c["path"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(host_paths, legacy_paths, "host list must match legacy argv");

    // A clean repository reports no conflicts (empty list, not an error).
    let clean = new_repo("aeroric-git-conflict-none");
    commit_file(&clean, "a.txt", "base\n", "base");
    let reply_clean = query(
        &mut reader,
        &mut writer,
        "conflict-files-clean",
        "git.git_conflict_files",
        json!({ "projectPath": clean.root.to_string_lossy() }),
    )
    .await;
    let result_clean = success_result(reply_clean);
    assert_eq!(result_clean, json!([]));

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "conflict-files-bad",
        "git.git_conflict_files",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "conflict-files-remote",
        "git.git_conflict_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_init_creates_repository_idempotently_and_validates_params() {
    let harness = Harness::start("git-init").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // A plain directory (not yet a repository), created outside `new_repo`.
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let plain = std::env::temp_dir().join(format!(
        "aeroric-git-init-test-{seq}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(&plain).expect("create plain dir");
    let guard = TempRepo {
        root: plain.clone(),
    };

    let reply = query(
        &mut reader,
        &mut writer,
        "init-1",
        "git.git_init",
        json!({ "projectPath": plain.to_string_lossy(), "initialBranch": "trunk" }),
    )
    .await;
    assert_eq!(success_result(reply), Value::Null);

    // The fresh repository has an unborn HEAD, so `rev-parse --abbrev-ref HEAD` prints the
    // literal "HEAD"; the branch name lives in the symbolic ref.
    let head = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&plain)
        .output()
        .expect("git symbolic-ref");
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "trunk");

    // Idempotent: a second call on an existing repository is a no-op success, and it must not
    // re-create the repo on a different branch.
    let reply_again = query(
        &mut reader,
        &mut writer,
        "init-2",
        "git.git_init",
        json!({ "projectPath": plain.to_string_lossy(), "initialBranch": "other" }),
    )
    .await;
    assert_eq!(success_result(reply_again), Value::Null);
    let head_again = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&plain)
        .output()
        .expect("git symbolic-ref");
    assert_eq!(String::from_utf8_lossy(&head_again.stdout).trim(), "trunk");

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "init-bad",
        "git.git_init",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "init-remote",
        "git.git_init",
        json!({
            "projectPath": plain.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);

    drop(guard);
}
#[tokio::test]
async fn git_stash_push_stashes_changes_and_validates_params() {
    let harness = Harness::start("git-stash-push").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stash-push-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");

    // Unstaged change + an untracked file: both are what `stash push -u` is supposed to capture.
    std::fs::write(repo.root.join("file1.txt"), "hello world\nmodified\n").expect("modify file");
    std::fs::write(repo.root.join("file2.txt"), "brand new\n").expect("new file");

    let reply = query(
        &mut reader,
        &mut writer,
        "stash-push-1",
        "git.git_stash_push",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "message": "wip",
            "includeUntracked": true,
        }),
    )
    .await;
    let result = success_result(reply);
    let text = result.as_str().expect("stash push returns text");
    // The legacy command returns git's combined stdout+stderr, which names the stash
    // ("... On main: wip") but never enumerates files — do not assert file names here.
    assert!(
        text.contains("wip"),
        "combined output carries the message: {text}"
    );

    // Index/worktree clean afterwards, and the untracked file really left the worktree.
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo.root)
        .output()
        .expect("git status");
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "worktree must be clean after stash push: {}",
        String::from_utf8_lossy(&status.stdout)
    );
    assert!(
        !repo.root.join("file2.txt").exists(),
        "untracked file stashed"
    );

    let list = std::process::Command::new("git")
        .args(["stash", "list", "--format=%gd%x1f%gs"])
        .current_dir(&repo.root)
        .output()
        .expect("git stash list");
    let entries = String::from_utf8_lossy(&list.stdout);
    assert!(entries.contains("wip"), "stash message recorded: {entries}");

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "stash-push-bad",
        "git.git_stash_push",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stash-push-remote",
        "git.git_stash_push",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_stash_apply_and_drop_roundtrip_and_validate_params() {
    let harness = Harness::start("git-stash-apply").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stash-apply-test");
    commit_file(&repo, "file1.txt", "hello world\n", "initial");
    std::fs::write(repo.root.join("file1.txt"), "hello world\nstashed\n").expect("modify file");
    let stashed = std::process::Command::new("git")
        .args(["stash", "push", "-m", "wip"])
        .current_dir(&repo.root)
        .output()
        .expect("git stash push");
    assert!(stashed.status.success());

    // apply restores the change.
    let reply = query(
        &mut reader,
        &mut writer,
        "stash-apply-1",
        "git.git_stash_apply",
        json!({ "projectPath": repo.root.to_string_lossy(), "stashRef": "stash@{0}" }),
    )
    .await;
    let applied = success_result(reply);
    assert!(applied.is_string(), "{applied}");
    let content = std::fs::read_to_string(repo.root.join("file1.txt")).expect("read file");
    assert!(
        content.contains("stashed"),
        "apply restored the change: {content}"
    );

    // drop removes the entry from the list.
    let reply_drop = query(
        &mut reader,
        &mut writer,
        "stash-drop-1",
        "git.git_stash_drop",
        json!({ "projectPath": repo.root.to_string_lossy(), "stashRef": "stash@{0}" }),
    )
    .await;
    assert!(success_result(reply_drop).is_string());
    let list = std::process::Command::new("git")
        .args(["stash", "list"])
        .current_dir(&repo.root)
        .output()
        .expect("git stash list");
    assert!(
        String::from_utf8_lossy(&list.stdout).trim().is_empty(),
        "stash dropped"
    );

    // Refs that are not `stash@{<digits>}` must be refused by BOTH methods. The legacy command
    // validates the ref itself; a looser revision check would let these reach real git.
    for bad_ref in ["HEAD", "main", "stash@{main}", "stash@{-1}", "--all"] {
        for method in ["git.git_stash_apply", "git.git_stash_drop"] {
            let reply_bad = query(
                &mut reader,
                &mut writer,
                "stash-bad-ref",
                method,
                json!({
                    "projectPath": repo.root.to_string_lossy(),
                    "stashRef": bad_ref,
                }),
            )
            .await;
            let Frame::Failure(failure) = reply_bad else {
                panic!("{method} must refuse {bad_ref}");
            };
            assert_eq!(
                failure.error.code,
                ErrorCode::IoError,
                "{method} on {bad_ref}"
            );
        }
    }

    // Missing stashRef -> InvalidArgument, for both methods.
    for method in ["git.git_stash_apply", "git.git_stash_drop"] {
        let reply_missing = query(
            &mut reader,
            &mut writer,
            "stash-missing",
            method,
            json!({ "projectPath": repo.root.to_string_lossy() }),
        )
        .await;
        let Frame::Failure(failure) = reply_missing else {
            panic!("{method} must require stashRef");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument);
    }

    // Remote target -> UnsupportedCapability.
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stash-remote",
        "git.git_stash_apply",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "stashRef": "stash@{0}",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_create_and_checkout_branch_validate_and_switch() {
    let harness = Harness::start("git-branch").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-branch-test");
    commit_file(&repo, "a.txt", "base\n", "initial");
    let head = |repo: &TempRepo| {
        let out = std::process::Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(&repo.root)
            .output()
            .expect("git symbolic-ref");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // create without checkout: branch exists, HEAD unchanged.
    let reply = query(
        &mut reader,
        &mut writer,
        "branch-create",
        "git.git_create_branch",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "branchName": "feature",
            "fromBranch": "main",
            "checkout": false,
        }),
    )
    .await;
    assert_eq!(success_result(reply), Value::Null);
    assert_eq!(head(&repo), "main", "HEAD must not move without checkout");
    let list = std::process::Command::new("git")
        .args(["branch", "--list", "feature"])
        .current_dir(&repo.root)
        .output()
        .expect("git branch --list");
    assert!(String::from_utf8_lossy(&list.stdout).contains("feature"));

    // checkout: HEAD moves.
    let reply_c = query(
        &mut reader,
        &mut writer,
        "branch-checkout",
        "git.git_checkout_branch",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "branchName": "feature",
            "isRemote": false,
        }),
    )
    .await;
    assert_eq!(success_result(reply_c), Value::Null);
    assert_eq!(head(&repo), "feature");

    // create with checkout: HEAD moves, and `fromBranch` is the base.
    let reply_cc = query(
        &mut reader,
        &mut writer,
        "branch-create-checkout",
        "git.git_create_branch",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "branchName": "feature2",
            "fromBranch": "main",
            "checkout": true,
        }),
    )
    .await;
    assert_eq!(success_result(reply_cc), Value::Null);
    assert_eq!(head(&repo), "feature2");

    // Option-injection-shaped names are refused by BOTH methods (legacy validator), and the
    // refusal must happen before any branch is created.
    for bad in ["-d", "--all", ""] {
        for method in ["git.git_create_branch", "git.git_checkout_branch"] {
            let params = if method == "git.git_create_branch" {
                json!({
                    "projectPath": repo.root.to_string_lossy(),
                    "branchName": bad,
                    "fromBranch": "main",
                    "checkout": false,
                })
            } else {
                json!({
                    "projectPath": repo.root.to_string_lossy(),
                    "branchName": bad,
                    "isRemote": false,
                })
            };
            let reply_bad = query(&mut reader, &mut writer, "branch-bad", method, params).await;
            let Frame::Failure(failure) = reply_bad else {
                panic!("{method} must refuse {bad:?}");
            };
            assert_eq!(failure.error.code, ErrorCode::IoError, "{method} {bad:?}");
        }
    }

    // Missing params -> InvalidArgument.
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "branch-missing",
        "git.git_create_branch",
        json!({ "projectPath": repo.root.to_string_lossy(), "branchName": "x" }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("fromBranch is required");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability.
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "branch-remote",
        "git.git_checkout_branch",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "branchName": "main",
            "isRemote": false,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_remote_counts_reports_ahead_and_validates_params() {
    let harness = Harness::start("git-remote-counts").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-remote-counts-test");
    commit_file(&repo, "a.txt", "base\n", "initial");

    // Local bare remote so `@{u}` resolves for real.
    let bare = std::env::temp_dir().join(format!(
        "aeroric-remote-counts-bare-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&bare);
    let init_bare = std::process::Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .output()
        .expect("git init --bare");
    assert!(init_bare.status.success());
    let bare_guard = TempRepo { root: bare.clone() };
    let add_remote = std::process::Command::new("git")
        .args(["remote", "add", "origin", bare.to_str().unwrap()])
        .current_dir(&repo.root)
        .output()
        .expect("git remote add");
    assert!(add_remote.status.success(), "git remote add failed");
    let push = std::process::Command::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo.root)
        .output()
        .expect("git push");
    assert!(push.status.success(), "push failed");

    // In sync: 0 ahead / 0 behind.
    let reply = query(
        &mut reader,
        &mut writer,
        "counts-1",
        "git.git_remote_counts",
        json!({ "projectPath": repo.root.to_string_lossy(), "branch": "main" }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["branch"], "main");
    assert_eq!(result["ahead"], 0, "{result}");
    assert_eq!(result["behind"], 0, "{result}");

    // One local commit -> 1 ahead / 0 behind.
    commit_file(&repo, "b.txt", "local only\n", "local commit");
    let reply2 = query(
        &mut reader,
        &mut writer,
        "counts-2",
        "git.git_remote_counts",
        json!({ "projectPath": repo.root.to_string_lossy(), "branch": "main" }),
    )
    .await;
    let result2 = success_result(reply2);
    assert_eq!(result2["ahead"], 1, "{result2}");
    assert_eq!(result2["behind"], 0, "{result2}");

    // Omitting `branch` falls back to the current branch (`rev-parse --abbrev-ref HEAD`).
    let reply3 = query(
        &mut reader,
        &mut writer,
        "counts-3",
        "git.git_remote_counts",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let result3 = success_result(reply3);
    assert_eq!(result3["branch"], "main", "{result3}");
    assert_eq!(result3["ahead"], 1, "{result3}");

    // A branch with no upstream is NOT an error: counts fall back to (0, 0) and the name is echoed.
    let reply4 = query(
        &mut reader,
        &mut writer,
        "counts-4",
        "git.git_remote_counts",
        json!({ "projectPath": repo.root.to_string_lossy(), "branch": "no-such-branch" }),
    )
    .await;
    let result4 = success_result(reply4);
    assert_eq!(result4["branch"], "no-such-branch");
    assert_eq!(result4["ahead"], 0, "{result4}");
    assert_eq!(result4["behind"], 0, "{result4}");

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "counts-bad",
        "git.git_remote_counts",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "counts-remote",
        "git.git_remote_counts",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);

    drop(bare_guard);
}
#[tokio::test]
async fn git_blame_file_returns_porcelain_lines_and_validates_params() {
    let harness = Harness::start("git-blame").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-blame-test");
    commit_file(&repo, "src/app.js", "const value = 1;\n", "initial commit");

    let reply = query(
        &mut reader,
        &mut writer,
        "blame-1",
        "git.git_blame_file",
        json!({ "projectPath": repo.root.to_string_lossy(), "filePath": "src/app.js" }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["filePath"], "src/app.js");
    let lines = result["lines"].as_array().expect("lines array");
    assert_eq!(lines.len(), 1, "{result}");
    assert_eq!(lines[0]["content"], "const value = 1;");
    assert_eq!(lines[0]["line"], 1);
    assert_eq!(lines[0]["author"], "Test User");
    assert_eq!(lines[0]["summary"], "initial commit");
    // `--line-porcelain` + `core.quotePath=false` matter: the commit hash must be a real 40-char
    // sha and the short form its first 8 characters (the legacy `short_commit_hash` rule).
    let commit = lines[0]["commit"].as_str().expect("commit");
    assert_eq!(commit.len(), 40, "{commit}");
    assert_eq!(lines[0]["shortCommit"], &commit[..8]);
    assert!(lines[0]["authorTime"].as_i64().unwrap_or(0) > 0, "{result}");

    // A path that escapes the worktree is refused by the SHARED guard, before git runs.
    // `validate_pathspec` (the looser validator) would let `sub/../x` through.
    for bad in ["../escape.txt", "sub/../x.txt", "/abs.txt", ""] {
        let reply_bad = query(
            &mut reader,
            &mut writer,
            "blame-bad",
            "git.git_blame_file",
            json!({ "projectPath": repo.root.to_string_lossy(), "filePath": bad }),
        )
        .await;
        let Frame::Failure(failure) = reply_bad else {
            panic!("{bad:?} must be refused");
        };
        assert_eq!(failure.error.code, ErrorCode::IoError, "{bad:?}");
    }

    // Missing filePath -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "blame-missing",
        "git.git_blame_file",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("filePath is required");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "blame-remote",
        "git.git_blame_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "src/app.js",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_branch_graph_returns_structured_commits_and_validates_params() {
    let harness = Harness::start("git-branch-graph").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-branch-graph-test");
    commit_file(&repo, "a.txt", "one\n", "first commit");
    commit_file(&repo, "b.txt", "two\n", "second commit");

    let reply = query(
        &mut reader,
        &mut writer,
        "graph-1",
        "git.git_branch_graph",
        json!({ "projectPath": repo.root.to_string_lossy(), "limit": 10 }),
    )
    .await;
    let result = success_result(reply);
    let commits = result["commits"].as_array().expect("commits array");
    assert_eq!(commits.len(), 2, "{result}");
    assert_eq!(result["truncated"], false);
    // Newest first: `log` order, and the payload carries per-commit metadata.
    assert_eq!(commits[0]["subject"], "second commit");
    assert_eq!(commits[1]["subject"], "first commit");
    assert_eq!(commits[0]["author"], "Test User");
    let hash = commits[0]["hash"].as_str().expect("hash");
    assert_eq!(hash.len(), 40, "{hash}");
    assert_eq!(commits[0]["shortHash"], &hash[..8]);
    // Parents are real shas (the second commit's parent is the first).
    let parents = commits[0]["parents"].as_array().expect("parents");
    assert_eq!(parents.len(), 1);
    assert_eq!(
        parents[0].as_str().unwrap(),
        commits[1]["hash"].as_str().unwrap()
    );
    assert!(
        !commits[0]["relativeTime"].as_str().unwrap_or("").is_empty(),
        "{result}"
    );
    // HEAD is decorated.
    let refs = commits[0]["refs"].as_array().expect("refs");
    assert!(
        refs.iter()
            .any(|r| r.as_str().unwrap_or("").contains("main")),
        "HEAD decoration: {result}"
    );

    // `limit` truncates: ask for 1 and expect `truncated: true`.
    let reply_limited = query(
        &mut reader,
        &mut writer,
        "graph-limited",
        "git.git_branch_graph",
        json!({ "projectPath": repo.root.to_string_lossy(), "limit": 1 }),
    )
    .await;
    let limited = success_result(reply_limited);
    assert_eq!(limited["commits"].as_array().unwrap().len(), 1, "{limited}");
    assert_eq!(limited["truncated"], true, "{limited}");

    // An empty repository (`no HEAD`) is a SUCCESS with no commits, not an error.
    let empty = new_repo("aeroric-git-branch-graph-empty");
    let reply_empty = query(
        &mut reader,
        &mut writer,
        "graph-empty",
        "git.git_branch_graph",
        json!({ "projectPath": empty.root.to_string_lossy() }),
    )
    .await;
    let empty_result = success_result(reply_empty);
    assert_eq!(empty_result["commits"], json!([]), "{empty_result}");
    assert_eq!(empty_result["truncated"], false);

    // A plain directory (not a repository) is an ERROR, not an empty graph: legacy
    // fails at the worktree-root resolution step (`rev-parse --show-toplevel`) and
    // surfaces git's stderr verbatim — which is locale-dependent, so the assertion
    // pins the mechanism, not the message: non-empty (git's stderr) and NOT the
    // log-step fallback (proving the probe step refused before log ever ran).
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let plain = std::env::temp_dir().join(format!(
        "aeroric-git-branch-graph-plain-{seq}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(&plain).expect("create plain dir");
    let reply_plain = query(
        &mut reader,
        &mut writer,
        "graph-plain",
        "git.git_branch_graph",
        json!({ "projectPath": plain.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_plain) = reply_plain else {
        panic!("non-repo path must fail");
    };
    assert_eq!(
        failure_plain.error.code,
        ErrorCode::IoError,
        "{failure_plain:?}"
    );
    assert!(!failure_plain.error.message.is_empty(), "{failure_plain:?}");
    assert_ne!(
        failure_plain.error.message, "Failed to load git branch graph",
        "error must come from the worktree-root probe, not the log step: {failure_plain:?}"
    );
    let _ = std::fs::remove_dir_all(&plain);

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "graph-bad",
        "git.git_branch_graph",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "graph-remote",
        "git.git_branch_graph",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_stash_list_returns_entries_and_validates_params() {
    let harness = Harness::start("git-stash-list").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stash-list");
    commit_file(&repo, "tracked.txt", "base\n", "base commit");
    // End state fixture: a real stash must exist, created by git itself.
    std::fs::write(repo.root.join("tracked.txt"), "changed\n").expect("modify file");
    let stash_out = std::process::Command::new("git")
        .args(["stash", "push", "-m", "wip change"])
        .current_dir(&repo.root)
        .output()
        .expect("git stash push");
    assert!(stash_out.status.success(), "git stash push failed");

    let reply = query(
        &mut reader,
        &mut writer,
        "stashes-1",
        "git.git_stash_list",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let result = success_result(reply);
    let stashes = result.as_array().expect("stash array");
    assert_eq!(stashes.len(), 1, "{result}");
    assert_eq!(stashes[0]["name"], "stash@{0}", "{result}");
    assert_eq!(stashes[0]["index"], 0, "{result}");
    assert_eq!(stashes[0]["message"], "On main: wip change", "{result}");
    let commit = stashes[0]["commit"].as_str().expect("commit hash");
    assert_eq!(commit.len(), 40, "{commit}");
    assert!(
        !stashes[0]["date"].as_str().unwrap_or("").is_empty(),
        "{result}"
    );

    // A repository with no stashes is a SUCCESS with an empty array.
    let empty = new_repo("aeroric-git-stash-list-empty");
    commit_file(&empty, "a.txt", "one\n", "first");
    let reply_empty = query(
        &mut reader,
        &mut writer,
        "stashes-empty",
        "git.git_stash_list",
        json!({ "projectPath": empty.root.to_string_lossy() }),
    )
    .await;
    assert_eq!(success_result(reply_empty), json!([]));

    // Missing projectPath -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "stashes-bad",
        "git.git_stash_list",
        json!({}),
    )
    .await;
    let Frame::Failure(failure) = reply_bad else {
        panic!("missing params must fail");
    };
    assert_eq!(failure.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stashes-remote",
        "git.git_stash_list",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_stash_diff_shows_stash_patch_and_validates_params() {
    let harness = Harness::start("git-stash-diff").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stash-diff");
    commit_file(&repo, "tracked.txt", "base\n", "base commit");
    std::fs::write(repo.root.join("tracked.txt"), "changed\n").expect("modify file");
    let stash_out = std::process::Command::new("git")
        .args(["stash", "push", "-m", "wip change"])
        .current_dir(&repo.root)
        .output()
        .expect("git stash push");
    assert!(stash_out.status.success(), "git stash push failed");

    let reply = query(
        &mut reader,
        &mut writer,
        "stashdiff-1",
        "git.git_stash_diff",
        json!({ "projectPath": repo.root.to_string_lossy(), "stashRef": "stash@{0}" }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["stashRef"], "stash@{0}", "{result}");
    assert_eq!(result["truncated"], false, "{result}");
    let diff = result["diff"].as_str().expect("diff text");
    assert!(diff.contains("tracked.txt"), "{result}");
    assert!(
        diff.contains("base") && diff.contains("changed"),
        "{result}"
    );

    // A loose ref is refused by the shape validator BEFORE git runs. The established
    // host mapping (slice 76's stash_ref_request) surfaces core validation failures
    // as IoError, not InvalidArgument; the exact message is what proves the refusal
    // came from the validator and not from git (git's failure text is its own stderr).
    let reply_bad_ref = query(
        &mut reader,
        &mut writer,
        "stashdiff-badref",
        "git.git_stash_diff",
        json!({ "projectPath": repo.root.to_string_lossy(), "stashRef": "HEAD" }),
    )
    .await;
    let Frame::Failure(failure_ref) = reply_bad_ref else {
        panic!("loose stash ref must fail");
    };
    assert_eq!(failure_ref.error.code, ErrorCode::IoError);
    assert_eq!(failure_ref.error.message, "Invalid stash reference");

    // Missing stashRef -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "stashdiff-missing",
        "git.git_stash_diff",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing stashRef must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stashdiff-remote",
        "git.git_stash_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "stashRef": "stash@{0}",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_stage_files_stages_the_listed_files_and_validates_params() {
    let harness = Harness::start("git-stage-files").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-stage-files");
    commit_file(&repo, "base.txt", "base\n", "base commit");
    std::fs::write(repo.root.join("a.txt"), "a\n").expect("write a");
    std::fs::write(repo.root.join("b.txt"), "b\n").expect("write b");
    std::fs::write(repo.root.join("c.txt"), "c\n").expect("write c");

    // Duplicates in the list are deduped; only the listed files are staged.
    let reply = query(
        &mut reader,
        &mut writer,
        "stagefiles-1",
        "git.git_stage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["a.txt", "b.txt", "a.txt"]
        }),
    )
    .await;
    let _ = success_result(reply);
    let staged = git_output(&repo, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "a.txt\nb.txt", "staged set: {staged:?}");
    let untracked_c = git_output(&repo, &["status", "--porcelain"]);
    assert!(untracked_c.contains("?? c.txt"), "{untracked_c:?}");

    // An empty list is a SILENT SUCCESS — no git run, nothing staged.
    let reply_empty = query(
        &mut reader,
        &mut writer,
        "stagefiles-empty",
        "git.git_stage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": []
        }),
    )
    .await;
    let _ = success_result(reply_empty);

    // Escape attempt is refused before git runs.
    let reply_escape = query(
        &mut reader,
        &mut writer,
        "stagefiles-escape",
        "git.git_stage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["sub/../a.txt"]
        }),
    )
    .await;
    let Frame::Failure(failure_escape) = reply_escape else {
        panic!("escape path must fail");
    };
    assert_eq!(failure_escape.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_escape.error.message,
        "File path must stay inside the git worktree"
    );

    // Missing filePaths -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "stagefiles-missing",
        "git.git_stage_files",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing filePaths must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "stagefiles-remote",
        "git.git_stage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["a.txt"],
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_unstage_files_unstages_with_head_fallback_and_validates_params() {
    let harness = Harness::start("git-unstage-files").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // With HEAD: staged changes come back to the worktree, the file stays modified.
    let repo = new_repo("aeroric-git-unstage-files");
    commit_file(&repo, "tracked.txt", "base\n", "base commit");
    std::fs::write(repo.root.join("tracked.txt"), "changed\n").expect("modify");
    let add = std::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(add.status.success());
    let reply = query(
        &mut reader,
        &mut writer,
        "unstagefiles-1",
        "git.git_unstage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["tracked.txt"]
        }),
    )
    .await;
    let _ = success_result(reply);
    let staged = git_output(&repo, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "", "staged must be empty: {staged:?}");
    let status = git_output(&repo, &["status", "--porcelain"]);
    // `git_output` trims, so the porcelain leading space is gone; `starts_with`
    // still separates "M tracked.txt" (unstaged only) from "MM tracked.txt".
    assert!(status.starts_with("M tracked.txt"), "{status:?}");

    // Without HEAD (unborn): `restore --staged` would fail; the legacy falls back
    // to `git reset --` — the file returns to untracked.
    let fresh = new_repo("aeroric-git-unstage-files-unborn");
    std::fs::write(fresh.root.join("new.txt"), "new\n").expect("write new");
    let add_new = std::process::Command::new("git")
        .args(["add", "new.txt"])
        .current_dir(&fresh.root)
        .output()
        .expect("git add unborn");
    assert!(add_new.status.success());
    let reply_unborn = query(
        &mut reader,
        &mut writer,
        "unstagefiles-unborn",
        "git.git_unstage_files",
        json!({
            "projectPath": fresh.root.to_string_lossy(),
            "filePaths": ["new.txt"]
        }),
    )
    .await;
    let _ = success_result(reply_unborn);
    let status_unborn = git_output(&fresh, &["status", "--porcelain"]);
    assert!(
        status_unborn.contains("?? new.txt"),
        "unborn fallback must leave the file untracked: {status_unborn:?}"
    );

    // Empty path -> IoError from the validator, before git runs.
    let reply_blank = query(
        &mut reader,
        &mut writer,
        "unstagefiles-blank",
        "git.git_unstage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": [""]
        }),
    )
    .await;
    let Frame::Failure(failure_blank) = reply_blank else {
        panic!("blank path must fail");
    };
    assert_eq!(failure_blank.error.code, ErrorCode::IoError);
    assert_eq!(failure_blank.error.message, "File path must not be empty");

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "unstagefiles-remote",
        "git.git_unstage_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["tracked.txt"],
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_file_diff_returns_worktree_diff_and_untracked_fallback() {
    let harness = Harness::start("git-file-diff").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-file-diff");
    commit_file(&repo, "a.txt", "one\n", "base commit");
    std::fs::write(repo.root.join("a.txt"), "two\n").expect("modify a");

    // Unstaged worktree diff for a tracked file.
    let reply = query(
        &mut reader,
        &mut writer,
        "filediff-1",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "staged": false
        }),
    )
    .await;
    let diff = success_result(reply)
        .as_str()
        .expect("diff string")
        .to_string();
    assert!(diff.contains("-one") && diff.contains("+two"), "{diff:?}");

    // Staged diff needs --cached to see the same change.
    let add = std::process::Command::new("git")
        .args(["add", "a.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(add.status.success());
    let reply_staged = query(
        &mut reader,
        &mut writer,
        "filediff-staged",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "staged": true
        }),
    )
    .await;
    let staged_diff = success_result(reply_staged)
        .as_str()
        .expect("diff string")
        .to_string();
    assert!(
        staged_diff.contains("-one") && staged_diff.contains("+two"),
        "{staged_diff:?}"
    );

    // An untracked file falls back to `diff --no-index` against an empty file.
    std::fs::write(repo.root.join("b.txt"), "brand new\n").expect("write b");
    let reply_untracked = query(
        &mut reader,
        &mut writer,
        "filediff-untracked",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "b.txt",
            "staged": false
        }),
    )
    .await;
    let untracked = success_result(reply_untracked)
        .as_str()
        .expect("diff string")
        .to_string();
    assert!(untracked.contains("b.txt"), "{untracked:?}");
    assert!(untracked.contains("brand new"), "{untracked:?}");

    // A `..` path is refused before git runs — the --no-index fallback would
    // otherwise turn this method into a file-read primitive outside the worktree.
    let reply_escape = query(
        &mut reader,
        &mut writer,
        "filediff-escape",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "../secrets.txt",
            "staged": false
        }),
    )
    .await;
    let Frame::Failure(failure_escape) = reply_escape else {
        panic!("escape path must fail");
    };
    assert_eq!(failure_escape.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_escape.error.message,
        "File path must stay inside the git worktree"
    );

    // Missing staged flag -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "filediff-missing",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing staged must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "filediff-remote",
        "git.git_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "staged": false,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_show_file_diff_shows_committed_content_and_validates_params() {
    let harness = Harness::start("git-show-file-diff").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-show-file-diff");
    commit_file(&repo, "a.txt", "one\n", "first commit");
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    commit_file(&repo, "a.txt", "two\n", "second commit");

    // `show --format= <hash> -- <file>` returns the PATCH of that file in that
    // commit (a pathspec turns `git show` into a per-path diff — the method name
    // says file_diff, not blob). The first commit adds the file, so the patch is
    // a new-file diff; assert its shape, not a guessed blob body.
    let reply = query(
        &mut reader,
        &mut writer,
        "showdiff-1",
        "git.git_show_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": first,
            "filePath": "a.txt"
        }),
    )
    .await;
    let result = success_result(reply);
    let content = result.as_str().expect("content string");
    assert!(
        content.contains("diff --git a/a.txt b/a.txt"),
        "{content:?}"
    );
    assert!(content.contains("new file mode"), "{content:?}");
    assert!(content.contains("+one"), "{content:?}");

    // An option-like revision is refused before git runs.
    let reply_option = query(
        &mut reader,
        &mut writer,
        "showdiff-option",
        "git.git_show_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": "--output=/tmp/evil",
            "filePath": "a.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_option) = reply_option else {
        panic!("option-like revision must fail");
    };
    assert_eq!(failure_option.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_option.error.message,
        "revision must not start with '-'"
    );

    // Missing commitHash -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "showdiff-missing",
        "git.git_show_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing commitHash must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "showdiff-remote",
        "git.git_show_file_diff",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": first,
            "filePath": "a.txt",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_commit_detail_returns_parsed_fields_and_validates_params() {
    let harness = Harness::start("git-commit-detail").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-commit-detail");
    commit_file(&repo, "a.txt", "one\n", "first commit");
    commit_file(&repo, "b.txt", "two\nthree\n", "second commit");

    let head = git_output(&repo, &["rev-parse", "HEAD"]);
    let reply = query(
        &mut reader,
        &mut writer,
        "detail-1",
        "git.git_commit_detail",
        json!({ "projectPath": repo.root.to_string_lossy(), "commitHash": head }),
    )
    .await;
    let result = success_result(reply);
    // The legacy wire shape is snake_case (no rename on the DTO) — assert it as-is.
    assert_eq!(result["hash"], head, "{result}");
    assert_eq!(
        result["short_hash"],
        head.chars().take(7).collect::<String>(),
        "{result}"
    );
    assert_eq!(result["author"], "Test User", "{result}");
    assert_eq!(result["message"], "second commit", "{result}");
    assert!(
        !result["date"].as_str().unwrap_or("").is_empty(),
        "{result}"
    );
    let files = result["files"].as_array().expect("files");
    assert_eq!(files.len(), 1, "{result}");
    assert_eq!(files[0]["path"], "b.txt", "{result}");
    assert_eq!(files[0]["additions"], 2, "{result}");
    assert_eq!(files[0]["deletions"], 0, "{result}");
    assert_eq!(result["total_additions"], 2, "{result}");
    assert_eq!(result["total_deletions"], 0, "{result}");

    // A bogus-but-well-formed revision parses to an empty detail (legacy never
    // checks the git exit codes — the parse of empty output is the behaviour).
    let reply_bogus = query(
        &mut reader,
        &mut writer,
        "detail-bogus",
        "git.git_commit_detail",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        }),
    )
    .await;
    let bogus = success_result(reply_bogus);
    assert_eq!(bogus["hash"], "", "{bogus}");
    assert_eq!(bogus["files"], json!([]), "{bogus}");

    // Option-like revision is refused by the validator before git runs.
    let reply_option = query(
        &mut reader,
        &mut writer,
        "detail-option",
        "git.git_commit_detail",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": "--output=/tmp/evil"
        }),
    )
    .await;
    let Frame::Failure(failure_option) = reply_option else {
        panic!("option-like revision must fail");
    };
    assert_eq!(failure_option.error.code, ErrorCode::IoError);

    // Missing commitHash -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "detail-missing",
        "git.git_commit_detail",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing commitHash must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "detail-remote",
        "git.git_commit_detail",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "commitHash": head,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_discard_file_restores_tracked_and_trashes_untracked() {
    let harness = Harness::start("git-discard-file").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-discard-file");
    commit_file(&repo, "tracked.txt", "base\n", "base commit");

    // Tracked: a modified file is restored to HEAD content.
    std::fs::write(repo.root.join("tracked.txt"), "changed\n").expect("modify");
    let reply = query(
        &mut reader,
        &mut writer,
        "discardfile-1",
        "git.git_discard_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "tracked.txt",
            "untracked": false
        }),
    )
    .await;
    let _ = success_result(reply);
    assert_eq!(
        std::fs::read_to_string(repo.root.join("tracked.txt")).expect("read back"),
        "base\n"
    );

    // Untracked: the file goes to the trash (gone from the worktree).
    std::fs::write(repo.root.join("fresh.txt"), "new\n").expect("write fresh");
    let reply_untracked = query(
        &mut reader,
        &mut writer,
        "discardfile-untracked",
        "git.git_discard_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "fresh.txt",
            "untracked": true
        }),
    )
    .await;
    let _ = success_result(reply_untracked);
    assert!(
        !repo.root.join("fresh.txt").exists(),
        "untracked file must leave the worktree"
    );

    // Protected metadata is refused before any delete.
    std::fs::create_dir_all(repo.root.join(".aeroric")).expect("create .aeroric");
    std::fs::write(repo.root.join(".aeroric/state.json"), "{}").expect("write state");
    let reply_protected = query(
        &mut reader,
        &mut writer,
        "discardfile-protected",
        "git.git_discard_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": ".aeroric/state.json",
            "untracked": true
        }),
    )
    .await;
    let Frame::Failure(failure_protected) = reply_protected else {
        panic!("protected path must fail");
    };
    assert_eq!(failure_protected.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_protected.error.message,
        "Refusing to delete protected project metadata"
    );
    assert!(
        repo.root.join(".aeroric/state.json").exists(),
        "protected file must survive"
    );

    // Missing untracked flag -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "discardfile-missing",
        "git.git_discard_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "tracked.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing untracked must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "discardfile-remote",
        "git.git_discard_file",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "tracked.txt",
            "untracked": false,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_discard_files_restores_batch_and_validates_params() {
    let harness = Harness::start("git-discard-files").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-git-discard-files");
    commit_file(&repo, "a.txt", "base a\n", "base commit");
    commit_file(&repo, "b.txt", "base b\n", "second commit");
    std::fs::write(repo.root.join("a.txt"), "changed a\n").expect("modify a");
    std::fs::write(repo.root.join("b.txt"), "changed b\n").expect("modify b");
    std::fs::write(repo.root.join("c.txt"), "changed c\n").expect("modify c");

    // Duplicates dedupe; only the listed files are restored, others stay changed.
    let reply = query(
        &mut reader,
        &mut writer,
        "discardfiles-1",
        "git.git_discard_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["a.txt", "b.txt", "a.txt"],
            "untracked": false
        }),
    )
    .await;
    let _ = success_result(reply);
    assert_eq!(
        std::fs::read_to_string(repo.root.join("a.txt")).expect("read a"),
        "base a\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.root.join("b.txt")).expect("read b"),
        "base b\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.root.join("c.txt")).expect("read c"),
        "changed c\n",
        "unlisted file must stay changed"
    );

    // An empty list is a SILENT SUCCESS (no git run).
    let reply_empty = query(
        &mut reader,
        &mut writer,
        "discardfiles-empty",
        "git.git_discard_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": [],
            "untracked": false
        }),
    )
    .await;
    let _ = success_result(reply_empty);

    // Escape path refused before git runs.
    let reply_escape = query(
        &mut reader,
        &mut writer,
        "discardfiles-escape",
        "git.git_discard_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["sub/../a.txt"],
            "untracked": false
        }),
    )
    .await;
    let Frame::Failure(failure_escape) = reply_escape else {
        panic!("escape path must fail");
    };
    assert_eq!(failure_escape.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_escape.error.message,
        "File path must stay inside the git worktree"
    );

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "discardfiles-remote",
        "git.git_discard_files",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePaths": ["a.txt"],
            "untracked": false,
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_discard_all_resets_worktree_and_trashes_untracked() {
    let harness = Harness::start("git-discard-all").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // With HEAD: tracked restored, staged-add and untracked trashed, protected kept.
    let repo = new_repo("aeroric-git-discard-all");
    commit_file(&repo, "tracked.txt", "base\n", "base commit");
    std::fs::write(repo.root.join("tracked.txt"), "changed\n").expect("modify");
    std::fs::write(repo.root.join("staged-new.txt"), "staged\n").expect("write staged");
    let add = std::process::Command::new("git")
        .args(["add", "staged-new.txt"])
        .current_dir(&repo.root)
        .output()
        .expect("git add");
    assert!(add.status.success());
    std::fs::write(repo.root.join("loose.txt"), "loose\n").expect("write loose");
    std::fs::create_dir_all(repo.root.join(".aeroric")).expect("create .aeroric");
    std::fs::write(repo.root.join(".aeroric/state.json"), "{}").expect("write state");

    let reply = query(
        &mut reader,
        &mut writer,
        "discardall-1",
        "git.git_discard_all",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let _ = success_result(reply);

    assert_eq!(
        std::fs::read_to_string(repo.root.join("tracked.txt")).expect("read tracked"),
        "base\n",
        "tracked file must be restored to HEAD"
    );
    assert!(
        !repo.root.join("staged-new.txt").exists(),
        "staged-only add resets to untracked and is then trashed"
    );
    assert!(
        !repo.root.join("loose.txt").exists(),
        "untracked file must be trashed"
    );
    assert!(
        repo.root.join(".aeroric/state.json").exists(),
        "protected metadata must survive discard_all"
    );
    let status = git_output(&repo, &["status", "--porcelain"]);
    // The surviving protected directory is itself untracked — the only thing
    // left in porcelain is exactly that.
    assert_eq!(status, "?? .aeroric/", "{status:?}");

    // Unborn HEAD: `rm -r --cached` unstages, then the untracked pass trashes.
    let fresh = new_repo("aeroric-git-discard-all-unborn");
    std::fs::write(fresh.root.join("only.txt"), "only\n").expect("write only");
    let add_only = std::process::Command::new("git")
        .args(["add", "only.txt"])
        .current_dir(&fresh.root)
        .output()
        .expect("git add unborn");
    assert!(add_only.status.success());

    let reply_unborn = query(
        &mut reader,
        &mut writer,
        "discardall-unborn",
        "git.git_discard_all",
        json!({ "projectPath": fresh.root.to_string_lossy() }),
    )
    .await;
    let _ = success_result(reply_unborn);
    assert!(
        !fresh.root.join("only.txt").exists(),
        "staged file on unborn HEAD must be unstaged and trashed"
    );

    // Missing projectPath -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "discardall-missing",
        "git.git_discard_all",
        json!({}),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing projectPath must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "discardall-remote",
        "git.git_discard_all",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
fn conflicting_repo(label: &str, ours: &str, theirs: &str) -> TempRepo {
    let repo = new_repo(label);
    commit_file(&repo, "a.txt", "base\n", "base commit");
    std::process::Command::new("git")
        .args(["checkout", "-b", "b"])
        .current_dir(&repo.root)
        .output()
        .expect("checkout -b");
    commit_file(&repo, "a.txt", &format!("{theirs}\n"), "theirs change");
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&repo.root)
        .output()
        .expect("checkout main");
    commit_file(&repo, "a.txt", &format!("{ours}\n"), "ours change");
    let merge = std::process::Command::new("git")
        .args(["merge", "b"])
        .current_dir(&repo.root)
        .output()
        .expect("merge");
    assert!(
        !merge.status.success(),
        "merge must conflict for the fixture"
    );
    repo
}

#[tokio::test]
async fn git_conflict_preview_returns_hunks_and_validates_params() {
    let harness = Harness::start("git-conflict-preview").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = conflicting_repo("aeroric-git-conflict-preview", "from main", "from b");
    let reply = query(
        &mut reader,
        &mut writer,
        "preview-1",
        "git.git_conflict_preview",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt"
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["filePath"], "a.txt", "{result}");
    let hunks = result["hunks"].as_array().expect("hunks");
    assert_eq!(hunks.len(), 1, "{result}");
    assert_eq!(hunks[0]["index"], 1, "{result}");
    assert_eq!(hunks[0]["ours"], "from main\n", "{result}");
    assert_eq!(hunks[0]["theirs"], "from b\n", "{result}");
    // Default merge style has no diff3 base section.
    assert!(hunks[0]["base"].is_null(), "{result}");

    // A file without markers is an ERROR ("No conflict markers found"). A separate
    // repo: the conflicted one can't commit while a.txt is unmerged.
    let clean_repo = new_repo("aeroric-git-conflict-preview-clean");
    commit_file(&clean_repo, "clean.txt", "no markers\n", "clean file");
    let reply_clean = query(
        &mut reader,
        &mut writer,
        "preview-clean",
        "git.git_conflict_preview",
        json!({
            "projectPath": clean_repo.root.to_string_lossy(),
            "filePath": "clean.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_clean) = reply_clean else {
        panic!("non-conflict file must fail");
    };
    assert_eq!(failure_clean.error.code, ErrorCode::IoError);
    assert_eq!(failure_clean.error.message, "No conflict markers found");

    // Escape path refused before any read.
    let reply_escape = query(
        &mut reader,
        &mut writer,
        "preview-escape",
        "git.git_conflict_preview",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "../secrets.txt"
        }),
    )
    .await;
    let Frame::Failure(failure_escape) = reply_escape else {
        panic!("escape path must fail");
    };
    assert_eq!(failure_escape.error.code, ErrorCode::IoError);
    assert_eq!(
        failure_escape.error.message,
        "File path must stay inside the git worktree"
    );

    // Missing filePath -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "preview-missing",
        "git.git_conflict_preview",
        json!({ "projectPath": repo.root.to_string_lossy() }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing filePath must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "preview-remote",
        "git.git_conflict_preview",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn git_resolve_conflict_resolves_and_stages() {
    let harness = Harness::start("git-resolve-conflict").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // ours: file content becomes the ours side and the path is staged.
    let repo = conflicting_repo("aeroric-git-resolve-ours", "from main", "from b");
    let reply = query(
        &mut reader,
        &mut writer,
        "resolve-ours",
        "git.git_resolve_conflict",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "resolution": "ours"
        }),
    )
    .await;
    let _ = success_result(reply);
    assert_eq!(
        std::fs::read_to_string(repo.root.join("a.txt")).expect("read ours"),
        "from main\n"
    );
    // Ours == HEAD here, so porcelain is clean once resolved — the resolution
    // criterion is "no unmerged entries left", not a staged-M status.
    let unmerged = git_output(&repo, &["ls-files", "-u"]);
    assert_eq!(unmerged, "", "ours must resolve the conflict: {unmerged:?}");
    let status = git_output(&repo, &["status", "--porcelain"]);
    assert_eq!(
        status, "",
        "ours (== HEAD) leaves a clean worktree: {status:?}"
    );

    // theirs
    let repo_theirs = conflicting_repo("aeroric-git-resolve-theirs", "from main", "from b");
    let reply_theirs = query(
        &mut reader,
        &mut writer,
        "resolve-theirs",
        "git.git_resolve_conflict",
        json!({
            "projectPath": repo_theirs.root.to_string_lossy(),
            "filePath": "a.txt",
            "resolution": "theirs"
        }),
    )
    .await;
    let _ = success_result(reply_theirs);
    assert_eq!(
        std::fs::read_to_string(repo_theirs.root.join("a.txt")).expect("read theirs"),
        "from b\n"
    );

    // both: keep-both concatenation, then staged.
    let repo_both = conflicting_repo("aeroric-git-resolve-both", "from main", "from b");
    let reply_both = query(
        &mut reader,
        &mut writer,
        "resolve-both",
        "git.git_resolve_conflict",
        json!({
            "projectPath": repo_both.root.to_string_lossy(),
            "filePath": "a.txt",
            "resolution": "both"
        }),
    )
    .await;
    let _ = success_result(reply_both);
    assert_eq!(
        std::fs::read_to_string(repo_both.root.join("a.txt")).expect("read both"),
        "from main\nfrom b\n"
    );
    let status_both = git_output(&repo_both, &["status", "--porcelain"]);
    assert!(
        status_both.starts_with("M  a.txt"),
        "both must be staged: {status_both:?}"
    );

    // An unknown resolution value is refused.
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "resolve-bad",
        "git.git_resolve_conflict",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "resolution": "mine"
        }),
    )
    .await;
    let Frame::Failure(failure_bad) = reply_bad else {
        panic!("unknown resolution must fail");
    };
    assert_eq!(failure_bad.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "resolve-remote",
        "git.git_resolve_conflict",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "filePath": "a.txt",
            "resolution": "ours",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
#[tokio::test]
async fn git_fetch_pull_push_roundtrip_against_local_remote() {
    let harness = Harness::start("git-network").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    // Fixture: a working repo pushed to a local bare remote, plus a second clone.
    let repo = new_repo("aeroric-git-network-repo");
    commit_file(&repo, "a.txt", "one\n", "first commit");
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let bare = std::env::temp_dir().join(format!(
        "aeroric-git-network-bare-{seq}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&bare);
    let init = std::process::Command::new("git")
        .args(["init", "--bare", "-b", "main"])
        .arg(&bare)
        .output()
        .expect("git init --bare");
    assert!(init.status.success(), "bare init failed");
    std::process::Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(&bare)
        .current_dir(&repo.root)
        .output()
        .expect("git remote add");
    let push_out = std::process::Command::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo.root)
        .output()
        .expect("git push -u");
    assert!(push_out.status.success(), "initial push failed");

    // A second clone whose origin is the same bare remote.
    let clone = std::env::temp_dir().join(format!(
        "aeroric-git-network-clone-{seq}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&clone);
    let clone_out = std::process::Command::new("git")
        .args([
            "clone",
            bare.to_str().expect("bare path"),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone");
    assert!(clone_out.status.success(), "clone failed");

    // New commit lands on the bare remote from the original repo.
    commit_file(&repo, "b.txt", "two\n", "second commit");
    let push2 = std::process::Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(&repo.root)
        .output()
        .expect("git push");
    assert!(push2.status.success());

    // FETCH: `fetch --all` (no prune) — the reply is the combined chatter; the
    // end state is that origin/main advanced in the clone.
    let reply_fetch = query(
        &mut reader,
        &mut writer,
        "net-fetch",
        "git.git_fetch",
        json!({ "projectPath": clone.to_string_lossy() }),
    )
    .await;
    let _ = success_result(reply_fetch);
    let fetched = std::process::Command::new("git")
        .args(["rev-parse", "origin/main"])
        .current_dir(&clone)
        .output()
        .expect("rev-parse origin/main");
    let bare_main = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&bare)
        .output()
        .expect("rev-parse bare main");
    assert_eq!(
        String::from_utf8_lossy(&fetched.stdout).trim(),
        String::from_utf8_lossy(&bare_main.stdout).trim(),
        "fetch must advance origin/main"
    );

    // PULL: the clone's working main advances to the fetched commit.
    let reply_pull = query(
        &mut reader,
        &mut writer,
        "net-pull",
        "git.git_pull",
        json!({ "projectPath": clone.to_string_lossy() }),
    )
    .await;
    let _ = success_result(reply_pull);
    let clone_main = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&clone)
        .output()
        .expect("rev-parse clone main");
    assert_eq!(
        String::from_utf8_lossy(&clone_main.stdout).trim(),
        String::from_utf8_lossy(&bare_main.stdout).trim(),
        "pull must advance the local main"
    );

    // PUSH: a commit in the clone reaches the bare remote via `push origin main`.
    std::fs::write(clone.join("c.txt"), "three\n").expect("write c");
    let add_c = std::process::Command::new("git")
        .args(["add", "c.txt"])
        .current_dir(&clone)
        .output()
        .expect("git add");
    assert!(add_c.status.success());
    let commit_c = std::process::Command::new("git")
        .args(["commit", "-m", "clone commit"])
        .current_dir(&clone)
        .output()
        .expect("git commit");
    assert!(commit_c.status.success());
    let clone_head = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&clone)
        .output()
        .expect("rev-parse clone main after commit");
    let reply_push = query(
        &mut reader,
        &mut writer,
        "net-push",
        "git.git_push",
        json!({ "projectPath": clone.to_string_lossy(), "branch": "main" }),
    )
    .await;
    let _ = success_result(reply_push);
    let pushed = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&bare)
        .output()
        .expect("rev-parse bare after push");
    assert_eq!(
        String::from_utf8_lossy(&pushed.stdout).trim(),
        String::from_utf8_lossy(&clone_head.stdout).trim(),
        "push must land the clone commit on the remote"
    );

    // Fetch with prune: publish a second branch, delete it on the remote, then
    // `--prune` drops the stale remote-tracking ref while plain fetch keeps it.
    // (The checked-out branch's own upstream is protected by git — pruning a
    // secondary branch keeps the scenario unambiguous.)
    std::process::Command::new("git")
        .args(["branch", "topic", "main"])
        .current_dir(&repo.root)
        .output()
        .expect("create topic");
    let push_topic = std::process::Command::new("git")
        .args(["push", "origin", "topic"])
        .current_dir(&repo.root)
        .output()
        .expect("push topic");
    assert!(push_topic.status.success());
    let fetch_topic = std::process::Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(&clone)
        .output()
        .expect("clone fetches topic");
    assert!(fetch_topic.status.success());
    let del = std::process::Command::new("git")
        .args(["push", "origin", "--delete", "topic"])
        .current_dir(&repo.root)
        .output()
        .expect("delete remote topic");
    assert!(del.status.success());

    let reply_noprune = query(
        &mut reader,
        &mut writer,
        "net-fetch-noprune",
        "git.git_fetch",
        json!({ "projectPath": clone.to_string_lossy(), "prune": false }),
    )
    .await;
    let _ = success_result(reply_noprune);
    let kept = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "origin/topic"])
        .current_dir(&clone)
        .output()
        .expect("verify origin/topic kept");
    assert!(kept.status.success(), "no-prune fetch keeps the stale ref");

    let reply_prune = query(
        &mut reader,
        &mut writer,
        "net-fetch-prune",
        "git.git_fetch",
        json!({ "projectPath": clone.to_string_lossy(), "prune": true }),
    )
    .await;
    let _ = success_result(reply_prune);
    let pruned = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "origin/topic"])
        .current_dir(&clone)
        .output()
        .expect("verify origin/topic pruned");
    assert!(!pruned.status.success(), "--prune drops the stale ref");

    // Missing projectPath -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "net-missing",
        "git.git_fetch",
        json!({}),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing projectPath must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);

    // Remote target -> UnsupportedCapability
    let reply_remote = query(
        &mut reader,
        &mut writer,
        "net-remote",
        "git.git_fetch",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&clone);
}
