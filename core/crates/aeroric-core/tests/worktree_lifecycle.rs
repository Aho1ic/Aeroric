//! P6-05 验收：tmp repo 完成 create→task→diff→merge；冲突/dirty/discard/
//! reopen 反例；worktree 策略判定。
use aeroric_core::git::service;
use aeroric_core::git::worktree;
use aeroric_core::git::worktree_policy;
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
    std::fs::write(repo.root.join(path), content).unwrap();
    service::stage(&repo.root, path, &service::GitService::new().locks).unwrap();
    service::commit(&repo.root, message, &service::GitService::new().locks).unwrap();
}

#[test]
fn create_task_diff_merge_roundtrip() {
    let repo = new_repo("aeroric-wt-roundtrip");
    commit_file(&repo, "a.txt", "base", "init");
    let svc = worktree::WorktreeService::new();
    // create。
    let created = svc.create(&repo.root, "task-1", "main").unwrap();
    assert!(created.worktree_branch.starts_with("aeroric-task/task-1"));
    // task：在 worktree 里改文件并提交。
    let wt_path = PathBuf::from(&created.worktree_path);
    std::fs::write(wt_path.join("a.txt"), "changed by task").unwrap();
    service::stage(&wt_path, "a.txt", &service::GitService::new().locks).unwrap();
    service::commit(&wt_path, "task change", &service::GitService::new().locks).unwrap();
    // diff（有界结果）。
    let stats = svc
        .diff_stats(&repo.root, &created.worktree_path, "main")
        .unwrap();
    assert!(stats.additions > 0 || stats.deletions > 0, "diffstats 非零");
    // merge（HEAD == base → 直接合并）。
    let merged = svc
        .merge(
            &repo.root,
            &created.worktree_path,
            &created.worktree_branch,
            "main",
        )
        .unwrap();
    assert!(!merged.is_empty());
    // 合并后主仓内容更新。
    assert_eq!(
        std::fs::read_to_string(repo.root.join("a.txt")).unwrap(),
        "changed by task"
    );
    // remove（清理）。
    let removed = svc
        .remove(
            &repo.root,
            &created.worktree_path,
            &created.worktree_branch,
            false,
        )
        .unwrap();
    assert!(!wt_path.exists(), "remove 后 worktree 目录清理");
    assert!(removed.branch_deleted, "已合并的分支安全删除：{removed:?}");
}

/// E 批 #14 (2026-11-09): removal finds the worktree by path and checks the
/// branch it really has; uncommitted work and an unmerged branch are kept
/// unless `force`.
#[test]
fn remove_checks_the_path_keeps_work_and_unmerged_branches() {
    let repo = new_repo("aeroric-wt-safe-remove");
    commit_file(&repo, "a.txt", "base", "init");
    let svc = worktree::WorktreeService::new();
    let created = svc.create(&repo.root, "task-1", "main").unwrap();
    let other = svc.create(&repo.root, "task-2", "main").unwrap();
    let wt = PathBuf::from(&created.worktree_path);

    let wrong = svc
        .remove(
            &repo.root,
            &created.worktree_path,
            &other.worktree_branch,
            false,
        )
        .unwrap_err();
    assert!(wrong.contains("不是这个 worktree 检出的分支"), "{wrong}");

    std::fs::write(wt.join("a.txt"), "unsaved").unwrap();
    let dirty = svc
        .remove(&repo.root, &created.worktree_path, "", false)
        .unwrap_err();
    assert!(dirty.contains("未提交的改动"), "{dirty}");
    assert!(wt.exists());

    service::stage(&wt, "a.txt", &service::GitService::new().locks).unwrap();
    service::commit(&wt, "unmerged work", &service::GitService::new().locks).unwrap();
    let kept = svc
        .remove(&repo.root, &created.worktree_path, "", false)
        .unwrap();
    assert!(!wt.exists());
    assert!(!kept.branch_deleted);
    assert!(kept.branch_kept.unwrap().contains("还没合并"));
    let branches = std::process::Command::new("git")
        .args([
            "-C",
            repo.root.to_str().unwrap(),
            "branch",
            "--list",
            &created.worktree_branch,
        ])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
        "分支还在"
    );

    std::fs::write(PathBuf::from(&other.worktree_path).join("a.txt"), "x").unwrap();
    let forced = svc
        .remove(
            &repo.root,
            &other.worktree_path,
            &other.worktree_branch,
            true,
        )
        .unwrap();
    assert!(forced.branch_deleted);
    assert!(!PathBuf::from(&other.worktree_path).exists());
}

#[test]
fn worktree_services_share_one_lock_table() {
    assert!(std::sync::Arc::ptr_eq(
        &worktree::WorktreeService::new().locks,
        &worktree::WorktreeService::new().locks
    ));
}

#[test]
fn reopen_same_path_rejected_not_overwriting_user_files() {
    let repo = new_repo("aeroric-wt-reopen");
    commit_file(&repo, "a.txt", "base", "init");
    let svc = worktree::WorktreeService::new();
    svc.create(&repo.root, "task-1", "main").unwrap();
    // 同名目录已存在拒绝（不能在按「继续」时重新创建覆盖用户文件）。
    let err = svc.create(&repo.root, "task-1", "main").unwrap_err();
    assert!(err.contains("already exists"), "reopen 拒绝：{err}");
    // 用户文件不被覆盖（worktree 里的内容保留）。
    assert!(worktree_policy::worktree_path_for(&repo.root, "task-1").exists());
}

#[test]
fn dirty_worktree_blocks_merge() {
    let repo = new_repo("aeroric-wt-dirty");
    commit_file(&repo, "a.txt", "base", "init");
    let svc = worktree::WorktreeService::new();
    let created = svc.create(&repo.root, "task-1", "main").unwrap();
    // dirty（未提交修改）→ merge 拒绝。
    std::fs::write(PathBuf::from(&created.worktree_path).join("a.txt"), "dirty").unwrap();
    let err = svc
        .merge(
            &repo.root,
            &created.worktree_path,
            &created.worktree_branch,
            "main",
        )
        .unwrap_err();
    assert!(
        err.contains("uncommitted changes"),
        "dirty 拒绝 merge：{err}"
    );
}

#[test]
fn worktree_outside_root_rejected() {
    let repo = new_repo("aeroric-wt-outside");
    commit_file(&repo, "a.txt", "base", "init");
    let svc = worktree::WorktreeService::new();
    // worktree 路径在根外 → RCE 守卫拒绝。
    let outside = std::env::temp_dir().join("aeroric-wt-outside-target");
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    assert!(svc
        .merge(&repo.root, outside.to_str().unwrap(), "b", "main")
        .is_err());
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn policy_lifecycle_decisions() {
    // merge 判定（running/dirty/归属/分支）。
    assert!(worktree_policy::can_merge(false, false, true, true).is_ok());
    assert!(
        worktree_policy::can_merge(true, false, true, true).is_err(),
        "running 拒绝"
    );
    assert!(
        worktree_policy::can_merge(false, true, true, true).is_err(),
        "dirty 拒绝"
    );
    assert!(
        worktree_policy::can_merge(false, false, false, true).is_err(),
        "根外拒绝"
    );
    assert!(
        worktree_policy::can_merge(false, false, true, false).is_err(),
        "非本 task 拒绝"
    );
    // remove 判定（确认）。
    assert!(
        worktree_policy::can_remove(false, true, false).is_err(),
        "未确认拒绝"
    );
    assert!(worktree_policy::can_remove(false, true, true).is_ok());
    // 已 discarded 禁用 resume/merge/discard（历史与路径审计保留）。
    assert!(worktree_policy::discarded_blocks_interaction(true).is_err());
    assert!(worktree_policy::discarded_blocks_interaction(false).is_ok());
    // 远程项目不得带 worktree。
    assert!(worktree_policy::remote_project_blocks_worktree(true).is_err());
    // create 判定（同名目录拒绝）。
    assert!(worktree_policy::can_create(false, true).is_err());
    // 分支名。
    assert_eq!(
        worktree_policy::task_worktree_branch_name("t1"),
        "aeroric-task/t1"
    );
    // numstat 累计。
    let mut a = 0i32;
    let mut d = 0i32;
    worktree_policy::accumulate_numstat("3\t1\tfile\n", &mut a, &mut d);
    assert_eq!((a, d), (3, 1));
}

#[test]
fn task_id_validation() {
    assert!(worktree::validate_task_id("").is_err());
    assert!(worktree::validate_task_id("a/b").is_err());
    assert!(worktree::validate_task_id("valid-id-123").is_ok());
}
