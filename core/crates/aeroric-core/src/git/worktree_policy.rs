//! P6-05 Action 3：worktree 策略（生命周期判定纯函数——merge/discard 前检查
//! running task、dirty/untracked、目标 branch、worktree 是否属于该 task；
//! **开发 Aeroric 本身禁止新增分支**；不能在按「继续」时重新创建同名目录
//! 覆盖用户文件）。

/// 任务关联 worktree 的分支名（与 `task_worktree_branch_name` 同源）。
pub fn task_worktree_branch_name(task_id: &str) -> String {
    format!("aeroric-task/{task_id}")
}

/// worktree 目录（`.aeroric/worktrees/<task_id>`；**同名目录已存在拒绝**——
/// 不能在按「继续」时重新创建覆盖用户文件）。
pub fn worktree_path_for(project_root: &std::path::Path, task_id: &str) -> std::path::PathBuf {
    project_root
        .join(".aeroric")
        .join("worktrees")
        .join(task_id)
}

/// worktree 是否落在 worktrees 根下（归属判定：worktree 是否属于该 task）。
pub fn is_under_worktrees_root(
    project_root: &std::path::Path,
    worktree_path: &std::path::Path,
) -> bool {
    worktree_path.starts_with(project_root.join(".aeroric").join("worktrees"))
}

/// 创建判定（merge/discard 前检查 running task：running task 不能创建新
/// worktree——任务已在运行）。
pub fn can_create(task_running: bool, worktree_exists: bool) -> Result<(), String> {
    if task_running {
        return Err("task is already running".to_string());
    }
    if worktree_exists {
        return Err("worktree path already exists".to_string());
    }
    Ok(())
}

/// merge 判定（Action 4：merge/discard 前检查 running task、dirty/untracked、
/// 目标 branch、worktree 是否属于该 task）。
pub fn can_merge(
    task_running: bool,
    worktree_dirty: bool,
    under_root: bool,
    branch_matches_task: bool,
) -> Result<(), String> {
    if task_running {
        return Err("task is running; stop it before merging".to_string());
    }
    if worktree_dirty {
        return Err(
            "Worktree has uncommitted changes; commit or stash them before merging".to_string(),
        );
    }
    if !under_root {
        return Err("worktree is not under the worktrees root".to_string());
    }
    if !branch_matches_task {
        return Err("worktree does not belong to this task".to_string());
    }
    Ok(())
}

/// remove 判定（移除需要确认——由调用方 UI 承载；失败返回逐步骤恢复信息）。
pub fn can_remove(task_running: bool, under_root: bool, confirmed: bool) -> Result<(), String> {
    if task_running {
        return Err("task is running; stop it before removing".to_string());
    }
    if !under_root {
        return Err("worktree is not under the worktrees root".to_string());
    }
    if !confirmed {
        return Err("removal requires explicit confirmation".to_string());
    }
    Ok(())
}

/// 已 discarded 的 task 禁用 resume/merge/discard（Action 5；历史与路径审计
/// 保留——判定只禁交互，不删数据）。
pub fn discarded_blocks_interaction(discarded: bool) -> Result<(), String> {
    if discarded {
        return Err("task is discarded; resume/merge/discard are disabled".to_string());
    }
    Ok(())
}

/// 远程项目不得带 worktree/附件（P2-05 core 侧已承担的三条判定之一；worktree
/// 只对本地项目）。
pub fn remote_project_blocks_worktree(is_remote: bool) -> Result<(), String> {
    if is_remote {
        return Err("remote projects cannot have worktrees".to_string());
    }
    Ok(())
}

/// diffstats（+/− 行数；accumulate_numstat 同语义）。
pub fn accumulate_numstat(stdout: &str, additions: &mut i32, deletions: &mut i32) {
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let (Some(add), Some(del)) = (parts.next(), parts.next()) else {
            continue;
        };
        if let Ok(a) = add.parse::<i32>() {
            *additions += a;
        }
        if let Ok(d) = del.parse::<i32>() {
            *deletions += d;
        }
    }
}
