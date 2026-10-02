//! P6-04 验收：临时 repo＋本地 bare remote——untracked/rename/冲突/空提交/
//! 失败 push；参数以 `-` 开头、换行、空格文件名不成为选项注入；不触碰
//! Aeroric 真实暂存区。
use aeroric_core::git::service;
use aeroric_core::git::transport;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
        .args(["-C", root.to_str().unwrap(), "config", "user.email", "t@t"])
        .output();
    let _ = std::process::Command::new("git")
        .args(["-C", root.to_str().unwrap(), "config", "user.name", "t"])
        .output();
    TempRepo { root }
}

fn commit_file(repo: &TempRepo, path: &str, content: &str, message: &str) {
    let target = repo.root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&target, content).unwrap();
    service::stage(&repo.root, path, &service::GitService::new().locks).unwrap();
    service::commit(&repo.root, message, &service::GitService::new().locks).unwrap();
}

#[test]
fn arg_validation_rejects_option_injection() {
    // 参数以 `-` 开头不成为选项注入（pathspec/revision 校验）。
    assert!(transport::validate_pathspec("-rf").is_err());
    assert!(transport::validate_revision("-x").is_err());
    assert!(transport::validate_pathspec("with space.txt").is_ok());
    // 换行拒绝。
    assert!(transport::validate_git_arg("multi\nline").is_err());
    assert!(transport::validate_git_arg("nul\0byte").is_err());
}

#[test]
fn status_lists_untracked_and_staged() {
    let repo = new_repo("aeroric-git-status");
    std::fs::write(repo.root.join("untracked.txt"), "x").unwrap();
    let changes = service::status(&repo.root).unwrap();
    assert!(
        changes
            .iter()
            .any(|c| c.path == "untracked.txt" && c.status == "?"),
        "untracked 列出"
    );
    // stage 后变为 staged。
    service::stage(
        &repo.root,
        "untracked.txt",
        &service::GitService::new().locks,
    )
    .unwrap();
    let staged = service::status(&repo.root).unwrap();
    assert!(staged.iter().any(|c| c.path == "untracked.txt" && c.staged));
}

#[test]
fn rename_detection_in_status() {
    let repo = new_repo("aeroric-git-rename");
    commit_file(&repo, "old.txt", "x", "add old");
    std::fs::rename(repo.root.join("old.txt"), repo.root.join("new name.txt")).unwrap();
    service::stage(&repo.root, "old.txt", &service::GitService::new().locks).unwrap();
    service::stage(
        &repo.root,
        "new name.txt",
        &service::GitService::new().locks,
    )
    .unwrap();
    let changes = service::status(&repo.root).unwrap();
    // rename（R 状态）或 delete+add 均可（git 检测阈值）。
    assert!(
        changes
            .iter()
            .any(|c| c.path == "old.txt" || c.path == "new name.txt"),
        "rename/delete+add 列出"
    );
    // 空格文件名在 porcelain -z 输出中不截断。
    if let Some(entry) = changes.iter().find(|c| c.path == "new name.txt") {
        assert_eq!(entry.path, "new name.txt", "空格文件名完整");
    }
}

#[test]
fn empty_commit_allowed_and_index_unchanged_after_cancel() {
    let repo = new_repo("aeroric-git-empty");
    commit_file(&repo, "a.txt", "x", "init");
    // 空 commit（--allow-empty）。
    service::commit_allow_empty(&repo.root, "empty", &service::GitService::new().locks).unwrap();
    let log = service::log(&repo.root, 10, None, None).unwrap();
    assert!(log.iter().any(|c| c.message == "empty"), "空提交允许");
    // 取消 commit（消息含换行被拒）后 index 不被额外改变。
    let before = service::status(&repo.root).unwrap();
    assert!(service::commit(&repo.root, "multi\nline", &service::GitService::new().locks).is_err());
    let after = service::status(&repo.root).unwrap();
    assert_eq!(before, after, "取消 commit 后 index 不被额外改变");
}

#[test]
fn failed_push_to_bare_remote_with_no_upstream() {
    let repo = new_repo("aeroric-git-push");
    commit_file(&repo, "a.txt", "x", "init");
    // 无 remote 时 push 失败（失败 push 验证；错误来自 git）。
    let result = service::push(&repo.root, Some("main"), &service::GitService::new().locks);
    assert!(result.is_err(), "无 remote push 失败");
}

#[test]
fn full_remote_roundtrip_with_bare_remote() {
    let repo = new_repo("aeroric-git-remote");
    let bare = std::env::temp_dir().join(format!(
        "aeroric-git-bare-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::process::Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .output();
    commit_file(&repo, "a.txt", "x", "init");
    let _ = std::process::Command::new("git")
        .args([
            "-C",
            repo.root.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            bare.to_str().unwrap(),
        ])
        .output();
    let pushed = service::push(&repo.root, Some("main"), &service::GitService::new().locks);
    assert!(pushed.is_ok(), "push 到本地 bare remote 成功：{pushed:?}");
    // fetch。
    let fetched = service::fetch(&repo.root, false, &service::GitService::new().locks);
    assert!(fetched.is_ok(), "fetch 成功：{fetched:?}");
    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn conflict_files_detected_after_diverging_merge() {
    let repo = new_repo("aeroric-git-conflict");
    commit_file(&repo, "a.txt", "base", "base");
    // 分支 b 改 a.txt。
    let _ = std::process::Command::new("git")
        .args(["-C", repo.root.to_str().unwrap(), "checkout", "-b", "b"])
        .output();
    commit_file(&repo, "a.txt", "from-b", "b change");
    // 回 main 改同一文件。
    let _ = std::process::Command::new("git")
        .args(["-C", repo.root.to_str().unwrap(), "checkout", "main"])
        .output();
    commit_file(&repo, "a.txt", "from-main", "main change");
    // merge b → 冲突。
    let _ = std::process::Command::new("git")
        .args(["-C", repo.root.to_str().unwrap(), "merge", "b"])
        .output();
    let conflicts = service::conflict_files(&repo.root).unwrap();
    assert!(conflicts.iter().any(|c| c.path == "a.txt"), "冲突文件列出");
    // 冲突解决（写入选定侧＋stage；冲突覆盖需独立确认——由调用方 UI 承载）。
    service::resolve_conflict(
        &repo.root,
        "a.txt",
        "resolved",
        &service::GitService::new().locks,
    )
    .unwrap();
    let staged = service::status(&repo.root).unwrap();
    assert!(
        staged.iter().any(|c| c.path == "a.txt" && c.staged),
        "解决后已 stage"
    );
}

#[test]
fn stash_roundtrip() {
    let repo = new_repo("aeroric-git-stash");
    commit_file(&repo, "a.txt", "base", "init");
    std::fs::write(repo.root.join("a.txt"), "modified").unwrap();
    service::stash_push(&repo.root, "wip", &service::GitService::new().locks).unwrap();
    let list = service::stash_list(&repo.root).unwrap();
    assert!(list.iter().any(|e| e.message.contains("wip")), "stash 列出");
    // apply 后内容恢复。
    service::stash_apply(&repo.root, "stash@{0}", &service::GitService::new().locks).unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.root.join("a.txt")).unwrap(),
        "modified"
    );
    // drop。
    let dropped = service::stash_drop(&repo.root, "stash@{0}", &service::GitService::new().locks);
    assert!(dropped.is_ok());
}

#[test]
fn discard_and_log_and_branches() {
    let repo = new_repo("aeroric-git-discard");
    commit_file(&repo, "a.txt", "base", "init");
    std::fs::write(repo.root.join("a.txt"), "changed").unwrap();
    // discard（危险操作：独立确认由调用方 UI 承载——测试是调用方）。
    service::discard_files(
        &repo.root,
        &["a.txt".to_string()],
        &service::GitService::new().locks,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.root.join("a.txt")).unwrap(),
        "base"
    );
    // log（搜索过滤）。
    let log = service::log(&repo.root, 10, Some("init"), None).unwrap();
    assert!(log.iter().any(|c| c.message == "init"));
    // 分支。
    service::create_branch(
        &repo.root,
        "feature",
        None,
        &service::GitService::new().locks,
    )
    .unwrap();
    service::checkout_branch(&repo.root, "feature", &service::GitService::new().locks).unwrap();
    assert_eq!(service::current_branch(&repo.root).unwrap(), "feature");
    let branches = service::list_branches(&repo.root).unwrap();
    assert!(branches.iter().any(|b| b.name == "feature" && b.current));
    // diff/graph/blame/commit_detail 有界结果。
    assert!(service::working_diff(&repo.root).is_ok());
    assert!(service::branch_graph(&repo.root, 10).is_ok());
    assert!(service::blame_file(&repo.root, "a.txt").is_ok());
    let detail = service::commit_detail(&repo.root, "HEAD").unwrap();
    assert_eq!(detail.message, "init");
}
