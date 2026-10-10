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

/// 分支图提交（legacy `GitBranchGraphCommit` 形状；wire 为 camelCase）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchGraphCommit {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub author: String,
    pub relative_time: String,
}

/// 分支图结果（legacy 形状）。`truncated` 的语义是 legacy 的：`limit + 1` 条里
/// 拿得回这么多就说明还有更多，于是截到 `limit` 并置 true。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchGraphResult {
    pub commits: Vec<GitBranchGraphCommit>,
    pub truncated: bool,
}

/// 解析 `log --pretty=%H%x1f%P%x1f%D%x1f%an%x1f%cr%x1f%s`（legacy verbatim：
/// hash 空则跳过；parents 按空白切；refs 按 `", "` 切并去空）。
pub fn parse_branch_graph_log(stdout: &[u8]) -> Vec<GitBranchGraphCommit> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(6, '\x1f');
            let hash = parts.next()?.trim().to_string();
            if hash.is_empty() {
                return None;
            }
            let parents = parts
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .map(|parent| parent.to_string())
                .collect::<Vec<_>>();
            let refs = parts
                .next()
                .unwrap_or_default()
                .split(", ")
                .map(str::trim)
                .filter(|reference| !reference.is_empty())
                .map(|reference| reference.to_string())
                .collect::<Vec<_>>();
            let author = parts.next().unwrap_or_default().trim().to_string();
            let relative_time = parts.next().unwrap_or_default().trim().to_string();
            let subject = parts.next().unwrap_or_default().trim().to_string();

            Some(GitBranchGraphCommit {
                short_hash: short_commit_hash(&hash),
                hash,
                parents,
                refs,
                subject,
                author,
                relative_time,
            })
        })
        .collect()
}

/// 分支图（legacy `git_branch_graph` verbatim）。
///
/// legacy 的机制是三步，**不是**"跑 log 看输出"：
/// ① 先解析 worktree root（legacy `git_worktree_root`：`rev-parse --show-toplevel`，
///    非 repo 在这一步就报错并原样上抛 git 的 stderr，随后 canonicalize 双方并做
///    root 包含判定）——把非 repo 路径回落成空图是行为变更；
/// ② 再探 HEAD（legacy `git_has_head`：`rev-parse --verify HEAD`，失败一律当 false），
///    无提交的仓库**返回空结果而不是错误**，且根本不跑 log；
/// ③ 最后才跑 log：`limit` 缺省 80 并 clamp 到 1..=200，取 `limit+1` 条判定
///    `truncated`；`-n` 与条数是两个独立 argv（legacy verbatim）。
///
/// 探测两步在 legacy 走不带超时的 `run_git`；core 的 `run_git` 必须给超时，取
/// `READ_TIMEOUT`（本地 rev-parse 正常即时返回，这不是行为差异的载体）。
pub fn branch_graph_result(
    repo_root: &Path,
    limit: Option<u32>,
) -> Result<GitBranchGraphResult, String> {
    let probed = transport::run_git(
        repo_root,
        &["rev-parse", "--show-toplevel"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !probed.success {
        return Err(probed.stderr.trim().to_string());
    }
    let root = probed.stdout.trim().to_string();
    if root.is_empty() {
        return Err("Cannot resolve git worktree root".to_string());
    }
    let root = std::path::PathBuf::from(&root)
        .canonicalize()
        .map_err(|error| format!("Cannot resolve git worktree root: {error}"))?;
    let project = repo_root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project path: {error}"))?;
    if !project.starts_with(&root) {
        return Err("Git worktree root does not contain project path".to_string());
    }

    let head = transport::run_git(
        &root,
        &["rev-parse", "--verify", "HEAD"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !head.success {
        return Ok(GitBranchGraphResult {
            commits: Vec::new(),
            truncated: false,
        });
    }

    let limit = limit.unwrap_or(80).clamp(1, 200);
    let fetch_limit = (limit + 1).to_string();
    let output = transport::run_git(
        &root,
        &[
            "log",
            "--all",
            "--decorate=short",
            "--date=relative",
            "--pretty=format:%H%x1f%P%x1f%D%x1f%an%x1f%cr%x1f%s",
            "-n",
            &fetch_limit,
        ],
        transport::BRANCH_GRAPH_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to load git branch graph",
        ));
    }
    let mut commits = parse_branch_graph_log(output.stdout.as_bytes());
    let truncated = commits.len() > limit as usize;
    if truncated {
        commits.truncate(limit as usize);
    }
    Ok(GitBranchGraphResult { commits, truncated })
}

/// blame 行（legacy `GitBlameLine` 形状；wire 为 camelCase）。
///
/// `rename_all` 是承重的：core 的字段是 snake_case（`short_commit`/`author_time`），
/// 而 legacy/Tauri 的 wire 是 camelCase。少了它宿主会发出 snake_case，
/// 客户端解码出 null——本片就是被这条断言当场抓住的。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBlameLine {
    pub line: u32,
    pub commit: String,
    pub short_commit: String,
    pub author: String,
    pub author_time: i64,
    pub summary: String,
    pub content: String,
}

/// blame 结果（legacy `GitBlameResult` 形状；同上，`filePath` 需 camelCase）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBlameResult {
    pub file_path: String,
    pub lines: Vec<GitBlameLine>,
}

/// 短 commit（legacy `short_commit_hash`：去 `^` 前缀后取**前 8 个字符**）。
fn short_commit_hash(commit: &str) -> String {
    commit
        .trim_start_matches('^')
        .chars()
        .take(8)
        .collect::<String>()
}

/// git 相对路径防御（legacy `git_path_guard::validate_git_relative_path` 的本地面
/// verbatim）：非空、非绝对、无 `.`/`..` 段。**比 `validate_pathspec` 严**——
/// 后者允许 `sub/../x` 这类写法，blame 的前端路径不该接受。
pub fn validate_git_relative_path(file_path: &str) -> Result<(), String> {
    if file_path.is_empty() {
        return Err("File path must not be empty".to_string());
    }
    if file_path.starts_with('/') || std::path::Path::new(file_path).is_absolute() {
        return Err("File path must be relative".to_string());
    }
    if file_path.split('/').any(|part| part == "." || part == "..") {
        return Err("File path must stay inside the git worktree".to_string());
    }
    Ok(())
}

/// 解析 `git blame --line-porcelain`（legacy `parse_blame_porcelain` verbatim：
/// `\t` 前缀行是内容行、`author `/`author-time `/`summary ` 是属性行，头部行需
/// ≥3 段且首段全为十六进制或 `^` 才算 commit 头；`author-time` 解析失败取 0）。
pub fn parse_blame_porcelain(stdout: &[u8]) -> Vec<GitBlameLine> {
    let mut lines = Vec::new();
    let mut commit = String::new();
    let mut final_line = 0u32;
    let mut author = String::new();
    let mut author_time = 0i64;
    let mut summary = String::new();

    for line in String::from_utf8_lossy(stdout).lines() {
        if let Some(content) = line.strip_prefix('\t') {
            lines.push(GitBlameLine {
                line: final_line,
                short_commit: short_commit_hash(&commit),
                commit: commit.clone(),
                author: author.clone(),
                author_time,
                summary: summary.clone(),
                content: content.to_string(),
            });
            continue;
        }
        if let Some(value) = line.strip_prefix("author ") {
            author = value.to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix("author-time ") {
            author_time = value.parse().unwrap_or(0);
            continue;
        }
        if let Some(value) = line.strip_prefix("summary ") {
            summary = value.to_string();
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 3
            && parts[0]
                .chars()
                .all(|ch| ch == '^' || ch.is_ascii_hexdigit())
        {
            commit = parts[0].to_string();
            final_line = parts[2].parse().unwrap_or(0);
            author.clear();
            author_time = 0;
            summary.clear();
        }
    }
    lines
}

/// blame 文件（legacy `git_blame_file` verbatim，两处 argv 与旧 core **不同**：
/// 带 `-c core.quotePath=false`，且用 `--line-porcelain` 而非默认 porcelain；
/// 10s 超时（不是 5s）；返回结构化结果而非原始文本）。
pub fn blame_file_result(repo_root: &Path, file_path: &str) -> Result<GitBlameResult, String> {
    validate_git_relative_path(file_path)?;
    let output = transport::run_git(
        repo_root,
        &[
            "-c",
            "core.quotePath=false",
            "blame",
            "--line-porcelain",
            "--",
            file_path,
        ],
        transport::BLAME_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to load git blame",
        ));
    }
    Ok(GitBlameResult {
        file_path: file_path.to_string(),
        lines: parse_blame_porcelain(output.stdout.as_bytes()),
    })
}

/// 与上游的领先/落后计数（legacy `GitRemoteCounts` 形状：camelCase wire）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitRemoteCounts {
    pub ahead: i32,
    pub behind: i32,
    pub branch: String,
}

/// `rev-list --count --left-right <branch>...@{u}` 的解析（legacy verbatim：
/// 两段十进制才取数，否则**回落 (0,0)**——无上游不是错误）。
pub fn remote_counts_from_rev_list(branch: &str, rev_list_stdout: Option<&str>) -> GitRemoteCounts {
    let (ahead, behind) = rev_list_stdout
        .and_then(|stdout| {
            let parts: Vec<&str> = stdout.split_whitespace().collect();
            if parts.len() == 2 {
                Some((parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0)))
            } else {
                None
            }
        })
        .unwrap_or((0, 0));
    GitRemoteCounts {
        ahead,
        behind,
        branch: branch.to_string(),
    }
}

/// 领先/落后计数（只读）。`branch` 为空/缺省时取当前分支
/// （`rev-parse --abbrev-ref HEAD`，取不到就用空串——legacy 不把它当错误）。
pub fn remote_counts(repo_root: &Path, branch: Option<&str>) -> Result<GitRemoteCounts, String> {
    let branch = match branch.filter(|value| !value.is_empty()) {
        Some(branch) => {
            transport::validate_revision(branch)?;
            branch.to_string()
        }
        None => {
            let output = transport::run_git(
                repo_root,
                &["rev-parse", "--abbrev-ref", "HEAD"],
                transport::READ_TIMEOUT,
                None,
            )?;
            output.stdout.trim().to_string()
        }
    };
    let rev_str = format!("{branch}...@{{u}}");
    let rev_out = transport::run_git(
        repo_root,
        &[
            "rev-list",
            "--count",
            "--left-right",
            "--end-of-options",
            &rev_str,
        ],
        transport::READ_TIMEOUT,
        None,
    );
    let rev_stdout = match rev_out {
        Ok(output) if output.success => Some(output.stdout.trim().to_string()),
        _ => None,
    };
    Ok(remote_counts_from_rev_list(&branch, rev_stdout.as_deref()))
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

/// 分支名形状校验（legacy `validate_git_branch_name` verbatim）：非空、不以 `-` 开头。
/// 与 `validate_revision` 的区别是**空串**：revision 校验空串报 "revision is empty"，
/// 这里报 "Branch name must not be empty"——报文按 legacy 保留，不合并成一条。
pub fn validate_branch_name(branch_name: &str) -> Result<(), String> {
    if branch_name.is_empty() {
        return Err("Branch name must not be empty".to_string());
    }
    if branch_name.starts_with('-') {
        return Err("Branch name must not start with '-'".to_string());
    }
    Ok(())
}

/// 建分支（legacy `git_create_branch` verbatim）：`checkout: true` 走
/// `checkout -b <branch> <from>`，否则走 `branch <branch> <from>`。
/// **`from` 是必填**——legacy 签名就是 `String`，不像旧 core 的 `Option` 会省略基准。
pub fn create_branch_at(
    repo_root: &Path,
    branch: &str,
    from: &str,
    checkout: bool,
    locks: &RepoLocks,
) -> Result<(), String> {
    validate_branch_name(branch)?;
    validate_branch_name(from)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let args: Vec<&str> = if checkout {
        vec!["checkout", "-b", branch, from]
    } else {
        vec!["branch", branch, from]
    };
    let output = transport::run_git(repo_root, &args, transport::READ_TIMEOUT, Some(&lock))?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to create branch",
        ));
    }
    Ok(())
}

/// 切分支（legacy `git_checkout_branch` verbatim）：`is_remote: true` 时把
/// `origin/main` 拆成本地名 `main` 并 `checkout -b main --track remotes/origin/main`
/// （**`remotes/` 前缀是 legacy 的写法**，不是 `origin/main`）。
pub fn checkout_branch_tracking(
    repo_root: &Path,
    branch: &str,
    is_remote: bool,
    locks: &RepoLocks,
) -> Result<(), String> {
    validate_branch_name(branch)?;
    let local_name = branch
        .split_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| branch.to_string());
    let tracking = format!("remotes/{branch}");
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let args: Vec<&str> = if is_remote {
        vec!["checkout", "-b", &local_name, "--track", &tracking]
    } else {
        vec!["checkout", branch]
    };
    let output = transport::run_git(repo_root, &args, transport::READ_TIMEOUT, Some(&lock))?;
    if !output.success {
        return Err(transport::git_command_error(&output, "Failed to checkout"));
    }
    Ok(())
}

/// 建分支（不带基准省略语义的旧 core 入口，仍被 core 测试使用）。
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

/// stash 引用校验：**必须**是 `stash@{<digits>}`。
///
/// 这不是 `validate_revision` 的重复：`validate_revision` 只要求非空且不以 `-` 开头，
/// 因此 `HEAD`、`main`、`stash@{-1}` 都能过；而 legacy 两个 stash 命令用的是更窄的
/// `strip_prefix("stash@{")` ＋ 全 ASCII 数字 判定。放宽成 revision 会让这些串真的
/// 到达 git（`stash@{-1}` 还能选中上一个 stash），所以照 legacy 收紧。
pub fn validate_stash_ref(stash_ref: &str) -> Result<(), String> {
    transport::validate_git_arg(stash_ref)?;
    let Some(index) = stash_ref
        .strip_prefix("stash@{")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return Err("Invalid stash reference".to_string());
    };
    if index.is_empty() || !index.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("Invalid stash reference".to_string());
    }
    Ok(())
}

/// stash apply（mutating；串行化；20s——与 legacy 同）。
pub fn stash_apply(repo_root: &Path, stash_ref: &str, locks: &RepoLocks) -> Result<String, String> {
    validate_stash_ref(stash_ref)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["stash", "apply", stash_ref],
        STASH_TIMEOUT,
        Some(&lock),
    )?;
    let combined = format!("{}{}", output.stdout, output.stderr)
        .trim()
        .to_string();
    if !output.success {
        return Err(if combined.is_empty() {
            "Failed to apply git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
}

pub fn stash_drop(repo_root: &Path, stash_ref: &str, locks: &RepoLocks) -> Result<String, String> {
    validate_stash_ref(stash_ref)?;
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &["stash", "drop", stash_ref],
        transport::STASH_DROP_TIMEOUT,
        Some(&lock),
    )?;
    let combined = format!("{}{}", output.stdout, output.stderr)
        .trim()
        .to_string();
    if !output.success {
        return Err(if combined.is_empty() {
            "Failed to drop git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
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
