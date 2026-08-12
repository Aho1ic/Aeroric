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
