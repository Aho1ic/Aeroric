use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

const WSL_PROGRAM: &str = "wsl.exe";
const WSL_OUTPUT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslStatus {
    pub supported: bool,
    pub installed: bool,
    pub distribution_count: usize,
    pub default_distribution: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslDistribution {
    pub name: String,
    pub state: String,
    pub version: Option<u8>,
    pub is_default: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslDistributionProbe {
    pub distribution: String,
    pub state: String,
    pub version: Option<u8>,
    pub home: String,
    pub shell: String,
    pub user: String,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslEnvironment {
    pub distribution: String,
    pub home: String,
    pub shell: String,
    pub path: String,
    pub variables: BTreeMap<String, String>,
    /// 名称命中敏感关键词的变量，前端默认遮蔽其值。
    pub sensitive_names: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslAgentStatus {
    pub agent: String,
    pub available: bool,
    pub executable_path: String,
    pub config_path: String,
    pub config_exists: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslDistributionSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_override: Option<String>,
    #[serde(default)]
    pub agent_paths: BTreeMap<String, String>,
    #[serde(default)]
    pub agent_config_paths: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_distribution: Option<String>,
    #[serde(default)]
    pub distributions: BTreeMap<String, WslDistributionSettings>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WslCommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

fn wsl_settings_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("wsl-settings.json"))
}

fn wsl_global_config_path() -> Result<PathBuf, String> {
    let home = crate::platform::home_dir()
        .ok_or_else(|| "Cannot find Windows home directory".to_string())?;
    Ok(home.join(".wslconfig"))
}

fn is_supported_platform() -> bool {
    cfg!(target_os = "windows")
}

pub(crate) fn validate_distribution_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed != name
        || trimmed.len() > 128
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\0' | '\r' | '\n'))
    {
        return Err("Invalid WSL distribution name".to_string());
    }
    Ok(trimmed)
}

pub(crate) fn validate_linux_absolute_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if !trimmed.starts_with('/') || trimmed.contains('\0') {
        return Err("WSL path must be an absolute Linux path".to_string());
    }
    if trimmed
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        return Err("WSL path cannot contain . or .. components".to_string());
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

fn validate_agent_id(agent: &str) -> Result<&str, String> {
    let trimmed = agent.trim();
    if trimmed.is_empty()
        || trimmed != agent
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("Invalid WSL Agent ID".to_string());
    }
    Ok(trimmed)
}

pub(crate) fn decode_wsl_output(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let likely_utf16 = bytes.starts_with(&[0xff, 0xfe])
        || (bytes.len() >= 4
            && bytes.len().is_multiple_of(2)
            && bytes
                .chunks_exact(2)
                .take(64)
                .filter(|pair| pair[1] == 0)
                .count()
                > bytes.chunks_exact(2).take(64).count() / 2);
    if likely_utf16 {
        let body = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        let units = body
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

pub(crate) fn parse_distribution_list(bytes: &[u8]) -> Vec<WslDistribution> {
    let raw = decode_wsl_output(bytes).replace('\0', "");
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty()
                || line.to_ascii_lowercase().starts_with("name ")
                || line
                    .to_ascii_lowercase()
                    .contains("windows subsystem for linux")
            {
                return None;
            }
            let is_default = line.starts_with('*');
            let fields = line
                .trim_start_matches('*')
                .split_whitespace()
                .collect::<Vec<_>>();
            if fields.is_empty() {
                return None;
            }
            let version = fields.last().and_then(|value| value.parse::<u8>().ok());
            let state_index = version.map(|_| fields.len().saturating_sub(2));
            let state = state_index
                .and_then(|index| fields.get(index))
                .copied()
                .unwrap_or("Unknown")
                .to_string();
            let name_end = state_index.unwrap_or(fields.len());
            let name = fields[..name_end].join(" ");
            if validate_distribution_name(&name).is_err() {
                return None;
            }
            Some(WslDistribution {
                name,
                state,
                version,
                is_default,
            })
        })
        .collect()
}

pub(crate) fn parse_env_nul(bytes: &[u8]) -> BTreeMap<String, String> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let text = std::str::from_utf8(entry).ok()?;
            let (key, value) = text.split_once('=')?;
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

pub(crate) fn is_sensitive_environment_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "COOKIE", "AUTH", "KEY"]
        .iter()
        .any(|needle| upper.contains(needle))
}

pub(crate) fn wsl_exec_spec(distribution: &str, args: &[String]) -> Result<WslCommandSpec, String> {
    validate_distribution_name(distribution)?;
    if args.is_empty() || args.iter().any(|arg| arg.contains('\0')) {
        return Err("Invalid WSL command arguments".to_string());
    }
    Ok(WslCommandSpec {
        program: WSL_PROGRAM.to_string(),
        args: std::iter::once("--distribution".to_string())
            .chain(std::iter::once(distribution.to_string()))
            .chain(std::iter::once("--exec".to_string()))
            .chain(args.iter().cloned())
            .collect(),
    })
}

fn command_from_spec(spec: WslCommandSpec) -> Command {
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    crate::subprocess::configure_background_command(&mut command);
    command
}

pub(crate) fn std_wsl_exec_command(distribution: &str, args: &[String]) -> Result<Command, String> {
    Ok(command_from_spec(wsl_exec_spec(distribution, args)?))
}

pub(crate) fn std_wsl_shell_command(distribution: &str, shell_command: String) -> Command {
    let spec = WslCommandSpec {
        program: WSL_PROGRAM.to_string(),
        args: vec![
            "--distribution".to_string(),
            distribution.to_string(),
            "--exec".to_string(),
            "/bin/sh".to_string(),
            "-lc".to_string(),
            shell_command,
        ],
    };
    command_from_spec(spec)
}

fn try_std_wsl_shell_command(distribution: &str, shell_command: String) -> Result<Command, String> {
    std_wsl_exec_command(
        distribution,
        &["/bin/sh".to_string(), "-lc".to_string(), shell_command],
    )
}

fn pty_command_from_spec(spec: WslCommandSpec) -> CommandBuilder {
    let mut command = CommandBuilder::new(spec.program);
    for arg in spec.args {
        command.arg(arg);
    }
    crate::pty::setup_env(&mut command);
    command
}

fn run_command_output(mut command: Command, stdin: Option<&[u8]>) -> Result<Output, String> {
    if stdin.is_none() {
        return command.output().map_err(|error| error.to_string());
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(data) = stdin {
        child
            .stdin
            .take()
            .ok_or_else(|| "Failed to open WSL stdin".to_string())?
            .write_all(data)
            .map_err(|error| error.to_string())?;
    }
    child.wait_with_output().map_err(|error| error.to_string())
}

pub(crate) fn run_wsl_output(
    distribution: &str,
    args: &[String],
    stdin: Option<&[u8]>,
) -> Result<Output, String> {
    let _ = WSL_OUTPUT_TIMEOUT;
    let output = run_command_output(std_wsl_exec_command(distribution, args)?, stdin)?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = decode_wsl_output(&output.stderr).trim().to_string();
        let stdout = decode_wsl_output(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() {
            if stdout.is_empty() {
                format!("WSL command exited with {}", output.status)
            } else {
                stdout
            }
        } else {
            stderr
        })
    }
}

pub(crate) fn run_wsl_shell_output(
    distribution: &str,
    shell_command: String,
    stdin: Option<&[u8]>,
) -> Result<Output, String> {
    let output = run_command_output(
        try_std_wsl_shell_command(distribution, shell_command)?,
        stdin,
    )?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = decode_wsl_output(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "WSL command failed".to_string()
        } else {
            stderr
        })
    }
}

fn list_distributions_blocking() -> Result<Vec<WslDistribution>, String> {
    if !is_supported_platform() {
        return Ok(Vec::new());
    }
    let mut command = Command::new(WSL_PROGRAM);
    command.args(["--list", "--verbose"]);
    crate::subprocess::configure_background_command(&mut command);
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(decode_wsl_output(&output.stderr).trim().to_string());
    }
    Ok(parse_distribution_list(&output.stdout))
}

fn ensure_distribution_available(distribution: &str) -> Result<WslDistribution, String> {
    validate_distribution_name(distribution)?;
    list_distributions_blocking()?
        .into_iter()
        .find(|item| item.name == distribution)
        .ok_or_else(|| format!("WSL distribution is not installed: {distribution}"))
}

fn load_wsl_settings_blocking() -> Result<WslSettings, String> {
    let path = wsl_settings_path()?;
    if !path.exists() {
        return Ok(WslSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn distribution_settings(settings: &WslSettings, distribution: &str) -> WslDistributionSettings {
    settings
        .distributions
        .get(distribution)
        .cloned()
        .unwrap_or_default()
}

fn parse_probe_output(
    distribution: &WslDistribution,
    output: &[u8],
) -> Result<WslDistributionProbe, String> {
    let fields = output
        .split(|byte| *byte == 0)
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect::<Vec<_>>();
    if fields.len() < 5 {
        return Err("Failed to parse WSL distribution probe".to_string());
    }
    Ok(WslDistributionProbe {
        distribution: distribution.name.clone(),
        state: distribution.state.clone(),
        version: distribution.version,
        user: fields[0].clone(),
        home: fields[1].clone(),
        shell: fields[2].clone(),
        claude_path: (!fields[3].is_empty()).then(|| fields[3].clone()),
        codex_path: (!fields[4].is_empty()).then(|| fields[4].clone()),
    })
}

fn probe_distribution_blocking(distribution: &str) -> Result<WslDistributionProbe, String> {
    let distro = ensure_distribution_available(distribution)?;
    let script = r#"user=$(id -un)
home=${HOME:-$(getent passwd "$user" 2>/dev/null | cut -d: -f6)}
shell=$(getent passwd "$user" 2>/dev/null | cut -d: -f7)
[ -n "$shell" ] || shell=${SHELL:-/bin/sh}
claude=$(command -v claude 2>/dev/null || true)
codex=$(command -v codex 2>/dev/null || true)
printf '%s\0%s\0%s\0%s\0%s\0' "$user" "$home" "$shell" "$claude" "$codex""#;
    let output = run_wsl_exec_output_shell(distribution, script)?;
    parse_probe_output(&distro, &output.stdout)
}

fn run_wsl_exec_output_shell(distribution: &str, script: &str) -> Result<Output, String> {
    run_wsl_output(
        distribution,
        &["/bin/sh".to_string(), "-lc".to_string(), script.to_string()],
        None,
    )
}

fn read_environment_blocking(distribution: &str) -> Result<WslEnvironment, String> {
    let probe = probe_distribution_blocking(distribution)?;
    let settings = load_wsl_settings_blocking()?;
    let distro_settings = distribution_settings(&settings, distribution);
    let shell = distro_settings
        .shell_override
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(probe.shell);
    let output = run_wsl_output(distribution, &login_shell_args(&shell, "env -0"), None)?;
    let variables = parse_env_nul(&output.stdout);
    let sensitive_names = variables
        .keys()
        .filter(|name| is_sensitive_environment_name(name))
        .cloned()
        .collect();
    Ok(WslEnvironment {
        distribution: distribution.to_string(),
        home: variables
            .get("HOME")
            .cloned()
            .unwrap_or_else(|| probe.home.clone()),
        shell,
        path: variables.get("PATH").cloned().unwrap_or_default(),
        variables,
        sensitive_names,
    })
}

fn default_agent_config_path(agent: &str, home: &str) -> Option<String> {
    match agent {
        "claude" | "claude_gpt55" => Some(format!("{home}/.claude/settings.json")),
        "codex" => Some(format!("{home}/.codex/config.toml")),
        _ => None,
    }
}

fn resolve_agent_paths(
    distribution: &str,
    agent: &str,
) -> Result<(String, String, WslDistributionProbe), String> {
    validate_agent_id(agent)?;
    let probe = probe_distribution_blocking(distribution)?;
    let settings = load_wsl_settings_blocking()?;
    let distro_settings = distribution_settings(&settings, distribution);
    let executable = distro_settings
        .agent_paths
        .get(agent)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| match agent {
            "claude" => probe.claude_path.clone(),
            "codex" | "claude_gpt55" => probe.codex_path.clone(),
            _ => None,
        })
        .unwrap_or_else(|| {
            if agent == "claude_gpt55" {
                "codex".to_string()
            } else {
                agent.to_string()
            }
        });
    let config_path = distro_settings
        .agent_config_paths
        .get(agent)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| default_agent_config_path(agent, &probe.home))
        .unwrap_or_default();
    Ok((executable, config_path, probe))
}

fn shell_word(value: &str) -> String {
    crate::ssh::shell_word_posix(value)
}

fn is_dsh_agent(agent: &str) -> bool {
    agent == "dsh"
}

fn dsh_permission_mode(permission_mode: &str) -> Option<&'static str> {
    match permission_mode {
        "ask" => Some("read-only"),
        "auto_edit" => Some("workspace-write"),
        "full_access" => Some("danger-full-access"),
        _ => None,
    }
}

fn agent_args(
    agent: &str,
    permission_mode: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Vec<String> {
    let agent = if matches!(agent, "codex" | "claude_gpt55") {
        "codex"
    } else {
        agent
    };
    let mut args = match agent {
        "claude" => match permission_mode {
            "ask" => vec!["--permission-mode".to_string(), "default".to_string()],
            "auto_edit" => vec!["--permission-mode".to_string(), "acceptEdits".to_string()],
            "full_access" => vec!["--dangerously-skip-permissions".to_string()],
            _ => Vec::new(),
        },
        "codex" => match permission_mode {
            "auto_edit" => vec![
                "--sandbox".to_string(),
                "workspace-write".to_string(),
                "-a".to_string(),
                "on-request".to_string(),
            ],
            "full_access" => vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    if matches!(agent, "codex" | "claude_gpt55") {
        if let Some(model) = selected_model {
            args.push("-m".to_string());
            args.push(model.to_string());
        }
        if let Some(effort) = reasoning_effort {
            args.push("-c".to_string());
            args.push(format!(
                "model_reasoning_effort={}",
                toml::Value::String(effort.to_string())
            ));
        }
        if speed == Some("fast") {
            args.push("-c".to_string());
            args.push("features.fast_mode=true".to_string());
            args.push("-c".to_string());
            args.push("service_tier=\"fast\"".to_string());
        }
    } else if !is_dsh_agent(agent) {
        if agent == "claude" {
            if let Some(model) = selected_model {
                args.push("--model".to_string());
                args.push(model.to_string());
            }
        }
        if let Some(effort) = reasoning_effort.filter(|effort| *effort != "ultracode") {
            args.push("--effort".to_string());
            args.push(effort.to_string());
        }
        if speed == Some("fast") {
            args.push("--settings".to_string());
            args.push(r#"{"fastMode":true}"#.to_string());
        }
    }

    args
}

fn build_agent_command(
    distribution: &str,
    agent: &str,
    permission_mode: &str,
    linux_project_path: &str,
    prompt: Option<&str>,
    session_id: Option<&str>,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Result<WslCommandSpec, String> {
    let project_path = validate_linux_absolute_path(linux_project_path)?;
    if is_dsh_agent(agent) && session_id.is_some() {
        return Err("DeepSeek Harness WSL sessions do not support native resume".to_string());
    }
    let (executable, _, probe) = resolve_agent_paths(distribution, agent)?;
    let args = agent_invocation_args(
        agent,
        permission_mode,
        prompt,
        session_id,
        selected_model,
        reasoning_effort,
        speed,
    );
    let shell = resolve_login_shell(distribution, probe.shell)?;
    wsl_exec_spec(
        distribution,
        &login_shell_args(
            &shell,
            &project_shell_command(
                &project_path,
                &executable,
                &args,
                selected_model,
                agent,
                permission_mode,
            ),
        ),
    )
}

/// Agent 参数：resume 优先，其次首轮 prompt；顺序与 SSH 远程任务保持一致。
fn agent_invocation_args(
    agent: &str,
    permission_mode: &str,
    prompt: Option<&str>,
    session_id: Option<&str>,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Vec<String> {
    let mut args = agent_args(
        agent,
        permission_mode,
        selected_model,
        reasoning_effort,
        speed,
    );
    let codex_like = matches!(agent, "codex" | "claude_gpt55");
    if let Some(session_id) = session_id {
        if codex_like {
            args.push("resume".to_string());
        } else {
            args.push("--resume".to_string());
        }
        args.push(session_id.to_string());
    } else if let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) {
        if is_dsh_agent(agent) {
            args.push("--profile".to_string());
            args.push("headless".to_string());
            args.push("--".to_string());
        } else if codex_like {
            args.push("--".to_string());
        }
        args.push(prompt.to_string());
    }
    args
}

/// `cd 项目目录 && exec <agent>`：所有参数走 POSIX quoting，避免注入。
fn project_shell_command(
    project_path: &str,
    executable: &str,
    args: &[String],
    selected_model: Option<&str>,
    agent: &str,
    permission_mode: &str,
) -> String {
    let command = std::iter::once(shell_word(executable))
        .chain(args.iter().map(|arg| shell_word(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut environment = Vec::new();
    if let Some(model) = selected_model {
        environment.push(format!("AERORIC_AGENT_MODEL={}", shell_word(model)));
    }
    if is_dsh_agent(agent) {
        if let Some(mode) = dsh_permission_mode(permission_mode) {
            environment.push(format!("DSH_PERMISSION_MODE={}", shell_word(mode)));
        }
        environment.push("DSH_TELEMETRY_DISABLED=1".to_string());
    }
    let command = if environment.is_empty() {
        command
    } else {
        format!("env {} {}", environment.join(" "), command)
    };
    format!(
        "cd -- {} && exec {}",
        crate::ssh::shell_quote_posix(project_path),
        command
    )
}

fn login_shell_args(shell: &str, shell_command: &str) -> Vec<String> {
    vec![
        shell.to_string(),
        "-l".to_string(),
        "-c".to_string(),
        shell_command.to_string(),
    ]
}

/// 登录 Shell 解析顺序：发行版覆盖 → 探测到的 passwd shell。
fn resolve_login_shell(distribution: &str, probe_shell: String) -> Result<String, String> {
    Ok(load_wsl_settings_blocking()?
        .distributions
        .get(distribution)
        .and_then(|item| item.shell_override.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(probe_shell))
}

fn parse_wsl_agent_version(bytes: &[u8]) -> String {
    decode_wsl_output(bytes)
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '-')
        })
        .find(|token| {
            !token.is_empty()
                && token.chars().any(|ch| ch.is_ascii_digit())
                && token.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        })
        .unwrap_or_default()
        .to_string()
}

fn read_wsl_agent_version(distribution: &str, shell: &str, executable: &str) -> String {
    let executable = shell_word(executable);
    let command = format!("({executable} --version 2>/dev/null || {executable} -v 2>/dev/null)");
    run_wsl_output(distribution, &login_shell_args(shell, &command), None)
        .map(|output| parse_wsl_agent_version(&output.stdout))
        .unwrap_or_default()
}

fn wsl_agent_upgrade_command(agent: &str, executable: &str) -> Result<(String, String), String> {
    let (package, brew_name) = match agent {
        "claude" => ("@anthropic-ai/claude-code@latest", "claude-code"),
        "codex" => ("@openai/codex@latest", "codex"),
        _ => return Err(format!("Unsupported WSL Agent: {agent}")),
    };
    let executable = shell_word(executable);
    // 先复用 WSL 内已存在的 Homebrew 安装；否则使用 npm。Claude 的 standalone
    // 安装支持 `update`，这样不会因为 Windows 主机找不到 Linux npm 而升级错环境。
    let command = format!(
        "if command -v brew >/dev/null 2>&1 && (brew list --formula --versions {brew_name} >/dev/null 2>&1 || brew list --cask --versions {brew_name} >/dev/null 2>&1); then if brew list --formula --versions {brew_name} >/dev/null 2>&1; then brew upgrade --formula {brew_name}; else brew upgrade --cask {brew_name}; fi; elif [ \"{agent}\" = \"claude\" ] && {executable} update; then :; elif command -v npm >/dev/null 2>&1; then npm install -g {package}; else echo 'Neither Homebrew nor npm is available in WSL' >&2; exit 127; fi"
    );
    Ok(("wsl".to_string(), command))
}

fn upgrade_wsl_agent_versions_blocking(
    distribution: &str,
    agents: Vec<String>,
) -> Result<Vec<crate::app_settings::AgentUpgradeResult>, String> {
    if !is_supported_platform() {
        return Err("WSL is only available on Windows".to_string());
    }
    ensure_distribution_available(distribution)?;
    let mut requested = Vec::new();
    for agent in agents {
        if !matches!(agent.as_str(), "claude" | "codex") {
            return Err(format!("Unsupported WSL Agent: {agent}"));
        }
        if !requested.contains(&agent) {
            requested.push(agent);
        }
    }
    if requested.is_empty() {
        return Err("Select at least one WSL Agent to upgrade".to_string());
    }

    requested
        .into_iter()
        .map(|agent| {
            let (executable, _, probe) = resolve_agent_paths(distribution, &agent)?;
            let shell = resolve_login_shell(distribution, probe.shell)?;
            let previous_version = read_wsl_agent_version(distribution, &shell, &executable);
            let (channel, command) = wsl_agent_upgrade_command(&agent, &executable)?;
            let (success, message) =
                match run_wsl_output(distribution, &login_shell_args(&shell, &command), None) {
                    Ok(output) => {
                        let output = decode_wsl_output(&output.stdout).trim().to_string();
                        (
                            true,
                            if output.is_empty() {
                                "upgraded".to_string()
                            } else {
                                output
                            },
                        )
                    }
                    Err(error) => (false, error),
                };
            let current_version = read_wsl_agent_version(distribution, &shell, &executable);
            let verified = success && !current_version.is_empty();
            let channels = vec![crate::app_settings::AgentUpgradeChannel {
                channel: channel.clone(),
                success: verified,
                message: if verified {
                    "upgraded and verified".to_string()
                } else if success {
                    "upgrade command succeeded but the WSL Agent version could not be verified"
                        .to_string()
                } else {
                    message.clone()
                },
            }];
            Ok(crate::app_settings::AgentUpgradeResult {
                agent,
                success: verified,
                previous_version,
                current_version,
                message,
                channels,
                channel,
                managed: false,
                runtime_recovery: None,
            })
        })
        .collect()
}

fn spawn_wsl_exit_monitor(
    app: AppHandle,
    task_id: String,
    child_handle: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
) {
    tokio::task::spawn_blocking(move || loop {
        let exit_status = child_handle.lock().try_wait().ok().flatten();
        if let Some(status) = exit_status {
            let manager = app.state::<crate::TaskManager>();
            if !manager.remove_pty_handles_if_current(&task_id, &child_handle) {
                return;
            }
            let (cancelled, manually_completed) = {
                let result = (
                    manager.cancelled_tasks.lock().remove(&task_id),
                    manager.manually_completed_tasks.lock().remove(&task_id),
                );
                result
            };
            manager.wsl_active_ids.lock().remove(&task_id);
            if cancelled || manually_completed {
                return;
            }
            let payload = if status.success() {
                serde_json::json!({ "task_id": task_id, "status": "done" })
            } else {
                serde_json::json!({
                    "task_id": task_id,
                    "status": "failed",
                    "failure_reason": format!("WSL process exited with code {}", status.exit_code())
                })
            };
            let _ = app.emit("task-status", payload);
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    });
}

fn spawn_wsl_task_pty(
    app: AppHandle,
    task_manager: &crate::TaskManager,
    task_id: &str,
    spec: WslCommandSpec,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
    initial_prelude: Option<Vec<u8>>,
    initial_prompt: Option<(Vec<u8>, Vec<u8>)>,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(50),
            cols: cols.unwrap_or(220),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let child = pair
        .slave
        .spawn_command(pty_command_from_spec(spec))
        .map_err(|error| error.to_string())?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let child_handle =
        crate::pty::register_pty_handles(task_manager, task_id, pair.master, writer, child)?;
    task_manager
        .wsl_active_ids
        .lock()
        .insert(task_id.to_string());
    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "running" }),
    );
    let needs_initial_input = initial_prelude.is_some() || initial_prompt.is_some();
    let (startup_tx, startup_rx) = std::sync::mpsc::channel();
    let startup_generation = needs_initial_input.then(|| {
        crate::pty::register_initial_input_signal(task_manager, task_id, startup_tx.clone())
    });
    crate::pty::spawn_pty_reader(
        app.clone(),
        task_id.to_string(),
        crate::pty::OutputSink::Channel(on_output),
        crate::pty::PtyEmitMode::Batched {
            flush_interval: crate::pty::PTY_EMIT_FLUSH_INTERVAL,
            max_batch_bytes: crate::pty::PTY_EMIT_MAX_BATCH_BYTES,
        },
        reader,
        true,
        None,
        needs_initial_input.then_some(startup_tx),
        None,
        None,
    );
    if needs_initial_input {
        if let Some(writer) = task_manager.pty_writers.lock().get(task_id).cloned() {
            let signals = Arc::clone(&task_manager.initial_input_signals);
            let cleanup_id = task_id.to_string();
            let cleanup_generation =
                startup_generation.expect("initial input registration must exist");
            crate::pty::spawn_initial_input_injection(
                writer,
                initial_prelude,
                initial_prompt,
                startup_rx,
                Some(Box::new(move || {
                    crate::pty::clear_initial_input_signal_if_current(
                        &signals,
                        &cleanup_id,
                        cleanup_generation,
                    );
                })),
            );
        } else {
            crate::pty::cancel_initial_input_signal(task_manager, task_id);
        }
    }
    spawn_wsl_exit_monitor(app, task_id.to_string(), child_handle);
    Ok(())
}

#[tauri::command]
pub async fn get_wsl_status() -> Result<WslStatus, String> {
    tokio::task::spawn_blocking(|| {
        if !is_supported_platform() {
            return Ok(WslStatus {
                supported: false,
                installed: false,
                distribution_count: 0,
                default_distribution: None,
                error: None,
            });
        }
        match list_distributions_blocking() {
            Ok(distributions) => Ok(WslStatus {
                supported: true,
                installed: true,
                distribution_count: distributions.len(),
                default_distribution: distributions
                    .iter()
                    .find(|item| item.is_default)
                    .map(|item| item.name.clone()),
                error: None,
            }),
            Err(error) => Ok(WslStatus {
                supported: true,
                installed: false,
                distribution_count: 0,
                default_distribution: None,
                error: Some(error),
            }),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_wsl_distributions() -> Result<Vec<WslDistribution>, String> {
    tokio::task::spawn_blocking(list_distributions_blocking)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn probe_wsl_distribution(distribution: String) -> Result<WslDistributionProbe, String> {
    tokio::task::spawn_blocking(move || probe_distribution_blocking(&distribution))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_wsl_environment(distribution: String) -> Result<WslEnvironment, String> {
    tokio::task::spawn_blocking(move || read_environment_blocking(&distribution))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn load_wsl_settings() -> Result<WslSettings, String> {
    tokio::task::spawn_blocking(load_wsl_settings_blocking)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn save_wsl_settings(settings: WslSettings) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if let Some(default_distribution) = settings.default_distribution.as_deref() {
            ensure_distribution_available(default_distribution)?;
        }
        let installed = list_distributions_blocking()?
            .into_iter()
            .map(|item| item.name)
            .collect::<std::collections::HashSet<_>>();
        for (distribution, values) in &settings.distributions {
            validate_distribution_name(distribution)?;
            if !installed.contains(distribution) {
                return Err(format!("WSL distribution is not installed: {distribution}"));
            }
            if let Some(shell) = values.shell_override.as_deref() {
                if !shell.trim().is_empty() {
                    validate_linux_absolute_path(shell)?;
                }
            }
            for agent in values
                .agent_paths
                .keys()
                .chain(values.agent_config_paths.keys())
            {
                validate_agent_id(agent)?;
            }
        }
        crate::storage::ensure_aeroric_dirs()?;
        let raw = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
        crate::storage::atomic_write_private(&wsl_settings_path()?, &raw)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_wsl_config_file(
    distribution: Option<String>,
    kind: String,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || match kind.as_str() {
        "global" => {
            let path = wsl_global_config_path()?;
            if path.exists() {
                fs::read_to_string(path)
                    .map(Some)
                    .map_err(|error| error.to_string())
            } else {
                Ok(None)
            }
        }
        "wslConf" => {
            let distribution =
                distribution.ok_or_else(|| "Distribution is required".to_string())?;
            ensure_distribution_available(&distribution)?;
            let output = run_wsl_exec_output_shell(
                &distribution,
                "[ ! -f /etc/wsl.conf ] || cat -- /etc/wsl.conf",
            )?;
            let raw = decode_wsl_output(&output.stdout);
            Ok((!raw.is_empty()).then_some(raw))
        }
        _ => Err("Unknown WSL config kind".to_string()),
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn write_wsl_config_file(
    distribution: Option<String>,
    kind: String,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || match kind.as_str() {
        "global" => crate::storage::atomic_write_private(&wsl_global_config_path()?, &content),
        "wslConf" => {
            let distribution =
                distribution.ok_or_else(|| "Distribution is required".to_string())?;
            ensure_distribution_available(&distribution)?;
            run_wsl_output(
                &distribution,
                &[
                    "--user".to_string(),
                    "root".to_string(),
                    "--".to_string(),
                ],
                None,
            )
            .ok();
            let mut command = Command::new(WSL_PROGRAM);
            command.args([
                "--distribution",
                &distribution,
                "--user",
                "root",
                "--exec",
                "/bin/sh",
                "-lc",
                "tmp=$(mktemp /etc/.wsl.conf.aeroric.XXXXXX) && cat > \"$tmp\" && chmod 644 \"$tmp\" && mv -f -- \"$tmp\" /etc/wsl.conf",
            ]);
            crate::subprocess::configure_background_command(&mut command);
            let output = run_command_output(command, Some(content.as_bytes()))?;
            if output.status.success() {
                Ok(())
            } else {
                Err(decode_wsl_output(&output.stderr).trim().to_string())
            }
        }
        _ => Err("Unknown WSL config kind".to_string()),
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_wsl_agent_status(
    distribution: String,
    agent: String,
) -> Result<WslAgentStatus, String> {
    tokio::task::spawn_blocking(move || {
        ensure_distribution_available(&distribution)?;
        let (executable_path, config_path, _) = resolve_agent_paths(&distribution, &agent)?;
        let script = format!(
            "test -x {} || command -v {} >/dev/null 2>&1; executable=$?; test -f {}; config=$?; printf '%s\\0%s\\0' \"$executable\" \"$config\"",
            crate::ssh::shell_quote_posix(&executable_path),
            shell_word(&executable_path),
            crate::ssh::shell_quote_posix(&config_path)
        );
        let output = run_wsl_exec_output_shell(&distribution, &script)?;
        let fields = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        let available = fields.first().is_some_and(|value| *value == b"0");
        let config_exists = fields.get(1).is_some_and(|value| *value == b"0");
        Ok(WslAgentStatus {
            agent,
            available,
            executable_path,
            config_path,
            config_exists,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn upgrade_wsl_agent_versions(
    distribution: String,
    agents: Vec<String>,
) -> Result<Vec<crate::app_settings::AgentUpgradeResult>, String> {
    tokio::task::spawn_blocking(move || upgrade_wsl_agent_versions_blocking(&distribution, agents))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_wsl_agent_config(
    distribution: String,
    agent: String,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        ensure_distribution_available(&distribution)?;
        let (_, config_path, _) = resolve_agent_paths(&distribution, &agent)?;
        if config_path.is_empty() {
            return Ok(None);
        }
        let output = run_wsl_exec_output_shell(
            &distribution,
            &format!(
                "[ ! -f {path} ] || cat -- {path}",
                path = crate::ssh::shell_quote_posix(&config_path)
            ),
        )?;
        let raw = decode_wsl_output(&output.stdout);
        Ok((!raw.is_empty()).then_some(raw))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn write_wsl_agent_config(
    distribution: String,
    agent: String,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_distribution_available(&distribution)?;
        let (_, config_path, _) = resolve_agent_paths(&distribution, &agent)?;
        if config_path.is_empty() {
            return Err("No WSL config path is configured for this Agent".to_string());
        }
        let parent = config_path
            .rsplit_once('/')
            .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
            .ok_or_else(|| "Invalid WSL Agent config path".to_string())?;
        let command = format!(
            "mkdir -p -- {parent} && tmp=$(mktemp {parent}/.aeroric-agent.XXXXXX) && cat > \"$tmp\" && chmod 600 \"$tmp\" && mv -f -- \"$tmp\" {path}",
            parent = crate::ssh::shell_quote_posix(parent),
            path = crate::ssh::shell_quote_posix(&config_path)
        );
        run_wsl_shell_output(&distribution, command, Some(content.as_bytes())).map(|_| ())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn restart_wsl(task_manager: State<'_, crate::TaskManager>) -> Result<(), String> {
    if !task_manager.wsl_active_ids.lock().is_empty() {
        return Err("Close active WSL terminals and tasks before restarting WSL".to_string());
    }
    tokio::task::spawn_blocking(|| {
        if !is_supported_platform() {
            return Err("WSL is only available on Windows".to_string());
        }
        let mut command = Command::new(WSL_PROGRAM);
        command.arg("--shutdown");
        crate::subprocess::configure_background_command(&mut command);
        let output = command.output().map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(decode_wsl_output(&output.stderr).trim().to_string())
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn validate_wsl_project_path(
    distribution: String,
    linux_path: String,
) -> Result<WslDistributionProbe, String> {
    tokio::task::spawn_blocking(move || {
        ensure_distribution_available(&distribution)?;
        let linux_path = validate_linux_absolute_path(&linux_path)?;
        run_wsl_exec_output_shell(
            &distribution,
            &format!(
                "test -d {path} && test -r {path}",
                path = crate::ssh::shell_quote_posix(&linux_path)
            ),
        )?;
        probe_distribution_blocking(&distribution)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_wsl_shell(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
    distribution: String,
    linux_project_path: String,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_wsl_shell_id(&shell_id)?;
    ensure_distribution_available(&distribution)?;
    let project_path = validate_linux_absolute_path(&linux_project_path)?;
    let probe = probe_distribution_blocking(&distribution)?;
    let shell = resolve_login_shell(&distribution, probe.shell)?;
    let spec = wsl_exec_spec(
        &distribution,
        &login_shell_args(
            &shell,
            &format!(
                "cd -- {} && exec {} -l",
                crate::ssh::shell_quote_posix(&project_path),
                crate::ssh::shell_quote_posix(&shell)
            ),
        ),
    )?;
    if let Some(child) = task_manager.child_handles.lock().get(&shell_id).cloned() {
        let mut child = child.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);
    task_manager.wsl_active_ids.lock().remove(&shell_id);
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(120),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let child = pair
        .slave
        .spawn_command(pty_command_from_spec(spec))
        .map_err(|error| error.to_string())?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    crate::pty::register_pty_handles(&task_manager, &shell_id, pair.master, writer, child)?;
    task_manager.wsl_active_ids.lock().insert(shell_id.clone());
    let cleanup_app = app.clone();
    let cleanup_id = shell_id.clone();
    crate::pty::spawn_pty_reader(
        app,
        shell_id,
        crate::pty::OutputSink::Channel(on_output),
        crate::pty::PtyEmitMode::Immediate,
        reader,
        false,
        None,
        None,
        None,
        Some(Box::new(move || {
            let manager = cleanup_app.state::<crate::TaskManager>();
            manager.remove_pty_handles(&cleanup_id);
            manager.wsl_active_ids.lock().remove(&cleanup_id);
        })),
    );
    Ok(())
}

#[tauri::command]
pub async fn kill_wsl_shell(
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
) -> Result<(), String> {
    crate::pty::validate_wsl_shell_id(&shell_id)?;
    if let Some(child) = task_manager.child_handles.lock().get(&shell_id).cloned() {
        let mut child = child.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);
    task_manager.wsl_active_ids.lock().remove(&shell_id);
    Ok(())
}

#[tauri::command]
pub async fn run_wsl_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    distribution: String,
    linux_project_path: String,
    prompt: String,
    agent: String,
    permission_mode: String,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    speed: Option<String>,
    force_prompt_injection: Option<bool>,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    ensure_distribution_available(&distribution)?;
    let is_codex = matches!(agent.as_str(), "codex" | "claude_gpt55");
    let is_dsh = is_dsh_agent(&agent);
    let family = if is_dsh {
        crate::app_settings::AgentFamily::Dsh
    } else if is_codex {
        crate::app_settings::AgentFamily::Codex
    } else {
        crate::app_settings::AgentFamily::Claude
    };
    let selected_model = crate::pty::normalized_selected_model(selected_model.as_deref());
    let reasoning_effort = crate::pty::normalized_reasoning_effort_for(
        reasoning_effort.as_deref(),
        family,
        selected_model.as_deref(),
    )?;
    let speed = crate::pty::normalized_speed_for(speed.as_deref(), family)?;
    let force_prompt_injection =
        !is_dsh && crate::pty::should_force_prompt_injection(is_codex, force_prompt_injection);
    let uses_ultracode = !is_codex && reasoning_effort.as_deref() == Some("ultracode");
    let spec = build_agent_command(
        &distribution,
        &agent,
        &permission_mode,
        &linux_project_path,
        (!uses_ultracode && !force_prompt_injection).then_some(prompt.as_str()),
        None,
        selected_model.as_deref(),
        reasoning_effort.as_deref(),
        speed.as_deref(),
    )?;
    let initial_prelude = uses_ultracode.then(crate::pty::initial_ultracode_command);
    let initial_prompt = (uses_ultracode || force_prompt_injection)
        .then(|| crate::pty::initial_prompt_input_chunks(&prompt))
        .flatten();
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    let _ = crate::storage::truncate_task_terminal_history(&task_id);
    crate::remote::terminal_hub::hub().reset_for_truncate(&task_id);
    spawn_wsl_task_pty(
        app,
        &task_manager,
        &task_id,
        spec,
        cols,
        rows,
        on_output,
        initial_prelude,
        initial_prompt,
    )
}

#[tauri::command]
pub async fn resume_wsl_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    distribution: String,
    linux_project_path: String,
    agent: String,
    session_id: String,
    permission_mode: String,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    speed: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    ensure_distribution_available(&distribution)?;
    let is_codex = matches!(agent.as_str(), "codex" | "claude_gpt55");
    let selected_model = crate::pty::normalized_selected_model(selected_model.as_deref());
    let reasoning_effort = crate::pty::normalized_reasoning_effort(
        reasoning_effort.as_deref(),
        is_codex,
        selected_model.as_deref(),
    )?;
    let speed = crate::pty::normalized_speed(speed.as_deref())?;
    let uses_ultracode = !is_codex && reasoning_effort.as_deref() == Some("ultracode");
    let spec = build_agent_command(
        &distribution,
        &agent,
        &permission_mode,
        &linux_project_path,
        None,
        Some(&session_id),
        selected_model.as_deref(),
        reasoning_effort.as_deref(),
        speed.as_deref(),
    )?;
    let initial_prelude = uses_ultracode.then(crate::pty::initial_ultracode_command);
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    spawn_wsl_task_pty(
        app,
        &task_manager,
        &task_id,
        spec,
        cols,
        rows,
        on_output,
        initial_prelude,
        None,
    )
}

#[tauri::command]
pub async fn cancel_wsl_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().insert(task_id.clone());
    if let Some(child) = task_manager.child_handles.lock().get(&task_id).cloned() {
        let mut child = child.lock();
        let _ = child.kill();
        let _ = child.wait();
    } else {
        task_manager.cancelled_tasks.lock().remove(&task_id);
    }
    task_manager.remove_pty_handles(&task_id);
    task_manager.wsl_active_ids.lock().remove(&task_id);
    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "cancelled" }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_and_utf16_distribution_lists() {
        let utf8 = b"  NAME            STATE           VERSION\n* Ubuntu-24.04    Running         2\n  Debian          Stopped         1\n";
        let parsed = parse_distribution_list(utf8);
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].is_default);
        assert_eq!(parsed[0].name, "Ubuntu-24.04");
        assert_eq!(parsed[1].version, Some(1));

        let text = "  NAME      STATE      VERSION\r\n* Ubuntu    Running    2\r\n";
        let mut utf16 = vec![0xff, 0xfe];
        for unit in text.encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        let parsed = parse_distribution_list(&utf16);
        assert_eq!(parsed[0].name, "Ubuntu");
        assert_eq!(parsed[0].version, Some(2));
    }

    #[test]
    fn parses_nul_environment_and_detects_sensitive_names() {
        let env = parse_env_nul(b"HOME=/home/me\0PATH=/usr/bin\0API_TOKEN=secret\0");
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/me"));
        assert!(is_sensitive_environment_name("api_token"));
        assert!(is_sensitive_environment_name("SSH_AUTH_SOCK"));
        assert!(!is_sensitive_environment_name("PATH"));
    }

    #[test]
    fn validates_distribution_paths_and_parameterized_command_specs() {
        assert!(validate_distribution_name("Ubuntu-24.04").is_ok());
        assert!(validate_distribution_name("Ubuntu\n--user root").is_err());
        assert_eq!(
            validate_linux_absolute_path("/home/me/project/").unwrap(),
            "/home/me/project"
        );
        assert!(validate_linux_absolute_path("../etc").is_err());
        assert!(validate_linux_absolute_path("/home/me/../etc").is_err());
        let spec = wsl_exec_spec("Ubuntu", &["git".to_string(), "status".to_string()]).unwrap();
        assert_eq!(
            spec.args,
            vec!["--distribution", "Ubuntu", "--exec", "git", "status"]
        );
    }

    #[test]
    fn settings_round_trip_with_distribution_overrides() {
        let mut settings = WslSettings {
            default_distribution: Some("Ubuntu".to_string()),
            distributions: BTreeMap::new(),
        };
        settings.distributions.insert(
            "Ubuntu".to_string(),
            WslDistributionSettings {
                shell_override: Some("/bin/zsh".to_string()),
                agent_paths: BTreeMap::from([(
                    "claude".to_string(),
                    "/home/me/bin/claude".to_string(),
                )]),
                agent_config_paths: BTreeMap::new(),
            },
        );
        let raw = serde_json::to_string(&settings).unwrap();
        let decoded: WslSettings = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn legacy_settings_without_optional_fields_deserialize() {
        let decoded: WslSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded, WslSettings::default());
        let decoded: WslSettings =
            serde_json::from_str(r#"{"distributions":{"Ubuntu":{}}}"#).unwrap();
        let ubuntu = decoded.distributions.get("Ubuntu").unwrap();
        assert!(ubuntu.shell_override.is_none());
        assert!(ubuntu.agent_paths.is_empty());
        assert!(ubuntu.agent_config_paths.is_empty());
        // 未设置的可选字段不应写回磁盘，避免污染用户配置。
        assert_eq!(
            serde_json::to_string(&WslSettings::default()).unwrap(),
            r#"{"distributions":{}}"#
        );
    }

    #[test]
    fn parses_probe_output_for_user_home_shell_and_agents() {
        let distro = WslDistribution {
            name: "Ubuntu".to_string(),
            state: "Running".to_string(),
            version: Some(2),
            is_default: true,
        };
        let probe =
            parse_probe_output(&distro, b"me\0/home/me\0/bin/zsh\0/usr/bin/claude\0\0").unwrap();
        assert_eq!(probe.user, "me");
        assert_eq!(probe.home, "/home/me");
        assert_eq!(probe.shell, "/bin/zsh");
        assert_eq!(probe.claude_path.as_deref(), Some("/usr/bin/claude"));
        assert_eq!(probe.codex_path, None);
        assert_eq!(probe.state, "Running");
        assert!(parse_probe_output(&distro, b"me\0/home/me\0").is_err());
    }

    #[test]
    fn default_agent_config_paths_follow_home() {
        assert_eq!(
            default_agent_config_path("claude", "/home/me").as_deref(),
            Some("/home/me/.claude/settings.json")
        );
        assert_eq!(
            default_agent_config_path("codex", "/home/me").as_deref(),
            Some("/home/me/.codex/config.toml")
        );
        // 自定义 Agent 没有内置默认路径，需要用户显式配置。
        assert_eq!(default_agent_config_path("my-agent", "/home/me"), None);
    }

    #[test]
    fn agent_invocation_args_match_permission_and_resume_semantics() {
        assert_eq!(
            agent_invocation_args(
                "claude",
                "full_access",
                Some("fix bug"),
                None,
                None,
                None,
                None,
            ),
            vec!["--dangerously-skip-permissions", "fix bug"]
        );
        assert_eq!(
            agent_invocation_args(
                "claude",
                "auto_edit",
                None,
                Some("session-1"),
                None,
                None,
                None,
            ),
            vec!["--permission-mode", "acceptEdits", "--resume", "session-1"]
        );
        assert_eq!(
            agent_invocation_args(
                "codex",
                "auto_edit",
                Some("ship it"),
                None,
                None,
                None,
                None,
            ),
            vec![
                "--sandbox",
                "workspace-write",
                "-a",
                "on-request",
                "--",
                "ship it"
            ]
        );
        assert_eq!(
            agent_invocation_args("codex", "ask", None, Some("session-2"), None, None, None,),
            vec!["resume", "session-2"]
        );
        // 空 prompt 不应追加空参数，只保留权限模式参数。
        assert_eq!(
            agent_invocation_args("claude", "ask", Some("   "), None, None, None, None,),
            vec!["--permission-mode", "default"]
        );
        assert!(
            agent_invocation_args("codex", "ask", Some("   "), None, None, None, None,).is_empty()
        );
        assert_eq!(
            agent_invocation_args(
                "dsh",
                "auto_edit",
                Some("inspect status"),
                None,
                None,
                Some("high"),
                None,
            ),
            vec!["--profile", "headless", "--", "inspect status"]
        );
        assert!(
            agent_invocation_args("dsh", "ask", Some("   "), None, None, None, None,).is_empty()
        );
    }

    #[test]
    fn agent_invocation_args_forward_model_reasoning_and_speed() {
        let args = agent_invocation_args(
            "codex",
            "ask",
            None,
            Some("session-2"),
            Some("gpt-5.6-terra"),
            Some("high"),
            Some("fast"),
        );
        assert_eq!(
            args,
            vec![
                "-m",
                "gpt-5.6-terra",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-c",
                "features.fast_mode=true",
                "-c",
                "service_tier=\"fast\"",
                "resume",
                "session-2",
            ]
        );
        let claude_args =
            agent_invocation_args("claude", "ask", None, None, None, None, Some("fast"));
        assert_eq!(
            claude_args,
            vec![
                "--permission-mode",
                "default",
                "--settings",
                r#"{"fastMode":true}"#
            ]
        );
        assert_eq!(
            project_shell_command(
                "/home/me/app",
                "claude",
                &[],
                Some("claude-sonnet"),
                "claude",
                "ask",
            ),
            "cd -- '/home/me/app' && exec env AERORIC_AGENT_MODEL=claude-sonnet claude"
        );
    }

    #[test]
    fn task_and_shell_commands_quote_paths_and_arguments() {
        assert_eq!(
            project_shell_command(
                "/home/me/my app",
                "/home/me/bin/claude",
                &["--resume".to_string(), "a b".to_string()],
                None,
                "claude",
                "ask",
            ),
            "cd -- '/home/me/my app' && exec /home/me/bin/claude --resume 'a b'"
        );
        assert_eq!(
            project_shell_command(
                "/home/me/app",
                "dsh",
                &["--profile".to_string(), "headless".to_string()],
                None,
                "dsh",
                "full_access",
            ),
            "cd -- '/home/me/app' && exec env DSH_PERMISSION_MODE=danger-full-access DSH_TELEMETRY_DISABLED=1 dsh --profile headless"
        );
        assert_eq!(
            project_shell_command("/home/me/app", "dsh", &[], None, "dsh", "ask"),
            "cd -- '/home/me/app' && exec env DSH_PERMISSION_MODE=read-only DSH_TELEMETRY_DISABLED=1 dsh"
        );
        assert_eq!(
            login_shell_args("/bin/zsh", "env -0"),
            vec!["/bin/zsh", "-l", "-c", "env -0"]
        );
        let spec = wsl_exec_spec(
            "Ubuntu-24.04",
            &login_shell_args(
                "/bin/zsh",
                &project_shell_command("/home/me/app", "claude", &[], None, "claude", "ask"),
            ),
        )
        .unwrap();
        assert_eq!(spec.program, "wsl.exe");
        assert_eq!(
            spec.args,
            vec![
                "--distribution",
                "Ubuntu-24.04",
                "--exec",
                "/bin/zsh",
                "-l",
                "-c",
                "cd -- '/home/me/app' && exec claude"
            ]
        );
    }

    #[test]
    fn command_arguments_reject_nul_and_unknown_distributions() {
        assert!(wsl_exec_spec("Ubuntu", &[]).is_err());
        assert!(wsl_exec_spec("Ubuntu", &["git\0status".to_string()]).is_err());
        assert!(wsl_exec_spec(" Ubuntu", &["git".to_string()]).is_err());
    }

    #[test]
    fn config_files_are_written_atomically_with_private_permissions() {
        let dir = std::env::temp_dir().join(format!("aeroric-wsl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wsl-settings.json");
        crate::storage::atomic_write_private(&path, "{\"distributions\":{}}").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"distributions\":{}}"
        );
        crate::storage::atomic_write_private(&path, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .count();
        assert_eq!(leftovers, 1, "atomic write should not leave temp files");
        std::fs::remove_dir_all(&dir).ok();
    }
}
