//! P6-04 Action 1：git status/diff/index/commit/history/refs/stash/conflicts
//! 的业务主体（core 独占；与 `src-tauri/src/git.rs` 同源语义 verbatim 上收）。
use super::transport::{self, RepoLocks, STASH_TIMEOUT};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// 文件变更（与 Tauri `GitFileChange` 对齐）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

/// 分支信息。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitBranchInfo {
    pub name: String,
    pub current: bool,
    pub remote: Option<String>,
}

/// 提交记录。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub refs: Vec<String>,
}

/// stash 条目。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitStashEntry {
    pub index: usize,
    pub message: String,
}

/// 冲突文件。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitConflictFile {
    pub path: String,
}

/// porcelain v1 -z 状态解析（verbatim 上收；`core.quotePath=false` 的 -z
/// 输出：X Y path\0[origPath\0]，R/C 带第二个 NUL 段）。
pub fn parse_porcelain_z_status(stdout: &[u8]) -> Vec<GitFileChange> {
    let mut changes = Vec::new();
    let mut entries = stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());

    while let Some(entry) = entries.next() {
        if entry.len() < 4 || entry[2] != b' ' {
            continue;
        }
        let x = entry[0] as char;
        let y = entry[1] as char;
        let display_path = String::from_utf8_lossy(&entry[3..]).into_owned();

        if x == 'R' || x == 'C' {
            let _ = entries.next();
        }

        if x == '?' && y == '?' {
            changes.push(GitFileChange {
                path: display_path,
                status: "?".to_string(),
                staged: false,
            });
        } else {
            if x != ' ' && x != '?' {
                changes.push(GitFileChange {
                    path: display_path.clone(),
                    status: x.to_string(),
                    staged: true,
                });
            }
            if y != ' ' && y != '?' {
                changes.push(GitFileChange {
                    path: display_path,
                    status: y.to_string(),
                    staged: false,
                });
            }
        }
    }
    changes
}

/// status（只读；5s timeout；不串行）。
pub fn status(repo_root: &Path) -> Result<Vec<GitFileChange>, String> {
    let args = vec![
        "-c",
        "core.quotePath=false",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    ];
    let output = transport::run_git(repo_root, &args, transport::READ_TIMEOUT, None)?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to get git status",
        ));
    }
    Ok(parse_porcelain_z_status(output.stdout.as_bytes()))
}

/// diff（有界结果；原始 diff 可下载到用户指定位置——返回完整字符串不截断，
/// 调用方负责保存到用户指定位置）。
pub fn show_diff(repo_root: &Path, commit_hash: &str) -> Result<String, String> {
    transport::validate_revision(commit_hash)?;
    let output = transport::run_git(
        repo_root,
        &["show", "--format=", commit_hash],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to get diff"));
    }
    Ok(output.stdout)
}

/// 工作区 diff。
pub fn working_diff(repo_root: &Path) -> Result<String, String> {
    let output = transport::run_git(repo_root, &["diff", "HEAD"], transport::READ_TIMEOUT, None)?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to get diff"));
    }
    Ok(output.stdout)
}

/// stage/unstage（mutating；串行化）。
pub fn stage(repo_root: &Path, file_path: &str, locks: &RepoLocks) -> Result<(), String> {
    transport::validate_pathspec(file_path)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["add", "--", file_path],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to stage"));
    }
    Ok(())
}

pub fn stage_all(repo_root: &Path, locks: &RepoLocks) -> Result<(), String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["add", "-A"],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to stage all"));
    }
    Ok(())
}

/// 取消暂存全部（与 `git_unstage_all` 同源：`restore --staged .`，首次提交前
/// 无 HEAD 会失败——legacy 语义如此，不做静默降级）。
pub fn unstage_all(repo_root: &Path, locks: &RepoLocks) -> Result<(), String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["restore", "--staged", "."],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to unstage all",
        ));
    }
    Ok(())
}

pub fn unstage(repo_root: &Path, file_path: &str, locks: &RepoLocks) -> Result<(), String> {
    transport::validate_pathspec(file_path)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["reset", "HEAD", "--", file_path],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to unstage"));
    }
    Ok(())
}

/// commit（mutating；串行化；message 带 `-m`——换行/空格安全，argv 传递）。
pub fn commit(repo_root: &Path, message: &str, locks: &RepoLocks) -> Result<(), String> {
    transport::validate_git_arg(message)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["commit", "-m", message],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to commit"));
    }
    Ok(())
}

/// 空 commit 允许（`--allow-empty`；CI 语义）。
pub fn commit_allow_empty(
    repo_root: &Path,
    message: &str,
    locks: &RepoLocks,
) -> Result<(), String> {
    transport::validate_git_arg(message)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["commit", "--allow-empty", "-m", message],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to commit"));
    }
    Ok(())
}

/// log（只读）。
pub fn log(
    repo_root: &Path,
    limit: usize,
    search: Option<&str>,
    branch: Option<&str>,
) -> Result<Vec<GitCommit>, String> {
    let limit_str = format!("--max-count={limit}");
    let mut args: Vec<&str> = vec![
        "log",
        &limit_str,
        "--pretty=format:%H%x1f%h%x1f%an%x1f%ad%x1f%s%x1f%D",
        "--date=short",
    ];
    if let Some(branch) = branch {
        transport::validate_revision(branch)?;
        args.push(branch);
    }
    let output = transport::run_git(repo_root, &args, transport::READ_TIMEOUT, None)?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to get log"));
    }
    let mut commits: Vec<GitCommit> = output
        .stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\x1f').collect();
            if parts.len() < 5 {
                return None;
            }
            Some(GitCommit {
                hash: parts[0].to_string(),
                short_hash: parts[1].to_string(),
                author: parts[2].to_string(),
                date: parts[3].to_string(),
                message: parts[4].to_string(),
                refs: parts
                    .get(5)
                    .map(|r| {
                        r.split(", ")
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    if let Some(search) = search {
        commits.retain(|c| c.message.contains(search) || c.author.contains(search));
    }
    Ok(commits)
}

/// 分支列表。
pub fn list_branches(repo_root: &Path) -> Result<Vec<GitBranchInfo>, String> {
    let output = transport::run_git(
        repo_root,
        &["branch", "--format=%(refname:short)%09%(upstream:short)"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to list branches",
        ));
    }
    let current = current_branch(repo_root)?;
    Ok(output
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            let branch_name = parts.next().unwrap_or("").to_string();
            let remote = parts.next().filter(|r| !r.is_empty()).map(str::to_string);
            let current_flag = branch_name == current;
            GitBranchInfo {
                name: branch_name,
                current: current_flag,
                remote,
            }
        })
        .collect())
}

pub fn current_branch(repo_root: &Path) -> Result<String, String> {
    let output = transport::run_git(
        repo_root,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to get branch",
        ));
    }
    Ok(output.stdout.trim().to_string())
}

/// checkout/create branch（mutating；串行化）。
pub fn checkout_branch(repo_root: &Path, branch: &str, locks: &RepoLocks) -> Result<(), String> {
    transport::validate_revision(branch)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["checkout", branch],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to checkout"));
    }
    Ok(())
}

pub fn create_branch(
    repo_root: &Path,
    branch: &str,
    from: Option<&str>,
    locks: &RepoLocks,
) -> Result<(), String> {
    transport::validate_revision(branch)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let mut args: Vec<&str> = vec!["branch", branch];
    if let Some(from) = from {
        transport::validate_revision(from)?;
        args.push(from);
    }
    let output = transport::run_git(repo_root, &args, transport::READ_TIMEOUT, Some(&lock))?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to create branch",
        ));
    }
    Ok(())
}

/// push/pull/fetch（mutating + 网络；60s timeout；危险操作确认由调用方 UI
/// 承载——不预设授权）。
pub fn push(repo_root: &Path, branch: Option<&str>, locks: &RepoLocks) -> Result<String, String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let mut args: Vec<&str> = vec!["push"];
    if let Some(branch) = branch {
        transport::validate_revision(branch)?;
        args.push("--set-upstream");
        args.push("origin");
        args.push(branch);
    }
    let output = transport::run_git(repo_root, &args, transport::NETWORK_TIMEOUT, Some(&lock))?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to push"));
    }
    Ok(output.combined())
}

pub fn pull(repo_root: &Path, locks: &RepoLocks) -> Result<String, String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["pull"],
        transport::NETWORK_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to pull"));
    }
    Ok(output.combined())
}

pub fn fetch(repo_root: &Path, prune: bool, locks: &RepoLocks) -> Result<String, String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let args: Vec<&str> = if prune {
        vec!["fetch", "--prune"]
    } else {
        vec!["fetch"]
    };
    let output = transport::run_git(repo_root, &args, transport::NETWORK_TIMEOUT, Some(&lock))?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to fetch"));
    }
    Ok(output.combined())
}

/// discard（危险操作：丢弃未提交更改——独立确认由调用方 UI 承载）。
pub fn discard_files(
    repo_root: &Path,
    file_paths: &[String],
    locks: &RepoLocks,
) -> Result<(), String> {
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    for file_path in file_paths {
        transport::validate_pathspec(file_path)?;
        let output = transport::run_git(
            repo_root,
            &["checkout", "--", file_path],
            transport::READ_TIMEOUT,
            Some(&lock),
        )?;
        if !output.success {
            return Err(transport::git_command_error(&output, "Failed to discard"));
        }
    }
    Ok(())
}

/// stash（mutating；串行化）。
pub fn stash_list(repo_root: &Path) -> Result<Vec<GitStashEntry>, String> {
    let output = transport::run_git(
        repo_root,
        &["stash", "list", "--format=%gd%x1f%gs"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to list stash",
        ));
    }
    Ok(output
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.splitn(2, '\x1f');
            let index = parts
                .next()
                .unwrap_or("")
                .trim_start_matches("stash@{")
                .trim_end_matches('}')
                .parse()
                .unwrap_or(0);
            let message = parts.next().unwrap_or("").to_string();
            GitStashEntry { index, message }
        })
        .collect())
}

/// stash push（mutating；串行化）。
///
/// 与 legacy `git_stash_push` 逐项对齐，三处**不是**等价替换、必须显式照抄：
/// ① message 先 `trim` 再去空——空白消息不发 `-m`；② 返回 `stdout + stderr`（**顺序**
/// 是 stdout 在前；core 的 `GitOutput::combined()` 是 stderr 在前，用它会换序）；
/// ③ 超时 20s（legacy 的 `Duration::from_secs(20)`，不是默认 `READ_TIMEOUT` 的 5s）。
/// 失败回落文案也照 legacy：`Failed to create git stash`。
pub fn stash_push(
    repo_root: &Path,
    message: Option<&str>,
    include_untracked: bool,
    locks: &RepoLocks,
) -> Result<String, String> {
    let mut args: Vec<&str> = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    let trimmed = message.map(str::trim).filter(|value| !value.is_empty());
    if let Some(message) = trimmed {
        transport::validate_git_arg(message)?;
        args.push("-m");
        args.push(message);
    }
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(repo_root, &args, STASH_TIMEOUT, Some(&lock))?;
    let combined = format!("{}{}", output.stdout, output.stderr)
        .trim()
        .to_string();
    if !output.success {
        return Err(if combined.is_empty() {
            "Failed to create git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
}

pub fn stash_apply(repo_root: &Path, stash_ref: &str, locks: &RepoLocks) -> Result<String, String> {
    transport::validate_revision(stash_ref)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["stash", "apply", stash_ref],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to apply stash",
        ));
    }
    Ok(output.combined())
}

pub fn stash_drop(repo_root: &Path, stash_ref: &str, locks: &RepoLocks) -> Result<String, String> {
    transport::validate_revision(stash_ref)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["stash", "drop", stash_ref],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to drop stash",
        ));
    }
    Ok(output.combined())
}

/// `git init`（幂等：已是仓库则**不重跑**——legacy `git_init` 先探 `rev-parse --git-dir`
/// 成功即返回 Ok，所以在既有仓库上换 `initial_branch` 不会改分支）。
pub fn init(
    repo_root: &Path,
    initial_branch: Option<&str>,
    locks: &RepoLocks,
) -> Result<(), String> {
    if let Some(branch) = initial_branch {
        if !branch.is_empty() {
            transport::validate_revision(branch)?;
        }
    }
    let branch = initial_branch
        .filter(|value| !value.is_empty())
        .unwrap_or("main");
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let probe = transport::run_git(
        repo_root,
        &["rev-parse", "--git-dir"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if probe.success {
        return Ok(());
    }
    let branch_arg = format!("--initial-branch={branch}");
    let output = transport::run_git(
        repo_root,
        &["init", &branch_arg],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to init"));
    }
    Ok(())
}

/// 冲突文件列表（与 legacy `git diff --name-only --diff-filter=U -z` 等价：**每个
/// 路径一条**。porcelain 对 `UU` 会各报 index/worktree 一侧，直接映射会重复计数，
/// 所以按首次出现去重并保留原顺序）。
pub fn conflict_files(repo_root: &Path) -> Result<Vec<GitConflictFile>, String> {
    let entries = status(repo_root)?;
    let mut seen = std::collections::HashSet::new();
    Ok(entries
        .into_iter()
        .filter(|c| c.status.contains('U') || c.status == "AA" || c.status == "DD")
        .filter(|c| seen.insert(c.path.clone()))
        .map(|c| GitConflictFile { path: c.path })
        .collect())
}

/// 冲突预览（conflict 标记行）。
pub fn conflict_preview(repo_root: &Path, file_path: &str) -> Result<String, String> {
    transport::validate_pathspec(file_path)?;
    let output = transport::run_git(
        repo_root,
        &["diff", "--", file_path],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to preview conflict",
        ));
    }
    Ok(output.stdout)
}

/// 冲突解决（写入选定侧＋stage；冲突覆盖需独立确认——由调用方 UI 承载）。
pub fn resolve_conflict(
    repo_root: &Path,
    file_path: &str,
    chosen_content: &str,
    locks: &RepoLocks,
) -> Result<(), String> {
    transport::validate_pathspec(file_path)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let target = repo_root.join(file_path);
    std::fs::write(&target, chosen_content.as_bytes())
        .map_err(|e| format!("Failed to write resolved file: {e}"))?;
    let output = transport::run_git(
        repo_root,
        &["add", "--", file_path],
        transport::READ_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to stage resolved file",
        ));
    }
    Ok(())
}

/// blame（只读）。
pub fn blame_file(repo_root: &Path, file_path: &str) -> Result<String, String> {
    transport::validate_pathspec(file_path)?;
    let output = transport::run_git(
        repo_root,
        &["blame", "--", file_path],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to blame"));
    }
    Ok(output.stdout)
}

/// branch graph（`--graph` 装饰输出）。
pub fn branch_graph(repo_root: &Path, limit: usize) -> Result<String, String> {
    let limit_str = format!("--max-count={limit}");
    let output = transport::run_git(
        repo_root,
        &[
            "log",
            "--graph",
            "--oneline",
            "--all",
            "--decorate",
            &limit_str,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to get graph"));
    }
    Ok(output.stdout)
}

/// commit detail（单提交的完整 message）。
pub fn commit_detail(repo_root: &Path, commit_hash: &str) -> Result<GitCommit, String> {
    transport::validate_revision(commit_hash)?;
    let output = transport::run_git(
        repo_root,
        &[
            "show",
            "--no-patch",
            "--pretty=format:%H%x1f%h%x1f%an%x1f%ad%x1f%s%x1f%D",
            "--date=short",
            commit_hash,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to get commit detail",
        ));
    }
    let parts: Vec<&str> = output.stdout.split('\x1f').collect();
    if parts.len() < 5 {
        return Err("malformed commit output".to_string());
    }
    Ok(GitCommit {
        hash: parts[0].to_string(),
        short_hash: parts[1].to_string(),
        author: parts[2].to_string(),
        date: parts[3].to_string(),
        message: parts[4].to_string(),
        refs: parts
            .get(5)
            .map(|r| {
                r.split(", ")
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// 服务句柄（RepoLocks 共享；相同 repo mutating 串行化）。
#[derive(Default, Clone)]
pub struct GitService {
    pub locks: Arc<RepoLocks>,
}

impl GitService {
    pub fn new() -> Self {
        Self::default()
    }
}
