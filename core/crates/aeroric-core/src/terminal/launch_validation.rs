//! Agent launch preflight validation and working directory resolution (P8-05 slice 55).
//!
//! Single core owner for validating external agent command launchability before task
//! execution, preserving background execution configuration, environment precedence,
//! timeout interactive-loop acceptance, and dedicated DSH NotFound guidance.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::settings::schema::{AgentFamily, AgentLaunchSpec};

pub const AGENT_LAUNCH_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(2);
pub const AGENT_LAUNCH_PREFLIGHT_POLL: Duration = Duration::from_millis(25);

pub fn stable_agent_spawn_cwd() -> PathBuf {
    crate::settings::home_dir()
        .filter(|path| path.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn agent_process_cwd(project_path: &str, is_codex: bool) -> PathBuf {
    if is_codex {
        stable_agent_spawn_cwd()
    } else {
        PathBuf::from(project_path)
    }
}

pub fn preflight_agent_launch_spec(
    launch: &AgentLaunchSpec,
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<(), String> {
    preflight_agent_launch_spec_with_env(
        launch,
        cwd,
        timeout,
        crate::settings::get_login_shell_env(),
    )
}

pub fn preflight_agent_launch_spec_with_env(
    launch: &AgentLaunchSpec,
    cwd: Option<&Path>,
    timeout: Duration,
    login_shell_env: &[(String, String)],
) -> Result<(), String> {
    let mut command = Command::new(&launch.program);
    crate::runtime::subprocess::configure_background_command(&mut command);
    command
        .args(&launch.args)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    for (key, value) in login_shell_env {
        command.env(key, value);
    }
    for (key, value) in &launch.extra_env {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|error| {
            if launch.family == AgentFamily::Dsh
                && error.kind() == std::io::ErrorKind::NotFound
            {
                "DeepSeek Harness is not installed or not found in PATH. Configure dsh_path with the dsh executable or the DeepSeek Harness source directory, then run `pnpm install` and `pnpm run build`.".to_string()
            } else {
                format!("Failed to launch `{}`: {error}", launch.program)
            }
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "Agent command `{}` exited during validation with {status}",
                    launch.program
                ));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(AGENT_LAUNCH_PREFLIGHT_POLL);
            }
            Ok(None) => {
                let _ = crate::runtime::subprocess::terminate_process_tree(&mut child);
                let _ = child.wait();
                // Some custom wrappers do not implement --version and enter
                // their normal interactive loop instead. Reaching that loop
                // still proves that the configured program can be executed.
                return Ok(());
            }
            Err(error) => {
                let _ = crate::runtime::subprocess::terminate_process_tree(&mut child);
                let _ = child.wait();
                return Err(format!(
                    "Failed to validate agent command `{}`: {error}",
                    launch.program
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_process_cwd_avoids_project_root() {
        let cwd = agent_process_cwd("/tmp/example-project", true);
        assert_ne!(cwd, PathBuf::from("/tmp/example-project"));
    }

    #[test]
    fn non_codex_process_cwd_preserves_project_root() {
        let cwd = agent_process_cwd("/tmp/example-project", false);
        assert_eq!(cwd, PathBuf::from("/tmp/example-project"));
    }
}
