//! P6-05 Action 1：WorktreeService（create/merge/remove/diffstats——与
//! `src-tauri/src/git.rs` 的 worktree 函数同源语义 verbatim 上收；RCE 守卫
//! ——branch/base_branch 首字符检查，refspec 无法靠 `--` 保护）。
use super::transport::{self, RepoLocks};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// 创建结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreated {
    pub worktree_path: String,
    pub worktree_branch: String,
    pub base_branch: String,
}

/// diff 统计。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiffStats {
    pub additions: i32,
    pub deletions: i32,
}

/// 路径转字符串。
fn path_to_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| "path is not valid UTF-8".to_string())
}

/// 校验 task id（非空、无 NUL/分隔符；与 storage::validate_storage_id 同源）。
pub fn validate_task_id(task_id: &str) -> Result<(), String> {
    if task_id.is_empty() {
        return Err("task id is empty".to_string());
    }
    if task_id.contains('\0') || task_id.contains('/') || task_id.contains('\\') {
        return Err("task id contains invalid characters".to_string());
    }
    Ok(())
}

/// worktrees 根归属校验（RCE 守卫：worktree_path 必须落在 `.aeroric/worktrees`
/// 下；canonicalize 后比对）。
fn ensure_path_under_worktrees_root(
    project_root: &Path,
    worktree_path: &str,
) -> Result<std::path::PathBuf, String> {
    let root = project_root.join(".aeroric").join("worktrees");
    let path = Path::new(worktree_path);
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {e}"))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve root: {e}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("worktree path is outside the worktrees root".to_string());
    }
    Ok(canonical)
}

/// WorktreeService（create/merge/remove/diffstats；mutating 串行化）。
#[derive(Default, Clone)]
pub struct WorktreeService {
    pub locks: Arc<RepoLocks>,
}

impl WorktreeService {
    pub fn new() -> Self {
        Self::default()
    }

    /// create（mutating；**同名目录已存在拒绝**——不能在按「继续」时重新
    /// 创建覆盖用户文件；branch 首字符检查 RCE 守卫）。
    pub fn create(
        &self,
        project_root: &Path,
        task_id: &str,
        base_branch: &str,
    ) -> Result<WorktreeCreated, String> {
        validate_task_id(task_id)?;
        transport::validate_revision(base_branch)?;
        let worktrees_dir = project_root.join(".aeroric").join("worktrees");
        std::fs::create_dir_all(&worktrees_dir)
            .map_err(|e| format!("Failed to create worktrees dir: {e}"))?;
        let worktree_path = super::worktree_policy::worktree_path_for(project_root, task_id);
        if worktree_path.exists() {
            return Err(format!(
                "Worktree path already exists: {}",
                worktree_path.display()
            ));
        }
        let wt_path_str = path_to_string(&worktree_path)?;
        let branch = super::worktree_policy::task_worktree_branch_name(task_id);
        let output = transport::run_git(
            project_root,
            &["worktree", "add", &wt_path_str, "-b", &branch, base_branch],
            transport::READ_TIMEOUT,
            Some(&self.locks.lock_for(&path_to_string(project_root)?)),
        )?;
        if !output.success {
            return Err(output.combined());
        }
        Ok(WorktreeCreated {
            worktree_path: wt_path_str,
            worktree_branch: branch,
            base_branch: base_branch.to_string(),
        })
    }

    /// merge（mutating；RCE 守卫 branch/base_branch 首字符检查——refspec 无法
    /// 靠 `--end-of-options` 保护，首字符检查是唯一可靠手段；worktree 自身有
    /// 未提交修改 → 拒绝合并）。
    pub fn merge(
        &self,
        project_root: &Path,
        worktree_path: &str,
        branch: &str,
        base_branch: &str,
    ) -> Result<String, String> {
        transport::validate_revision(branch)?;
        transport::validate_revision(base_branch)?;
        let canonical = ensure_path_under_worktrees_root(project_root, worktree_path)?;
        // 0) worktree 自身有未提交修改 → 拒绝合并。
        let wt_status = transport::run_git(
            &canonical,
            &["status", "--porcelain"],
            transport::READ_TIMEOUT,
            None,
        )?;
        if !wt_status.success {
            return Err(wt_status.combined());
        }
        if !wt_status.stdout.is_empty() {
            return Err(
                "Worktree has uncommitted changes; commit or stash them before merging".into(),
            );
        }
        // 主仓当前 HEAD：HEAD == base 时直接 merge，否则 fetch ff（不切走 HEAD）。
        let head_out = transport::run_git(
            project_root,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            transport::READ_TIMEOUT,
            None,
        )?;
        if !head_out.success {
            return Err(head_out.combined());
        }
        let original_branch = head_out.stdout.trim().to_string();
        let lock = self.locks.lock_for(&path_to_string(project_root)?);
        if original_branch == base_branch {
            // 直接合并（保留 merge commit 让历史可追溯）。
            let merge_out = transport::run_git(
                project_root,
                &["merge", "--no-ff", "--end-of-options", branch],
                transport::READ_TIMEOUT,
                Some(&lock),
            )?;
            let combined = merge_out.combined();
            if !merge_out.success {
                return Err(format!(
                    "Merge failed (main repo on '{base_branch}'; please resolve manually): {combined}"
                ));
            }
            return Ok(combined.trim().to_string());
        }
        // fetch ff（不切走 HEAD；无 `+` 前缀只允许 fast-forward）。
        let refspec = format!("{branch}:{base_branch}");
        let ff_out = transport::run_git(
            project_root,
            &["fetch", ".", &refspec],
            transport::READ_TIMEOUT,
            Some(&lock),
        )?;
        if !ff_out.success {
            return Err(format!(
                "Cannot fast-forward '{}' (worktree may have diverged from base). \
                 Pull base into the worktree and retry, or merge manually. Detail: {}",
                base_branch,
                ff_out.combined()
            ));
        }
        Ok(format!("Fast-forwarded '{base_branch}' to '{branch}'"))
    }

    /// remove（mutating；`--force` 清理元数据＋`-D` 删除未合并分支；移除需要
    /// 确认——由调用方 UI 承载）。
    pub fn remove(
        &self,
        project_root: &Path,
        worktree_path: &str,
        branch: &str,
    ) -> Result<(), String> {
        let canonical = ensure_path_under_worktrees_root(project_root, worktree_path)?;
        let wt_path_str = path_to_string(&canonical)?;
        let lock = self.locks.lock_for(&path_to_string(project_root)?);
        let remove_out = transport::run_git(
            project_root,
            &["worktree", "remove", "--force", &wt_path_str],
            transport::READ_TIMEOUT,
            Some(&lock),
        )?;
        if !remove_out.success {
            return Err(remove_out.combined());
        }
        if !branch.trim().is_empty() {
            // -D 允许删除未合并分支（丢弃语义）。
            let branch_out = transport::run_git(
                project_root,
                &["branch", "-D", branch],
                transport::READ_TIMEOUT,
                Some(&lock),
            )?;
            if !branch_out.success {
                return Err(branch_out.combined());
            }
        }
        Ok(())
    }

    /// diffstats（+/− 行数；merge-base 而非 base_branch 本身——避免主仓 base
    /// 推进后把别人提交的改动算到本任务头上）。
    pub fn diff_stats(
        &self,
        project_root: &Path,
        worktree_path: &str,
        base_branch: &str,
    ) -> Result<WorktreeDiffStats, String> {
        transport::validate_revision(base_branch)?;
        let canonical = ensure_path_under_worktrees_root(project_root, worktree_path)?;
        let mb_out = transport::run_git(
            &canonical,
            &["merge-base", base_branch, "HEAD"],
            transport::READ_TIMEOUT,
            None,
        )?;
        if !mb_out.success {
            return Err(mb_out.combined());
        }
        let merge_base = mb_out.stdout.trim().to_string();
        let mut additions = 0i32;
        let mut deletions = 0i32;
        if !merge_base.is_empty() {
            let num_out = transport::run_git(
                &canonical,
                &["diff", "--numstat", &merge_base],
                transport::READ_TIMEOUT,
                None,
            )?;
            if !num_out.success {
                return Err(num_out.combined());
            }
            super::worktree_policy::accumulate_numstat(
                &num_out.stdout,
                &mut additions,
                &mut deletions,
            );
        }
        // 未跟踪文件：逐个 --no-index 与空文件比对（退出码 1＝不同，不能用
        // status 判断成败）。
        let untracked = self.list_untracked(&canonical)?;
        for rel in untracked {
            let abs = canonical.join(&rel);
            let abs_str = abs.to_string_lossy().into_owned();
            let no_index = transport::run_git(
                &canonical,
                &["diff", "--no-index", "--numstat", "/dev/null", &abs_str],
                transport::READ_TIMEOUT,
                None,
            );
            if let Ok(out) = no_index {
                super::worktree_policy::accumulate_numstat(
                    &out.stdout,
                    &mut additions,
                    &mut deletions,
                );
            }
        }
        Ok(WorktreeDiffStats {
            additions,
            deletions,
        })
    }

    /// 未跟踪文件列表（`ls-files --others`）。
    fn list_untracked(&self, worktree_root: &Path) -> Result<Vec<String>, String> {
        let output = transport::run_git(
            worktree_root,
            &["ls-files", "--others", "--exclude-standard"],
            transport::READ_TIMEOUT,
            None,
        )?;
        if !output.success {
            return Err(output.combined());
        }
        Ok(output
            .stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }
}
