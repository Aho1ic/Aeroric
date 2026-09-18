use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::Emitter;
use tauri::{AppHandle, Manager, State};

use crate::agent_family_maps::{
    claude_permission_args, codex_permission_args, dsh_permission_mode, omp_permission_flag,
    omp_thinking_level,
};
use crate::remote_os::RemoteOs;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(rename = "identityFile", skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// 明文密码。**只在反序列化方向有值** —— `skip_serializing` 让它永远不被写出去:
    /// 既不进 IPC 返回值,也不进 `ssh-connections.json`。
    ///
    /// 为什么用属性而不是在 `load_ssh_connections` 里手动清空:手动清空是一个可以被忘掉
    /// 的步骤(下一个人加一条返回连接的命令就漏了),而属性让"明文出不去"成为结构性事实。
    /// 前端要判断有没有密码看 `hasPassword`,要取明文走 `get_ssh_connection_password`。
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
    /// 这条连接在 `ssh-passwords.json` 里存了密码。
    ///
    /// 为什么需要它:落盘侧刻意把密码摘出去单独存 0o600 文件,而读取侧原先又把明文合并
    /// 回连接对象整体交给渲染进程 —— 拆分的意义被抵消,全部主机的登录凭据常驻 WebView
    /// 堆。前端真正需要的只是"有没有密码"(决定禁不禁用「复制密码」、编辑框显不显示
    /// "已保存"),而不是密码本身。取明文走 `get_ssh_connection_password`,一次操作一条。
    #[serde(rename = "hasPassword", default, skip_serializing_if = "is_false")]
    pub has_password: bool,
    #[serde(rename = "remotePath", skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    #[serde(
        rename = "autoSudoWithPassword",
        default,
        skip_serializing_if = "is_false"
    )]
    pub auto_sudo_with_password: bool,
    /// 勾选后这条连接每次都经设置里的全局代理建立。
    #[serde(rename = "useProxy", default, skip_serializing_if = "is_false")]
    pub use_proxy: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastConnectedAt", skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<i64>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn ssh_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("ssh-connections.json"))
}

fn ssh_passwords_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("ssh-passwords.json"))
}

static SSH_CONNECTIONS_STORAGE_LOCK: Mutex<()> = Mutex::new(());

/// 拆出公共信息与密码表。
///
/// `existing` 是盘上已存的密码表。传入连接的 `password` 为 `None`/空串时**保留** `existing`
/// 里那一条 —— 这是"留空不改密码"的语义,而且现在它必须在后端兑现:`load_ssh_connections`
/// 不再把明文交给前端,前端手里的连接对象 `password` 恒为 `None`,若这里照旧从零重建
/// 密码表,用户改一次备注就会把**所有**主机的密码抹掉。
fn prepare_ssh_connections_for_storage(
    connections: Vec<SshConnection>,
    existing: &BTreeMap<String, String>,
) -> (Vec<SshConnection>, BTreeMap<String, String>) {
    let mut public_connections = Vec::with_capacity(connections.len());
    let mut passwords = BTreeMap::new();
    for mut connection in connections {
        match connection.password.take().filter(|value| !value.is_empty()) {
            Some(password) => {
                passwords.insert(connection.id.clone(), password);
            }
            None => {
                if let Some(kept) = existing.get(&connection.id) {
                    passwords.insert(connection.id.clone(), kept.clone());
                }
            }
        }
        // `has_password` 是从密码表派生的,不落盘 —— 落了就成了第二个事实来源,而它会与
        // 密码文件独立变化(比如手工删 ssh-passwords.json),之后前端会看见一个说"有密码"
        // 的连接却取不到明文。`skip_serializing_if = "is_false"` 让 false 根本不写出去。
        connection.has_password = false;
        public_connections.push(connection);
    }
    (public_connections, passwords)
}

fn load_ssh_passwords() -> Result<BTreeMap<String, String>, String> {
    let path = ssh_passwords_path()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    crate::storage::ensure_private_file_permissions(&path)?;
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn write_ssh_connections_storage(connections: Vec<SshConnection>) -> Result<(), String> {
    crate::storage::ensure_aeroric_dirs()?;
    // 先读盘上已存的密码表:传入连接密码为空时要保留它,否则"留空不改"变成"留空清空"。
    let existing = load_ssh_passwords()?;
    let (public_connections, passwords) =
        prepare_ssh_connections_for_storage(connections, &existing);
    let public_raw =
        serde_json::to_string_pretty(&public_connections).map_err(|e| e.to_string())?;
    let password_raw = serde_json::to_string_pretty(&passwords).map_err(|e| e.to_string())?;
    crate::storage::atomic_write_private(&ssh_passwords_path()?, &format!("{password_raw}\n"))?;
    crate::storage::atomic_write_private(&ssh_connections_path()?, &format!("{public_raw}\n"))
}

/// 把盘上存的密码补回一条从前端传来的连接。
///
/// 连接动作(`open_ssh_shell` / `run_remote_task` / `resume_remote_task`,以及 SFTP /
/// 远程文件浏览那条链路)的入参是前端手里的连接对象,而那个对象的 `password` 现在恒为
/// `None`。明文由后端按 id 自己取,不再往渲染进程走一趟再回来。
///
/// **已有明文时不覆盖**:新建/编辑对话框里"测试连接"用的是用户刚敲进去、还没保存的
/// 密码,那一条必须优先。
pub(crate) fn hydrate_ssh_password(connection: &mut SshConnection) -> Result<(), String> {
    if connection
        .password
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    let passwords = load_ssh_passwords()?;
    connection.password = passwords.get(&connection.id).cloned();
    connection.has_password = connection.password.is_some();
    Ok(())
}

fn save_ssh_connections_sync(connections: Vec<SshConnection>) -> Result<(), String> {
    let _guard = SSH_CONNECTIONS_STORAGE_LOCK.lock();
    write_ssh_connections_storage(connections)
}

fn remove_ssh_connection(
    mut connections: Vec<SshConnection>,
    connection_id: &str,
) -> Vec<SshConnection> {
    connections.retain(|connection| connection.id != connection_id);
    connections
}

fn delete_ssh_connection_sync(connection_id: &str) -> Result<Vec<SshConnection>, String> {
    let _guard = SSH_CONNECTIONS_STORAGE_LOCK.lock();
    let path = ssh_connections_path()?;
    let mut connections = if path.exists() {
        crate::storage::ensure_private_file_permissions(&path)?;
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str::<Vec<SshConnection>>(&raw).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let passwords = load_ssh_passwords()?;
    for connection in &mut connections {
        if let Some(password) = passwords.get(&connection.id) {
            connection.password = Some(password.clone());
        }
    }
    let remaining = remove_ssh_connection(connections, connection_id);
    write_ssh_connections_storage(remaining.clone())?;
    Ok(remaining)
}

pub(crate) fn shell_quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn shell_word_posix(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        value.to_string()
    } else {
        shell_quote_posix(value)
    }
}

fn build_remote_start_command(remote_path: &str) -> String {
    format!(
        "cd -- {} && exec \"${{SHELL:-/bin/sh}}\" -l",
        shell_quote_posix(remote_path)
    )
}

/// Windows 上的远端起始命令。
///
/// POSIX 那一套 `cd -- '...' && exec $SHELL -l` 在 cmd.exe 下必然失败:`--` 不是路径、
/// 单引号也不是引用字符,cmd 会把 `'C:\Users\...'` 连引号一起当成目录名,报
/// 「文件名、目录名或卷标语法不正确。」然后 sshd 立刻结束会话(本地表现为
/// `Connection to <host> closed.`)。远端默认 shell 就是 cmd.exe 的 Windows 机器上,
/// 这条命令没有任何一个字符能被正确解释。
///
/// 路径经 `-EncodedCommand`(base64 UTF-16LE)送进去,理由与 `remote_os` 里那套文件操作
/// 一致:命令串要先过本地 ssh、远端 sshd、再落到 cmd.exe 或 PowerShell,base64 字符集里
/// 没有任何一层会特殊对待的字符,路径里的空格、单引号、`&` 都不需要转义。
fn build_windows_remote_start_command(remote_path: &str) -> String {
    let native_path = crate::remote_os::to_windows_native_path(remote_path);
    crate::remote_os::build_interactive_powershell_command(&format!(
        "Set-Location -LiteralPath {}",
        crate::remote_os::powershell_quote(&native_path)
    ))
}

/// 起始命令按远端 OS 分流。
///
/// Windows 分支不参与 `auto_sudo_with_password`:那个开关的语义是"登录后进 root shell",
/// 而 Windows 没有 sudo,也没有 root。勾了它的 Windows 连接只应该得到一个普通会话,
/// 而不是一条注定失败的命令。
fn build_remote_start_command_for(os: RemoteOs, remote_path: &str, auto_sudo: bool) -> String {
    match os {
        RemoteOs::Windows => build_windows_remote_start_command(remote_path),
        RemoteOs::Posix if auto_sudo => build_remote_start_command_with_sudo(remote_path),
        RemoteOs::Posix => build_remote_start_command(remote_path),
    }
}

const SUDO_PASSWORD_READY_MARKER: &str = "__AERORIC_SUDO_PASSWORD_READY__";

fn build_remote_start_command_with_sudo(remote_path: &str) -> String {
    format!(
        "cd -- {} && trap 'stty echo' EXIT HUP INT TERM && stty -echo && printf '%s\\n' {} && IFS= read -r aeroric_sudo_password && stty echo && trap - EXIT HUP INT TERM && printf '\\n' && printf '%s\\n' \"$aeroric_sudo_password\" | sudo -S -p '' -v && unset aeroric_sudo_password && exec sudo -n \"${{SHELL:-/bin/sh}}\" -l",
        shell_quote_posix(remote_path),
        shell_quote_posix(SUDO_PASSWORD_READY_MARKER)
    )
}

fn connection_can_auto_sudo(connection: &SshConnection) -> bool {
    connection.auto_sudo_with_password
        && connection.username.trim() != "root"
        && connection
            .password
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn is_remote_codex_like_agent(agent: &str) -> bool {
    matches!(agent, "codex" | "claude_gpt55")
}

fn is_remote_dsh_agent(agent: &str) -> bool {
    agent == "dsh"
}

fn is_remote_omp_agent(agent: &str) -> bool {
    agent == "omp"
}

/// 远端 agent id → 协议族。**穷尽的**四族映射:漏一族的后果不是编译错误而是运行时
/// 静默走错分支 —— omp 曾落到 `else => Claude`,于是 `normalized_reasoning_effort_for`
/// 用 claude 词表校验 omp 的思考档,`off`/`minimal` 被判非法,SSH 项目上的 omp 任务
/// 永远起不来;反过来 `ultracode` 被判合法,又会把 ultracode prelude 注进 omp 的 TUI。
fn remote_agent_family(agent: &str) -> crate::app_settings::AgentFamily {
    use crate::app_settings::AgentFamily;
    if is_remote_dsh_agent(agent) {
        AgentFamily::Dsh
    } else if is_remote_omp_agent(agent) {
        AgentFamily::Omp
    } else if is_remote_codex_like_agent(agent) {
        AgentFamily::Codex
    } else {
        AgentFamily::Claude
    }
}

fn validate_remote_agent_id(agent: &str) -> Result<&str, String> {
    let trimmed = agent.trim();
    let edge_is_separator = trimmed
        .as_bytes()
        .first()
        .into_iter()
        .chain(trimmed.as_bytes().last())
        .any(|byte| matches!(byte, b'.' | b'_' | b'-'));
    if trimmed.is_empty()
        || trimmed != agent
        || edge_is_separator
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("Invalid remote Agent ID".to_string());
    }
    Ok(trimmed)
}

/// 远端 agent 的可执行文件字,已按远端 OS 引用好。
///
/// `claude_gpt55` 是个例外:它不是 PATH 上的程序,而是本机历史兼容的启动脚本
/// `~/.claude/start-gpt55.sh`。那个脚本在 Windows 主机上不存在,所以宁可明确报错,
/// 也不要发一条注定 `not recognized` 的命令让用户去猜。
fn remote_agent_program_word_for(os: RemoteOs, agent: &str) -> Result<String, String> {
    let agent = validate_remote_agent_id(agent)?;
    Ok(match (os, agent) {
        (RemoteOs::Posix, "claude_gpt55") => "\"$HOME/.claude/start-gpt55.sh\"".to_string(),
        (RemoteOs::Windows, "claude_gpt55") => {
            return Err(
                "The claude_gpt55 profile is a POSIX shell script and cannot run on a Windows remote host"
                    .to_string(),
            )
        }
        (RemoteOs::Posix, _) => shell_quote_posix(agent),
        (RemoteOs::Windows, _) => crate::remote_os::powershell_quote(agent),
    })
}

/// 远端 agent 的启动参数。
///
/// omp 走自己一支:它的 flag 名与其余三族都不同(`--approval-mode` / `--thinking`,
/// 而不是 `--permission-mode` / `--effort`),且**所有 flag 必须在位置参数之前** ——
/// omp 把位置参数当作"启动后自动提交的首条消息"。落到 claude 那支的后果是被推一个
/// omp 不认识的 `--effort`。
fn remote_agent_args(
    agent: &str,
    permission_mode: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Vec<String> {
    if is_remote_omp_agent(agent) {
        return remote_omp_args(permission_mode, selected_model, reasoning_effort);
    }
    // 权限档 → 启动参数的唯一映射表在 agent_family_maps(本地 pty.rs 共用)。
    let mut args = match if is_remote_codex_like_agent(agent) {
        "codex"
    } else {
        agent
    } {
        "claude" => claude_permission_args(permission_mode)
            .into_iter()
            .map(str::to_string)
            .collect(),
        "codex" => codex_permission_args(permission_mode)
            .into_iter()
            .map(str::to_string)
            .collect(),
        _ => vec![],
    };

    if is_remote_codex_like_agent(agent) {
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
    } else if !is_remote_dsh_agent(agent) {
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

/// omp 远端启动参数,与本地 `pty.rs::build_omp_cmd` + `add_omp_launch_args` 同形。
///
/// 两处刻意的差异:
/// - 不给 `--cwd`。远端命令已经是 `cd -- <path> && omp ...`(见 `build_remote_task_command`),
///   再传一次是重复;本地那边没有 `cd` 这一步所以必须显式给。
/// - 不设 `PI_CODING_AGENT_DIR`。托管 home 在本机,远端主机上没有这个目录;远端 omp
///   用对端用户自己的配置,与远端 claude / codex 一致。
/// - 不传 `speed`。omp 没有对应 flag。
fn remote_omp_args(
    permission_mode: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(flag) = omp_permission_flag(permission_mode) {
        args.push("--approval-mode".to_string());
        args.push(flag.to_string());
    }
    if let Some(model) = selected_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    if let Some(level) = reasoning_effort
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .and_then(omp_thinking_level)
    {
        args.push("--thinking".to_string());
        args.push(level.to_string());
    }
    args
}

/// 远端 agent 的启动串,按远端 OS 拼装。
///
/// 两侧不只是引号不同,语法结构也不同:
/// - POSIX:环境变量是**命令前缀**,`NAME='v' prog 'a' 'b'` 整体是一个 `sh` 语句。
/// - PowerShell:`$env:NAME = 'v'` 是**独立语句**,必须用 `;` 分隔;而且可执行文件要经
///   调用运算符 `&` 启动 —— 直接写 `'claude' --flag`,PowerShell 只会把那个字符串
///   原样输出到 stdout,agent 根本不会启动。
///
/// `program_word` 已由 [`remote_agent_program_word_for`] 按同一 OS 引用好,这里只管参数。
fn build_remote_command(
    os: RemoteOs,
    agent: &str,
    permission_mode: &str,
    program_word: String,
    args: &[String],
    selected_model: Option<&str>,
) -> String {
    let mut environment: Vec<(String, String)> = Vec::new();
    if let Some(model) = selected_model {
        environment.push(("AERORIC_AGENT_MODEL".to_string(), model.to_string()));
    }
    if is_remote_dsh_agent(agent) {
        if let Some(mode) = dsh_permission_mode(permission_mode) {
            environment.push(("DSH_PERMISSION_MODE".to_string(), mode.to_string()));
        }
        environment.push(("DSH_TELEMETRY_DISABLED".to_string(), "1".to_string()));
    }
    let invocation = std::iter::once(program_word)
        .chain(args.iter().map(|arg| match os {
            RemoteOs::Posix => shell_word_posix(arg),
            RemoteOs::Windows => crate::remote_os::powershell_quote(arg),
        }))
        .collect::<Vec<_>>()
        .join(" ");
    match os {
        RemoteOs::Posix => environment
            .into_iter()
            .map(|(name, value)| format!("{name}={}", shell_word_posix(&value)))
            .chain(std::iter::once(invocation))
            .collect::<Vec<_>>()
            .join(" "),
        RemoteOs::Windows => environment
            .into_iter()
            .map(|(name, value)| {
                format!(
                    "$env:{name} = {}",
                    crate::remote_os::powershell_quote(&value)
                )
            })
            .chain(std::iter::once(format!("& {invocation}")))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// 「先进入项目目录,再启动 agent」的远端命令。
///
/// POSIX 是 `cd -- '<path>' && <cmd>`;Windows 的默认 shell 是 cmd.exe,既不认 `--`
/// 也不认单引号(与终端起始命令同一个坑),所以整串改走 PowerShell:
/// `Set-Location -LiteralPath '<native>'; <cmd>`,再经 `-EncodedCommand` 下发。
///
/// 用 `-NonInteractive` 那一版而不是 `-NoExit` 那版:agent 跑完就该退出,留着 PowerShell
/// 的提示符会让任务永远停在 running。`-NonInteractive` 只约束 PowerShell 自己要不要
/// 提问,agent 作为子进程仍然继承 PTY 的 stdin,交互界面照常。
fn build_remote_project_command(
    os: RemoteOs,
    remote_project_path: &str,
    command: String,
) -> String {
    match os {
        RemoteOs::Posix => format!(
            "cd -- {} && {}",
            shell_quote_posix(remote_project_path),
            command
        ),
        RemoteOs::Windows => crate::remote_os::build_powershell_command(&format!(
            "Set-Location -LiteralPath {}; {}",
            crate::remote_os::powershell_quote(&crate::remote_os::to_windows_native_path(
                remote_project_path
            )),
            command
        )),
    }
}

/// 远端 Agent 任务的启动命令,按远端 OS 分流。
///
/// 与交互式终端同一个坑:Windows 主机的默认 shell 是 cmd.exe,`cd -- '<path>'` 会被它
/// 当成目录名(见 [`build_remote_project_command`])。所以 `os` 必须由调用方探测后传进来,
/// 不能在这里按 `cfg!(windows)` 猜 —— 那是**本机**平台,与远端平台无关。
fn build_remote_task_command(
    os: RemoteOs,
    agent: &str,
    permission_mode: &str,
    remote_project_path: &str,
    prompt: Option<&str>,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Result<String, String> {
    let program_word = remote_agent_program_word_for(os, agent)?;
    let mut args = remote_agent_args(
        agent,
        permission_mode,
        selected_model,
        reasoning_effort,
        speed,
    );
    if let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) {
        if is_remote_dsh_agent(agent) {
            args.push("--profile".to_string());
            args.push("headless".to_string());
            args.push("--".to_string());
        } else if is_remote_codex_like_agent(agent)
            || is_remote_omp_agent(agent)
            || agent == "claude"
        {
            // codex / omp / claude 都把位置参数当首条消息,且都支持 `--` 终止 flag
            // 解析。缺 `--` 时,一个以 `-` 开头的 prompt 会被当成 flag 解析。
            args.push("--".to_string());
        }
        args.push(prompt.to_string());
    }
    Ok(build_remote_project_command(
        os,
        remote_project_path,
        build_remote_command(os, agent, permission_mode, program_word, &args, selected_model),
    ))
}

/// 远端会话恢复,按远端 OS 分流。`os` 的由来见 [`build_remote_task_command`]。
fn build_remote_resume_command(
    os: RemoteOs,
    agent: &str,
    permission_mode: &str,
    remote_project_path: &str,
    session_id: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) -> Result<String, String> {
    if is_remote_dsh_agent(agent) {
        return Err("DeepSeek Harness remote sessions do not support native resume".to_string());
    }
    let program_word = remote_agent_program_word_for(os, agent)?;
    let mut args = remote_agent_args(
        agent,
        permission_mode,
        selected_model,
        reasoning_effort,
        speed,
    );
    if is_remote_codex_like_agent(agent) {
        args.push("resume".to_string());
        args.push(session_id.to_string());
    } else {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }
    Ok(build_remote_project_command(
        os,
        remote_project_path,
        build_remote_command(os, agent, permission_mode, program_word, &args, selected_model),
    ))
}

/// ssh 在 host key 出问题时会用这些说法。整句匹配而不是拆成关键词组合,
/// 因为 PTY 过滤器是在滑动窗口上找它们,拆开会被窗口边界切散。
pub(crate) const HOST_KEY_FAILURE_PHRASES: [&str; 4] = [
    "host key verification failed",
    "remote host identification has changed",
    "no matching host key",
    "host key is known for",
];

/// `StrictHostKeyChecking=yes` makes ssh refuse hosts that are absent from
/// known_hosts. That is the safe default, but ssh's own wording gives the user
/// no way forward, so attach the concrete remediation to the raw stderr.
pub(crate) fn annotate_ssh_error(connection: &SshConnection, error: impl Into<String>) -> String {
    let error = error.into();
    let lowered = error.to_ascii_lowercase();
    let is_host_key_failure = HOST_KEY_FAILURE_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
        || (lowered.contains("host key") && lowered.contains("changed"));
    if !is_host_key_failure {
        return error;
    }
    format!("{error}\n\n{}", host_key_remediation(connection))
}

/// 出现 host key 失败时该怎么办。两条路都给:App 内确认(首次连接),
/// 或者手动核对指纹(key 变更 —— 那可能是 MITM,必须人工介入)。
pub(crate) fn host_key_remediation(connection: &SshConnection) -> String {
    let target = if connection.port == 22 {
        connection.host.clone()
    } else {
        format!("[{}]:{}", connection.host, connection.port)
    };
    let scan = if connection.port == 22 {
        format!("ssh-keyscan {} >> ~/.ssh/known_hosts", connection.host)
    } else {
        format!(
            "ssh-keyscan -p {} {} >> ~/.ssh/known_hosts",
            connection.port, connection.host
        )
    };
    format!(
        "Aeroric requires a verified host key (StrictHostKeyChecking=yes). \
If this is your first connection to {target}, open it from the SSH panel and \
Aeroric will show you the host key fingerprint to confirm. You can also add it \
yourself with `{scan}` after checking the fingerprint. If the key legitimately \
changed, remove the stale entry with `ssh-keygen -R {target}` first — until then \
every connection is refused, because a changed host key can also mean someone is \
intercepting the connection."
    )
}

/// host key 失败只会出现在握手阶段。扫过这么多字节还没命中,说明会话已经进入
/// 正常交互,继续扫描是纯开销:每个 chunk 都要做一次全量 `to_ascii_lowercase`
/// 堆分配加四次全串搜索,整个会话持续付这笔钱。到点即解除。
const HOST_KEY_SCAN_BYTE_BUDGET: usize = 64 * 1024;

/// 在输出流里认出 host key 失败,并追加一段补救说明。
///
/// PTY 路径把 ssh 的原始输出直接流给终端,`annotate_ssh_error` 完全不在链上,
/// 所以用户只能看到 ssh 那句没有出路的报错。这里在窗口里找整句,命中一次就
/// 追加说明并停止扫描;始终没命中也会在 [`HOST_KEY_SCAN_BYTE_BUDGET`] 后停。
///
/// 数据本身原样放行,只在末尾追加 —— 过滤器不缓冲任何输出,不影响终端延迟。
pub(crate) fn host_key_failure_hint_filter(
    connection: &SshConnection,
) -> crate::pty::PtyOutputFilter {
    let hint = host_key_remediation(connection);
    // 窗口至少要容纳最长的那句话,因为句子可能横跨两次读取。取两倍留出余量:
    // 按字符边界裁剪会比目标位置多切掉几个字节,刚好等长会漏掉跨块的句子。
    let window_size = HOST_KEY_FAILURE_PHRASES
        .iter()
        .map(|phrase| phrase.len())
        .max()
        .unwrap_or(64)
        * 2;
    let mut window = String::new();
    let mut fired = false;
    let mut scanned = 0usize;
    Box::new(move |data| {
        if fired {
            return Some(data);
        }
        scanned = scanned.saturating_add(data.len());
        window.push_str(&data.to_ascii_lowercase());
        let matched = HOST_KEY_FAILURE_PHRASES
            .iter()
            .any(|phrase| window.contains(phrase));
        if matched {
            fired = true;
            window.clear();
            // PTY 是裸终端,换行必须带 \r,否则续行会从上一行末尾接着写。
            return Some(format!("{data}\r\n\r\n{}\r\n", hint.replace('\n', "\r\n")));
        }
        if scanned >= HOST_KEY_SCAN_BYTE_BUDGET {
            fired = true;
            window.clear();
            window.shrink_to_fit();
            return Some(data);
        }
        if window.len() > window_size {
            // 从字符边界处裁剪,否则多字节序列会被切断导致 panic。
            let cut = window.len() - window_size;
            let boundary = (cut..window.len())
                .find(|index| window.is_char_boundary(*index))
                .unwrap_or(window.len());
            window.drain(..boundary);
        }
        Some(data)
    })
}

/// 连接前的最后一道闸:主机确定不在 known_hosts 时,给一条带出路的错误,
/// 而不是让用户从 PTY 里读 ssh 那句干巴巴的失败。
///
/// 只有**确定**未登记才拦。判断不了(`ssh-keygen` 跑不动等)一律放行,
/// 让 ssh 去报真实错误 —— 这道闸的作用是改善措辞,不是增加拦截。
pub(crate) fn ensure_host_key_known(connection: &SshConnection) -> Result<(), String> {
    if crate::ssh_hostkey::is_host_known(connection) == Some(false) {
        return Err(format!(
            "Host key verification failed: {} is not in known_hosts.\n\n{}",
            crate::ssh_hostkey::known_hosts_target(connection),
            host_key_remediation(connection)
        ));
    }
    Ok(())
}

/// 让 ssh 经全局代理连接:`ProxyCommand` 指向 Aeroric 自己的可执行文件。
///
/// ssh 会把这个字符串交给 shell 执行,所以路径必须按平台规则引用。`%h`/`%p` 由 ssh
/// 替换成真实的目标主机和端口 —— 也正因为 ssh 始终知道真实主机名,
/// known_hosts 校验不受代理影响。
///
/// 取不到自身路径时返回 `None`,调用方退化成直连,而不是让整条连接失败。
pub(crate) fn proxy_command_arg() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.to_str()?;
    // Windows 的 ssh 用 cmd.exe 语义,单引号不是引用字符。
    let quoted = if cfg!(windows) {
        format!("\"{exe}\"")
    } else {
        shell_quote_posix(exe)
    };
    Some(format!(
        "ProxyCommand={quoted} {} %h %p",
        crate::ssh_proxy::SSH_PROXY_BRIDGE_FLAG
    ))
}

/// `os` 只影响远端起始命令。除交互式终端以外的调用点(`scp`、端口转发、一次性远端
/// 命令)都会把 `remote_path` 清空,根本拿不到起始命令,所以它们继续用 POSIX 默认值 ——
/// 唯一需要真实 OS 的路径是 [`build_ssh_command`],它由调用方先探测再传进来。
fn build_ssh_args(connection: &SshConnection, force_tty: bool) -> Vec<String> {
    build_ssh_args_for_os(connection, force_tty, RemoteOs::Posix)
}

fn build_ssh_args_for_os(
    connection: &SshConnection,
    force_tty: bool,
    os: RemoteOs,
) -> Vec<String> {
    let mut args = vec![if force_tty { "-tt" } else { "-T" }.to_string()];
    // Never silently trust a changed or previously unseen host key. Users can
    // provision the host key in their normal SSH known_hosts file first.
    args.extend(["-o".to_string(), "StrictHostKeyChecking=yes".to_string()]);
    if connection.use_proxy {
        if let Some(proxy_command) = proxy_command_arg() {
            args.extend(["-o".to_string(), proxy_command]);
        }
    }
    if force_tty {
        args.extend(["-o".to_string(), "IPQoS=none".to_string()]);
        // 交互会话必须能发现对端已经不在了。默认 ServerAliveInterval=0 意味着
        // NAT 超时或 Wi-Fi 切换后,ssh 不知道链路已断,终端表现为"敲了完全没反应"
        // 一直到 TCP 自己超时(可能几分钟)。15s × 3 让失败在 45s 内明确暴露。
        args.extend(["-o".to_string(), "ServerAliveInterval=15".to_string()]);
        args.extend(["-o".to_string(), "ServerAliveCountMax=3".to_string()]);
        args.extend(["-o".to_string(), "TCPKeepAlive=yes".to_string()]);
    }
    args.extend(["-p".to_string(), connection.port.to_string()]);
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        args.push("-i".to_string());
        args.push(identity_file.to_string());
    }
    args.push(format!("{}@{}", connection.username, connection.host));
    if let Some(remote_path) = remote_start_path(connection) {
        args.push(build_remote_start_command_for(
            os,
            remote_path,
            connection_can_auto_sudo(connection),
        ));
    }
    args
}

/// 连接上配置的远端起始目录。空白串与 `None` 等价 —— 两者都表示"不指定"。
///
/// 抽出来是因为它同时决定两件事:要不要发起始命令,以及要不要为起始命令去探测远端 OS。
/// 两处各写一遍 `trim().filter(!is_empty())`,迟早会分叉成"探测了却不发命令"这种白跑。
fn remote_start_path(connection: &SshConnection) -> Option<&str> {
    connection
        .remote_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SshCommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn sshpass_program() -> String {
    let detected = crate::platform::detect_path("sshpass");
    if detected.is_empty() {
        "sshpass".to_string()
    } else {
        detected
    }
}

fn ssh_command_spec(
    connection: &SshConnection,
    remote_command: Option<String>,
    force_tty: bool,
) -> SshCommandSpec {
    ssh_command_spec_for_os(connection, remote_command, force_tty, RemoteOs::Posix)
}

fn ssh_command_spec_for_os(
    connection: &SshConnection,
    remote_command: Option<String>,
    force_tty: bool,
    os: RemoteOs,
) -> SshCommandSpec {
    let mut ssh_args = build_ssh_args_for_os(connection, force_tty, os);
    if let Some(remote_command) = remote_command {
        ssh_args.push(remote_command);
    }
    ssh_command_spec_from_args(connection, ssh_args)
}

fn ssh_command_spec_from_args(
    connection: &SshConnection,
    mut ssh_args: Vec<String>,
) -> SshCommandSpec {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    if let Some(password) = password {
        ssh_args.splice(
            0..0,
            [
                "-o".to_string(),
                "PreferredAuthentications=password,keyboard-interactive".to_string(),
                "-o".to_string(),
                "PubkeyAuthentication=no".to_string(),
            ],
        );
        let mut args = vec!["-e".to_string(), "ssh".to_string()];
        args.extend(ssh_args);
        SshCommandSpec {
            program: sshpass_program(),
            args,
            env: vec![("SSHPASS".to_string(), password.to_string())],
        }
    } else {
        SshCommandSpec {
            program: "ssh".to_string(),
            args: ssh_args,
            env: Vec::new(),
        }
    }
}

pub(crate) fn ssh_port_forward_command_spec(
    connection: &SshConnection,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> SshCommandSpec {
    let mut ssh_args = build_ssh_args(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        false,
    );
    let target_index = ssh_args.len().saturating_sub(1);
    ssh_args.splice(
        target_index..target_index,
        [
            "-N".to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-L".to_string(),
            format!("127.0.0.1:{local_port}:{remote_host}:{remote_port}"),
        ],
    );
    ssh_command_spec_from_args(connection, ssh_args)
}

pub(crate) fn std_ssh_port_forward_command(
    connection: &SshConnection,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Command {
    let spec = ssh_port_forward_command_spec(connection, local_port, remote_host, remote_port);
    let mut cmd = Command::new(spec.program);
    // 端口转发是后台常驻进程,Windows 上不压窗口会留一个 ssh 控制台在桌面。
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(spec.args);
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    cmd
}

fn command_builder_from_spec(spec: SshCommandSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(spec.program);
    for arg in spec.args {
        cmd.arg(arg);
    }
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    crate::pty::setup_env(&mut cmd);
    cmd
}

/// 交互式远端终端。`os` 决定起始命令是 POSIX 还是 PowerShell,由调用方探测后传入。
fn build_ssh_command(connection: &SshConnection, os: RemoteOs) -> CommandBuilder {
    command_builder_from_spec(ssh_command_spec_for_os(connection, None, true, os))
}

/// 同步跑一条远端命令并取回 stdout。给远端 OS 探测用。
///
/// 探测结果由 `remote_os::detect_remote_os_with` 按 `user@host:port` 缓存,所以整条链路
/// 只为"第一次遇到这台机器"付一次往返;SFTP 面板通常已经先探过了。
fn run_remote_command_capture(
    connection: &SshConnection,
    remote_command: String,
) -> Result<Vec<u8>, String> {
    let mut cmd = std_ssh_command_for_remote_command(connection, remote_command);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

/// 远端该发哪一套命令 —— 交互式终端的起始命令、远端 Agent 任务的启动命令都问它。
///
/// 探测失败(凭据拿不到、`sshpass` 缺失、host key 还没确认、对端只允许交互式登录等)时
/// **不能**一律按 POSIX 处理:配置里写的是 `C:\Users\...` 的 Windows 主机,发 POSIX 命令
/// 只会换来 cmd.exe 那句「文件名、目录名或卷标语法不正确」。所以兜底看路径形态。
///
/// 反过来,探测成功时一律以探测结果为准、不再看路径:MSYS / Cygwin 的 `uname -s` 会命中
/// POSIX 列表,而它们确实吃 `sh` 语法,此时按 `C:/...` 的形态改发 PowerShell 反而会错。
///
/// `fallback_path` 由调用方给,而不是固定读 `connection.remote_path`:远端 Agent 任务用的是
/// 它自己的 `remote_project_path`,两者未必是同一个值。
fn resolve_remote_shell_os_for<F>(
    connection: &SshConnection,
    fallback_path: Option<&str>,
    run: F,
) -> RemoteOs
where
    F: FnMut(&SshConnection, String) -> Result<Vec<u8>, String>,
{
    if let Ok(os) = crate::remote_os::detect_remote_os_with(connection, run) {
        return os;
    }
    match fallback_path {
        Some(path) if crate::remote_os::is_windows_remote_path(path) => RemoteOs::Windows,
        _ => RemoteOs::Posix,
    }
}

fn resolve_remote_shell_os(connection: &SshConnection) -> RemoteOs {
    resolve_remote_shell_os_for(
        connection,
        connection.remote_path.as_deref(),
        run_remote_command_capture,
    )
}

/// 远端 Agent 任务的远端 shell 判定。与终端同源,只是兜底路径换成任务自己的项目路径。
///
/// 调用点必须**先** `ensure_host_key_known`:探测本身就是一次 ssh 往返,host key 还没确认
/// 时它必然失败,判定就只会退化成路径形态 —— 放在确认之后才可能真的探到。
async fn resolve_remote_task_shell_os(
    connection: &SshConnection,
    remote_project_path: &str,
) -> RemoteOs {
    let probe_connection = connection.clone();
    let fallback_path = remote_project_path.to_string();
    tokio::task::spawn_blocking(move || {
        resolve_remote_shell_os_for(
            &probe_connection,
            Some(fallback_path.as_str()),
            run_remote_command_capture,
        )
    })
    .await
    .unwrap_or(RemoteOs::Posix)
}

fn build_ssh_remote_command(connection: &SshConnection, remote_command: String) -> CommandBuilder {
    command_builder_from_spec(ssh_command_spec(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        Some(remote_command),
        true,
    ))
}

pub(crate) fn ssh_command_spec_for_remote_command(
    connection: &SshConnection,
    remote_command: String,
) -> SshCommandSpec {
    ssh_command_spec(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        Some(remote_command),
        false,
    )
}

pub(crate) fn std_ssh_command_for_remote_command(
    connection: &SshConnection,
    remote_command: String,
) -> Command {
    let spec = ssh_command_spec_for_remote_command(connection, remote_command);
    let mut cmd = Command::new(spec.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(spec.args);
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    cmd
}

fn marker_overlap_len(value: &str, marker: &str) -> usize {
    (1..=value.len().min(marker.len()))
        .rev()
        .find(|length| value.ends_with(&marker[..*length]))
        .unwrap_or(0)
}

fn sudo_password_output_filter(
    writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
    password: String,
) -> crate::pty::PtyOutputFilter {
    let mut pending = String::new();
    let mut password_sent = false;
    Box::new(move |data| {
        if password_sent {
            return Some(data);
        }
        pending.push_str(&data);
        if let Some(marker_start) = pending.find(SUDO_PASSWORD_READY_MARKER) {
            let marker_end = marker_start + SUDO_PASSWORD_READY_MARKER.len();
            let mut visible = String::with_capacity(pending.len());
            visible.push_str(&pending[..marker_start]);
            visible.push_str(&pending[marker_end..]);
            pending.clear();
            {
                let mut writer = writer.lock();
                let _ = writer.write_all(password.as_bytes());
                let _ = writer.write_all(b"\n");
                let _ = writer.flush();
            }
            password_sent = true;
            return Some(visible);
        }

        let overlap = marker_overlap_len(&pending, SUDO_PASSWORD_READY_MARKER);
        let emit_len = pending.len().saturating_sub(overlap);
        if emit_len == 0 {
            return None;
        }
        Some(pending.drain(..emit_len).collect())
    })
}

/// 串接两个过滤器。`first` 会缓冲数据(sudo 那个在等 marker),所以 `second`
/// 只看得到 `first` 实际放出来的部分 —— 顺序不能反,否则 sudo 的 marker
/// 检测会被打断。
fn chain_output_filters(
    mut first: crate::pty::PtyOutputFilter,
    mut second: crate::pty::PtyOutputFilter,
) -> crate::pty::PtyOutputFilter {
    Box::new(move |data| first(data).and_then(&mut second))
}

fn spawn_remote_task_exit_monitor(
    app: AppHandle,
    task_id: String,
    child_handle: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
) {
    tokio::task::spawn_blocking(move || loop {
        let exit_status = child_handle.lock().try_wait().ok().flatten();

        if let Some(status) = exit_status {
            let tm = app.state::<crate::TaskManager>();
            if !tm.remove_pty_handles_if_current(&task_id, &child_handle) {
                return;
            }
            let ok = status.success();
            let (was_cancelled, was_manually_completed) = {
                let mut cancelled_tasks = tm.cancelled_tasks.lock();
                let was_cancelled = cancelled_tasks.remove(&task_id);
                let was_manually_completed = tm.manually_completed_tasks.lock().remove(&task_id);
                (was_cancelled, was_manually_completed)
            };
            if was_cancelled || was_manually_completed {
                return;
            }
            let payload = if ok {
                serde_json::json!({ "task_id": task_id, "status": "done" })
            } else {
                serde_json::json!({
                    "task_id": task_id,
                    "status": "failed",
                    "failure_reason": format!("Remote process exited with code {}", status.exit_code())
                })
            };
            let _ = app.emit(crate::event_names::TASK_STATUS, payload);
            return;
        }

        std::thread::sleep(Duration::from_millis(100));
    });
}

fn spawn_remote_task_pty(
    app: AppHandle,
    task_manager: &crate::TaskManager,
    task_id: &str,
    cmd: CommandBuilder,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
    initial_prelude: Option<Vec<u8>>,
    initial_prompt: Option<(Vec<u8>, Vec<u8>)>,
    output_filter: Option<crate::pty::PtyOutputFilter>,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(50),
            cols: cols.unwrap_or(220),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Box<dyn Write + Send> = pair.master.take_writer().map_err(|e| e.to_string())?;
    let child_handle =
        crate::pty::register_pty_handles(task_manager, task_id, pair.master, writer, child)?;

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
        crate::pty::PtyEmitMode::agent(),
        reader,
        true,
        None,
        needs_initial_input.then_some(startup_tx),
        output_filter,
        None,
    );
    if needs_initial_input {
        if let Some(writer) = task_manager.pty_writers.lock().get(task_id).cloned() {
            match startup_generation {
                Some(cleanup_generation) => {
                    let signals = Arc::clone(&task_manager.initial_input_signals);
                    let cleanup_id = task_id.to_string();
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
                }
                None => {
                    // Registration should always succeed when needs_initial_input,
                    // but a missing generation must not panic a live SSH session.
                    crate::pty::cancel_initial_input_signal(task_manager, task_id);
                }
            }
        } else {
            crate::pty::cancel_initial_input_signal(task_manager, task_id);
        }
    }
    spawn_remote_task_exit_monitor(app, task_id.to_string(), child_handle);
    Ok(())
}

/// 带明文密码的连接列表。**进程内用**,不经 IPC。
///
/// 连接动作需要明文(`ssh_command_spec_from_args` 要把它塞进 `sshpass`,sudo 过滤器要
/// 拿它应答提示),但那是后端自己的事 —— 明文没有理由往渲染进程走一趟再回来。
fn load_ssh_connections_with_passwords() -> Result<Vec<SshConnection>, String> {
    let path = ssh_connections_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    crate::storage::ensure_private_file_permissions(&path)?;
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut connections: Vec<SshConnection> =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let passwords = load_ssh_passwords()?;
    let mut has_legacy_password = false;
    for connection in &mut connections {
        if let Some(password) = passwords.get(&connection.id) {
            connection.password = Some(password.clone());
        } else if connection.password.is_some() {
            // 老格式把密码内联在 ssh-connections.json 里。下一次写入会把它迁到
            // ssh-passwords.json。
            has_legacy_password = true;
        }
        connection.has_password = connection
            .password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    }
    if has_legacy_password {
        save_ssh_connections_sync(connections.clone())?;
    }
    Ok(connections)
}

/// 按 id 取一条连接的明文密码。
///
/// 单独一条命令而不是让 `load_ssh_connections` 带上明文:暴露窗口从"列表加载后常驻"
/// 缩到"用户点复制密码的那一刻",泄漏面从全部连接缩到一条。
#[tauri::command]
pub async fn get_ssh_connection_password(connection_id: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let passwords = load_ssh_passwords()?;
        Ok(passwords.get(&connection_id).cloned())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 连接列表。**返回值里 `password` 恒为 `None`**,只用 `hasPassword` 表示存过密码。
#[tauri::command]
pub async fn load_ssh_connections() -> Result<Vec<SshConnection>, String> {
    tokio::task::spawn_blocking(|| {
        let mut connections = load_ssh_connections_with_passwords()?;
        for connection in &mut connections {
            connection.password = None;
        }
        Ok(connections)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_ssh_connections(connections: Vec<SshConnection>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || save_ssh_connections_sync(connections))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_ssh_connection(connection_id: String) -> Result<Vec<SshConnection>, String> {
    tokio::task::spawn_blocking(move || delete_ssh_connection_sync(&connection_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_ssh_shell(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
    mut connection: SshConnection,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_ssh_shell_id(&shell_id)?;
    // 明文密码由后端按 id 自取,不经渲染进程。已带明文(未保存的新连接)时不覆盖。
    hydrate_ssh_password(&mut connection)?;
    let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);

    // 起始命令要按远端 OS 分流,而探测是一次同步的 ssh 往返 —— 所以只在真的会用到起始
    // 命令(连接配了远端路径)时才探。没配路径的连接照旧直接开终端,不多付这一次往返;
    // 对端不可达时也就不会出现"终端空着等超时"的假死观感。
    // 探测失败会退化成按路径形态判断,不会拦住终端。
    let shell_os = if remote_start_path(&connection).is_none() {
        RemoteOs::Posix
    } else {
        let probe_connection = connection.clone();
        tokio::task::spawn_blocking(move || resolve_remote_shell_os(&probe_connection))
            .await
            .unwrap_or(RemoteOs::Posix)
    };

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(120),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair
        .slave
        .spawn_command(build_ssh_command(&connection, shell_os))
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Box<dyn Write + Send> = pair.master.take_writer().map_err(|e| e.to_string())?;
    crate::pty::register_pty_handles(&task_manager, &shell_id, pair.master, writer, child)?;
    let hint_filter = host_key_failure_hint_filter(&connection);
    let output_filter = if connection_can_auto_sudo(&connection) {
        let writer = task_manager
            .pty_writers
            .lock()
            .get(&shell_id)
            .cloned()
            .ok_or_else(|| "Failed to initialize SSH sudo input".to_string())?;
        Some(chain_output_filters(
            sudo_password_output_filter(
                writer,
                connection
                    .password
                    .as_deref()
                    .unwrap_or_default()
                    .to_string(),
            ),
            hint_filter,
        ))
    } else {
        Some(hint_filter)
    };

    let app_cleanup = app.clone();
    let sid_cleanup = shell_id.clone();
    let on_finish = Box::new(move || {
        let tm = app_cleanup.state::<crate::TaskManager>();
        tm.remove_pty_handles(&sid_cleanup);
    });

    crate::pty::spawn_pty_reader(
        app,
        shell_id,
        crate::pty::OutputSink::Channel(on_output),
        crate::pty::PtyEmitMode::interactive(),
        reader,
        false,
        None,
        None,
        output_filter,
        Some(on_finish),
    );

    Ok(())
}

#[tauri::command]
pub async fn kill_ssh_shell(
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
) -> Result<(), String> {
    crate::pty::validate_ssh_shell_id(&shell_id)?;
    let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);
    Ok(())
}

#[tauri::command]
pub async fn run_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    mut connection: SshConnection,
    remote_project_path: String,
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
    // 明文密码由后端按 id 自取,不经渲染进程。已带明文(未保存的新连接)时不覆盖。
    hydrate_ssh_password(&mut connection)?;
    let is_codex = is_remote_codex_like_agent(&agent);
    let is_dsh = is_remote_dsh_agent(&agent);
    let family = remote_agent_family(&agent);
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
    // 历史清零 → 远程终端流水位换代,已订阅的手机端自动重新快照
    crate::remote::terminal_hub::hub().reset_for_truncate(&task_id);
    ensure_host_key_known(&connection)?;
    // 启动命令要按远端 OS 分流,所以这里多一次探测(结果按 `user@host:port` 缓存,
    // 同一台机器在一个 App 生命周期里只探一次)。顺序不能提到 `ensure_host_key_known`
    // 之前:host key 没确认时探测必然失败,判定会退化成纯路径形态。
    let task_os = resolve_remote_task_shell_os(&connection, &remote_project_path).await;
    let remote_command = build_remote_task_command(
        task_os,
        &agent,
        &permission_mode,
        &remote_project_path,
        (!uses_ultracode && !force_prompt_injection).then_some(prompt.as_str()),
        selected_model.as_deref(),
        reasoning_effort.as_deref(),
        speed.as_deref(),
    )?;
    let cmd = build_ssh_remote_command(&connection, remote_command);
    let hint_filter = host_key_failure_hint_filter(&connection);
    spawn_remote_task_pty(
        app,
        &task_manager,
        &task_id,
        cmd,
        cols,
        rows,
        on_output,
        initial_prelude,
        initial_prompt,
        Some(hint_filter),
    )
}

#[tauri::command]
pub async fn resume_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    mut connection: SshConnection,
    remote_project_path: String,
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
    // 明文密码由后端按 id 自取,不经渲染进程。已带明文(未保存的新连接)时不覆盖。
    hydrate_ssh_password(&mut connection)?;
    let is_codex = is_remote_codex_like_agent(&agent);
    let selected_model = crate::pty::normalized_selected_model(selected_model.as_deref());
    let reasoning_effort = crate::pty::normalized_reasoning_effort(
        reasoning_effort.as_deref(),
        is_codex,
        selected_model.as_deref(),
    )?;
    let speed = crate::pty::normalized_speed(speed.as_deref())?;
    let uses_ultracode = !is_codex && reasoning_effort.as_deref() == Some("ultracode");
    let initial_prelude = uses_ultracode.then(crate::pty::initial_ultracode_command);
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    ensure_host_key_known(&connection)?;
    // 见 `run_remote_task`:恢复命令同样按远端 OS 分流。
    let task_os = resolve_remote_task_shell_os(&connection, &remote_project_path).await;
    let remote_command = build_remote_resume_command(
        task_os,
        &agent,
        &permission_mode,
        &remote_project_path,
        &session_id,
        selected_model.as_deref(),
        reasoning_effort.as_deref(),
        speed.as_deref(),
    )?;
    let cmd = build_ssh_remote_command(&connection, remote_command);
    let hint_filter = host_key_failure_hint_filter(&connection);
    spawn_remote_task_pty(
        app,
        &task_manager,
        &task_id,
        cmd,
        cols,
        rows,
        on_output,
        initial_prelude,
        None,
        Some(hint_filter),
    )
}

#[tauri::command]
pub async fn cancel_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    let child_arc = {
        let mut cancelled_tasks = task_manager.cancelled_tasks.lock();
        cancelled_tasks.insert(task_id.clone());
        task_manager.child_handles.lock().get(&task_id).cloned()
    };
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    } else {
        task_manager.cancelled_tasks.lock().remove(&task_id);
        task_manager.remove_pty_handles(&task_id);
    }
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
    fn ssh_connection_deserializes_without_optional_fields() {
        let raw = r#"{
          "id":"conn-1",
          "name":"prod",
          "host":"prod.example.com",
          "port":22,
          "username":"deploy",
          "createdAt":1700000000000
        }"#;

        let connection: SshConnection = serde_json::from_str(raw).unwrap();

        assert_eq!(connection.identity_file, None);
        assert_eq!(connection.remote_path, None);
        assert_eq!(connection.last_connected_at, None);
    }

    #[test]
    fn host_key_failures_carry_remediation_and_other_errors_pass_through() {
        let connection = SshConnection {
            id: "conn-1".to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port: 2200,
            username: "deploy".to_string(),
            identity_file: None,
            password: None,
            has_password: false,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        };

        let annotated = annotate_ssh_error(&connection, "Host key verification failed.");
        assert!(annotated.contains("Host key verification failed."));
        assert!(annotated.contains("StrictHostKeyChecking=yes"));
        // Non-default ports must be bracketed the way known_hosts records them.
        assert!(annotated.contains("[prod.example.com]:2200"));
        assert!(annotated.contains("ssh-keyscan -p 2200 prod.example.com"));

        // An unrelated failure must not gain host-key advice.
        let unrelated = annotate_ssh_error(&connection, "Permission denied (publickey).");
        assert_eq!(unrelated, "Permission denied (publickey).");
    }

    #[test]
    fn persisted_ssh_connections_keep_passwords_out_of_public_json() {
        let (public, passwords) = prepare_ssh_connections_for_storage(
            vec![SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            }],
            &BTreeMap::new(),
        );

        let public_json = serde_json::to_string(&public).unwrap();

        assert!(!public_json.contains("secret"));
        assert!(!public_json.contains("password"));
        assert_eq!(passwords.get("conn-1"), Some(&"secret".to_string()));
    }

    #[test]
    fn removing_ssh_connection_drops_the_record_and_its_password() {
        let connections = vec![
            SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "old.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("old-secret".to_string()),
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            SshConnection {
                id: "conn-2".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "new.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("new-secret".to_string()),
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 2,
                last_connected_at: None,
            },
        ];

        let remaining = remove_ssh_connection(connections, "conn-1");
        let (public, passwords) = prepare_ssh_connections_for_storage(remaining, &BTreeMap::new());

        assert_eq!(public.len(), 1);
        assert_eq!(public[0].id, "conn-2");
        assert_eq!(public[0].name, "prod");
        assert!(!passwords.contains_key("conn-1"));
        assert_eq!(passwords.get("conn-2"), Some(&"new-secret".to_string()));
    }

    /// 落盘侧刻意把密码摘出去单独存 0o600 文件。读取侧若把明文合并回连接对象整体交给
    /// 渲染进程,那个拆分就白做了 —— 全部主机的登录凭据(含 sudo 口令)常驻 WebView 堆。
    #[test]
    fn public_connections_carry_only_a_has_password_flag() {
        let stored = BTreeMap::from([("conn-1".to_string(), "secret".to_string())]);
        let (public, _) = prepare_ssh_connections_for_storage(
            vec![SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                has_password: true,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            }],
            &stored,
        );
        let json = serde_json::to_string(&public).unwrap();
        assert!(!json.contains("secret"), "公共 JSON 不得含明文: {json}");
        // 派生标记也不落盘 —— 否则它会与密码文件独立漂移。
        assert!(!json.contains("hasPassword"), "派生标记不该落盘: {json}");
    }

    /// 这条防的是一个**数据丢失**回归:`load_ssh_connections` 不再返回明文之后,前端手里
    /// 的连接对象 `password` 恒为 `None`。若保存时照旧从零重建密码表,用户改一次备注就会
    /// 把所有主机的密码抹掉。
    #[test]
    fn saving_without_a_password_keeps_the_stored_one() {
        let stored = BTreeMap::from([
            ("conn-1".to_string(), "kept".to_string()),
            ("conn-2".to_string(), "old".to_string()),
        ]);
        let connection = |id: &str, password: Option<&str>| SshConnection {
            id: id.to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            password: password.map(str::to_string),
            has_password: false,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        };

        let (_, passwords) = prepare_ssh_connections_for_storage(
            vec![
                // 前端回传的形态:密码位为空。
                connection("conn-1", None),
                // 用户真的敲了新密码:必须覆盖。
                connection("conn-2", Some("fresh")),
                // 空串与 None 同义(既有语义)。
                connection("conn-3", Some("")),
            ],
            &stored,
        );

        assert_eq!(passwords.get("conn-1"), Some(&"kept".to_string()));
        assert_eq!(passwords.get("conn-2"), Some(&"fresh".to_string()));
        assert_eq!(passwords.get("conn-3"), None);
    }

    /// 未保存的新连接在"测试连接"时带着用户刚敲进去的密码。补水不得覆盖它。
    #[test]
    fn hydration_never_overwrites_an_explicit_password() {
        let mut connection = SshConnection {
            id: "conn-1".to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            password: Some("typed-just-now".to_string()),
            has_password: false,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        };
        hydrate_ssh_password(&mut connection).expect("hydrate");
        assert_eq!(connection.password.as_deref(), Some("typed-just-now"));
    }

    #[test]
    fn ssh_args_include_default_port_and_target() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: None,
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(
            args,
            vec![
                "-tt",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "IPQoS=none",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "TCPKeepAlive=yes",
                "-p",
                "22",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn ssh_args_include_identity_file() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 2200,
                username: "deploy".to_string(),
                identity_file: Some("/Users/me/.ssh/prod key".to_string()),
                password: None,
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(
            args,
            vec![
                "-tt",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "IPQoS=none",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "TCPKeepAlive=yes",
                "-p",
                "2200",
                "-i",
                "/Users/me/.ssh/prod key",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn shell_quote_posix_escapes_single_quotes() {
        assert_eq!(
            shell_quote_posix("/srv/app's repo"),
            "'/srv/app'\\''s repo'"
        );
    }

    #[test]
    fn remote_command_changes_directory_before_login_shell() {
        assert_eq!(
            build_remote_start_command("/srv/aeroric app"),
            "cd -- '/srv/aeroric app' && exec \"${SHELL:-/bin/sh}\" -l"
        );
    }

    /// 断言落在解出来的 PowerShell 正文上,而不是 base64 串上:base64 让"内容对不对"
    /// 这件事在 diff 里完全不可读。规则与 `remote_os` 里那条同名意图的测试一致。
    fn decode_powershell_payload(command: &str) -> String {
        decode_powershell_script(command, "powershell -NoProfile -NoExit -EncodedCommand ")
    }

    /// 一次性脚本那一版(`-NonInteractive`):远端 Agent 任务用它。
    fn decode_task_powershell_payload(command: &str) -> String {
        decode_powershell_script(
            command,
            "powershell -NoProfile -NonInteractive -EncodedCommand ",
        )
    }

    fn decode_powershell_script(command: &str, prefix: &str) -> String {
        let payload = command
            .strip_prefix(prefix)
            .unwrap_or_else(|| panic!("expected {prefix:?} to lead the command"));
        let bytes = crate::remote_os::tests_decode_base64(payload);
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).expect("utf16 payload")
    }

    fn windows_connection(host: &str, remote_path: &str) -> SshConnection {
        SshConnection {
            id: format!("conn-{host}"),
            name: "win".to_string(),
            group: None,
            host: host.to_string(),
            port: 22,
            username: "administrator".to_string(),
            identity_file: None,
            password: Some("123".to_string()),
            has_password: true,
            remote_path: Some(remote_path.to_string()),
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn windows_remote_start_command_uses_encoded_powershell() {
        let command = build_windows_remote_start_command("C:\\Users\\Administrator\\Documents");

        assert_eq!(
            decode_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\Users\\Administrator\\Documents'"
        );
    }

    /// 回归:cmd.exe 不认 `--`、也不认单引号,路径里再出现空格或引号时更没有出路。
    /// base64 之后命令行里只剩字母数字,这些字符一个都不会到达 shell 解析层。
    #[test]
    fn windows_remote_start_command_survives_paths_cmd_cannot_parse() {
        let command = build_windows_remote_start_command("C:\\Program Files\\it's here");

        assert!(!command.contains('"'));
        assert!(!command.contains('\''));
        assert_eq!(
            decode_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\Program Files\\it''s here'"
        );
    }

    #[test]
    fn windows_start_command_ignores_auto_sudo() {
        let windows = build_remote_start_command_for(RemoteOs::Windows, "C:\\srv\\app", true);
        assert!(windows.starts_with("powershell -NoProfile -NoExit -EncodedCommand "));
        assert!(!windows.contains("sudo"));

        let posix = build_remote_start_command_for(RemoteOs::Posix, "/srv/app", true);
        assert!(posix.starts_with("cd -- '/srv/app'"));
        assert!(posix.contains("sudo -S"));
    }

    #[test]
    fn windows_connection_ssh_args_carry_powershell_start_command() {
        let connection =
            windows_connection("win.example.com", "C:\\Users\\Administrator\\Documents");

        let args = build_ssh_args_for_os(&connection, true, RemoteOs::Windows);
        let start = args.last().expect("start command");
        assert!(start.starts_with("powershell -NoProfile -NoExit -EncodedCommand "));
        assert!(!args.iter().any(|arg| arg.contains("cd --")));

        // 同一份连接在 POSIX 判定下仍是原来那条命令:分流没有污染默认路径。
        let posix_args = build_ssh_args_for_os(&connection, true, RemoteOs::Posix);
        assert_eq!(
            posix_args.last().unwrap(),
            "cd -- 'C:\\Users\\Administrator\\Documents' && exec \"${SHELL:-/bin/sh}\" -l"
        );
    }

    /// 探测失败时兜底看路径形态。每个用例传自己的 `host`:探测结果按 `user@host:port`
    /// 缓存,共用一个 host 会让并行跑的用例互相看见对方的缓存条目。
    #[test]
    fn failed_probe_falls_back_to_path_shape() {
        let windows = windows_connection("probe-fallback-win.test", "C:\\Users\\Administrator");
        assert_eq!(
            resolve_remote_shell_os_for(
                &windows,
                windows.remote_path.as_deref(),
                |_, _| Err("boom".to_string())
            ),
            RemoteOs::Windows
        );

        let posix = windows_connection("probe-fallback-posix.test", "/srv/app");
        assert_eq!(
            resolve_remote_shell_os_for(
                &posix,
                posix.remote_path.as_deref(),
                |_, _| Err("boom".to_string())
            ),
            RemoteOs::Posix
        );

        let no_path = SshConnection {
            remote_path: None,
            ..windows_connection("probe-fallback-none.test", "/srv/app")
        };
        assert_eq!(
            resolve_remote_shell_os_for(
                &no_path,
                no_path.remote_path.as_deref(),
                |_, _| Err("boom".to_string())
            ),
            RemoteOs::Posix
        );
    }

    /// 兜底路径是调用方给的,不是固定读 `connection.remote_path`:远端 Agent 任务用的是它
    /// 自己的 `remote_project_path`。连接没配远端路径、而任务给了 `C:\...` 时,探测失败
    /// 仍必须判 Windows —— 否则任务会照发 POSIX 的 `cd -- 'C:\...'`。
    #[test]
    fn fallback_uses_the_path_the_caller_supplied() {
        let connection = SshConnection {
            remote_path: None,
            ..windows_connection("probe-fallback-caller.test", "/srv/app")
        };

        assert_eq!(
            resolve_remote_shell_os_for(
                &connection,
                Some("C:\\Users\\Administrator\\repo"),
                |_, _| Err("boom".to_string())
            ),
            RemoteOs::Windows
        );
        // 对照:改用连接自己的路径(没配)时落回 POSIX。
        assert_eq!(
            resolve_remote_shell_os_for(
                &connection,
                connection.remote_path.as_deref(),
                |_, _| Err("boom".to_string())
            ),
            RemoteOs::Posix
        );
    }

    /// 探测成功时以探测结果为准:MSYS / Cygwin 的 `uname -s` 命中 POSIX 列表,
    /// 即便配置里写的是 `C:\...` 也应该继续发 `sh` 语法。
    #[test]
    fn successful_probe_overrides_path_shape() {
        let connection = windows_connection("probe-wins.test", "C:\\Users\\Administrator");
        assert_eq!(
            resolve_remote_shell_os_for(
                &connection,
                connection.remote_path.as_deref(),
                |_, _| Ok(b"Linux\n".to_vec())
            ),
            RemoteOs::Posix
        );
    }

    /// 空白远端路径与"没配路径"等价:两者都不该发起始命令,也不该为它去探测 OS。
    #[test]
    fn blank_remote_path_means_no_start_command() {
        let connection = windows_connection("blank-path.test", "   ");
        assert_eq!(remote_start_path(&connection), None);

        let args = build_ssh_args_for_os(&connection, true, RemoteOs::Windows);
        assert_eq!(args.last().unwrap(), "administrator@blank-path.test");
    }

    #[test]
    fn remote_start_path_trims_surrounding_whitespace() {
        let connection = windows_connection("trim-path.test", "  C:\\Users\\Administrator  ");
        assert_eq!(
            remote_start_path(&connection),
            Some("C:\\Users\\Administrator")
        );
    }

    #[test]
    fn remote_start_command_can_enter_sudo_shell_with_saved_password() {
        assert_eq!(
            build_remote_start_command_with_sudo("/srv/aeroric app"),
            "cd -- '/srv/aeroric app' && trap 'stty echo' EXIT HUP INT TERM && stty -echo && printf '%s\\n' '__AERORIC_SUDO_PASSWORD_READY__' && IFS= read -r aeroric_sudo_password && stty echo && trap - EXIT HUP INT TERM && printf '\\n' && printf '%s\\n' \"$aeroric_sudo_password\" | sudo -S -p '' -v && unset aeroric_sudo_password && exec sudo -n \"${SHELL:-/bin/sh}\" -l"
        );
    }

    #[test]
    fn ssh_command_uses_auto_sudo_only_for_non_root_password_connections() {
        let connection = SshConnection {
            id: "conn-1".to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            password: Some("secret".to_string()),
            has_password: false,
            remote_path: Some("/srv/app".to_string()),
            auto_sudo_with_password: true,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        };

        let spec = ssh_command_spec(&connection, None, true);

        assert!(spec
            .args
            .iter()
            .any(|arg| arg.contains("exec sudo -n \"${SHELL:-/bin/sh}\" -l")));
        assert!(spec
            .args
            .iter()
            .any(|arg| arg.contains("__AERORIC_SUDO_PASSWORD_READY__")));
        assert!(!spec.args.iter().any(|arg| arg.contains("secret")));
        assert_eq!(
            spec.env,
            vec![("SSHPASS".to_string(), "secret".to_string())]
        );
    }

    #[test]
    fn ssh_args_keep_target_as_single_ssh_argument() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com; touch /tmp/bad".to_string(),
                port: 22,
                username: "deploy && whoami".to_string(),
                identity_file: None,
                password: None,
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(
            args.last().unwrap(),
            "deploy && whoami@prod.example.com; touch /tmp/bad"
        );
        assert_eq!(args.len(), 14);
    }

    #[test]
    fn ssh_command_spec_uses_sshpass_env_for_passwords() {
        let spec = ssh_command_spec_for_remote_command(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            "echo ok".to_string(),
        );

        assert!(spec.program.ends_with("sshpass"));
        assert_eq!(spec.args[0], "-e");
        assert_eq!(spec.args[1], "ssh");
        assert!(spec.args.iter().any(|arg| arg == "-T"));
        assert!(!spec.args.iter().any(|arg| arg == "-tt"));
        assert_eq!(spec.args.last().map(String::as_str), Some("echo ok"));
        assert_eq!(
            spec.env,
            vec![("SSHPASS".to_string(), "secret".to_string())]
        );
    }

    #[test]
    fn ssh_command_spec_disables_publickey_for_passwords() {
        let spec = ssh_command_spec_for_remote_command(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                has_password: false,
                remote_path: None,
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            "echo ok".to_string(),
        );

        assert!(spec.args.windows(2).any(|pair| pair
            == [
                "-o",
                "PreferredAuthentications=password,keyboard-interactive"
            ]));
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["-o", "PubkeyAuthentication=no"]));
    }

    #[test]
    fn ssh_port_forward_spec_places_forward_before_target() {
        let spec = ssh_port_forward_command_spec(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 2200,
                username: "deploy".to_string(),
                identity_file: None,
                password: None,
                has_password: false,
                remote_path: Some("/srv/app".to_string()),
                auto_sudo_with_password: false,
                use_proxy: false,
                created_at: 1,
                last_connected_at: None,
            },
            49152,
            "127.0.0.1",
            5678,
        );

        assert_eq!(spec.program, "ssh");
        assert_eq!(
            spec.args,
            vec![
                "-T",
                "-o",
                "StrictHostKeyChecking=yes",
                "-p",
                "2200",
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-L",
                "127.0.0.1:49152:127.0.0.1:5678",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn remote_claude_task_command_maps_permission_and_quotes_prompt() {
        assert_eq!(
            build_remote_task_command(
                RemoteOs::Posix,
                "claude",
                "auto_edit",
                "/srv/app's repo",
                Some("fix Bob's bug"),
                None,
                None,
                None,
            )
            .unwrap(),
            "cd -- '/srv/app'\\''s repo' && 'claude' --permission-mode acceptEdits -- 'fix Bob'\\''s bug'"
        );
    }

    #[test]
    fn remote_codex_task_command_uses_sandbox_flags_and_separator() {
        assert_eq!(
            build_remote_task_command(
                RemoteOs::Posix,
                "codex",
                "auto_edit",
                "/srv/app",
                Some("inspect status"),
                None,
                None,
                None,
            )
                .unwrap(),
            "cd -- '/srv/app' && 'codex' --sandbox workspace-write -a on-request -- 'inspect status'"
        );
    }

    /// omp 的 flag 名与其余三族都不同,且 prompt 是位置参数所以必须有 `--` 分隔。
    /// 回归:omp 曾走 claude 那支,被推一个它不认识的 `--effort`、且没有 `--`。
    #[test]
    fn remote_omp_task_command_uses_omp_flags_and_separator() {
        assert_eq!(
            build_remote_task_command(
                RemoteOs::Posix,
                "omp",
                "auto_edit",
                "/srv/app",
                Some("inspect status"),
                Some("gpt-5"),
                Some("minimal"),
                None,
            )
            .unwrap(),
            // `AERORIC_AGENT_MODEL` 由 `build_remote_command` 对所有家族统一前置。
            "cd -- '/srv/app' && AERORIC_AGENT_MODEL=gpt-5 'omp' --approval-mode write --model gpt-5 --thinking minimal -- 'inspect status'"
        );
    }

    /// omp 没有 `--effort`,也不认 claude 的 `ultracode`;`ultra` 封顶到 `max`。
    #[test]
    fn remote_omp_never_receives_claude_effort_vocabulary() {
        let args = remote_agent_args("omp", "full_access", None, Some("ultra"), Some("fast"));
        assert_eq!(
            args,
            vec![
                "--approval-mode".to_string(),
                "yolo".to_string(),
                "--thinking".to_string(),
                "max".to_string(),
            ]
        );
        assert!(!args.iter().any(|arg| arg == "--effort"));
        // omp 没有 speed 概念,`fast` 不该漏出任何 flag。
        assert!(!args.iter().any(|arg| arg == "--settings"));
    }

    /// 这是"SSH 项目上 omp 任务永远起不来"那条 bug 的直接断言:family 推断把 omp 判成
    /// claude 时,`normalized_reasoning_effort_for` 会用 claude 词表拒掉 `off`/`minimal`。
    #[test]
    fn remote_agent_family_maps_all_four_families() {
        use crate::app_settings::AgentFamily;
        assert_eq!(remote_agent_family("omp"), AgentFamily::Omp);
        assert_eq!(remote_agent_family("dsh"), AgentFamily::Dsh);
        assert_eq!(remote_agent_family("codex"), AgentFamily::Codex);
        assert_eq!(remote_agent_family("claude_gpt55"), AgentFamily::Codex);
        assert_eq!(remote_agent_family("claude"), AgentFamily::Claude);

        // 端到端:omp 的 off/minimal 必须被判合法,ultracode 必须被拒。
        for level in ["off", "minimal", "max"] {
            assert!(
                crate::pty::normalized_reasoning_effort_for(
                    Some(level),
                    remote_agent_family("omp"),
                    None
                )
                .is_ok(),
                "omp 应接受 {level}"
            );
        }
        assert!(crate::pty::normalized_reasoning_effort_for(
            Some("ultracode"),
            remote_agent_family("omp"),
            None
        )
        .is_err());
    }

    #[test]
    fn remote_claude_gpt55_uses_script_with_codex_compatible_args() {
        assert_eq!(
            build_remote_task_command(
                RemoteOs::Posix,
                "claude_gpt55",
                "full_access",
                "/srv/app",
                Some("inspect status"),
                None,
                None,
                None,
            )
            .unwrap(),
            "cd -- '/srv/app' && \"$HOME/.claude/start-gpt55.sh\" --dangerously-bypass-approvals-and-sandbox -- 'inspect status'"
        );
    }

    #[test]
    fn remote_dsh_uses_headless_only_when_a_prompt_is_present() {
        assert_eq!(
            build_remote_task_command(
                RemoteOs::Posix,
                "dsh",
                "auto_edit",
                "/srv/app",
                Some("inspect status"),
                None,
                Some("high"),
                None,
            )
            .unwrap(),
            "cd -- '/srv/app' && DSH_PERMISSION_MODE=workspace-write DSH_TELEMETRY_DISABLED=1 'dsh' --profile headless -- 'inspect status'"
        );
        assert_eq!(
            build_remote_task_command(RemoteOs::Posix, "dsh", "ask", "/srv/app", Some("   "), None, None, None,)
                .unwrap(),
            "cd -- '/srv/app' && DSH_PERMISSION_MODE=read-only DSH_TELEMETRY_DISABLED=1 'dsh'"
        );
    }

    #[test]
    fn remote_resume_command_uses_agent_specific_session_flags() {
        assert_eq!(
            build_remote_resume_command(
                RemoteOs::Posix,
                "claude",
                "ask",
                "/srv/app",
                "claude-session",
                None,
                None,
                None,
            )
            .unwrap(),
            "cd -- '/srv/app' && 'claude' --permission-mode default --resume claude-session"
        );
        assert_eq!(
            build_remote_resume_command(
                RemoteOs::Posix,
                "codex",
                "full_access",
                "/srv/app",
                "codex-session",
                None,
                None,
                None,
            )
            .unwrap(),
            "cd -- '/srv/app' && 'codex' --dangerously-bypass-approvals-and-sandbox resume codex-session"
        );
        assert!(build_remote_resume_command(
            RemoteOs::Posix,
            "dsh",
            "ask",
            "/srv/app",
            "dsh-session",
            None,
            None,
            None,
        )
        .is_err());
    }

    /// Windows 远端上的 Agent 任务:整串必须走 PowerShell。
    ///
    /// 回归:cmd.exe 不认 `--`、也不认单引号,`cd -- 'C:\...' && 'claude' ...` 只会换来
    /// 「文件名、目录名或卷标语法不正确。」,sshd 随即结束会话。
    #[test]
    fn windows_remote_task_command_runs_through_powershell() {
        let command = build_remote_task_command(
            RemoteOs::Windows,
            "claude",
            "auto_edit",
            "C:\\Users\\Administrator\\my repo",
            Some("fix Bob's bug"),
            None,
            None,
            None,
        )
        .unwrap();

        assert!(command.starts_with("powershell -NoProfile -NonInteractive -EncodedCommand "));
        assert!(!command.contains("cd --"));
        assert_eq!(
            decode_task_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\Users\\Administrator\\my repo'; & 'claude' '--permission-mode' 'acceptEdits' '--' 'fix Bob''s bug'"
        );
    }

    /// Windows 上环境变量是**独立语句**(`$env:X = 'v'`),必须用 `;` 分隔;而且可执行文件
    /// 要用调用运算符 `&` 启动 —— 少了 `&`,PowerShell 只会把 `'dsh'` 这个字符串原样输出
    /// 到 stdout,任务看起来"启动了"却什么都没做。
    #[test]
    fn windows_remote_task_command_separates_env_assignments() {
        let command = build_remote_task_command(
            RemoteOs::Windows,
            "dsh",
            "auto_edit",
            "C:\\srv\\app",
            Some("inspect status"),
            None,
            Some("high"),
            None,
        )
        .unwrap();

        assert_eq!(
            decode_task_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\srv\\app'; $env:DSH_PERMISSION_MODE = 'workspace-write'; $env:DSH_TELEMETRY_DISABLED = '1'; & 'dsh' '--profile' 'headless' '--' 'inspect status'"
        );
    }

    /// `AERORIC_AGENT_MODEL` 对所有家族统一前置:POSIX 上是命令前缀,Windows 上是独立语句。
    #[test]
    fn windows_remote_task_command_prefixes_the_model_env_var() {
        let command = build_remote_task_command(
            RemoteOs::Windows,
            "omp",
            "auto_edit",
            "C:\\srv\\app",
            Some("inspect status"),
            Some("gpt-5"),
            Some("minimal"),
            None,
        )
        .unwrap();

        assert_eq!(
            decode_task_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\srv\\app'; $env:AERORIC_AGENT_MODEL = 'gpt-5'; & 'omp' '--approval-mode' 'write' '--model' 'gpt-5' '--thinking' 'minimal' '--' 'inspect status'"
        );
    }

    /// 恢复命令与启动命令走同一套 OS 分流。
    #[test]
    fn windows_remote_resume_command_runs_through_powershell() {
        let command = build_remote_resume_command(
            RemoteOs::Windows,
            "codex",
            "full_access",
            "C:\\srv\\app",
            "codex-session",
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            decode_task_powershell_payload(&command),
            "Set-Location -LiteralPath 'C:\\srv\\app'; & 'codex' '--dangerously-bypass-approvals-and-sandbox' 'resume' 'codex-session'"
        );
    }

    /// `claude_gpt55` 是本机的 POSIX 启动脚本,Windows 远端上不存在。明确报错好过发一条
    /// 注定 `not recognized` 的命令让用户去猜。同一份输入在 POSIX 上仍照发脚本。
    #[test]
    fn windows_remote_task_rejects_the_posix_only_gpt55_profile() {
        let error = build_remote_task_command(
            RemoteOs::Windows,
            "claude_gpt55",
            "ask",
            "C:\\srv\\app",
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert!(error.contains("claude_gpt55"));
        assert!(build_remote_task_command(
            RemoteOs::Posix,
            "claude_gpt55",
            "ask",
            "/srv/app",
            None,
            None,
            None,
            None,
        )
        .is_ok());
    }

    #[test]
    fn remote_agent_id_rejects_shell_metacharacters() {
        assert!(validate_remote_agent_id("claude; touch /tmp/pwn").is_err());
        assert!(build_remote_task_command(
            RemoteOs::Posix,
            "custom_agent",
            "ask",
            "/srv/app",
            None,
            None,
            None,
            None,
        )
        .is_ok());
    }

    #[test]
    fn remote_task_command_forwards_model_reasoning_and_speed() {
        let command = build_remote_task_command(
            RemoteOs::Posix,
            "codex",
            "auto_edit",
            "/srv/app",
            Some("inspect status"),
            Some("gpt-5.6-terra"),
            Some("high"),
            Some("fast"),
        )
        .unwrap();
        assert!(command.contains("AERORIC_AGENT_MODEL=gpt-5.6-terra"));
        assert!(command.contains("-m gpt-5.6-terra"));
        assert!(command.contains("model_reasoning_effort=\"high\""));
        assert!(command.contains("features.fast_mode=true"));
        assert!(command.contains("service_tier=\"fast\""));
    }

    #[test]
    fn remote_claude_fast_mode_uses_settings_json() {
        let command =
            build_remote_task_command(
                RemoteOs::Posix, "claude", "ask", "/srv/app", None, None, None, Some("fast")
            )
                .unwrap();
        let unsupported_fast_flag = ["--", "fast"].concat();

        assert!(command.contains("--settings '{\"fastMode\":true}'"));
        assert!(!command.contains(&unsupported_fast_flag));
    }

    #[test]
    fn sudo_password_filter_sends_secret_only_after_ready_marker() {
        struct SharedWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

        impl Write for SharedWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let captured = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>> = Arc::new(
            parking_lot::Mutex::new(Box::new(SharedWriter(captured.clone()))),
        );
        let mut filter = sudo_password_output_filter(writer, "sec'ret".to_string());

        assert_eq!(
            filter("banner __AERORIC_SUDO_".to_string()),
            Some("banner ".to_string())
        );
        assert_eq!(
            filter("PASSWORD_READY__\r\n".to_string()),
            Some("\r\n".to_string())
        );
        assert_eq!(captured.lock().as_slice(), b"sec'ret\n");
        assert_eq!(
            filter("shell ready\n".to_string()),
            Some("shell ready\n".to_string())
        );
    }

    fn test_connection(port: u16) -> SshConnection {
        SshConnection {
            id: "conn-1".to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port,
            username: "deploy".to_string(),
            identity_file: None,
            password: None,
            has_password: false,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        }
    }

    /// PTY 路径不经过 `annotate_ssh_error`,用户原本只能看到 ssh 那句没有出路的
    /// 报错。过滤器必须原样放行数据并在后面追加补救说明。
    #[test]
    fn hint_filter_appends_remediation_to_host_key_failure() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        let out = filter("Host key verification failed.\r\n".to_string()).expect("passes through");

        assert!(out.starts_with("Host key verification failed.\r\n"));
        assert!(out.contains("StrictHostKeyChecking=yes"));
        assert!(out.contains("ssh-keygen -R prod.example.com"));
        // PTY 是裸终端,补充说明里不能留裸 \n。
        assert!(!out.replace("\r\n", "").contains('\n'));
    }

    /// 用户实际遇到的那句报错(`No ED25519 host key is known for ...`)也必须命中。
    #[test]
    fn hint_filter_catches_the_unknown_host_wording() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        let out = filter(
            "No ED25519 host key is known for prod.example.com and you have requested strict checking.\r\n"
                .to_string(),
        )
        .expect("passes through");

        assert!(out.contains("open it from the SSH panel"));
    }

    /// PTY 每次读取的边界是任意的,报错常被切成两半。
    #[test]
    fn hint_filter_matches_phrases_split_across_chunks() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        let first = filter("Host key verifi".to_string()).expect("passes through");
        assert_eq!(first, "Host key verifi");
        assert!(!first.contains("StrictHostKeyChecking"));

        let second = filter("cation failed.\r\n".to_string()).expect("passes through");
        assert!(second.contains("StrictHostKeyChecking=yes"));
    }

    #[test]
    fn hint_filter_fires_once_and_then_passes_data_through() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        let first = filter("Host key verification failed.".to_string()).expect("passes through");
        assert!(first.contains("StrictHostKeyChecking=yes"));

        let second = filter("Host key verification failed.".to_string()).expect("passes through");
        assert_eq!(second, "Host key verification failed.");
    }

    #[test]
    fn hint_filter_leaves_normal_output_untouched() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        assert_eq!(
            filter("Permission denied (publickey).\r\n".to_string()),
            Some("Permission denied (publickey).\r\n".to_string())
        );
    }

    /// 远程终端里有中文输出,窗口裁剪必须落在字符边界上,否则会 panic。
    #[test]
    fn hint_filter_survives_multibyte_output() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        for _ in 0..40 {
            assert!(filter("正在构建项目…".to_string()).is_some());
        }

        let out = filter("Host key verification failed.".to_string()).expect("passes through");
        assert!(out.contains("StrictHostKeyChecking=yes"));
    }

    /// sudo 过滤器会吞掉 marker 并缓冲数据,串接后两个功能都不能失效。
    #[test]
    fn chained_filters_keep_both_behaviours() {
        struct NoopWriter;
        impl Write for NoopWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let connection = test_connection(22);
        let writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>> =
            Arc::new(parking_lot::Mutex::new(Box::new(NoopWriter)));
        let mut filter = chain_output_filters(
            sudo_password_output_filter(writer, "pw".to_string()),
            host_key_failure_hint_filter(&connection),
        );

        // marker 被 sudo 过滤器吃掉,不会流到终端。
        let out = filter("a__AERORIC_SUDO_PASSWORD_READY__b".to_string()).expect("passes through");
        assert_eq!(out, "ab");

        // host key 提示仍然生效。
        let failure = filter("Host key verification failed.".to_string()).expect("passes through");
        assert!(failure.contains("StrictHostKeyChecking=yes"));
    }

    /// host key 失败只出现在握手阶段。扫过预算还没命中就必须解除,否则整个会话
    /// 的每个 chunk 都要付一次全量 lowercase 堆分配 + 四次全串搜索,SSH 终端
    /// 输出越多越粘手。
    #[test]
    fn host_key_hint_filter_disarms_after_the_scan_budget() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        // 先灌满预算,期间数据原样透传。
        let chunk = "x".repeat(8 * 1024);
        let mut pushed = 0usize;
        while pushed < HOST_KEY_SCAN_BYTE_BUDGET {
            assert_eq!(filter(chunk.clone()).expect("passes through"), chunk);
            pushed += chunk.len();
        }

        // 解除之后即使真出现那句话,也不再追加提示 —— 这是刻意的取舍:
        // 握手早已结束,此时的字样只可能来自远端 shell 的普通输出。
        let out = filter("Host key verification failed.".to_string()).expect("passes through");
        assert_eq!(out, "Host key verification failed.");
    }

    /// 预算之内仍要命中,别把提示优化掉。
    #[test]
    fn host_key_hint_filter_still_fires_within_the_budget() {
        let connection = test_connection(22);
        let mut filter = host_key_failure_hint_filter(&connection);

        assert_eq!(
            filter("x".repeat(1024)).expect("passes through"),
            "x".repeat(1024)
        );
        let out = filter("Host key verification failed.".to_string()).expect("passes through");
        assert!(out.contains("StrictHostKeyChecking=yes"));
    }

    #[test]
    fn remediation_mentions_both_the_in_app_and_manual_route() {
        let remediation = host_key_remediation(&test_connection(2200));

        assert!(remediation.contains("open it from the SSH panel"));
        assert!(remediation.contains("ssh-keyscan -p 2200 prod.example.com"));
        assert!(remediation.contains("ssh-keygen -R [prod.example.com]:2200"));
    }

    /// 没勾代理的连接不能被塞进 ProxyCommand,否则所有既有连接的行为都变了。
    #[test]
    fn proxy_command_is_absent_unless_the_connection_opts_in() {
        let args = build_ssh_args(&test_connection(22), true);

        assert!(!args.iter().any(|arg| arg.starts_with("ProxyCommand=")));
    }

    /// 勾了代理就必须带上 ProxyCommand,并且用 `%h %p` 让 ssh 填真实目标 ——
    /// 写死主机名会让 known_hosts 校验和实际连接的对象脱钩。
    #[test]
    fn proxy_command_routes_through_our_own_bridge() {
        let connection = SshConnection {
            use_proxy: true,
            ..test_connection(2200)
        };

        let args = build_ssh_args(&connection, true);
        let proxy_command = args
            .iter()
            .find(|arg| arg.starts_with("ProxyCommand="))
            .expect("proxy command present");

        assert!(proxy_command.ends_with(" --ssh-proxy-bridge %h %p"));
        // StrictHostKeyChecking 不能因为走代理而被放宽。
        assert!(args.iter().any(|arg| arg == "StrictHostKeyChecking=yes"));
        // 目标仍然是真实主机,代理只改传输层。
        assert!(args.iter().any(|arg| arg == "deploy@prod.example.com"));
        assert!(args.iter().any(|arg| arg == "2200"));
    }

    /// 自身路径含空格(macOS 的 `/Applications/Aeroric.app/...` 就是)时,
    /// ssh 会把整个 ProxyCommand 交给 shell,没引用就会被切断。
    #[test]
    fn proxy_command_quotes_the_executable_path() {
        let Some(proxy_command) = proxy_command_arg() else {
            return;
        };
        let path_part = proxy_command
            .trim_start_matches("ProxyCommand=")
            .trim_end_matches(" --ssh-proxy-bridge %h %p");

        if cfg!(windows) {
            assert!(path_part.starts_with('"') && path_part.ends_with('"'));
        } else {
            assert!(path_part.starts_with('\'') && path_part.ends_with('\''));
        }
    }
}
