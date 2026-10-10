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
