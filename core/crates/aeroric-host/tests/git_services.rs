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
