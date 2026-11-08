//! P6-04 Action 2：git transport（core 独占；参数 argv＋`--`、pathspec/
//! revision 校验、相同 repo mutating 操作串行化、read timeout 不留持锁进程
//! ——与 `src-tauri/src/git.rs` 同源语义 verbatim 上收；不在 Swift 直接调用
//! `git`）。
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 带路径的写命令按字面解释路径：`a*` 只指名叫 `a*` 的文件，放弃/暂存不会扩大
/// 到 `ab`（与 diff 的 `--literal-pathspecs` 一致）。只给这几个子命令：`stash -u`
/// 等内部依赖 pathspec 魔法，全局打开会让未跟踪文件留在工作区。
fn literal_pathspecs(args: &[&str]) -> Option<(&'static str, &'static str)> {
    const PATH_WRITES: [&str; 5] = ["add", "restore", "reset", "checkout", "rm"];
    args.first()
        .filter(|command| PATH_WRITES.contains(command))
        .map(|_| ("GIT_LITERAL_PATHSPECS", "1"))
}

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

    /// 进程内共用的一张锁表（2026-11-09 E 批 #14）：宿主原先每个请求
    /// `default()` 一张新表，同一仓库的两个写请求互不排队，锁形同虚设。
    pub fn shared() -> Arc<RepoLocks> {
        static SHARED: std::sync::OnceLock<Arc<RepoLocks>> = std::sync::OnceLock::new();
        SHARED.get_or_init(Default::default).clone()
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

/// 等待子进程并收集输出（run_git/run_git_network 共用；read timeout kill
/// child——不留持锁进程）。stdout/stderr 由两个线程边跑边读：管道缓冲约
/// 64KB，等退出后再读会让大输出的 git 写满管道、永远不退出。
fn collect_output(mut child: std::process::Child, timeout: Duration) -> Result<GitOutput, String> {
    fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(GitOutput {
                    stdout: stdout.join().unwrap_or_default(),
                    stderr: stderr.join().unwrap_or_default(),
                    success: status.success(),
                });
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // 不 join：孙进程可能仍握着管道；读线程随管道关闭自行结束。
                    return Err("git command timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// 运行 git 并把 `input` 写进它的标准输入（提交说明走 `-F -`：多行说明不经
/// argv，也就不受 validate_git_arg 的换行限制）。其余同 [`run_git`]。
pub fn run_git_with_input(
    repo_root: &Path,
    args: &[&str],
    input: &str,
    timeout: Duration,
    serial: Option<&Arc<Mutex<()>>>,
) -> Result<GitOutput, String> {
    for arg in args {
        validate_git_arg(arg)?;
    }
    if input.contains('\0') {
        return Err("git input contains NUL".to_string());
    }
    let run = || -> Result<GitOutput, String> {
        let mut child = Command::new("git")
            .args(args)
            .envs(literal_pathspecs(args))
            .current_dir(repo_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run git: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let input = input.to_string();
            // 写端在线程里：git 不读 stdin 时不会把这里卡住；写完 drop 即 EOF。
            std::thread::spawn(move || {
                use std::io::Write;
                let _ = stdin.write_all(input.as_bytes());
            });
        }
        collect_output(child, timeout)
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
        let child = Command::new("git")
            .args(args)
            .envs(literal_pathspecs(args))
            .current_dir(repo_root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run git: {e}"))?;
        collect_output(child, timeout)
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

/// 网络命令（legacy `run_git_network` 的 core 形状）：与 `run_git` 同一条
/// collect 循环，外加防挂死网络环境——`GIT_TERMINAL_PROMPT=0` 管 HTTP(S)，
/// `ssh -oBatchMode=yes` 管 SSH（口令/host key 确认都会失败而不是挂住）；
/// **只在用户没设过 `GIT_SSH_COMMAND` 时补默认值**（有人靠它指定专用私钥或
/// 跳板机，覆盖掉会把「能同步」变成「不能同步」——补默认，不抢配置权）。
/// legacy 的同步 run_git_network 无超时；core 取 `NETWORK_TIMEOUT` 并入 doc。
pub fn run_git_network(
    repo_root: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<GitOutput, String> {
    for arg in args {
        validate_git_arg(arg)?;
    }
    let mut command = Command::new("git");
    command
        .args(args)
        .envs(literal_pathspecs(args))
        .current_dir(repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
    let child = command
        .spawn()
        .map_err(|e| format!("Failed to run git: {e}"))?;
    collect_output(child, timeout)
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
