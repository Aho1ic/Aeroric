use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};

/// Validate that the renderer-provided project path resolves to a directory.
pub(super) fn validate_project_path(project_path: &str) -> Result<(), String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    if !path.exists() {
        return Err("Project path does not exist".to_string());
    }

    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project path: {}", error))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(())
}

/// Execute git synchronously. Command façades remain in the parent module.
pub(super) fn run_git<S: AsRef<std::ffi::OsStr>>(
    project_path: &str,
    args: &[S],
) -> Result<Output, String> {
    validate_project_path(project_path)?;

    let mut command = std::process::Command::new("git");
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(args)
        .current_dir(project_path)
        .envs(crate::app_settings::get_login_shell_env().iter().cloned())
        .output()
        .map_err(|error| error.to_string())
}

/// 让走网络的 git 子进程**失败而不是挂住**。
///
/// GUI 里没有可交互的 stdin。一个要凭据的远端会让 git 停在「Username for ...」上等一个
/// 永远不会来的回车 —— 对用户来说这不是提示,是同步永久卡住,而且看不出卡在哪。定时
/// 触发的后台同步尤其糟:每一轮都留一个挂住的进程。
///
/// 两条都要:`GIT_TERMINAL_PROMPT=0` 管 HTTP(S),`ssh -oBatchMode=yes` 管 SSH
/// (口令、以及首次连接的 host key 确认)。少任何一条都还剩一条挂住的路。
///
/// **只在用户没有自己设过的时候才设** `GIT_SSH_COMMAND`:有人靠它指定专用私钥或跳板机,
/// 覆盖掉会把「能同步」变成「不能同步」。这里要的是补一个默认值,不是抢走配置权。
fn configure_git_network_env<F>(existing: &[(String, String)], mut set: F)
where
    F: FnMut(&str, &str),
{
    set("GIT_TERMINAL_PROMPT", "0");
    if !existing.iter().any(|(key, _)| key == "GIT_SSH_COMMAND") {
        set("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
}

/// `run_git` 的网络版:同样同步执行,但不会因为等凭据而挂住。
///
/// `cwd` 允许是 clone 的**父目录**,所以这里不走 `validate_project_path`(它要求路径
/// 已存在且是目录 —— 对父目录成立,但语义上这一层校验由调用方按自己的场景做,
/// `git_clone` 用 `validate_clone_target`,其余用 `validate_project_path`)。
pub(super) fn run_git_network<S: AsRef<std::ffi::OsStr>>(
    cwd: &str,
    args: &[S],
) -> Result<Output, String> {
    let env = crate::app_settings::get_login_shell_env();
    let mut command = std::process::Command::new("git");
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().cloned())
        .stdin(Stdio::null());
    configure_git_network_env(env, |key, value| {
        command.env(key, value);
    });
    command.output().map_err(|error| error.to_string())
}

async fn read_pipe_to_end<R: AsyncRead + Unpin>(
    mut pipe: R,
    stream_name: &str,
) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    pipe.read_to_end(&mut data)
        .await
        .map_err(|error| format!("Failed to read git {}: {}", stream_name, error))?;
    Ok(data)
}

/// Execute git with a deadline and terminate the child process on timeout.
pub(super) async fn run_git_with_timeout(
    project_path: String,
    args: Vec<String>,
    timeout: Duration,
) -> Result<Output, String> {
    validate_project_path(&project_path)?;

    let mut command = tokio::process::Command::new("git");
    crate::subprocess::configure_background_tokio_command(&mut command);
    let mut child = command
        .args(&args)
        .current_dir(&project_path)
        .envs(crate::app_settings::get_login_shell_env().iter().cloned())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture git stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture git stderr".to_string())?;
    let stdout_task = tokio::spawn(read_pipe_to_end(stdout, "stdout"));
    let stderr_task = tokio::spawn(read_pipe_to_end(stderr, "stderr"));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => result.map_err(|error| error.to_string())?,
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            stdout_task.abort();
            stderr_task.abort();
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            return Err(format!("Git 命令执行超时（{}秒）", timeout.as_secs()));
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|error| format!("Git stdout task failed: {}", error))??;
    let stderr = stderr_task
        .await
        .map_err(|error| format!("Git stderr task failed: {}", error))??;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

pub(super) fn run_git_check<S: AsRef<std::ffi::OsStr>>(
    project_path: &str,
    args: &[S],
) -> Result<(), String> {
    let output = run_git(project_path, args)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub(super) fn git_command_error(output: &Output, fallback: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = format!("{}{}", stderr, stdout).trim().to_string();
    if message.is_empty() {
        fallback.to_string()
    } else {
        message
    }
}
