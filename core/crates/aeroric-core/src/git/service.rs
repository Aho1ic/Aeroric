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

/// stash 条目（legacy `GitStashEntry` 形状：五个字段逐字——index/name/commit/date/message；
/// 字段名都是单词，camelCase 与 snake_case 同形，但 rename_all 仍要挂着以防未来加多词字段）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub index: u32,
    pub name: String,
    pub commit: String,
    pub date: String,
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

/// 校验并去重一批相对路径（legacy `unique_git_file_paths` 本地面 verbatim：
/// 逐个过 `validate_git_relative_path`，再按首次出现去重——重复且非法的条目
/// 也报错，因为校验先于去重）。
fn unique_git_file_paths(file_paths: &[String]) -> Result<Vec<&str>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<&str> = Vec::new();
    for file_path in file_paths {
        validate_git_relative_path(file_path)?;
        if seen.insert(file_path.as_str()) {
            unique.push(file_path.as_str());
        }
    }
    Ok(unique)
}

/// 暂存一批文件（legacy `git_stage_files` verbatim）：`add -- <paths>`；
/// **空列表是静默成功**（不跑 git——调用方在清空选择时也会走到这里）；
/// 超时 10s（`STAGE_FILES_TIMEOUT`，比单文件 stage 的 5s 宽）；mutating 串行化。
pub fn stage_files(
    repo_root: &Path,
    file_paths: &[String],
    locks: &RepoLocks,
) -> Result<(), String> {
    let unique = unique_git_file_paths(file_paths)?;
    if unique.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["add", "--"];
    args.extend(unique);
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &args,
        transport::STAGE_FILES_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to stage files",
        ));
    }
    Ok(())
}

/// 取消暂存一批文件（legacy `git_unstage_files` verbatim）：先探 HEAD
/// （`rev-parse --verify HEAD`，5s），有 HEAD 走 `restore --staged -- <paths>`、
/// 无 HEAD 回落到不依赖 HEAD 的 `reset -- <paths>`（unborn HEAD 上
/// `restore --staged` 会失败而 `reset` 不会——这条回落是契约，单文件版
/// `unstage` 的 `reset HEAD --` 是另一个形状，不合并）；空列表静默成功；
/// 主命令超时 10s；mutating 串行化。
pub fn unstage_files(
    repo_root: &Path,
    file_paths: &[String],
    locks: &RepoLocks,
) -> Result<(), String> {
    let unique = unique_git_file_paths(file_paths)?;
    if unique.is_empty() {
        return Ok(());
    }
    let head = transport::run_git(
        repo_root,
        &["rev-parse", "--verify", "HEAD"],
        transport::READ_TIMEOUT,
        None,
    )?;
    let mut args: Vec<&str> = if head.success {
        vec!["restore", "--staged"]
    } else {
        vec!["reset"]
    };
    args.push("--");
    args.extend(unique);
    let lock = locks.lock_for(repo_root.to_str().unwrap_or_default());
    let output = transport::run_git(
        repo_root,
        &args,
        transport::UNSTAGE_FILES_TIMEOUT,
        Some(&lock),
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to unstage files",
        ));
    }
    Ok(())
}

/// 提交内单文件（legacy `GitCommitFile` 形状）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitCommitFile {
    pub path: String,
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
}

/// 提交详情（legacy `GitCommitDetail` 形状）。**没有 camelCase rename**——legacy
/// wire 就是 snake_case（`short_hash`/`total_additions`），这里"无 rename"才是
/// 承重件（切片 79 的 camelCase 规则只适用于 legacy wire 为 camelCase 的 DTO）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GitCommitDetail {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files: Vec<GitCommitFile>,
    pub total_additions: i32,
    pub total_deletions: i32,
}

/// 解析三段 git 输出（legacy `parse_git_commit_detail` 逐字）：info 的前缀行、
/// name-status 的 `[st, path]`/`[st, old, new]` 两形（`R*` 一律归 `"R"`）、
/// numstat 的 `adds\t deletes\t path`（认不出按 0）。
pub fn parse_commit_detail(
    info_stdout: &str,
    name_status_stdout: &str,
    numstat_stdout: &str,
) -> GitCommitDetail {
    let mut hash = String::new();
    let mut short_hash = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut message = String::new();
    for line in info_stdout.lines() {
        if let Some(v) = line.strip_prefix("HASH:") {
            hash = v.to_string();
        } else if let Some(v) = line.strip_prefix("SHORT:") {
            short_hash = v.to_string();
        } else if let Some(v) = line.strip_prefix("AUTHOR:") {
            author = v.to_string();
        } else if let Some(v) = line.strip_prefix("DATE:") {
            date = v.to_string();
        } else if let Some(v) = line.strip_prefix("SUBJECT:") {
            message = v.to_string();
        }
    }

    let mut file_statuses: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for line in name_status_stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        match parts.as_slice() {
            [st, path] => {
                file_statuses.insert(
                    path.to_string(),
                    if st.starts_with('R') {
                        "R".to_string()
                    } else {
                        st.to_string()
                    },
                );
            }
            [st, _old, new_path] => {
                file_statuses.insert(
                    new_path.to_string(),
                    if st.starts_with('R') {
                        "R".to_string()
                    } else {
                        st.to_string()
                    },
                );
            }
            _ => {}
        }
    }

    let mut files = Vec::new();
    let mut total_additions = 0i32;
    let mut total_deletions = 0i32;

    for line in numstat_stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() == 3 {
            let additions: i32 = parts[0].parse().unwrap_or(0);
            let deletions: i32 = parts[1].parse().unwrap_or(0);
            let path = parts[2].to_string();
            total_additions += additions;
            total_deletions += deletions;
            let status = file_statuses
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "M".to_string());
            files.push(GitCommitFile {
                path,
                status,
                additions,
                deletions,
            });
        }
    }

    GitCommitDetail {
        hash,
        short_hash,
        author,
        date,
        message,
        files,
        total_additions,
        total_deletions,
    }
}

/// 提交详情（legacy `git_commit_detail` verbatim；名字带 `_result` 后缀——core
/// 已有一个返回 `GitCommit` 的旧 `commit_detail`，属另一查询，按切片 79 先例
/// 旧入口保留不做别名改写）：三次独立 git 调用（info/name-status/numstat），
/// **三次都不检查退出码**——坏 hash 解析成全空详情是 legacy 行为不是错误路径；
/// revision 只过形状校验（core `validate_revision`，报文与 legacy
/// `validate_git_revision` 不同——切片 66 起既定采用 core 版）。
/// legacy 的同步 run_git 无超时；core 取 `READ_TIMEOUT` 并入 doc。
pub fn commit_detail_result(
    repo_root: &Path,
    commit_hash: &str,
) -> Result<GitCommitDetail, String> {
    transport::validate_revision(commit_hash)?;
    let info_out = transport::run_git(
        repo_root,
        &[
            "show",
            "--no-patch",
            "--format=HASH:%H%nSHORT:%h%nAUTHOR:%an%nDATE:%ar%nSUBJECT:%s",
            "--end-of-options",
            commit_hash,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    let ns_out = transport::run_git(
        repo_root,
        &[
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--name-status",
            "--end-of-options",
            commit_hash,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    let num_out = transport::run_git(
        repo_root,
        &[
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--numstat",
            "--end-of-options",
            commit_hash,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    Ok(parse_commit_detail(
        &info_out.stdout,
        &ns_out.stdout,
        &num_out.stdout,
    ))
}

// ===== generate_commit_message（切片 89；agent 孵化面）=====

/// 生成提交信息（legacy `generate_commit_message` verbatim，两处**刻意差异**
/// 见 doc）：① staged diff 为空 ⇒ 原文拒绝（"No staged changes to generate a
/// commit message for."）；② 50_000 **字节**截断＋`...(diff truncated)` 后缀
/// （legacy 直接 `&diff[..50_000]` 切 String——多字节字符跨界会 panic，是
/// latent bug 不是契约，按 file_diff 的字节语义实现）；③ 项目配置的
/// `git.commit_prompt` 与 `git.commit_message_timeout_secs.clamp(1, 120)`；
/// ④ 临时策略逐字：**一律用 codex**（claude `-p` 计费变动期间），未安装原文
/// 拒绝、不回落项目默认 agent；⑤ prompt 模板逐字；⑥ agent 以 launch spec
/// 孵化（codex_like ⇒ `exec <prompt>`，否则 `-p <prompt> --output-format text`）
/// ，失败 `Agent failed: <stderr+stdout>`、空结果 "Agent returned empty
/// response."。
///
/// **刻意差异（两处，都写死在注释里）**：超时从"放弃等待但留下进程继续跑"
/// 改为 **deadline 到点 kill**——遗留实现把 spawn_blocking 挂在 tokio timeout
/// 下，超时后 JoinHandle 被丢弃、codex 进程继续消耗付费 API；kill 是泄漏
/// 修复不是行为变更（agent 结果本来就没人读了）。core 是同步体，deadline
/// 用 try_wait 轮询实现（同 transport::collect_output 形状）。
pub fn generate_commit_message(project_path: &Path) -> Result<String, String> {
    let diff_output = transport::run_git(
        project_path,
        &["diff", "--staged"],
        transport::READ_TIMEOUT,
        None,
    )?;
    let diff = diff_output.stdout;
    if diff.trim().is_empty() {
        return Err("No staged changes to generate a commit message for.".to_string());
    }

    let diff = if diff.len() > 50_000 {
        let bytes = diff.as_bytes();
        format!(
            "{}...(diff truncated)",
            String::from_utf8_lossy(&bytes[..50_000])
        )
    } else {
        diff
    };

    let config = crate::settings::project_config::read_project_config(
        project_path.to_str().unwrap_or_default(),
    )?;
    let commit_prompt = config.git.commit_prompt;
    let timeout_secs = config.git.commit_message_timeout_secs.clamp(1, 120);

    let agent = if crate::settings::detect_codex_version().is_some() {
        "codex"
    } else {
        return Err("codex 未安装，无法生成提交信息。请安装 codex 后重试。".to_string());
    };

    let full_prompt = format!(
        "{}\n\nGit diff:\n```diff\n{}\n```\n\nOutput only the commit message, nothing else.",
        commit_prompt, diff
    );

    let launch = crate::settings::get_agent_launch_spec(agent);
    let mut cmd = std::process::Command::new(&launch.program);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args);
    if launch.codex_like {
        cmd.args(["exec", &full_prompt]);
    } else {
        cmd.args(["-p", &full_prompt, "--output-format", "text"]);
    }
    cmd.current_dir(project_path);
    cmd.stdin(std::process::Stdio::null());
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }

    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Failed to run {agent}: {error}"))?;
    // `commit_message_timeout_secs` is already u64 (settings::project_config), so no
    // cast: clippy's `unnecessary_cast` fires on the same-type cast, and it is a lint
    // the local toolchain only raises in some installs — CI caught it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let output = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        let _ = s.read_to_string(&mut buf);
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        let _ = s.read_to_string(&mut buf);
                        buf
                    })
                    .unwrap_or_default();
                break (status, stdout, stderr);
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("生成提交信息超时（{}秒）", timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => return Err(format!("生成提交信息线程错误: {error}")),
        }
    };
    let (status, stdout, stderr) = output;
    if !status.success() {
        return Err(format!("Agent failed: {stderr}{stdout}"));
    }

    let result = stdout.trim().to_string();
    if result.is_empty() {
        return Err("Agent returned empty response.".to_string());
    }
    Ok(result)
}

// ===== clone（切片 88）=====

/// 仓库 URL 校验（legacy `validate_git_url` verbatim）：非空、不以 `-` 开头、
/// 无控制字符（NUL/换行/回车）、拒 `scheme::` 传输助手形态（先看 `://`——
/// `https://a::b` 的 `::` 在路径部分是合法的）；本地路径放行。
pub fn validate_git_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Repository URL must not be empty".to_string());
    }
    if trimmed.starts_with('-') {
        return Err("Repository URL must not start with '-'".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("Repository URL contains control characters".to_string());
    }

    let scheme_end = trimmed.find("://");
    let helper_sep = trimmed.find("::");
    if let Some(helper) = helper_sep {
        if scheme_end.is_none_or(|scheme| helper < scheme) {
            return Err(format!(
                "Unsupported repository transport: {}",
                &trimmed[..helper]
            ));
        }
    }
    Ok(())
}

/// clone 目标校验（legacy `validate_clone_target` verbatim）：目标按定义还不
/// 存在，`validate_project_path` 用不了——绝对路径、父目录必须在（不替用户
/// 凭空造一层）、目标要么不存在要么是**空目录**（空目录放行：用户可能先用
/// 文件管理器建好了文件夹，git 自己也允许）；非空一律拒。
pub fn validate_clone_target(target: &str) -> Result<(), String> {
    let path = Path::new(target);
    if !path.is_absolute() {
        return Err("Target path must be absolute".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Target path has no parent directory".to_string())?;
    if !parent.is_dir() {
        return Err(format!(
            "Parent directory does not exist: {}",
            parent.display()
        ));
    }
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err("Target directory is not empty".to_string());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot use target directory: {error}")),
    }
}

/// clone（legacy `git_clone` verbatim）：先过 URL/目标/branch 三个校验，再在
/// **父目录**跑 `clone [--branch <b>] -- <url> <target>`（网络环境）；失败
/// `Err(combined.trim())`、成功 `Ok(combined.trim())`。clone 没有 projectPath
/// 也没有 target 轴（frozen takesTarget 为空），不走 `local_target_error`。
pub fn clone(url: &str, target_path: &str, branch: Option<&str>) -> Result<String, String> {
    validate_git_url(url)?;
    validate_clone_target(target_path)?;
    if let Some(branch) = branch.filter(|value| !value.is_empty()) {
        transport::validate_revision(branch)?;
    }
    let target = Path::new(target_path);
    let parent = target
        .parent()
        .ok_or_else(|| "Target path has no parent directory".to_string())?
        .to_string_lossy()
        .to_string();

    let mut args: Vec<&str> = vec!["clone"];
    if let Some(branch) = branch.filter(|value| !value.is_empty()) {
        args.push("--branch");
        args.push(branch);
    }
    args.push("--");
    args.push(url.trim());
    args.push(target_path);

    let output = transport::run_git_network(Path::new(&parent), &args, transport::NETWORK_TIMEOUT)?;
    let combined = format!("{}{}", output.stdout, output.stderr);
    if !output.success {
        return Err(combined.trim().to_string());
    }
    Ok(combined.trim().to_string())
}

// ===== 网络三件套（切片 87）=====

/// fetch（legacy `git_fetch` verbatim；名带 `_result` 后缀——core 已有 P6-04 的
/// 旧 `fetch`（无 `--all`、prune 为 bool、git_command_error 回落、core 测试在
/// 用），旧入口保留）：`fetch --all` ＋可选 `--prune`（缺省 false＝不带旗标）；
/// combined ＝ **stdout+stderr**（legacy 顺序）；失败 `Err(combined.trim())`、
/// 成功 `Ok(combined.trim())`（都 trim）。走网络环境（防挂死）。
pub fn fetch_result(repo_root: &Path, prune: Option<bool>) -> Result<String, String> {
    let mut args: Vec<&str> = vec!["fetch", "--all"];
    if prune.unwrap_or(false) {
        args.push("--prune");
    }
    let output = transport::run_git_network(repo_root, &args, transport::NETWORK_TIMEOUT)?;
    let combined = format!("{}{}", output.stdout, output.stderr);
    if !output.success {
        return Err(combined.trim().to_string());
    }
    Ok(combined.trim().to_string())
}

/// pull（legacy `git_pull` verbatim；名带 `_result` 后缀，同上）：单 `pull`；
/// 失败 `Err(combined)` **不 trim**（legacy 三条各自的 trim 不对称——fetch 失败
/// trim、pull/push 失败不 trim——逐字保留，别"顺手统一"）。
pub fn pull_result(repo_root: &Path) -> Result<String, String> {
    let output = transport::run_git_network(repo_root, &["pull"], transport::NETWORK_TIMEOUT)?;
    let combined = format!("{}{}", output.stdout, output.stderr);
    if !output.success {
        return Err(combined);
    }
    Ok(combined.trim().to_string())
}

/// push（legacy `git_push` verbatim；名带 `_result` 后缀，同上）：branch 非空 ⇒
/// `push origin <branch>`，否则裸 `push`（靠上游配置）；失败 `Err(combined)`
/// 不 trim。
pub fn push_result(repo_root: &Path, branch: Option<&str>) -> Result<String, String> {
    let mut args: Vec<&str> = vec!["push"];
    if let Some(branch) = branch.filter(|value| !value.is_empty()) {
        args.push("origin");
        args.push(branch);
    }
    let output = transport::run_git_network(repo_root, &args, transport::NETWORK_TIMEOUT)?;
    let combined = format!("{}{}", output.stdout, output.stderr);
    if !output.success {
        return Err(combined);
    }
    Ok(combined.trim().to_string())
}

// ===== 冲突预览/解决家族（切片 86）=====

/// 冲突解决选项（legacy `GitConflictResolution` verbatim：wire 小写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitConflictResolution {
    Ours,
    Theirs,
    Both,
}

/// 冲突块（legacy `GitConflictHunk` 形状；wire 为 camelCase——`base` 在无
/// diff3 段时为 null）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitConflictHunk {
    pub index: u32,
    pub ours: String,
    pub base: Option<String>,
    pub theirs: String,
}

/// 冲突预览（legacy `GitConflictPreview` 形状；wire 为 camelCase——`filePath`
/// 承重）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitConflictPreview {
    pub file_path: String,
    pub hunks: Vec<GitConflictHunk>,
}

/// 解析冲突标记（legacy `parse_conflict_hunks` 逐字）：`<<<<<<< `/`||||||| `/`
/// =======`/`>>>>>>> ` 四态机；split_inclusive 保留行尾；未闭合/无块各报各错；
/// index 从 1 起；无 diff3 段 ⇒ base 为 None。
pub fn parse_conflict_hunks(content: &str) -> Result<Vec<GitConflictHunk>, String> {
    enum State {
        Normal,
        Ours,
        Base,
        Theirs,
    }

    let mut state = State::Normal;
    let mut ours = String::new();
    let mut base = String::new();
    let mut theirs = String::new();
    let mut hunks = Vec::new();

    for line in content.split_inclusive('\n') {
        let marker = line.trim_end_matches(['\r', '\n']);
        match state {
            State::Normal if marker.starts_with("<<<<<<< ") => {
                state = State::Ours;
                ours.clear();
                base.clear();
                theirs.clear();
            }
            State::Ours if marker.starts_with("||||||| ") => {
                state = State::Base;
            }
            State::Ours if marker.starts_with("=======") => {
                state = State::Theirs;
            }
            State::Ours => ours.push_str(line),
            State::Base if marker.starts_with("=======") => {
                state = State::Theirs;
            }
            State::Base => base.push_str(line),
            State::Theirs if marker.starts_with(">>>>>>> ") => {
                hunks.push(GitConflictHunk {
                    index: hunks.len() as u32 + 1,
                    ours: ours.clone(),
                    base: (!base.is_empty()).then(|| base.clone()),
                    theirs: theirs.clone(),
                });
                state = State::Normal;
            }
            State::Normal => {}
            State::Theirs => theirs.push_str(line),
        }
    }

    if !matches!(state, State::Normal) {
        return Err("Unclosed conflict marker block".to_string());
    }
    if hunks.is_empty() {
        return Err("No conflict markers found".to_string());
    }
    Ok(hunks)
}

/// keep-both 合并（legacy `resolve_conflict_markers_keep_both` 逐字）：先过
/// `parse_conflict_hunks` 的校验（未闭合/无块先报错），再按 ours＋theirs 顺序
/// 拼接——两侧都非空且 ours 不以换行结尾时补一个换行。
pub fn resolve_conflict_markers_keep_both(content: &str) -> Result<String, String> {
    enum State {
        Normal,
        Ours,
        Base,
        Theirs,
    }

    parse_conflict_hunks(content)?;

    let mut state = State::Normal;
    let mut output = String::new();
    let mut ours = String::new();
    let mut theirs = String::new();

    for line in content.split_inclusive('\n') {
        let marker = line.trim_end_matches(['\r', '\n']);
        match state {
            State::Normal if marker.starts_with("<<<<<<< ") => {
                state = State::Ours;
                ours.clear();
                theirs.clear();
            }
            State::Normal => output.push_str(line),
            State::Ours if marker.starts_with("||||||| ") => {
                state = State::Base;
            }
            State::Ours if marker.starts_with("=======") => {
                state = State::Theirs;
            }
            State::Ours => ours.push_str(line),
            State::Base if marker.starts_with("=======") => {
                state = State::Theirs;
            }
            State::Base => {}
            State::Theirs if marker.starts_with(">>>>>>> ") => {
                output.push_str(&ours);
                if !ours.is_empty() && !theirs.is_empty() && !ours.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&theirs);
                state = State::Normal;
            }
            State::Theirs => theirs.push_str(line),
        }
    }
    Ok(output)
}

/// 项目相对路径解析（legacy `resolve_project_relative_file_path` verbatim）：
/// 相对校验＋canonical 双方＋parent 包含判定（symlink 落点在项目内才算合法）。
fn resolve_project_relative_file_path(
    project_path: &Path,
    file_path: &str,
) -> Result<std::path::PathBuf, String> {
    validate_git_relative_path(file_path)?;
    let project = project_path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project path: {error}"))?;
    let target = project.join(file_path);
    let parent = target
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?
        .canonicalize()
        .map_err(|error| format!("Cannot resolve parent directory: {error}"))?;
    if !parent.starts_with(&project) {
        return Err("File path is outside project root".to_string());
    }
    let file_name = target
        .file_name()
        .ok_or_else(|| "Cannot resolve file name".to_string())?;
    Ok(parent.join(file_name))
}

/// 冲突预览（legacy `git_conflict_preview` verbatim；名带 `_result` 后缀——core
/// 已有 P6-04 的旧 `conflict_preview`（git diff 形状、core 测试在用），按切片
/// 77/79 先例旧入口保留）。读文件＋解析标记；读失败与解析失败各自回落原文。
pub fn conflict_preview_result(
    project_path: &Path,
    file_path: &str,
) -> Result<GitConflictPreview, String> {
    let target = resolve_project_relative_file_path(project_path, file_path)?;
    let content = std::fs::read_to_string(&target)
        .map_err(|error| format!("Failed to read conflict file: {error}"))?;
    Ok(GitConflictPreview {
        file_path: file_path.to_string(),
        hunks: parse_conflict_hunks(&content)?,
    })
}

/// 冲突解决（legacy `git_resolve_conflict` verbatim；名带后缀——core 已有
/// P6-04 的旧 `resolve_conflict`（写入选定内容形状、core 测试在用），旧入口
/// 保留）。ours/theirs 走 `checkout --ours|--theirs -- <file>`，both 走
/// keep-both 原地写回；**三条都以 `add -- <file>` 收尾**（legacy 如此——
/// resolve 即 stage）。mutating 串行化；失败回落仅 stderr trim（run_git_check
/// 形状）。
pub fn resolve_conflict_with_resolution(
    project_path: &Path,
    file_path: &str,
    resolution: GitConflictResolution,
    locks: &RepoLocks,
) -> Result<(), String> {
    let lock = locks.lock_for(project_path.to_str().unwrap_or_default());
    match resolution {
        GitConflictResolution::Ours => {
            run_git_check(
                project_path,
                &["checkout", "--ours", "--", file_path],
                transport::READ_TIMEOUT,
                Some(&lock),
            )?;
        }
        GitConflictResolution::Theirs => {
            run_git_check(
                project_path,
                &["checkout", "--theirs", "--", file_path],
                transport::READ_TIMEOUT,
                Some(&lock),
            )?;
        }
        GitConflictResolution::Both => {
            let target = resolve_project_relative_file_path(project_path, file_path)?;
            let content = std::fs::read_to_string(&target)
                .map_err(|error| format!("Failed to read conflict file: {error}"))?;
            let resolved = resolve_conflict_markers_keep_both(&content)?;
            std::fs::write(&target, resolved)
                .map_err(|error| format!("Failed to write resolved conflict file: {error}"))?;
        }
    }
    run_git_check(
        project_path,
        &["add", "--", file_path],
        transport::READ_TIMEOUT,
        Some(&lock),
    )
}

// ===== discard 家族（切片 85）=====

/// 解析 worktree root（legacy `git_worktree_root` verbatim）：`rev-parse
/// --show-toplevel` 失败原样上抛 git stderr；空 root / canonicalize 失败 /
/// 越出 root 各有专属文案。切片 80 的 `branch_graph_result` 内联了同一段逻辑，
/// 本助手自本片起共享（branch_graph 的内联副本留待后续统一，不在本片回改）。
pub fn worktree_root_of(project_path: &Path) -> Result<std::path::PathBuf, String> {
    let output = transport::run_git(
        project_path,
        &["rev-parse", "--show-toplevel"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(output.stderr.trim().to_string());
    }
    let root = output.stdout.trim().to_string();
    if root.is_empty() {
        return Err("Cannot resolve git worktree root".to_string());
    }
    let root = std::path::PathBuf::from(&root)
        .canonicalize()
        .map_err(|error| format!("Cannot resolve git worktree root: {error}"))?;
    let project = project_path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project path: {error}"))?;
    if !project.starts_with(&root) {
        return Err("Git worktree root does not contain project path".to_string());
    }
    Ok(root)
}

/// 保护路径判定（legacy 经 git_path_guard `is_protected_git_relative_path`
/// verbatim：首段 `.git`/`.aeroric`，大小写不敏感，`Path::components` 取段）。
pub fn is_protected_project_relative_path(file_path: &str) -> bool {
    const PROTECTED_FIRST_SEGMENTS: &[&str] = &[".git", ".aeroric"];
    Path::new(file_path)
        .components()
        .find_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str().map(|name| {
                PROTECTED_FIRST_SEGMENTS
                    .iter()
                    .any(|protected| name.eq_ignore_ascii_case(protected))
            }),
            _ => None,
        })
        .unwrap_or(false)
}

/// 列出 untracked（legacy `list_untracked_files` verbatim）：`-c
/// core.quotePath=false ls-files --others --exclude-standard -z`，NUL 分隔；
/// 失败回落**仅 stderr trim**（同 run_git_check 形状，非 combined）。
pub fn list_untracked_files(repo_root: &Path) -> Result<Vec<String>, String> {
    let output = transport::run_git(
        repo_root,
        &[
            "-c",
            "core.quotePath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(output.stderr.trim().to_string());
    }
    Ok(output
        .stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect())
}

/// 目录下的 untracked 条目（legacy `untracked_files_under_directory` verbatim，
/// 纯函数：路径本身不算"其目录之下"）。
pub fn untracked_files_under_directory<'a>(
    directory_path: &str,
    untracked_files: &'a [String],
) -> Vec<&'a str> {
    let directory = Path::new(directory_path);
    untracked_files
        .iter()
        .map(String::as_str)
        .filter(|path| {
            let path = Path::new(path);
            path != directory && path.starts_with(directory)
        })
        .collect()
}

/// 是否在 untracked 列表（legacy `is_listed_untracked_file` verbatim，
/// 按 Path 语义比较而非字符串相等）。
pub fn is_listed_untracked_file(relative_path: &str, untracked_files: &[String]) -> bool {
    let relative_path = Path::new(relative_path);
    untracked_files
        .iter()
        .any(|path| Path::new(path) == relative_path)
}

/// worktree 相对路径是否受保护（legacy `is_protected_worktree_relative_path`
/// verbatim：项目内相对路径过保护判定，越出项目的路径不受本判定管——
/// trash 步另有 root 包含守卫）。
pub fn is_protected_worktree_relative_path(
    worktree_root: &Path,
    project_path: &Path,
    relative_path: &str,
) -> bool {
    if is_protected_project_relative_path(relative_path) {
        return true;
    }

    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return false;
    }

    let canonical_project = match project_path.canonicalize() {
        Ok(path) => path,
        Err(_) => return false,
    };
    let target = worktree_root.join(rel);
    let Some(file_name) = target.file_name() else {
        return false;
    };
    let Some(parent) = target.parent() else {
        return false;
    };
    let Ok(canonical_parent) = parent.canonicalize() else {
        return false;
    };
    let resolved = canonical_parent.join(file_name);

    resolved
        .strip_prefix(&canonical_project)
        .ok()
        .map(|rel_from_project| {
            is_protected_project_relative_path(&rel_from_project.to_string_lossy())
        })
        .unwrap_or(false)
}

/// 回收站删除（legacy `trash_worktree_relative_path` verbatim）：canonical
/// 双方＋root 包含判定＋拒根＋拒保护路径（相对 root 与相对项目各查一次）＋
/// `trash::delete`——**进废纸篓，不默认永久删除**（P6-01 同款）。
fn trash_worktree_relative_path(
    worktree_root: &Path,
    project_path: &Path,
    relative_path: &str,
) -> Result<(), String> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err("Untracked path must be relative".to_string());
    }

    let target = worktree_root.join(rel);
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?
        .to_string();
    let parent = target
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;

    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("Cannot resolve parent directory: {error}"))?;
    let canonical_root = worktree_root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve git worktree root: {error}"))?;
    let canonical_project = project_path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project path: {error}"))?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err("Path is outside the git worktree".to_string());
    }

    let resolved = canonical_parent.join(&file_name);
    if resolved == canonical_root {
        return Err("Refusing to delete project root".to_string());
    }
    if resolved.symlink_metadata().is_err() {
        return Err("Path does not exist".to_string());
    }
    if is_protected_project_relative_path(relative_path) {
        return Err("Refusing to delete protected project metadata".to_string());
    }
    if let Ok(rel_from_project) = resolved.strip_prefix(&canonical_project) {
        let rel_from_project = rel_from_project.to_string_lossy();
        if is_protected_project_relative_path(&rel_from_project) {
            return Err("Refusing to delete protected project metadata".to_string());
        }
    }

    trash::delete(&resolved).map_err(|error| error.to_string())
}

/// 删除一个 untracked 路径（legacy `discard_untracked_path` verbatim）：绝对
/// 路径拒、保护路径拒、symlink_metadata 探存在；目录 ⇒ 逐个 trash 其 untracked
/// 条目（受保护的跳过）；文件 ⇒ 必须真的在 untracked 列表里才 trash。
fn discard_untracked_path(
    project_path: &Path,
    worktree_root: &Path,
    relative_path: &str,
    untracked_files: &[String],
) -> Result<(), String> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err("Untracked path must be relative".to_string());
    }
    if is_protected_worktree_relative_path(worktree_root, project_path, relative_path) {
        return Err("Refusing to delete protected project metadata".to_string());
    }

    let target = worktree_root.join(rel);
    let metadata = target
        .symlink_metadata()
        .map_err(|_| "Path does not exist".to_string())?;

    if metadata.file_type().is_dir() {
        for rel in untracked_files_under_directory(relative_path, untracked_files) {
            if is_protected_worktree_relative_path(worktree_root, project_path, rel) {
                continue;
            }
            trash_worktree_relative_path(worktree_root, project_path, rel)?;
        }
        return Ok(());
    }

    if !is_listed_untracked_file(relative_path, untracked_files) {
        return Err("Path is not an untracked file".to_string());
    }

    trash_worktree_relative_path(worktree_root, project_path, relative_path)
}

/// run_git_check 的 core 形状（legacy `run_git_check` verbatim：失败回落
/// **仅 stderr trim**，非 combined）。
fn run_git_check(
    repo_root: &Path,
    args: &[&str],
    timeout: std::time::Duration,
    lock: Option<&Arc<std::sync::Mutex<()>>>,
) -> Result<(), String> {
    let output = transport::run_git(repo_root, args, timeout, lock)?;
    if !output.success {
        return Err(output.stderr.trim().to_string());
    }
    Ok(())
}

/// 丢单文件改动（legacy `git_discard_file` verbatim）：tracked 走
/// `restore -- <file>`（在 worktree root 上执行）；untracked 走回收站路径
/// （`discard_untracked_file` ＝ 现列 untracked 再 `discard_untracked_path`）。
/// mutating 串行化；10s（legacy 的 run_git_check 无超时，core 取
/// `STAGE_FILES_TIMEOUT` 同款 10s 级别——这里用独立常量）。
pub fn discard_file(
    project_path: &Path,
    file_path: &str,
    untracked: bool,
    locks: &RepoLocks,
) -> Result<(), String> {
    let worktree_root = worktree_root_of(project_path)?;
    if untracked {
        let worktree_root_string = worktree_root.to_string_lossy().into_owned();
        let untracked_files = list_untracked_files(Path::new(&worktree_root_string))?;
        discard_untracked_path(project_path, &worktree_root, file_path, &untracked_files)
    } else {
        let lock = locks.lock_for(project_path.to_str().unwrap_or_default());
        run_git_check(
            &worktree_root,
            &["restore", "--", file_path],
            transport::DISCARD_TIMEOUT,
            Some(&lock),
        )
    }
}

/// 丢一批改动（legacy `git_discard_files` verbatim；名带 `_at` 后缀——core 已有
/// P6-04 的旧 `discard_files`（`checkout --` 形状、core 自身测试在用），按切片
/// 77/79 先例旧入口保留不做合并）：先校验＋去重（`validate_git_relative_path`
/// 逐个＋首次出现去重），**空列表静默成功**；untracked 批量逐个
/// `discard_untracked_path`（共享一次 untracked 枚举）；tracked 批量一条
/// `restore -- <paths...>`；mutating 串行化。
pub fn discard_files_at(
    project_path: &Path,
    file_paths: &[String],
    untracked: bool,
    locks: &RepoLocks,
) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<&str> = Vec::new();
    for file_path in file_paths {
        validate_git_relative_path(file_path)?;
        if seen.insert(file_path.as_str()) {
            unique.push(file_path.as_str());
        }
    }
    if unique.is_empty() {
        return Ok(());
    }

    let worktree_root = worktree_root_of(project_path)?;
    if untracked {
        let worktree_root_string = worktree_root.to_string_lossy().into_owned();
        let untracked_files = list_untracked_files(Path::new(&worktree_root_string))?;
        for file_path in unique {
            discard_untracked_path(project_path, &worktree_root, file_path, &untracked_files)?;
        }
        return Ok(());
    }

    let mut args: Vec<&str> = vec!["restore", "--"];
    args.extend(unique);
    let lock = locks.lock_for(project_path.to_str().unwrap_or_default());
    run_git_check(
        &worktree_root,
        &args,
        transport::DISCARD_TIMEOUT,
        Some(&lock),
    )
}

/// 丢全部改动（legacy `git_discard_all` verbatim）：有 HEAD 走
/// `restore --source=HEAD --staged --worktree .`（staged-only 的新文件在这一步
/// 变回 untracked）；无 HEAD 走 `rm -r --cached --ignore-unmatch -- .`；随后
/// 枚举 untracked 逐个回收站（**受保护路径跳过而非报错**——批量语义）。
/// mutating 串行化。
pub fn discard_all(project_path: &Path, locks: &RepoLocks) -> Result<(), String> {
    let worktree_root = worktree_root_of(project_path)?;
    let lock = locks.lock_for(project_path.to_str().unwrap_or_default());
    let head = transport::run_git(
        &worktree_root,
        &["rev-parse", "--verify", "HEAD"],
        transport::READ_TIMEOUT,
        None,
    )?;
    if head.success {
        run_git_check(
            &worktree_root,
            &["restore", "--source=HEAD", "--staged", "--worktree", "."],
            transport::DISCARD_TIMEOUT,
            Some(&lock),
        )?;
    } else {
        run_git_check(
            &worktree_root,
            &["rm", "-r", "--cached", "--ignore-unmatch", "--", "."],
            transport::DISCARD_TIMEOUT,
            Some(&lock),
        )?;
    }

    let worktree_root_string = worktree_root.to_string_lossy().into_owned();
    for rel in list_untracked_files(Path::new(&worktree_root_string))? {
        if is_protected_worktree_relative_path(&worktree_root, project_path, &rel) {
            continue;
        }
        trash_worktree_relative_path(&worktree_root, project_path, &rel)?;
    }
    Ok(())
}

/// 字节截断（legacy 的 `&raw[..limit]`＋`from_utf8_lossy` 形状：按**字节**切，
/// 可能劈开 UTF-8 字符——lossy 用 U+FFFD 补上；core 的 stdout 已是 String，
/// 不能直接 `&s[..limit]`（非边界会 panic），经 as_bytes 走同一形状）。
fn truncate_bytes(raw: &str, limit: usize) -> String {
    let bytes = raw.as_bytes();
    String::from_utf8_lossy(if bytes.len() > limit {
        &bytes[..limit]
    } else {
        bytes
    })
    .into_owned()
}

/// 单文件只读 diff（安全完整版）。
///
/// - 路径先过 `validate_git_relative_path`，再以 `--literal-pathspecs` 和 `--`
///   传递；文件名中的 `*`/`[`/`:` 不会变成 pathspec 模式。
/// - staged 用 `diff --cached`；unstaged 对 tracked 文件用 `diff`，对
///   untracked 文件用 `diff --no-index /dev/null <canonical path>`，不写
///   临时文件，也不再把空结果回落成任意文件读取。
/// - 返回完整 stdout；超过 `MAX_FILE_DIFF_BYTES` 时明确报错，不截断。
pub fn file_diff(repo_root: &Path, file_path: &str, staged: bool) -> Result<String, String> {
    validate_git_relative_path(file_path)?;
    if staged {
        if !tracked_file_is_exact(repo_root, file_path)? {
            return Err("Staged diff target is not a tracked file".to_string());
        }
        let output = transport::run_git(
            repo_root,
            &[
                "--literal-pathspecs",
                "diff",
                "--cached",
                "--no-ext-diff",
                "--no-color",
                "--unified=3",
                "--",
                file_path,
            ],
            transport::FILE_DIFF_TIMEOUT,
            None,
        )?;
        if !output.success {
            return Err(transport::git_command_error(
                &output,
                "Failed to get staged diff",
            ));
        }
        return bounded_file_diff(output.stdout);
    }

    if tracked_file_is_exact(repo_root, file_path)? {
        let output = transport::run_git(
            repo_root,
            &[
                "--literal-pathspecs",
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--unified=3",
                "--",
                file_path,
            ],
            transport::FILE_DIFF_TIMEOUT,
            None,
        )?;
        if !output.success {
            return Err(transport::git_command_error(
                &output,
                "Failed to get working diff",
            ));
        }
        return bounded_file_diff(output.stdout);
    }

    let root = repo_root.canonicalize().map_err(untracked_root_error)?;
    let candidate = root.join(file_path);
    let metadata = std::fs::symlink_metadata(&candidate).map_err(untracked_inspect_error)?;
    if metadata.file_type().is_symlink() {
        return Err("Untracked diff target is not a regular file".to_string());
    }
    if !metadata.is_file() {
        return Err("Untracked diff target is not a regular file".to_string());
    }
    let resolved = candidate.canonicalize().map_err(untracked_resolve_error)?;
    if !resolved.starts_with(&root) {
        return Err("Untracked diff target escapes the project root".to_string());
    }
    let resolved = resolved.to_string_lossy().into_owned();
    let output = transport::run_git(
        repo_root,
        &[
            "diff",
            "--no-index",
            "--no-color",
            "--unified=3",
            "--",
            "/dev/null",
            resolved.as_str(),
        ],
        transport::FILE_DIFF_TIMEOUT,
        None,
    )?;
    // `git diff --no-index` exits 1 when the files differ; that is the normal
    // success path here. Empty stdout with non-success is a real failure.
    if !output.success && output.stdout.is_empty() {
        return Err(transport::git_command_error(
            &output,
            "Failed to get untracked diff",
        ));
    }
    bounded_file_diff(output.stdout)
}

/// `git ls-files -z --error-unmatch -- <path>` returns the exact NUL-terminated
/// path only when the pathspec names one tracked file. A directory pathspec
/// expands to every entry under it (or exits non-zero when empty), so requiring
/// byte equality keeps `file_diff` from turning a directory into a directory-wide
/// diff in either the staged or unstaged branch.
fn tracked_file_is_exact(repo_root: &Path, file_path: &str) -> Result<bool, String> {
    let mut expected = String::from(file_path);
    expected.push('\0');
    let tracked = transport::run_git(
        repo_root,
        &[
            "--literal-pathspecs",
            "ls-files",
            "-z",
            "--error-unmatch",
            "--",
            file_path,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    Ok(tracked.success && tracked.stdout == expected)
}

/// The UI refuses to render a partial diff as complete. Core therefore returns
/// the full output or a named error instead of silently truncating at a byte
/// boundary (the old 200 KiB path could split a UTF-8 scalar).
const MAX_FILE_DIFF_BYTES: usize = 1024 * 1024;

fn bounded_file_diff(raw: String) -> Result<String, String> {
    if raw.len() > MAX_FILE_DIFF_BYTES {
        let mut message = String::from("diff exceeds ");
        message.push_str(&MAX_FILE_DIFF_BYTES.to_string());
        message.push_str(" bytes; refusing to return a truncated preview");
        return Err(message);
    }
    Ok(raw)
}

fn untracked_root_error(error: std::io::Error) -> String {
    let mut message = String::from("Failed to resolve project root for untracked diff: ");
    message.push_str(&error.to_string());
    message
}

fn untracked_inspect_error(error: std::io::Error) -> String {
    let mut message = String::from("Failed to inspect untracked file for diff: ");
    message.push_str(&error.to_string());
    message
}

fn untracked_resolve_error(error: std::io::Error) -> String {
    let mut message = String::from("Failed to resolve untracked file for diff: ");
    message.push_str(&error.to_string());
    message
}
/// 提交里某文件的完整内容（legacy `git_show_file_diff` verbatim）：
/// `show --format= --end-of-options <hash> -- <file>`；失败回落**仅 stderr**
/// （不是 combined()）；截断上限 500 * 1024 **字节**。legacy 的同步 run_git
/// 没有超时；core 的 run_git 必须给，取 `READ_TIMEOUT`（本地单文件 blob 读取，
/// 不是行为差异的载体）。legacy 只校验 revision（`validate_git_revision`，
/// 报文与 core `validate_revision` 不同——切片 66 起既定采用 core 版）。
pub fn show_file_diff(
    repo_root: &Path,
    commit_hash: &str,
    file_path: &str,
) -> Result<String, String> {
    transport::validate_revision(commit_hash)?;
    let output = transport::run_git(
        repo_root,
        &[
            "show",
            "--format=",
            "--end-of-options",
            commit_hash,
            "--",
            file_path,
        ],
        transport::READ_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(output.stderr);
    }
    Ok(truncate_bytes(&output.stdout, 500 * 1024))
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

/// 解析 `stash list --format=%gd%x1f%H%x1f%cr%x1f%s`（legacy `parse_stash_list`
/// 逐字：四段按 \x1f 切；name 空则整行跳过；index 从 `stash@{N}` 提取，认不出
/// 就回落 0）。
pub fn parse_stash_list(stdout: &[u8]) -> Vec<GitStashEntry> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\x1f');
            let name = parts.next()?.trim().to_string();
            let commit = parts.next().unwrap_or_default().trim().to_string();
            let date = parts.next().unwrap_or_default().trim().to_string();
            let message = parts.next().unwrap_or_default().trim().to_string();
            if name.is_empty() {
                return None;
            }
            let index = name
                .strip_prefix("stash@{")
                .and_then(|value| value.strip_suffix('}'))
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            Some(GitStashEntry {
                index,
                name,
                commit,
                date,
                message,
            })
        })
        .collect()
}

/// stash 列表（legacy `git_stash_list` verbatim）。
///
/// 与旧 core `stash_list` 的四条轴差异（旧版无消费方，按 legacy 改齐而非并存）：
/// ① argv 的 format 是**四段** `%gd%x1f%H%x1f%cr%x1f%s`（旧版两段 `%gd%x1f%gs`，
///    丢掉 commit/date）；② 超时 10s（旧版 5s）；③ 回落文案 `Failed to list git
///    stashes`（旧版 `Failed to list stash`）；④ 条目形状五个字段（旧版两个）。
/// 空行过滤的差异也在此对齐：legacy 的解析按"**name** 为空跳过"，不是按整行空。
pub fn stash_list(repo_root: &Path) -> Result<Vec<GitStashEntry>, String> {
    let output = transport::run_git(
        repo_root,
        &["stash", "list", "--format=%gd%x1f%H%x1f%cr%x1f%s"],
        transport::STASH_LIST_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to list git stashes",
        ));
    }
    Ok(parse_stash_list(output.stdout.as_bytes()))
}

/// stash diff 截断上限（legacy `MAX_STASH_DIFF_CHARS`：按**字符**数计，不是字节）。
pub const MAX_STASH_DIFF_CHARS: usize = 200_000;

/// 截断长文本（legacy `truncate_text` 逐字：超出按字符数切，补 `...(truncated)`
/// 后缀并置 true；字符边界用 `char_indices` 对齐，不劈开 UTF-8 字符）。
pub fn truncate_text(value: String, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value, false);
    }
    let cutoff = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    (format!("{}...(truncated)", &value[..cutoff]), true)
}

/// stash diff 结果（legacy `GitStashDiff` 形状；wire 为 camelCase——`stash_ref`
/// 变 `stashRef` 是承重的，缺 rename 客户端解到 null）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitStashDiff {
    pub stash_ref: String,
    pub diff: String,
    pub truncated: bool,
}

/// stash diff（legacy `git_stash_diff` verbatim）：形状校验在跑 git 之前拒绝
/// 松散引用（`HEAD` 不是 stash）；argv 七个开关逐字；diff 按 `MAX_STASH_DIFF_CHARS`
/// 字符截断（截断是正常分支不是错误）。
pub fn stash_diff(repo_root: &Path, stash_ref: &str) -> Result<GitStashDiff, String> {
    validate_stash_ref(stash_ref)?;
    let output = transport::run_git(
        repo_root,
        &[
            "stash",
            "show",
            "--patch",
            "--stat",
            "--include-untracked",
            "--no-ext-diff",
            "--no-color",
            stash_ref,
        ],
        transport::STASH_DIFF_TIMEOUT,
        None,
    )?;
    if !output.success {
        return Err(transport::git_command_error(
            &output,
            "Failed to load git stash diff",
        ));
    }
    let raw = output.stdout;
    let (diff, truncated) = truncate_text(raw, MAX_STASH_DIFF_CHARS);
    Ok(GitStashDiff {
        stash_ref: stash_ref.to_string(),
        diff,
        truncated,
    })
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
