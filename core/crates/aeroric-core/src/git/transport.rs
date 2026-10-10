//! P6-04 Action 2：git transport（core 独占；参数 argv＋`--`、pathspec/
//! revision 校验、相同 repo mutating 操作串行化、read timeout 不留持锁进程
//! ——与 `src-tauri/src/git.rs` 同源语义 verbatim 上收；不在 Swift 直接调用
//! `git`）。
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// read timeout（不留下持锁进程：超时 kill child）。
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// 网络命令 timeout（fetch/pull/push——不会因为等凭据而挂住的策略由调用方
/// 凭据助手承载）。
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);
/// stash timeout（legacy `git_stash_push`/`git_stash_apply` 用 20s；比 `READ_TIMEOUT`
/// 宽——大仓库的 stash 可能超过 5s）。
pub const STASH_TIMEOUT: Duration = Duration::from_secs(20);
/// `git_stash_drop` 的 timeout（legacy 给它 10s，与 push/apply 的 20s **不是**同一个值；
/// 合并成一个常量会静默改掉其中一边）。
pub const STASH_DROP_TIMEOUT: Duration = Duration::from_secs(10);
/// discard 家族的 timeout（legacy `run_git_check` 无超时；core 必须给——
/// `restore`/`rm` 本地即时，取 10s 级别并独立成常量）。
pub const DISCARD_TIMEOUT: Duration = Duration::from_secs(10);
/// `git_file_diff` 主命令与 untracked 回落的 timeout（legacy 两处都用
/// `Duration::from_secs(10)`）。
pub const FILE_DIFF_TIMEOUT: Duration = Duration::from_secs(10);
/// `git_stage_files`/`git_unstage_files` 主命令的 timeout（legacy 用
/// `Duration::from_secs(10)`——比单文件 stage/unstage 的 `READ_TIMEOUT` 宽）。
pub const STAGE_FILES_TIMEOUT: Duration = Duration::from_secs(10);
/// `git_unstage_files` 的 timeout（legacy 用 `Duration::from_secs(10)`；与 stage_files
/// 同值但不是同一个死线——合并常量会让改一边静默改掉另一边）。
pub const UNSTAGE_FILES_TIMEOUT: Duration = Duration::from_secs(10);
/// `git_stash_list` 的 timeout（legacy 用 `Duration::from_secs(10)`）。
pub const STASH_LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// `git_stash_diff` 的 timeout（legacy 用 `Duration::from_secs(10)`；与 list 同值但
/// 不是同一个死线——合并常量会让改一边静默改掉另一边）。
pub const STASH_DIFF_TIMEOUT: Duration = Duration::from_secs(10);
/// blame timeout（legacy `git_blame_file` 用 `Duration::from_secs(10)`）。
pub const BLAME_TIMEOUT: Duration = Duration::from_secs(10);
/// branch-graph timeout（legacy `git_branch_graph` 用 `Duration::from_secs(10)`；
/// 与 blame 恰好同值但**不是**同一个死线——合并常量会让改一边静默改掉另一边）。
pub const BRANCH_GRAPH_TIMEOUT: Duration = Duration::from_secs(10);

/// mutating 操作串行化锁（相同 repo 串行——并发 mutating 会撞 index.lock）。
#[derive(Default)]
pub struct RepoLocks {
    locks: Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>,
}

impl RepoLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 同一 repo 根的 mutating 操作共用一把锁（路径 canonicalize 后为键）。
    pub fn lock_for(&self, repo_root: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().expect("repo locks");
        map.entry(repo_root.to_string()).or_default().clone()
    }
}

/// git 参数校验：pathspec/revision 不得以 `-` 开头（选项注入守卫；换行/
/// NUL 拒绝）。参数以 argv 传递（不经 shell）＋`--` 分隔——`-` 开头的文件名
/// 不成为选项。
pub fn validate_git_arg(arg: &str) -> Result<(), String> {
    if arg.contains('\0') {
        return Err("git argument contains NUL".to_string());
    }
    if arg.contains('\n') {
        return Err("git argument contains newline".to_string());
    }
    Ok(())
}

/// batch 校验。
pub fn validate_git_args(args: &[String]) -> Result<(), String> {
    for arg in args {
        validate_git_arg(arg)?;
    }
    Ok(())
}

/// pathspec 校验：非空、不以 `-` 开头（选项注入守卫；`--` 分隔后仍显式拒绝
/// 以免双保险失效）。
pub fn validate_pathspec(pathspec: &str) -> Result<(), String> {
    validate_git_arg(pathspec)?;
    if pathspec.is_empty() {
        return Err("pathspec is empty".to_string());
    }
    if pathspec.starts_with('-') {
        return Err("pathspec must not start with '-'".to_string());
    }
    Ok(())
}

/// revision 校验（非空、不以 `-` 开头）。
pub fn validate_revision(revision: &str) -> Result<(), String> {
    validate_git_arg(revision)?;
    if revision.is_empty() {
        return Err("revision is empty".to_string());
    }
    if revision.starts_with('-') {
        return Err("revision must not start with '-'".to_string());
    }
    Ok(())
}

/// git 执行结果。
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl GitOutput {
    pub fn combined(&self) -> String {
        format!("{}{}", self.stderr, self.stdout).trim().to_string()
    }
}

/// 运行 git（业务主体；read timeout kill child——不留持锁进程；mutating
/// 操作传 `serial: Some(&lock)` 串行化）。
pub fn run_git(
    repo_root: &Path,
    args: &[&str],
    timeout: Duration,
    serial: Option<&Arc<Mutex<()>>>,
) -> Result<GitOutput, String> {
    // 守卫：参数以 argv 传递（不经 shell），但 arg 内容仍校验 NUL/换行。
    for arg in args {
        validate_git_arg(arg)?;
    }
    let run = || -> Result<GitOutput, String> {
        let mut child = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run git: {e}"))?;
        // read timeout：超时 kill child（不留持锁进程）。
        let deadline = std::time::Instant::now() + timeout;
        loop {
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
                    return Ok(GitOutput {
                        stdout,
                        stderr,
                        success: status.success(),
                    });
                }
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("git command timed out".to_string());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    };
    match serial {
        Some(lock) => {
            let guard = lock.lock().map_err(|e| e.to_string())?;
            let result = run();
            drop(guard);
            result
        }
        None => run(),
    }
}

/// git 命令错误（combined 输出即错误文案）。
pub fn git_command_error(output: &GitOutput, fallback: &str) -> String {
    let message = output.combined();
    if message.is_empty() {
        fallback.to_string()
    } else {
        message
    }
}
