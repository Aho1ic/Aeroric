//! P8-05 slice 60: Worktree query services over authenticated UDS.
//! Uses synthetic temporary git repositories.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use aeroric_core::git::{service, worktree};
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
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("{label}-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create repo");
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&root)
        .output()
        .expect("git init");
    let _ = std::process::Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "config",
            "user.email",
            "t@t.com",
        ])
        .output();
    let _ = std::process::Command::new("git")
        .args(["-C", root.to_str().unwrap(), "config", "user.name", "test"])
        .output();
    TempRepo { root }
}

fn commit_file(repo: &TempRepo, path: &str, content: &str, message: &str) {
    std::fs::write(repo.root.join(path), content).unwrap();
    service::stage(&repo.root, path, &service::GitService::new().locks).unwrap();
    service::commit(&repo.root, message, &service::GitService::new().locks).unwrap();
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
async fn worktree_diff_stats_calculates_stats_and_validates_params() {
    let harness = Harness::start("worktree-diff-stats").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-wt-diff-test");
    commit_file(&repo, "file.txt", "line 1\nline 2\n", "initial");

    let svc = worktree::WorktreeService::new();
    let created = svc
        .create(&repo.root, "task-diff-1", "main")
        .expect("create worktree");
    let wt_path = PathBuf::from(&created.worktree_path);

    // Modify file in worktree: add 2 lines, delete 1 line
    std::fs::write(wt_path.join("file.txt"), "line 1\nline 3\nline 4\n").expect("write modified");

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "diff-1",
        "git.worktree_diff_stats",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "baseBranch": "main"
        }),
    )
    .await;
    let stats = success_result(reply);
    assert_eq!(stats["additions"], json!(2));
    assert_eq!(stats["deletions"], json!(1));

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "diff-bad",
        "git.worktree_diff_stats",
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
        "git.worktree_diff_stats",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "baseBranch": "main",
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
async fn remove_task_worktree_removes_worktree_and_branch_and_validates_params() {
    let harness = Harness::start("worktree-remove").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-wt-remove-test");
    commit_file(&repo, "file.txt", "line 1\n", "initial");

    let svc = worktree::WorktreeService::new();
    let created = svc
        .create(&repo.root, "task-remove-1", "main")
        .expect("create worktree");
    let wt_path = PathBuf::from(&created.worktree_path);
    assert!(wt_path.exists());

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "rm-1",
        "git.remove_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "branch": created.worktree_branch
        }),
    )
    .await;
    let _ = success_result(reply);
    assert!(!wt_path.exists());

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "rm-bad",
        "git.remove_task_worktree",
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
        "rm-remote",
        "git.remove_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "branch": "some-branch",
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
async fn create_task_worktree_creates_worktree_and_branch_and_validates_params() {
    let harness = Harness::start("worktree-create").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-wt-create-test");
    commit_file(&repo, "file.txt", "line 1\n", "initial");

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "create-1",
        "git.create_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "taskId": "task-create-1",
            "baseBranch": "main"
        }),
    )
    .await;
    let created = success_result(reply);
    let wt_path = created["worktreePath"].as_str().expect("worktreePath");
    assert!(std::path::Path::new(wt_path).exists());
    assert_eq!(created["baseBranch"], "main");

    // Duplicate creation -> error (already exists)
    let reply_dup = query(
        &mut reader,
        &mut writer,
        "create-dup",
        "git.create_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "taskId": "task-create-1",
            "baseBranch": "main"
        }),
    )
    .await;
    let Frame::Failure(failure_dup) = reply_dup else {
        panic!("duplicate worktree creation must fail");
    };
    assert_eq!(failure_dup.error.code, ErrorCode::IoError);

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "create-bad",
        "git.create_task_worktree",
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
        "create-remote",
        "git.create_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "taskId": "task-remote",
            "baseBranch": "main",
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
async fn merge_task_worktree_merges_worktree_and_validates_params() {
    let harness = Harness::start("worktree-merge").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let repo = new_repo("aeroric-wt-merge-test");
    commit_file(&repo, "file.txt", "line 1\n", "initial");

    let svc = worktree::WorktreeService::new();
    let created = svc
        .create(&repo.root, "task-merge-1", "main")
        .expect("create worktree");
    let wt_path = PathBuf::from(&created.worktree_path);

    // Commit file in worktree
    let wt_file = wt_path.join("file.txt");
    std::fs::write(&wt_file, "line 1\nline 2\n").expect("write file in worktree");
    let output = aeroric_core::git::transport::run_git(
        &wt_path,
        &["commit", "-am", "commit in wt"],
        aeroric_core::git::transport::READ_TIMEOUT,
        None,
    )
    .expect("commit in worktree");
    assert!(output.success);

    // Success call
    let reply = query(
        &mut reader,
        &mut writer,
        "merge-1",
        "git.merge_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "branch": created.worktree_branch,
            "baseBranch": "main"
        }),
    )
    .await;
    let result = success_result(reply);
    assert!(result.is_string());

    // Missing parameter -> InvalidArgument
    let reply_bad = query(
        &mut reader,
        &mut writer,
        "merge-bad",
        "git.merge_task_worktree",
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
        "merge-remote",
        "git.merge_task_worktree",
        json!({
            "projectPath": repo.root.to_string_lossy(),
            "worktreePath": wt_path.to_string_lossy(),
            "branch": created.worktree_branch,
            "baseBranch": "main",
            "target": { "kind": "ssh", "connectionId": "c1", "remotePath": "/var/remote" }
        }),
    )
    .await;
    let Frame::Failure(failure_remote) = reply_remote else {
        panic!("remote target must fail");
    };
    assert_eq!(failure_remote.error.code, ErrorCode::UnsupportedCapability);
}
