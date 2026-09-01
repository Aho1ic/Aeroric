use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

mod transport;

use transport::{
    git_command_error, run_git, run_git_check, run_git_network, run_git_with_timeout,
    validate_project_path,
};

pub(crate) const MAX_STASH_DIFF_CHARS: usize = 200_000;

fn validate_git_relative_path(relative_path: &str) -> Result<(), String> {
    if relative_path.is_empty() {
        return Err("File path must not be empty".to_string());
    }

    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err("File path must be relative".to_string());
    }

    for component in path.components() {
        match component {
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err("File path must stay inside the git worktree".to_string());
            }
            _ => {}
        }
    }

    Ok(())
}

/// 允许 `git clone` / `git remote add` 接收的 URL 形态。
///
/// ## 为什么必须校验:以 `-` 开头的 URL 是一条实测成立的 RCE
///
/// `git clone <url> <dir>` 里的 `<url>` 只是一个位置参数,git 照常把以 `-` 开头的东西
/// 当选项解析。本机 git 2.50 上实测:
///
/// ```text
/// git clone "--upload-pack=touch /tmp/marker;git-upload-pack" src.git out
/// → /tmp/marker 被创建
/// ```
///
/// 所以这不是理论风险。`--` 挡不住它:`--upload-pack` 出现在 `--` **之前**才生效,而我们
/// 拦的正是那个位置上的值。唯一可靠的做法是先看第一个字符。
///
/// ## 传输白名单
///
/// `ext::` 这类传输助手会**执行任意命令**(`ext::sh -c ...`)。git 2.50 默认已经拦住
/// (`protocol.ext.allow` 默认 `never`,实测报「传输 'ext' 不允许」),但那是 git 的默认值,
/// 不是我们的保证 —— 用户的 `~/.gitconfig` 里一句 `protocol.ext.allow=always` 就能把它打开,
/// 旧版 git 也未必是这个默认。白名单是我们自己这一侧的、不依赖外部配置的保证。
fn validate_git_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Repository URL must not be empty".to_string());
    }
    // 选项注入。见上面的实测。
    if trimmed.starts_with('-') {
        return Err("Repository URL must not start with '-'".to_string());
    }
    // NUL 会截断传给子进程的 C 字符串;换行会让 URL 在写进 config 时多出一行。
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("Repository URL contains control characters".to_string());
    }

    // `scheme::` 形式的传输助手一律拒绝(`ext::`、`transport-helper::`……)。注意判断顺序:
    // 先看 `://`,因为 `https://a::b` 里的 `::` 出现在路径部分,那是合法的。
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

    // 剩下的形态:`scheme://…`、`user@host:path`(scp 式 ssh)、以及本地路径。
    // 本地路径放行 —— 用户拿一个本地裸库当远端是正当用法,而 clone 到本地也不比
    // 「用文件管理器复制一个目录」更危险。
    Ok(())
}

/// clone 的目标目录。`validate_project_path` 在这里用不了 —— 它要求路径**已存在**,而
/// clone 的目标按定义还不存在。
///
/// 判据:父目录得在(不替用户凭空造一层),目标本身要么不存在,要么是个空目录。
/// 允许空目录是因为用户很可能先用文件管理器建好了那个文件夹 —— git 自己也允许。
/// 非空目录一律拒:clone 到一个有东西的目录上,轻则失败,重则把用户的文件和一个陌生
/// 仓库混在一起,而那种状态很难看出来、更难还原。
fn validate_clone_target(target: &str) -> Result<(), String> {
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
        // 存在且能列 —— 必须是空的。
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err("Target directory is not empty".to_string());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // 存在但不是目录(普通文件),或者没权限列。两种都不能往下走。
        Err(error) => Err(format!("Cannot use target directory: {error}")),
    }
}

fn unique_git_file_paths(file_paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();

    for file_path in file_paths {
        validate_git_relative_path(&file_path)?;
        if seen.insert(file_path.clone()) {
            paths.push(file_path);
        }
    }

    Ok(paths)
}

fn git_path_args(base_args: &[&str], file_paths: Vec<String>) -> Result<Vec<String>, String> {
    let paths = unique_git_file_paths(file_paths)?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut args: Vec<String> = base_args.iter().map(|arg| (*arg).to_string()).collect();
    args.push("--".to_string());
    args.extend(paths);
    Ok(args)
}

fn git_worktree_root(project_path: &str) -> Result<PathBuf, String> {
    let output = run_git(project_path, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return Err("Cannot resolve git worktree root".to_string());
    }

    let root = PathBuf::from(root)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve git worktree root: {}", e))?;
    let project = Path::new(project_path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {}", e))?;

    if !project.starts_with(&root) {
        return Err("Git worktree root does not contain project path".to_string());
    }

    Ok(root)
}

fn path_to_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|path| path.to_string())
        .ok_or_else(|| "Path contains invalid UTF-8".to_string())
}

fn git_has_head(worktree_root: &str) -> Result<bool, String> {
    let output = run_git(worktree_root, &["rev-parse", "--verify", "HEAD"])?;
    Ok(output.status.success())
}

const PROTECTED_FIRST_SEGMENTS: &[&str] = &[".git", ".aeroric"];

fn is_protected_project_relative_path(relative_path: &str) -> bool {
    Path::new(relative_path)
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

fn apply_login_shell_env(cmd: &mut Command) {
    for (key, value) in crate::app_settings::get_login_shell_env() {
        cmd.env(key, value);
    }
}

fn run_agent_commit_message_command(
    agent: &str,
    project_path: &str,
    prompt: &str,
) -> Result<Output, String> {
    let launch = crate::app_settings::get_agent_launch_spec(agent);
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args);
    if launch.codex_like {
        cmd.args(["exec", prompt]);
    } else {
        cmd.args(["-p", prompt, "--output-format", "text"]);
    }
    cmd.current_dir(project_path);
    cmd.stdin(Stdio::null());
    apply_login_shell_env(&mut cmd);
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }
    cmd.output()
        .map_err(|e| format!("Failed to run {agent}: {e}"))
}

fn create_empty_temp_file() -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("aeroric-empty-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::File::create(&path)
        .map_err(|e| format!("Failed to create temporary file for git diff: {e}"))?;
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitBlameLine {
    pub(crate) line: u32,
    pub(crate) commit: String,
    pub(crate) short_commit: String,
    pub(crate) author: String,
    pub(crate) author_time: i64,
    pub(crate) summary: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitBlameResult {
    pub(crate) file_path: String,
    pub(crate) lines: Vec<GitBlameLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitBranchGraphCommit {
    pub(crate) hash: String,
    pub(crate) short_hash: String,
    pub(crate) parents: Vec<String>,
    pub(crate) refs: Vec<String>,
    pub(crate) subject: String,
    pub(crate) author: String,
    pub(crate) relative_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitBranchGraphResult {
    pub(crate) commits: Vec<GitBranchGraphCommit>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashEntry {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) commit: String,
    pub(crate) date: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashDiff {
    pub(crate) stash_ref: String,
    pub(crate) diff: String,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitConflictFile {
    pub(crate) path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitConflictHunk {
    pub(crate) index: u32,
    pub(crate) ours: String,
    pub(crate) base: Option<String>,
    pub(crate) theirs: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitConflictPreview {
    pub(crate) file_path: String,
    pub(crate) hunks: Vec<GitConflictHunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GitConflictResolution {
    Ours,
    Theirs,
    Both,
}

fn short_commit_hash(commit: &str) -> String {
    commit
        .trim_start_matches('^')
        .chars()
        .take(8)
        .collect::<String>()
}

pub(crate) fn parse_blame_porcelain(stdout: &[u8]) -> Vec<GitBlameLine> {
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

pub(crate) fn parse_branch_graph_log(stdout: &[u8]) -> Vec<GitBranchGraphCommit> {
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

pub(crate) fn parse_stash_list(stdout: &[u8]) -> Vec<GitStashEntry> {
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

pub(crate) fn parse_conflict_paths_z(stdout: &[u8]) -> Vec<GitConflictFile> {
    stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| GitConflictFile {
            path: String::from_utf8_lossy(entry).into_owned(),
        })
        .collect()
}

fn validate_git_branch_name(branch_name: &str) -> Result<(), String> {
    if branch_name.is_empty() {
        return Err("Branch name must not be empty".to_string());
    }
    // Reject leading '-' so the value can never be parsed by git as an option
    // (option injection), even before the "--" separator is applied.
    if branch_name.starts_with('-') {
        return Err("Branch name must not start with '-'".to_string());
    }
    Ok(())
}

/// Reject a user-supplied revision / commit hash that git could parse as an
/// option. A malicious repository can carry a ref such as
/// `refs/heads/--output=/path` (creatable via `git update-ref`), which then
/// shows up in the branch list and flows back here as `branch`/`commit_hash`.
/// Without this guard, `git show`/`git log` would treat it as `--output=` and
/// write attacker-controlled content to an arbitrary path (option injection,
/// CWE-88). Callers should additionally pass `--end-of-options` where the local
/// git version supports it.
pub(crate) fn validate_git_revision(revision: &str) -> Result<(), String> {
    if revision.is_empty() {
        return Err("Git revision must not be empty".to_string());
    }
    if revision.starts_with('-') || revision.contains('\0') {
        return Err("Invalid git revision".to_string());
    }
    Ok(())
}

pub(crate) fn validate_stash_ref(stash_ref: &str) -> Result<(), String> {
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

pub(crate) fn truncate_text(value: String, max_chars: usize) -> (String, bool) {
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

pub(crate) fn parse_conflict_hunks(content: &str) -> Result<Vec<GitConflictHunk>, String> {
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

pub(crate) fn resolve_conflict_markers_keep_both(content: &str) -> Result<String, String> {
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

fn resolve_project_relative_file_path(
    project_path: &str,
    file_path: &str,
) -> Result<PathBuf, String> {
    validate_git_relative_path(file_path)?;
    let project = Path::new(project_path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {}", e))?;
    let target = project.join(file_path);
    let parent = target
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent directory: {}", e))?;
    if !parent.starts_with(&project) {
        return Err("File path is outside project root".to_string());
    }
    let file_name = target
        .file_name()
        .ok_or_else(|| "Cannot resolve file name".to_string())?;
    Ok(parent.join(file_name))
}

// ── Tauri 命令 ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn generate_commit_message(project_path: String) -> Result<String, String> {
    // 1. Get staged diff
    let diff_output = run_git(&project_path, &["diff", "--staged"])?;
    let diff = String::from_utf8_lossy(&diff_output.stdout).into_owned();
    if diff.trim().is_empty() {
        return Err("No staged changes to generate a commit message for.".to_string());
    }

    // Truncate diff if too large to avoid CLI arg limits
    let diff = if diff.len() > 50_000 {
        format!("{}...(diff truncated)", &diff[..50_000])
    } else {
        diff
    };

    // 2. Read project config for prompt and default agent
    let config = crate::config::read_project_config(project_path.clone())?;
    let commit_prompt = config.git.commit_prompt;
    let timeout_secs = config.git.commit_message_timeout_secs.clamp(1, 120);
    // 临时策略：claude `-p` 计费变动期间，提交信息一律改用 codex（headless）生成，
    // 规避 claude headless 额度消耗。codex 未安装则直接报错——不回落项目默认 agent
    // （claude -p 当前不可用，回落也无意义）。
    let agent = if crate::app_settings::codex_available() {
        "codex".to_string()
    } else {
        return Err("codex 未安装，无法生成提交信息。请安装 codex 后重试。".to_string());
    };

    // 3. Build full prompt
    let full_prompt = format!(
        "{}\n\nGit diff:\n```diff\n{}\n```\n\nOutput only the commit message, nothing else.",
        commit_prompt, diff
    );

    // 4. Run agent in non-interactive exec mode with configurable timeout
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::task::spawn_blocking(move || {
            run_agent_commit_message_command(&agent, &project_path, &full_prompt)
        }),
    )
    .await
    .map_err(|_| format!("生成提交信息超时（{}秒）", timeout_secs))?
    .map_err(|e| format!("生成提交信息线程错误: {}", e))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Agent failed: {}{}", stderr, stdout));
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result.is_empty() {
        return Err("Agent returned empty response.".to_string());
    }
    Ok(result)
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct GitFileChange {
    path: String,
    status: String,
    staged: bool,
}

pub(crate) fn parse_porcelain_z_status(stdout: &[u8]) -> Vec<GitFileChange> {
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

#[tauri::command]
pub async fn git_status(project_path: String) -> Result<Vec<GitFileChange>, String> {
    let args = vec![
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "-z".to_string(),
        "--untracked-files=all".to_string(),
    ];

    let output = run_git_with_timeout(project_path, args, Duration::from_secs(5)).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = format!("{}{}", stderr, stdout).trim().to_string();

        return Err(if message.is_empty() {
            "Failed to get git status".to_string()
        } else {
            message
        });
    }

    Ok(parse_porcelain_z_status(&output.stdout))
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct GitCommit {
    hash: String,
    short_hash: String,
    author: String,
    date: String,
    message: String,
    refs: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct GitBranchInfo {
    name: String,
    current: bool,
    remote: Option<String>,
}

pub(crate) fn parse_git_branch_list(stdout: &str) -> Vec<GitBranchInfo> {
    let mut branches = Vec::new();
    for line in stdout.lines() {
        if line.len() < 2 {
            continue;
        }
        let current = line.starts_with("* ");
        let raw = line[2..].trim();
        // Skip HEAD pointer lines like "remotes/origin/HEAD -> origin/main"
        if raw.contains(" -> ") {
            continue;
        }
        if let Some(without_remotes) = raw.strip_prefix("remotes/") {
            // "origin/main" -> remote = "origin", name = "origin/main"
            let name = without_remotes.to_string();
            let remote = name.split('/').next().map(|s| s.to_string());
            branches.push(GitBranchInfo {
                name,
                current,
                remote,
            });
        } else if !raw.is_empty() {
            branches.push(GitBranchInfo {
                name: raw.to_string(),
                current,
                remote: None,
            });
        }
    }
    branches
}

#[tauri::command]
pub async fn git_list_branches(project_path: String) -> Result<Vec<GitBranchInfo>, String> {
    let output = run_git_with_timeout(
        project_path,
        vec!["branch".to_string(), "-a".to_string()],
        Duration::from_secs(5),
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(parse_git_branch_list(&stdout))
}

#[tauri::command]
pub async fn git_checkout_branch(
    project_path: String,
    branch_name: String,
    is_remote: bool,
) -> Result<(), String> {
    validate_git_branch_name(&branch_name)?;
    let args: Vec<String> = if is_remote {
        // "origin/main" -> local name "main", track remote
        let local_name = branch_name
            .split_once('/')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| branch_name.clone());
        vec![
            "checkout".into(),
            "-b".into(),
            local_name,
            "--track".into(),
            format!("remotes/{}", branch_name),
        ]
    } else {
        vec!["checkout".into(), branch_name.clone()]
    };
    tauri::async_runtime::spawn_blocking(move || run_git_check(&project_path, &args))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_create_branch(
    project_path: String,
    branch_name: String,
    from_branch: String,
    checkout: bool,
) -> Result<(), String> {
    validate_git_branch_name(&branch_name)?;
    validate_git_branch_name(&from_branch)?;
    tauri::async_runtime::spawn_blocking(move || {
        let args: Vec<&str> = if checkout {
            vec!["checkout", "-b", &branch_name, &from_branch]
        } else {
            vec!["branch", &branch_name, &from_branch]
        };
        run_git_check(&project_path, &args)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_log(
    project_path: String,
    limit: u32,
    search: Option<String>,
    branch: Option<String>,
) -> Result<Vec<GitCommit>, String> {
    if let Some(branch) = branch.as_deref().filter(|value| !value.is_empty()) {
        validate_git_revision(branch)?;
    }
    let args = build_git_log_args(limit, search.as_deref(), branch.as_deref());

    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(parse_git_log_output(&stdout))
}

pub(crate) fn build_git_log_args(
    limit: u32,
    search: Option<&str>,
    branch: Option<&str>,
) -> Vec<String> {
    let limit_str = limit.to_string();
    let format = "COMMIT:%H%nSHORT:%h%nAUTHOR:%an%nDATE:%ar%nSUBJECT:%s%nREFS:%D%nEND_RECORD";
    let mut args: Vec<String> = vec![
        "log".into(),
        format!("--format={}", format),
        "-n".into(),
        limit_str,
    ];
    if let Some(s) = search {
        if !s.is_empty() {
            args.push(format!("--grep={}", s));
        }
    }
    if let Some(b) = branch {
        if !b.is_empty() {
            args.push(b.to_string());
        }
    }
    args
}

pub(crate) fn parse_git_log_output(stdout: &str) -> Vec<GitCommit> {
    let mut commits = Vec::new();
    let mut hash = String::new();
    let mut short_hash = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut message = String::new();
    let mut refs: Vec<String> = Vec::new();

    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("COMMIT:") {
            hash = v.to_string();
        } else if let Some(v) = line.strip_prefix("SHORT:") {
            short_hash = v.to_string();
        } else if let Some(v) = line.strip_prefix("AUTHOR:") {
            author = v.to_string();
        } else if let Some(v) = line.strip_prefix("DATE:") {
            date = v.to_string();
        } else if let Some(v) = line.strip_prefix("SUBJECT:") {
            message = v.to_string();
        } else if let Some(v) = line.strip_prefix("REFS:") {
            refs = v
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect();
        } else if line == "END_RECORD" && !hash.is_empty() {
            commits.push(GitCommit {
                hash: hash.clone(),
                short_hash: short_hash.clone(),
                author: author.clone(),
                date: date.clone(),
                message: message.clone(),
                refs: refs.clone(),
            });
            hash.clear();
            short_hash.clear();
            author.clear();
            date.clear();
            message.clear();
            refs.clear();
        }
    }
    commits
}

#[derive(serde::Serialize)]
pub(crate) struct GitCommitFile {
    path: String,
    status: String,
    additions: i32,
    deletions: i32,
}

#[derive(serde::Serialize)]
pub(crate) struct GitCommitDetail {
    hash: String,
    short_hash: String,
    author: String,
    date: String,
    message: String,
    files: Vec<GitCommitFile>,
    total_additions: i32,
    total_deletions: i32,
}

pub(crate) fn parse_git_commit_detail(
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

    let mut file_statuses: HashMap<String, String> = HashMap::new();
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

#[tauri::command]
pub async fn git_commit_detail(
    project_path: String,
    commit_hash: String,
) -> Result<GitCommitDetail, String> {
    validate_git_revision(&commit_hash)?;
    tauri::async_runtime::spawn_blocking(move || {
        let info_out = run_git(
            &project_path,
            &[
                "show",
                "--no-patch",
                "--format=HASH:%H%nSHORT:%h%nAUTHOR:%an%nDATE:%ar%nSUBJECT:%s",
                "--end-of-options",
                &commit_hash,
            ],
        )?;

        let ns_out = run_git(
            &project_path,
            &[
                "diff-tree",
                "--no-commit-id",
                "-r",
                "--name-status",
                "--end-of-options",
                &commit_hash,
            ],
        )?;

        let num_out = run_git(
            &project_path,
            &[
                "diff-tree",
                "--no-commit-id",
                "-r",
                "--numstat",
                "--end-of-options",
                &commit_hash,
            ],
        )?;

        let info_str = String::from_utf8_lossy(&info_out.stdout).into_owned();
        let ns_str = String::from_utf8_lossy(&ns_out.stdout).into_owned();
        let num_str = String::from_utf8_lossy(&num_out.stdout).into_owned();
        Ok(parse_git_commit_detail(&info_str, &ns_str, &num_str))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_show_diff(project_path: String, commit_hash: String) -> Result<String, String> {
    validate_git_revision(&commit_hash)?;
    let args = vec![
        "show".to_string(),
        "--format=".to_string(),
        "--end-of-options".to_string(),
        commit_hash,
    ];
    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let raw = output.stdout;
    let limit = 500 * 1024;
    Ok(String::from_utf8_lossy(if raw.len() > limit {
        &raw[..limit]
    } else {
        &raw
    })
    .into_owned())
}

#[tauri::command]
pub async fn git_file_diff(
    project_path: String,
    file_path: String,
    staged: bool,
) -> Result<String, String> {
    let mut args = vec!["diff".to_string()];
    if staged {
        args.push("--cached".to_string());
    }
    args.push("--".to_string());
    args.push(file_path.clone());

    let output = run_git_with_timeout(project_path.clone(), args, Duration::from_secs(10)).await?;
    let raw = output.stdout;

    // For untracked files, git diff returns nothing — fall back to --no-index diff
    if raw.is_empty() && !staged {
        let abs_path = std::path::Path::new(&project_path).join(&file_path);
        let abs_path_str = abs_path.to_string_lossy().into_owned();
        let empty_file = create_empty_temp_file()?;
        let fallback_args = vec![
            "diff".to_string(),
            "--no-index".to_string(),
            empty_file.to_string_lossy().into_owned(),
            abs_path_str,
        ];
        let fallback =
            run_git_with_timeout(project_path, fallback_args, Duration::from_secs(10)).await;
        let _ = std::fs::remove_file(&empty_file);
        let fallback = fallback?;
        let fallback_raw = fallback.stdout;
        let limit = 200 * 1024;
        return Ok(String::from_utf8_lossy(if fallback_raw.len() > limit {
            &fallback_raw[..limit]
        } else {
            &fallback_raw
        })
        .into_owned());
    }

    let limit = 200 * 1024;
    Ok(String::from_utf8_lossy(if raw.len() > limit {
        &raw[..limit]
    } else {
        &raw
    })
    .into_owned())
}

#[tauri::command]
pub async fn git_stage(project_path: String, file_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_git_check(&project_path, &["add", "--", &file_path])
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_unstage(project_path: String, file_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if git_has_head(&project_path)? {
            run_git_check(&project_path, &["restore", "--staged", "--", &file_path])
        } else {
            // 首次提交前无 HEAD，改用 `git reset` 将暂存项退回。
            run_git_check(&project_path, &["reset", "--", &file_path])
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_stage_files(project_path: String, file_paths: Vec<String>) -> Result<(), String> {
    let args = git_path_args(&["add"], file_paths)?;
    if args.is_empty() {
        return Ok(());
    }

    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to stage files"));
    }
    Ok(())
}

#[tauri::command]
pub async fn git_unstage_files(
    project_path: String,
    file_paths: Vec<String>,
) -> Result<(), String> {
    // 首次提交前无 HEAD，`git restore --staged` 会失败，退回到不依赖 HEAD 的 `git reset`。
    // 此处用异步 run_git_with_timeout（而非同步 git_has_head）做检测，避免阻塞 Tokio 运行时。
    let head_check = run_git_with_timeout(
        project_path.clone(),
        vec![
            "rev-parse".to_string(),
            "--verify".to_string(),
            "HEAD".to_string(),
        ],
        Duration::from_secs(5),
    )
    .await?;
    let base: &[&str] = if head_check.status.success() {
        &["restore", "--staged"]
    } else {
        &["reset"]
    };

    let args = git_path_args(base, file_paths)?;
    if args.is_empty() {
        return Ok(());
    }

    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to unstage files"));
    }
    Ok(())
}

#[tauri::command]
pub async fn git_stage_all(project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_git_check(&project_path, &["add", "-A"]))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_unstage_all(project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_git_check(&project_path, &["restore", "--staged", "."])
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_commit(project_path: String, message: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_git_check(&project_path, &["commit", "-m", &message])
    })
    .await
    .map_err(|e| e.to_string())?
}

fn untracked_files_under_directory<'a>(
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

fn is_listed_untracked_file(relative_path: &str, untracked_files: &[String]) -> bool {
    let relative_path = Path::new(relative_path);
    untracked_files
        .iter()
        .any(|path| Path::new(path) == relative_path)
}

fn is_protected_worktree_relative_path(
    worktree_root: &Path,
    project_path: &str,
    relative_path: &str,
) -> bool {
    if is_protected_project_relative_path(relative_path) {
        return true;
    }

    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return false;
    }

    let canonical_project = match Path::new(project_path).canonicalize() {
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

/// Move a worktree-relative path to the system trash. Canonicalize only the parent directory so
/// symlinks at the leaf are deleted as themselves rather than followed to their target. Reject
/// absolute or `..`-escaping relative paths defensively even though `git status` should never emit them.
fn trash_worktree_relative_path(
    worktree_root: &Path,
    project_path: &str,
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
        .map_err(|e| format!("Cannot resolve parent directory: {}", e))?;
    let canonical_root = worktree_root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve git worktree root: {}", e))?;
    let canonical_project = Path::new(project_path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {}", e))?;

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

    trash::delete(&resolved).map_err(|e| e.to_string())
}

fn discard_untracked_path(
    project_path: &str,
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

fn discard_untracked_file(
    project_path: &str,
    worktree_root: &Path,
    relative_path: &str,
) -> Result<(), String> {
    let worktree_root = worktree_root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve git worktree root: {}", e))?;
    let worktree_root_string = path_to_string(&worktree_root)?;
    let untracked_files = list_untracked_files(&worktree_root_string)?;

    discard_untracked_path(
        project_path,
        &worktree_root,
        relative_path,
        &untracked_files,
    )
}

fn list_untracked_files(project_path: &str) -> Result<Vec<String>, String> {
    let output = run_git(
        project_path,
        &[
            "-c",
            "core.quotePath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect())
}

/// Discard a single file's pending changes.
///
/// - Untracked files: moved to the system trash.
/// - Tracked unstaged changes: `git restore -- <file>` resets the worktree to the index, leaving
///   any staged half intact (so MM files don't lose their staged portion).
///
/// We deliberately don't expose a "discard staged" path here — staged-only files have no per-row
/// discard button in the UI (matching VSCode), and "Discard All" handles the staged side via
/// `git_discard_all` which correctly undoes renames too.
#[tauri::command]
pub async fn git_discard_file(
    project_path: String,
    file_path: String,
    untracked: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        validate_project_path(&project_path)?;
        let worktree_root = git_worktree_root(&project_path)?;
        let worktree_root_string = path_to_string(&worktree_root)?;
        if untracked {
            discard_untracked_file(&project_path, &worktree_root, &file_path)
        } else {
            run_git_check(&worktree_root_string, &["restore", "--", &file_path])
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_discard_files(
    project_path: String,
    file_paths: Vec<String>,
    untracked: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        validate_project_path(&project_path)?;
        let file_paths = unique_git_file_paths(file_paths)?;
        if file_paths.is_empty() {
            return Ok(());
        }

        let worktree_root = git_worktree_root(&project_path)?;
        let worktree_root_string = path_to_string(&worktree_root)?;
        if untracked {
            let untracked_files = list_untracked_files(&worktree_root_string)?;
            for file_path in file_paths {
                discard_untracked_path(
                    &project_path,
                    &worktree_root,
                    &file_path,
                    &untracked_files,
                )?;
            }
            return Ok(());
        }

        let mut args = vec!["restore".to_string(), "--".to_string()];
        args.extend(file_paths);
        run_git_check(&worktree_root_string, &args)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_discard_all(project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        validate_project_path(&project_path)?;
        let worktree_root = git_worktree_root(&project_path)?;
        let worktree_root_string = path_to_string(&worktree_root)?;
        // Reset every tracked file (staged + worktree) back to HEAD.
        // Staged-only adds become untracked after this; they are cleaned in the second pass.
        if git_has_head(&worktree_root_string)? {
            run_git_check(
                &worktree_root_string,
                &["restore", "--source=HEAD", "--staged", "--worktree", "."],
            )?;
        } else {
            run_git_check(
                &worktree_root_string,
                &["rm", "-r", "--cached", "--ignore-unmatch", "--", "."],
            )?;
        }

        for rel in list_untracked_files(&worktree_root_string)? {
            if is_protected_worktree_relative_path(&worktree_root, &project_path, &rel) {
                continue;
            }
            trash_worktree_relative_path(&worktree_root, &project_path, &rel)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_show_file_diff(
    project_path: String,
    commit_hash: String,
    file_path: String,
) -> Result<String, String> {
    validate_git_revision(&commit_hash)?;
    tauri::async_runtime::spawn_blocking(move || {
        let output = run_git(
            &project_path,
            &[
                "show",
                "--format=",
                "--end-of-options",
                &commit_hash,
                "--",
                &file_path,
            ],
        )?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let raw = output.stdout;
        let limit = 500 * 1024;
        Ok(String::from_utf8_lossy(if raw.len() > limit {
            &raw[..limit]
        } else {
            &raw
        })
        .into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_push(project_path: String, branch: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut args = vec!["push".to_string()];
        if let Some(ref b) = branch.filter(|s| !s.is_empty()) {
            args.push("origin".to_string());
            args.push(b.clone());
        }
        validate_project_path(&project_path)?;
        let output = run_git_network(&project_path, &args)?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            return Err(combined);
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_pull(project_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_project_path(&project_path)?;
        let output = run_git_network(&project_path, &["pull"])?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            return Err(combined);
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 在一个已存在的目录里建仓库。
///
/// 幂等:已经是仓库了就直接返回,不报错也不重新 init。上层(随手记的 vault 绑定)会在
/// 每次进入 git 同步模式时调一次,报错会让一个正常的重复调用看起来像故障。
///
/// `--initial-branch` 显式给出而不是靠 git 的默认值:那个默认值取决于用户的
/// `init.defaultBranch`,不给的话两台机器可能一台 `main` 一台 `master`,推送时就成了
/// 两条无关的分支。
#[tauri::command]
pub async fn git_init(project_path: String, initial_branch: Option<String>) -> Result<(), String> {
    if let Some(branch) = initial_branch.as_deref().filter(|value| !value.is_empty()) {
        validate_git_revision(branch)?;
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        validate_project_path(&project_path)?;
        if git_dir(&project_path).is_ok() {
            return Ok(());
        }
        let branch = initial_branch
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "main".to_string());
        run_git_check(
            &project_path,
            &["init", &format!("--initial-branch={branch}")],
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// clone 到一个空的/不存在的目录。
///
/// git 在**父目录**里执行,目标作为参数传 —— 目标自己还不存在,不能当 cwd。
///
/// `--` 放在 URL 前面:URL 是外部输入,而 `--` 之后 git 不再把参数当选项。
/// 注意这**不能替代** [`validate_git_url`] 的第一个字符检查:`--upload-pack=` 这类选项
/// 是在 `--` 之前被解析的,而攻击者控制的正是 URL 那个位置的值。两道都要。
#[tauri::command]
pub async fn git_clone(
    url: String,
    target_path: String,
    branch: Option<String>,
) -> Result<String, String> {
    validate_git_url(&url)?;
    validate_clone_target(&target_path)?;
    if let Some(branch) = branch.as_deref().filter(|value| !value.is_empty()) {
        validate_git_revision(branch)?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let target = Path::new(&target_path);
        let parent = target
            .parent()
            .ok_or_else(|| "Target path has no parent directory".to_string())?
            .to_string_lossy()
            .to_string();

        let mut args: Vec<String> = vec!["clone".to_string()];
        if let Some(branch) = branch.filter(|value| !value.is_empty()) {
            args.push("--branch".to_string());
            args.push(branch);
        }
        args.push("--".to_string());
        args.push(url.trim().to_string());
        args.push(target_path.clone());

        let output = run_git_network(&parent, &args)?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            return Err(combined.trim().to_string());
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 取远端最新状态但**不动工作区**。
///
/// 同步流程里先 fetch 再判断要不要 pull:fetch 不改工作区也不产生冲突,所以可以放心
/// 定时跑;拿到 ahead/behind 之后才决定是否合并。
#[tauri::command]
pub async fn git_fetch(project_path: String, prune: Option<bool>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_project_path(&project_path)?;
        let mut args: Vec<&str> = vec!["fetch", "--all"];
        if prune.unwrap_or(false) {
            args.push("--prune");
        }
        let output = run_git_network(&project_path, &args)?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            return Err(combined.trim().to_string());
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 这个目录是不是 git 仓库(工作区)。返回 `.git` 的位置。
///
/// `run_git` 只校验「是绝对路径且是目录」,不校验是仓库。对已有 37 条命令来说那没问题
/// —— 不是仓库时 git 自己会报错。但同步编排要**先分支**(不是仓库就 init 或 clone),
/// 所以需要一个不靠错误消息判断的问法。
pub(crate) fn git_dir(project_path: &str) -> Result<String, String> {
    let output = run_git(project_path, &["rev-parse", "--git-dir"])?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Not a git repository"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(serde::Serialize)]
pub(crate) struct GitRemoteCounts {
    ahead: i32,
    behind: i32,
    branch: String,
}

pub(crate) fn git_remote_counts_from_rev_list(
    branch: String,
    rev_list_stdout: Option<&str>,
) -> GitRemoteCounts {
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
        branch,
    }
}

#[tauri::command]
pub async fn git_remote_counts(
    project_path: String,
    branch: Option<String>,
) -> Result<GitRemoteCounts, String> {
    if let Some(branch) = branch.as_deref().filter(|value| !value.is_empty()) {
        validate_git_revision(branch)?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let branch = if let Some(b) = branch.filter(|s| !s.is_empty()) {
            b
        } else {
            let branch_out = run_git(&project_path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
            String::from_utf8_lossy(&branch_out.stdout)
                .trim()
                .to_string()
        };

        let rev_str = format!("{}...@{{u}}", branch);
        let rev_out = run_git(
            &project_path,
            &[
                "rev-list",
                "--count",
                "--left-right",
                "--end-of-options",
                &rev_str,
            ],
        );

        let rev_stdout = match &rev_out {
            Ok(o) if o.status.success() => {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            }
            _ => None,
        };

        Ok(git_remote_counts_from_rev_list(
            branch,
            rev_stdout.as_deref(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_blame_file(
    project_path: String,
    file_path: String,
) -> Result<GitBlameResult, String> {
    validate_git_relative_path(&file_path)?;
    let args = vec![
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "blame".to_string(),
        "--line-porcelain".to_string(),
        "--".to_string(),
        file_path.clone(),
    ];
    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to load git blame"));
    }
    Ok(GitBlameResult {
        file_path,
        lines: parse_blame_porcelain(&output.stdout),
    })
}

#[tauri::command]
pub async fn git_branch_graph(
    project_path: String,
    limit: Option<u32>,
) -> Result<GitBranchGraphResult, String> {
    let worktree_root = git_worktree_root(&project_path)?;
    let worktree_root = path_to_string(&worktree_root)?;
    if !git_has_head(&worktree_root)? {
        return Ok(GitBranchGraphResult {
            commits: Vec::new(),
            truncated: false,
        });
    }

    let limit = limit.unwrap_or(80).clamp(1, 200);
    let fetch_limit = limit + 1;
    let args = vec![
        "log".to_string(),
        "--all".to_string(),
        "--decorate=short".to_string(),
        "--date=relative".to_string(),
        "--pretty=format:%H%x1f%P%x1f%D%x1f%an%x1f%cr%x1f%s".to_string(),
        "-n".to_string(),
        fetch_limit.to_string(),
    ];
    let output = run_git_with_timeout(worktree_root, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(git_command_error(
            &output,
            "Failed to load git branch graph",
        ));
    }

    let mut commits = parse_branch_graph_log(&output.stdout);
    let truncated = commits.len() > limit as usize;
    if truncated {
        commits.truncate(limit as usize);
    }
    Ok(GitBranchGraphResult { commits, truncated })
}

#[tauri::command]
pub async fn git_stash_list(project_path: String) -> Result<Vec<GitStashEntry>, String> {
    let args = vec![
        "stash".to_string(),
        "list".to_string(),
        "--format=%gd%x1f%H%x1f%cr%x1f%s".to_string(),
    ];
    let output = run_git_with_timeout(project_path, args, Duration::from_secs(10)).await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to list git stashes"));
    }
    Ok(parse_stash_list(&output.stdout))
}

#[tauri::command]
pub async fn git_stash_diff(
    project_path: String,
    stash_ref: String,
) -> Result<GitStashDiff, String> {
    validate_stash_ref(&stash_ref)?;
    let output = run_git_with_timeout(
        project_path,
        vec![
            "stash".to_string(),
            "show".to_string(),
            "--patch".to_string(),
            "--stat".to_string(),
            "--include-untracked".to_string(),
            "--no-ext-diff".to_string(),
            "--no-color".to_string(),
            stash_ref.clone(),
        ],
        Duration::from_secs(10),
    )
    .await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to load git stash diff"));
    }
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (diff, truncated) = truncate_text(raw, MAX_STASH_DIFF_CHARS);
    Ok(GitStashDiff {
        stash_ref,
        diff,
        truncated,
    })
}

#[tauri::command]
pub async fn git_stash_push(
    project_path: String,
    message: Option<String>,
    include_untracked: bool,
) -> Result<String, String> {
    let mut args = vec!["stash".to_string(), "push".to_string()];
    if include_untracked {
        args.push("--include-untracked".to_string());
    }
    if let Some(message) = message
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push("-m".to_string());
        args.push(message);
    }
    let output = run_git_with_timeout(project_path, args, Duration::from_secs(20)).await?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if !output.status.success() {
        return Err(if combined.is_empty() {
            "Failed to create git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
}

#[tauri::command]
pub async fn git_stash_apply(project_path: String, stash_ref: String) -> Result<String, String> {
    validate_stash_ref(&stash_ref)?;
    let output = run_git_with_timeout(
        project_path,
        vec!["stash".to_string(), "apply".to_string(), stash_ref],
        Duration::from_secs(20),
    )
    .await?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if !output.status.success() {
        return Err(if combined.is_empty() {
            "Failed to apply git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
}

#[tauri::command]
pub async fn git_stash_drop(project_path: String, stash_ref: String) -> Result<String, String> {
    validate_stash_ref(&stash_ref)?;
    let output = run_git_with_timeout(
        project_path,
        vec!["stash".to_string(), "drop".to_string(), stash_ref],
        Duration::from_secs(10),
    )
    .await?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if !output.status.success() {
        return Err(if combined.is_empty() {
            "Failed to drop git stash".to_string()
        } else {
            combined
        });
    }
    Ok(combined)
}

#[tauri::command]
pub async fn git_conflict_files(project_path: String) -> Result<Vec<GitConflictFile>, String> {
    let output = run_git_with_timeout(
        project_path,
        vec![
            "diff".to_string(),
            "--name-only".to_string(),
            "--diff-filter=U".to_string(),
            "-z".to_string(),
        ],
        Duration::from_secs(10),
    )
    .await?;
    if !output.status.success() {
        return Err(git_command_error(&output, "Failed to list conflict files"));
    }
    Ok(parse_conflict_paths_z(&output.stdout))
}

#[tauri::command]
pub async fn git_conflict_preview(
    project_path: String,
    file_path: String,
) -> Result<GitConflictPreview, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<GitConflictPreview, String> {
        validate_project_path(&project_path)?;
        let target = resolve_project_relative_file_path(&project_path, &file_path)?;
        let content = std::fs::read_to_string(&target)
            .map_err(|e| format!("Failed to read conflict file: {}", e))?;
        Ok(GitConflictPreview {
            file_path,
            hunks: parse_conflict_hunks(&content)?,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_resolve_conflict(
    project_path: String,
    file_path: String,
    resolution: GitConflictResolution,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        validate_project_path(&project_path)?;
        validate_git_relative_path(&file_path)?;
        match resolution {
            GitConflictResolution::Ours => {
                run_git_check(&project_path, &["checkout", "--ours", "--", &file_path])?;
            }
            GitConflictResolution::Theirs => {
                run_git_check(&project_path, &["checkout", "--theirs", "--", &file_path])?;
            }
            GitConflictResolution::Both => {
                let target = resolve_project_relative_file_path(&project_path, &file_path)?;
                let content = std::fs::read_to_string(&target)
                    .map_err(|e| format!("Failed to read conflict file: {}", e))?;
                let resolved = resolve_conflict_markers_keep_both(&content)?;
                std::fs::write(&target, resolved)
                    .map_err(|e| format!("Failed to write resolved conflict file: {}", e))?;
            }
        }
        run_git_check(&project_path, &["add", "--", &file_path])
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Task worktree management ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct WorktreeCreated {
    #[serde(rename = "worktreePath")]
    worktree_path: String,
    #[serde(rename = "worktreeBranch")]
    worktree_branch: String,
    #[serde(rename = "baseBranch")]
    base_branch: String,
}

fn task_worktree_branch_name(task_id: &str) -> String {
    let short = if task_id.len() > 6 {
        &task_id[task_id.len() - 6..]
    } else {
        task_id
    };
    format!("aeroric/task-{}", short)
}

/// 校验 worktree 路径必须落在 `<project>/.aeroric/worktrees/` 之下，
/// 防止 remove_task_worktree 被传入任意路径。
fn ensure_path_under_worktrees_root(project_path: &str, worktree_path: &str) -> Result<(), String> {
    let project = Path::new(project_path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {}", e))?;
    let expected_root = project.join(".aeroric").join("worktrees");
    let target = Path::new(worktree_path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree path: {}", e))?;
    if !target.starts_with(&expected_root) {
        return Err("Worktree path is outside .aeroric/worktrees".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn create_task_worktree(
    project_path: String,
    task_id: String,
    base_branch: String,
) -> Result<WorktreeCreated, String> {
    validate_project_path(&project_path)?;
    crate::storage::validate_storage_id(&task_id, "task")?;
    if base_branch.trim().is_empty() {
        return Err("Base branch is required".to_string());
    }

    tokio::task::spawn_blocking(move || -> Result<WorktreeCreated, String> {
        let worktrees_dir = Path::new(&project_path).join(".aeroric").join("worktrees");
        std::fs::create_dir_all(&worktrees_dir)
            .map_err(|e| format!("Failed to create worktrees dir: {}", e))?;

        let worktree_path = worktrees_dir.join(&task_id);
        if worktree_path.exists() {
            return Err(format!(
                "Worktree path already exists: {}",
                worktree_path.display()
            ));
        }

        let wt_path_str = path_to_string(&worktree_path)?;
        let branch = task_worktree_branch_name(&task_id);

        let output = run_git(
            &project_path,
            &["worktree", "add", &wt_path_str, "-b", &branch, &base_branch],
        )?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }

        Ok(WorktreeCreated {
            worktree_path: wt_path_str,
            worktree_branch: branch,
            base_branch,
        })
    })
    .await
    .map_err(|e| format!("Worktree task panicked: {}", e))?
}

#[tauri::command]
pub async fn merge_task_worktree(
    project_path: String,
    worktree_path: String,
    branch: String,
    base_branch: String,
) -> Result<String, String> {
    validate_project_path(&project_path)?;
    ensure_path_under_worktrees_root(&project_path, &worktree_path)?;
    if branch.trim().is_empty() || base_branch.trim().is_empty() {
        return Err("Branch and base branch are required".to_string());
    }

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        // 0) worktree 自身有未提交修改 → 拒绝合并，避免丢失工作进度
        let wt_status = run_git(&worktree_path, &["status", "--porcelain"])?;
        if !wt_status.status.success() {
            return Err(String::from_utf8_lossy(&wt_status.stderr)
                .trim()
                .to_string());
        }
        if !wt_status.stdout.is_empty() {
            return Err(
                "Worktree has uncommitted changes; commit or stash them before merging".into(),
            );
        }

        // 拿主仓当前 HEAD：HEAD == base 时直接 merge，否则用 fetch ff（不切走 HEAD）。
        let head_out = run_git(&project_path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        if !head_out.status.success() {
            return Err(String::from_utf8_lossy(&head_out.stderr).trim().to_string());
        }
        let original_branch = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

        if original_branch == base_branch {
            // 主仓正在 base 上，直接合并（保留 merge commit 让历史可追溯）
            let merge_out = run_git(&project_path, &["merge", "--no-ff", &branch])?;
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&merge_out.stdout),
                String::from_utf8_lossy(&merge_out.stderr)
            );
            if !merge_out.status.success() {
                return Err(format!(
                    "Merge failed (main repo on '{}'; please resolve manually): {}",
                    base_branch, combined
                ));
            }
            return Ok(combined.trim().to_string());
        }

        // 主仓不在 base：用 `git fetch . <src>:<dst>` 把 worktree 分支 ff 到 base ref，不动主仓 HEAD。
        // git fetch 默认仅允许 fast-forward 更新（用 `+` 前缀才强制），刚好阻止误覆盖 base 的提交。
        let refspec = format!("{}:{}", branch, base_branch);
        let ff_out = run_git(&project_path, &["fetch", ".", &refspec])?;
        if !ff_out.status.success() {
            let err = String::from_utf8_lossy(&ff_out.stderr);
            return Err(format!(
                "Cannot fast-forward '{}' (worktree may have diverged from base). \
                 Pull base into the worktree and retry, or merge manually. Detail: {}",
                base_branch,
                err.trim()
            ));
        }
        Ok(format!("Fast-forwarded '{}' to '{}'", base_branch, branch))
    })
    .await
    .map_err(|e| format!("Merge task panicked: {}", e))?
}

#[tauri::command]
pub async fn remove_task_worktree(
    project_path: String,
    worktree_path: String,
    branch: String,
) -> Result<(), String> {
    validate_project_path(&project_path)?;
    ensure_path_under_worktrees_root(&project_path, &worktree_path)?;

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        // worktree remove --force 既可移除有未提交修改的工作树，也会清理元数据。
        let remove_out = run_git(
            &project_path,
            &["worktree", "remove", "--force", &worktree_path],
        )?;
        if !remove_out.status.success() {
            return Err(String::from_utf8_lossy(&remove_out.stderr)
                .trim()
                .to_string());
        }

        if !branch.trim().is_empty() {
            // -D 允许删除未合并分支（丢弃语义）。已合并分支也能成功。
            let branch_out = run_git(&project_path, &["branch", "-D", &branch])?;
            if !branch_out.status.success() {
                return Err(String::from_utf8_lossy(&branch_out.stderr)
                    .trim()
                    .to_string());
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Remove worktree task panicked: {}", e))?
}

#[derive(serde::Serialize)]
pub(crate) struct WorktreeDiffStats {
    pub additions: i32,
    pub deletions: i32,
}

/// 计算 worktree 工作树（含未提交改动 + 未跟踪文件）相对于 `base_branch` 与 HEAD 的 merge-base 的 +/− 行数。
/// 用 merge-base 而非 base_branch 本身，避免主仓 base 推进后把别人提交的改动算到本任务头上。
#[tauri::command]
pub async fn worktree_diff_stats(
    project_path: String,
    worktree_path: String,
    base_branch: String,
) -> Result<WorktreeDiffStats, String> {
    if base_branch.trim().is_empty() {
        return Err("Base branch is required".to_string());
    }

    tokio::task::spawn_blocking(move || -> Result<WorktreeDiffStats, String> {
        // 路径校验包含同步 canonicalize，必须留在 spawn_blocking 内，避免阻塞 Tokio 运行时。
        validate_project_path(&project_path)?;
        ensure_path_under_worktrees_root(&project_path, &worktree_path)?;

        // 1) 已跟踪改动（含已 stage / 未 stage）：working tree vs merge-base
        let mb_out = run_git(&worktree_path, &["merge-base", &base_branch, "HEAD"])?;
        if !mb_out.status.success() {
            return Err(String::from_utf8_lossy(&mb_out.stderr).trim().to_string());
        }
        let merge_base = String::from_utf8_lossy(&mb_out.stdout).trim().to_string();

        let mut additions = 0i32;
        let mut deletions = 0i32;

        if !merge_base.is_empty() {
            let num_out = run_git(&worktree_path, &["diff", "--numstat", &merge_base])?;
            if !num_out.status.success() {
                return Err(String::from_utf8_lossy(&num_out.stderr).trim().to_string());
            }
            accumulate_numstat(&num_out.stdout, &mut additions, &mut deletions);
        }

        // 2) 未跟踪文件：git diff 不会列出，需要逐个用 --no-index 与空文件比对
        let untracked = list_untracked_files(&worktree_path)?;
        if !untracked.is_empty() {
            let empty_file = create_empty_temp_file()?;
            let empty_path = empty_file.to_string_lossy().into_owned();
            for rel in &untracked {
                let abs = Path::new(&worktree_path).join(rel);
                let abs_str = abs.to_string_lossy().into_owned();
                // git diff --no-index 在文件不同时返回退出码 1，故不能用 status 判断成败
                let no_index = run_git(
                    &worktree_path,
                    &["diff", "--no-index", "--numstat", &empty_path, &abs_str],
                )?;
                accumulate_numstat(&no_index.stdout, &mut additions, &mut deletions);
            }
            let _ = std::fs::remove_file(&empty_file);
        }

        Ok(WorktreeDiffStats {
            additions,
            deletions,
        })
    })
    .await
    .map_err(|e| format!("Diff stats task panicked: {}", e))?
}

/// 解析 `git diff --numstat` 输出累加 +/− 行数。
/// numstat 对二进制文件输出 `-\t-\t<path>`，parse 失败时按 0 跳过。
fn accumulate_numstat(stdout: &[u8], additions: &mut i32, deletions: &mut i32) {
    for line in String::from_utf8_lossy(stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        *additions += parts[0].parse::<i32>().unwrap_or(0);
        *deletions += parts[1].parse::<i32>().unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        git_blame_file, git_branch_graph, git_clone, git_commit_detail, git_conflict_files,
        git_conflict_preview, git_dir, git_fetch, git_has_head, git_init, git_log,
        git_resolve_conflict, git_show_diff, git_stash_diff, git_stash_list, git_stash_push,
        git_worktree_root, is_protected_project_relative_path, list_untracked_files,
        parse_blame_porcelain, parse_branch_graph_log, parse_conflict_hunks,
        parse_conflict_paths_z, parse_porcelain_z_status, parse_stash_list, path_to_string,
        resolve_conflict_markers_keep_both, run_git, run_git_check,
        untracked_files_under_directory, validate_clone_target, validate_git_revision,
        validate_git_url, validate_project_path, validate_stash_ref, GitBlameLine,
        GitBranchGraphCommit, GitConflictFile, GitConflictResolution, GitFileChange, GitStashEntry,
    };
    use std::{fs, path::PathBuf, process::Command};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("aeroric-git-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            let output = Command::new("git").arg("init").arg(&path).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Self { path }
        }

        fn path_string(&self) -> String {
            path_to_string(&self.path.canonicalize().unwrap()).unwrap()
        }

        fn configure_identity(&self) {
            let repo_path = self.path_string();
            run_git_check(
                &repo_path,
                &["config", "user.email", "aeroric@example.test"],
            )
            .unwrap();
            run_git_check(&repo_path, &["config", "user.name", "Aeroric Test"]).unwrap();
        }

        fn commit_file(&self, file_path: &str, content: &str, message: &str) {
            let repo_path = self.path_string();
            let path = self.path.join(file_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
            run_git_check(&repo_path, &["add", file_path]).unwrap();
            run_git_check(&repo_path, &["commit", "-m", message]).unwrap();
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn parses_untracked_path_with_spaces_without_quotes() {
        let changes = parse_porcelain_z_status(b"?? te st2.txt\0");

        assert_eq!(
            changes,
            vec![GitFileChange {
                path: "te st2.txt".to_string(),
                status: "?".to_string(),
                staged: false,
            }]
        );
    }

    #[test]
    fn rejects_file_path_as_git_project_root() {
        let repo = TempRepo::new();
        let file = repo.path.join("not-a-project");
        fs::write(&file, "content").unwrap();

        let error = validate_project_path(&path_to_string(&file).unwrap()).unwrap_err();
        assert_eq!(error, "Project path is not a directory");
    }

    #[test]
    fn parses_staged_and_unstaged_changes_for_same_path() {
        let changes = parse_porcelain_z_status(b"MM src/file name.ts\0");

        assert_eq!(
            changes,
            vec![
                GitFileChange {
                    path: "src/file name.ts".to_string(),
                    status: "M".to_string(),
                    staged: true,
                },
                GitFileChange {
                    path: "src/file name.ts".to_string(),
                    status: "M".to_string(),
                    staged: false,
                },
            ]
        );
    }

    #[test]
    fn parses_rename_destination_and_skips_source_path() {
        let changes = parse_porcelain_z_status(b"R  new name.txt\0old name.txt\0");

        assert_eq!(
            changes,
            vec![GitFileChange {
                path: "new name.txt".to_string(),
                status: "R".to_string(),
                staged: true,
            }]
        );
    }

    #[test]
    fn parses_line_porcelain_blame_output() {
        let blame = parse_blame_porcelain(
            b"abcdef1234567890 1 1 1\nauthor Ada Lovelace\nauthor-time 1710000000\nsummary initial commit\n\tconst value = 1;\n",
        );

        assert_eq!(
            blame,
            vec![GitBlameLine {
                line: 1,
                commit: "abcdef1234567890".to_string(),
                short_commit: "abcdef12".to_string(),
                author: "Ada Lovelace".to_string(),
                author_time: 1710000000,
                summary: "initial commit".to_string(),
                content: "const value = 1;".to_string(),
            }]
        );
    }

    #[test]
    fn parses_stash_list_format() {
        let stashes =
            parse_stash_list(b"stash@{1}\x1fabc123\x1f2 hours ago\x1fWIP on main: change UI\n");

        assert_eq!(
            stashes,
            vec![GitStashEntry {
                index: 1,
                name: "stash@{1}".to_string(),
                commit: "abc123".to_string(),
                date: "2 hours ago".to_string(),
                message: "WIP on main: change UI".to_string(),
            }]
        );
    }

    #[test]
    fn parses_branch_graph_log_records() {
        let commits = parse_branch_graph_log(
            b"abcdef123456\x1f111111111111 222222222222\x1fHEAD -> main, origin/main, tag: v1.0.0\x1fAda\x1f2 minutes ago\x1fAdd graph\n",
        );

        assert_eq!(
            commits,
            vec![GitBranchGraphCommit {
                hash: "abcdef123456".to_string(),
                short_hash: "abcdef12".to_string(),
                parents: vec!["111111111111".to_string(), "222222222222".to_string()],
                refs: vec![
                    "HEAD -> main".to_string(),
                    "origin/main".to_string(),
                    "tag: v1.0.0".to_string(),
                ],
                subject: "Add graph".to_string(),
                author: "Ada".to_string(),
                relative_time: "2 minutes ago".to_string(),
            }]
        );
    }

    #[test]
    fn validates_stash_refs_conservatively() {
        assert!(validate_stash_ref("stash@{0}").is_ok());
        assert!(validate_stash_ref("stash@{12}").is_ok());
        assert!(validate_stash_ref("stash@{main}").is_err());
        assert!(validate_stash_ref("HEAD").is_err());
    }

    #[test]
    fn parses_conflict_paths_from_nul_separated_output() {
        assert_eq!(
            parse_conflict_paths_z(b"src/app.ts\0README.md\0"),
            vec![
                GitConflictFile {
                    path: "src/app.ts".to_string(),
                },
                GitConflictFile {
                    path: "README.md".to_string(),
                },
            ]
        );
    }

    #[test]
    fn resolves_conflict_markers_by_keeping_both_sides() {
        let resolved = resolve_conflict_markers_keep_both(
            "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\nafter\n",
        )
        .unwrap();

        assert_eq!(resolved, "before\nours\ntheirs\nafter\n");
    }

    #[test]
    fn resolves_diff3_conflict_markers_by_dropping_base() {
        let resolved = resolve_conflict_markers_keep_both(
            "before\n<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> feature\nafter\n",
        )
        .unwrap();

        assert_eq!(resolved, "before\nours\ntheirs\nafter\n");
    }

    #[test]
    fn parses_conflict_hunks_for_three_column_preview() {
        let hunks = parse_conflict_hunks(
            "before\n<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> feature\nafter\n",
        )
        .unwrap();

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].index, 1);
        assert_eq!(hunks[0].ours, "ours\n");
        assert_eq!(hunks[0].base.as_deref(), Some("base\n"));
        assert_eq!(hunks[0].theirs, "theirs\n");
    }

    #[tokio::test]
    async fn git_blame_file_returns_committed_lines() {
        let repo = TempRepo::new();
        repo.configure_identity();
        repo.commit_file("src/app.js", "const value = 1;\n", "initial");

        let blame = git_blame_file(repo.path_string(), "src/app.js".to_string())
            .await
            .unwrap();

        assert_eq!(blame.file_path, "src/app.js");
        assert_eq!(blame.lines.len(), 1);
        assert_eq!(blame.lines[0].author, "Aeroric Test");
        assert_eq!(blame.lines[0].content, "const value = 1;");
    }

    #[tokio::test]
    async fn git_stash_commands_create_and_list_stashes() {
        let repo = TempRepo::new();
        repo.configure_identity();
        repo.commit_file("app.js", "one\n", "initial");
        fs::write(repo.path.join("app.js"), "two\n").unwrap();

        git_stash_push(
            repo.path_string(),
            Some("save app change".to_string()),
            false,
        )
        .await
        .unwrap();
        let stashes = git_stash_list(repo.path_string()).await.unwrap();

        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].name, "stash@{0}");
        assert!(stashes[0].message.contains("save app change"));
    }

    #[tokio::test]
    async fn git_stash_diff_returns_patch() {
        let repo = TempRepo::new();
        repo.configure_identity();
        repo.commit_file("app.js", "one\n", "initial");
        fs::write(repo.path.join("app.js"), "two\n").unwrap();

        git_stash_push(repo.path_string(), Some("preview".to_string()), false)
            .await
            .unwrap();
        let diff = git_stash_diff(repo.path_string(), "stash@{0}".to_string())
            .await
            .unwrap();

        assert_eq!(diff.stash_ref, "stash@{0}");
        assert!(!diff.truncated);
        assert!(diff.diff.contains("diff --git"));
        assert!(diff.diff.contains("-one"));
        assert!(diff.diff.contains("+two"));
    }

    #[tokio::test]
    async fn git_branch_graph_returns_recent_commits_and_refs() {
        let repo = TempRepo::new();
        repo.configure_identity();
        let repo_path = repo.path_string();
        repo.commit_file("app.txt", "base\n", "initial");
        let base_branch_output =
            run_git(&repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
        let base_branch = String::from_utf8_lossy(&base_branch_output.stdout)
            .trim()
            .to_string();
        run_git_check(&repo_path, &["checkout", "-b", "feature"]).unwrap();
        repo.commit_file("feature.txt", "feature\n", "feature change");
        run_git_check(&repo_path, &["checkout", base_branch.as_str()]).unwrap();
        repo.commit_file("main.txt", "main\n", "main change");

        let graph = git_branch_graph(repo_path, Some(20)).await.unwrap();

        assert!(!graph.truncated);
        assert!(graph.commits.len() >= 3);
        assert!(graph
            .commits
            .iter()
            .any(|commit| commit.refs.iter().any(|name| name.contains(&base_branch))));
        assert!(graph
            .commits
            .iter()
            .any(|commit| commit.refs.iter().any(|name| name.contains("feature"))));
    }

    #[tokio::test]
    async fn git_conflict_files_detects_and_resolves_ours() {
        let repo = TempRepo::new();
        repo.configure_identity();
        let repo_path = repo.path_string();
        repo.commit_file("app.txt", "base\n", "initial");

        run_git_check(&repo_path, &["checkout", "-b", "feature"]).unwrap();
        repo.commit_file("app.txt", "feature\n", "feature change");
        run_git_check(&repo_path, &["checkout", "-"]).unwrap();
        repo.commit_file("app.txt", "main\n", "main change");
        let merge = run_git(&repo_path, &["merge", "feature"]).unwrap();
        assert!(!merge.status.success());

        let conflicts = git_conflict_files(repo_path.clone()).await.unwrap();
        assert_eq!(
            conflicts,
            vec![GitConflictFile {
                path: "app.txt".to_string(),
            }]
        );

        git_resolve_conflict(
            repo_path.clone(),
            "app.txt".to_string(),
            GitConflictResolution::Ours,
        )
        .await
        .unwrap();

        assert!(git_conflict_files(repo_path.clone())
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            fs::read_to_string(repo.path.join("app.txt")).unwrap(),
            "main\n"
        );
    }

    #[tokio::test]
    async fn git_conflict_preview_returns_marker_hunks() {
        let repo = TempRepo::new();
        repo.configure_identity();
        let repo_path = repo.path_string();
        repo.commit_file("app.txt", "base\n", "initial");

        run_git_check(&repo_path, &["checkout", "-b", "feature"]).unwrap();
        repo.commit_file("app.txt", "feature\n", "feature change");
        run_git_check(&repo_path, &["checkout", "-"]).unwrap();
        repo.commit_file("app.txt", "main\n", "main change");
        let merge = run_git(&repo_path, &["merge", "feature"]).unwrap();
        assert!(!merge.status.success());

        let preview = git_conflict_preview(repo_path, "app.txt".to_string())
            .await
            .unwrap();

        assert_eq!(preview.file_path, "app.txt");
        assert_eq!(preview.hunks.len(), 1);
        assert!(preview.hunks[0].ours.contains("main"));
        assert!(preview.hunks[0].theirs.contains("feature"));
    }

    #[test]
    fn detects_protected_project_metadata_paths() {
        assert!(is_protected_project_relative_path(".aeroric/config.toml"));
        assert!(is_protected_project_relative_path("./.git/index"));
        assert!(is_protected_project_relative_path(
            ".Aeroric/attachments/file.png"
        ));
        assert!(!is_protected_project_relative_path(
            "src/.aeroric/config.toml"
        ));
        assert!(!is_protected_project_relative_path(".gitignore"));
        assert!(!is_protected_project_relative_path("src/git.rs"));
    }

    #[test]
    fn lists_only_untracked_files_under_requested_directory() {
        let untracked_files = vec![
            "dir/file.txt".to_string(),
            "dir/nested/other.txt".to_string(),
            "dir2/file.txt".to_string(),
            "other.txt".to_string(),
        ];

        assert_eq!(
            untracked_files_under_directory("dir/", &untracked_files),
            vec!["dir/file.txt", "dir/nested/other.txt"]
        );
    }

    #[test]
    fn resolves_worktree_root_for_nested_project_paths() {
        let repo = TempRepo::new();
        let nested_project = repo.path.join("nested/project");
        fs::create_dir_all(&nested_project).unwrap();

        let resolved = git_worktree_root(nested_project.to_str().unwrap()).unwrap();

        assert_eq!(resolved, repo.path.canonicalize().unwrap());
    }

    #[test]
    fn unborn_repository_can_prepare_staged_files_for_untracked_cleanup() {
        let repo = TempRepo::new();
        let repo_path = repo.path_string();
        fs::write(repo.path.join("new-file.txt"), "content").unwrap();

        assert!(!git_has_head(&repo_path).unwrap());
        run_git_check(&repo_path, &["add", "new-file.txt"]).unwrap();
        run_git_check(
            &repo_path,
            &["rm", "-r", "--cached", "--ignore-unmatch", "--", "."],
        )
        .unwrap();

        assert_eq!(
            list_untracked_files(&repo_path).unwrap(),
            vec!["new-file.txt".to_string()]
        );
    }

    #[test]
    fn validate_git_revision_rejects_option_like_values() {
        assert!(validate_git_revision("HEAD").is_ok());
        assert!(validate_git_revision("HEAD~3").is_ok());
        assert!(validate_git_revision("origin/main").is_ok());
        assert!(validate_git_revision("@{-1}").is_ok());
        assert!(validate_git_revision("").is_err());
        assert!(validate_git_revision("--output=/tmp/evil").is_err());
        assert!(validate_git_revision("-n").is_err());
        assert!(validate_git_revision("bad\0ref").is_err());
    }

    // Regression: a malicious repository can carry a ref like
    // `refs/heads/--output=<path>` (creatable with `git update-ref`) that shows
    // up in the branch list and flows back as `branch`/`commit_hash`. Without
    // the option-injection guard, git would treat it as `--output=` and write
    // attacker-controlled content to an arbitrary path. These commands must
    // refuse the value instead of writing the file.
    #[tokio::test]
    async fn git_history_commands_reject_option_injection_revisions() {
        let repo = TempRepo::new();
        repo.configure_identity();
        repo.commit_file("app.js", "one\n", "initial");
        let repo_path = repo.path_string();
        let sentinel = repo.path.join("PWNED.txt");
        let payload = format!("--output={}", sentinel.to_string_lossy());

        // Confirm git itself would honor the option if it reached the CLI, so
        // this test fails loudly if the guard is ever removed.
        run_git_check(&repo_path, &["update-ref", "refs/heads/injected", "HEAD"]).unwrap();

        assert!(git_show_diff(repo_path.clone(), payload.clone())
            .await
            .is_err());
        assert!(git_commit_detail(repo_path.clone(), payload.clone())
            .await
            .is_err());
        assert!(super::git_show_file_diff(
            repo_path.clone(),
            payload.clone(),
            "app.js".to_string()
        )
        .await
        .is_err());
        assert!(git_log(repo_path.clone(), 50, None, Some(payload.clone()))
            .await
            .is_err());

        assert!(
            !sentinel.exists(),
            "option injection wrote a file to {}",
            sentinel.to_string_lossy()
        );

        // A legitimate revision still works.
        assert!(git_log(repo_path, 50, None, Some("injected".to_string()))
            .await
            .is_ok());
    }

    #[test]
    fn a_repository_url_starting_with_a_dash_is_refused() {
        // 这一条防的是实测成立的 RCE:git clone 把以 `-` 开头的位置参数当选项解析,
        // `--upload-pack=<cmd>` 于是变成任意命令执行(本机 git 2.50 上验证过)。
        for payload in [
            "--upload-pack=touch /tmp/pwned",
            "-c protocol.ext.allow=always",
            "--config=core.pager=sh",
            "-",
        ] {
            assert!(
                validate_git_url(payload).is_err(),
                "{payload} 必须被拒绝 —— 它会被 git 当成选项"
            );
        }
    }

    #[test]
    fn a_transport_helper_url_is_refused() {
        // `ext::` 会执行任意命令。git 2.50 的默认值已经拦住,但那是 git 的默认值而不是
        // 我们的保证:用户 gitconfig 里一句 protocol.ext.allow=always 就能打开。
        assert!(validate_git_url("ext::sh -c whoami").is_err());
        assert!(validate_git_url("weird-helper::payload").is_err());
    }

    #[test]
    fn ordinary_repository_urls_are_accepted() {
        for url in [
            "https://github.com/owner/repo.git",
            "http://example.test/repo.git",
            "ssh://git@example.test:2222/owner/repo.git",
            "git@github.com:owner/repo.git",
            "/Users/someone/local/bare.git",
            "file:///Users/someone/local/bare.git",
        ] {
            assert!(validate_git_url(url).is_ok(), "{url} 应该放行");
        }
    }

    #[test]
    fn a_double_colon_inside_a_url_path_is_not_mistaken_for_a_helper() {
        // 判断顺序要紧:`://` 之后出现的 `::` 是路径的一部分,不是传输助手。
        // 弄反的话合法的 URL 会被拒,而用户看不出为什么。
        assert!(validate_git_url("https://example.test/weird::name.git").is_ok());
        assert!(validate_git_url("ssh://host/a::b").is_ok());
    }

    #[test]
    fn a_url_with_control_characters_is_refused() {
        assert!(validate_git_url("https://example.test/\nfetch = evil").is_err());
        assert!(validate_git_url("https://example.test/repo\0.git").is_err());
        assert!(validate_git_url("   ").is_err());
    }

    #[test]
    fn a_clone_target_must_be_absolute_with_an_existing_parent() {
        assert!(validate_clone_target("relative/path").is_err());
        assert!(validate_clone_target("/definitely/not/here/at/all/target").is_err());
    }

    #[test]
    fn a_clone_target_may_be_missing_or_empty_but_not_occupied() {
        let repo = TempRepo::new();
        let base = repo.path.canonicalize().unwrap();

        let missing = base.join("fresh");
        assert!(validate_clone_target(&path_to_string(&missing).unwrap()).is_ok());

        let empty = base.join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(validate_clone_target(&path_to_string(&empty).unwrap()).is_ok());

        // 非空目录一律拒:clone 到有东西的目录上会把用户的文件和一个陌生仓库混在一起,
        // 那种状态很难看出来、更难还原。
        let occupied = base.join("occupied");
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("mine.txt"), "important").unwrap();
        assert_eq!(
            validate_clone_target(&path_to_string(&occupied).unwrap()).unwrap_err(),
            "Target directory is not empty"
        );

        let as_file = base.join("a-file");
        fs::write(&as_file, "not a dir").unwrap();
        assert!(validate_clone_target(&path_to_string(&as_file).unwrap()).is_err());
    }

    #[tokio::test]
    async fn git_init_creates_a_repository_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("aeroric-git-init-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = path_to_string(&dir.canonicalize().unwrap()).unwrap();

        git_init(path.clone(), None).await.expect("init");
        assert!(dir.join(".git").exists());
        // 分支名显式给出,不靠用户的 init.defaultBranch —— 否则两台机器可能一台 main
        // 一台 master,推送时成了两条无关的分支。
        let head = fs::read_to_string(dir.join(".git").join("HEAD")).unwrap();
        assert!(head.contains("refs/heads/main"), "HEAD = {head}");

        // 再调一次不该报错:上层每次进入 git 同步模式都会调,报错会让正常的重复调用
        // 看起来像故障。
        git_init(path, None).await.expect("second init is a no-op");

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn git_init_refuses_an_option_like_branch_name() {
        let dir = std::env::temp_dir().join(format!("aeroric-git-init-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = path_to_string(&dir.canonicalize().unwrap()).unwrap();

        assert!(git_init(path, Some("--bare".to_string())).await.is_err());
        assert!(!dir.join(".git").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn git_clone_copies_a_local_repository() {
        let source = TempRepo::new();
        source.configure_identity();
        source.commit_file("note.md", "hello", "first");

        let target =
            std::env::temp_dir().join(format!("aeroric-git-clone-{}", uuid::Uuid::new_v4()));
        let target_path = path_to_string(&target).unwrap();

        git_clone(source.path_string(), target_path.clone(), None)
            .await
            .expect("clone");
        assert_eq!(fs::read_to_string(target.join("note.md")).unwrap(), "hello");

        let _ = fs::remove_dir_all(&target);
    }

    #[tokio::test]
    async fn git_clone_refuses_an_option_like_url_without_running_git() {
        // 端到端地钉住那条 RCE:`--upload-pack=` 会执行命令,所以它必须在 git 被调起
        // **之前**就被拒。
        let sentinel =
            std::env::temp_dir().join(format!("aeroric-clone-rce-{}", uuid::Uuid::new_v4()));
        let target =
            std::env::temp_dir().join(format!("aeroric-git-clone-{}", uuid::Uuid::new_v4()));

        let error = git_clone(
            format!("--upload-pack=touch {};git-upload-pack", sentinel.display()),
            path_to_string(&target).unwrap(),
            None,
        )
        .await
        .unwrap_err();

        assert!(error.contains("must not start with"), "error = {error}");
        assert!(!sentinel.exists(), "选项注入执行了命令");
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn git_fetch_brings_in_a_new_remote_commit() {
        let source = TempRepo::new();
        source.configure_identity();
        source.commit_file("note.md", "one", "first");

        let target =
            std::env::temp_dir().join(format!("aeroric-git-fetch-{}", uuid::Uuid::new_v4()));
        let target_path = path_to_string(&target).unwrap();
        git_clone(source.path_string(), target_path.clone(), None)
            .await
            .expect("clone");

        source.commit_file("note.md", "two", "second");
        git_fetch(target_path.clone(), None).await.expect("fetch");

        // fetch 只动 remote-tracking ref,**不动工作区** —— 所以工作区还是旧内容。
        // 这一点是同步编排敢定时跑 fetch 的前提:它不产生冲突。
        assert_eq!(fs::read_to_string(target.join("note.md")).unwrap(), "one");
        let output = run_git(&target_path, &["rev-list", "--count", "HEAD..origin/HEAD"]).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "1",
            "fetch 之后应该看到落后一个提交"
        );

        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn git_dir_distinguishes_a_repository_from_a_plain_directory() {
        // 同步编排要先分支(不是仓库就 init/clone),所以需要一个不靠错误消息判断的问法。
        let repo = TempRepo::new();
        assert!(git_dir(&repo.path_string()).is_ok());

        let plain =
            std::env::temp_dir().join(format!("aeroric-git-plain-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&plain).unwrap();
        assert!(git_dir(&path_to_string(&plain.canonicalize().unwrap()).unwrap()).is_err());
        let _ = fs::remove_dir_all(&plain);
    }
}
