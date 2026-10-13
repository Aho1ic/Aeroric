//! P7-02 Action 1：platform 模块（cfg 隔离实现——不删除 Windows 分支；与
//! `src-tauri/src/platform/` 同源语义上收；平台探测纯函数）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 平台形态（macOS/Windows/Linux）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    MacOS,
    Windows,
    Linux,
}

/// 当前平台（编译期 cfg）。
pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

/// 平台能力快照（Action 3：core capability 在 macOS 为 `wsl=false`——**不得
/// 由 client OS 直接推断 host 能力**：未来远程 Windows host 可以声明 WSL）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub platform: Platform,
    /// WSL 可用（macOS/Linux＝false；Windows＝true）。
    pub wsl: bool,
    /// WSL 可用的原因（诊断用；macOS＝client OS 不推断 host 能力）。
    pub wsl_reason: String,
}

pub fn capabilities() -> PlatformCapabilities {
    match current_platform() {
        Platform::MacOS => PlatformCapabilities {
            platform: Platform::MacOS,
            wsl: false,
            wsl_reason: "client OS is macOS; WSL requires a Windows host".to_string(),
        },
        Platform::Windows => PlatformCapabilities {
            platform: Platform::Windows,
            wsl: true,
            wsl_reason: "client OS is Windows".to_string(),
        },
        Platform::Linux => PlatformCapabilities {
            platform: Platform::Linux,
            wsl: false,
            wsl_reason: "client OS is Linux; WSL requires a Windows host".to_string(),
        },
    }
}

/// client OS 推断 host 能力的守卫（Action 3：**不得由 client OS 直接推断
/// host 能力**——远程 Windows host 可以声明 WSL；这个函数只回答 client OS
/// 自身，不回答远程 host）。
pub fn client_os_declares_wsl() -> bool {
    cfg!(target_os = "windows")
}

// detect_path（P8-02：由登录 shell imp 分发——`which` 走登录 shell 的 PATH，
// 与宿主 `src-tauri/src/platform/` 同语义；原纯路径候选版已并入 imp）。
// ── P8-02：登录 shell 面与 detect_path（与宿主 `src-tauri/src/platform/` 同源
// 上收；settings 模块的 launch_spec/versions/agent_scripts 消费）。

#[cfg(not(windows))]
pub(crate) mod login_shell_unix;
#[cfg(windows)]
pub(crate) mod login_shell_windows;

#[cfg(not(windows))]
use self::login_shell_unix as imp;
#[cfg(windows)]
use self::login_shell_windows as imp;

pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn login_shell_env() -> &'static [(String, String)] {
    imp::login_shell_env()
}

pub fn login_shell_path() -> &'static str {
    imp::login_shell_path()
}

pub fn detect_path(binary: &str) -> String {
    imp::detect_path(binary)
}

/// Windows 上的脚本启动形态(`.ps1`/`.cmd`/`.bat` → 解释器 + 参数)。与
/// [`default_shell_command`] 同一转发形态(`imp` 别名按 cfg 指向
/// `login_shell_{unix,windows}`);Unix 侧没有等价物,所以只在 windows 目标上导出。
///
/// 为什么必须补这一条:core 自己的 `settings/launch_spec.rs` 就调
/// `crate::platform::agent_script_command`(P8-02 刀二把该文件上收时留下的调用),
/// 而 mod.rs 一直没转发它 ⇒ **windows 目标编译失败**,因为该分支只在 windows 上编译,
/// macOS/Linux 开发机从不编到那里。
#[cfg(windows)]
pub fn agent_script_command(script: &std::path::Path) -> Option<ShellCommand> {
    imp::agent_script_command(script)
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRuntimeInfo {
    pub os: String,
    pub arch: String,
    pub shell_kind: String,
    pub shell_label: String,
    pub path_separator: String,
    pub can_run_shell_scripts: bool,
    pub shell_script_unavailable_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunnableFileCommand {
    pub command: Option<String>,
    pub unavailable_reason: Option<String>,
}

pub fn default_shell_command() -> ShellCommand {
    imp::default_shell_command()
}

pub fn shell_kind_and_label(program: &str) -> (String, String) {
    let executable = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let file_name = executable
        .strip_suffix(".exe")
        .unwrap_or(executable)
        .to_ascii_lowercase();
    match file_name.as_str() {
        "pwsh" => ("powershell".to_string(), "PowerShell".to_string()),
        "powershell" => ("powershell".to_string(), "Windows PowerShell".to_string()),
        "cmd" => ("cmd".to_string(), "Command Prompt".to_string()),
        "bash" => ("bash".to_string(), "bash".to_string()),
        "zsh" => ("zsh".to_string(), "zsh".to_string()),
        "fish" => ("fish".to_string(), "fish".to_string()),
        "sh" => ("sh".to_string(), "sh".to_string()),
        _ => (file_name.clone(), file_name),
    }
}

pub fn current_platform_runtime_info() -> PlatformRuntimeInfo {
    let shell = default_shell_command();
    let (shell_kind, shell_label) = shell_kind_and_label(&shell.program);
    #[cfg(windows)]
    let can_run_shell_scripts = !detect_path("bash").is_empty();
    #[cfg(not(windows))]
    let can_run_shell_scripts = true;
    PlatformRuntimeInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        shell_kind,
        shell_label,
        path_separator: std::path::MAIN_SEPARATOR.to_string(),
        can_run_shell_scripts,
        shell_script_unavailable_reason: if can_run_shell_scripts {
            String::new()
        } else {
            "Git Bash is required to run legacy .sh Agent launchers on Windows.".to_string()
        },
    }
}

pub fn local_python_program() -> String {
    if let Some(configured) = std::env::var_os("PYTHON").filter(|value| !value.is_empty()) {
        return configured.to_string_lossy().into_owned();
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["python.exe", "python3.exe", "py.exe"]
    } else {
        &["python3", "python"]
    };
    for candidate in candidates {
        let detected = detect_path(candidate);
        if !detected.is_empty() {
            return detected;
        }
    }
    candidates[0].to_string()
}

fn validate_command_value(value: &str, label: &str) -> Result<(), String> {
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err(format!("{label} contains unsupported control characters"));
    }
    Ok(())
}

fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn validate_cmd_value(value: &str, label: &str) -> Result<(), String> {
    if value.contains('"') {
        return Err(format!("{label} cannot contain double quotes"));
    }
    if value.contains('%') {
        return Err(format!("{label} cannot contain '%' on Windows"));
    }
    Ok(())
}

fn quote_cmd(value: &str) -> Result<String, String> {
    validate_cmd_value(value, "Windows command paths")?;
    Ok(format!("\"{value}\""))
}

fn conda_bin_path(conda_path: &str, shell_kind: &str) -> String {
    if shell_kind == "powershell" || shell_kind == "cmd" {
        format!(
            "{};{}",
            PathBuf::from(conda_path).join("Scripts").to_string_lossy(),
            PathBuf::from(conda_path)
                .join("Library")
                .join("bin")
                .to_string_lossy()
        )
    } else {
        format!("{}/bin", conda_path.trim_end_matches('/'))
    }
}

fn build_remote_runnable_file_command(
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
) -> RunnableFileCommand {
    let lower = file_path.to_ascii_lowercase();
    let command = if lower.ends_with(".py") {
        let python = conda_python_path
            .filter(|path| !path.trim().is_empty())
            .unwrap_or("python3");
        format!("{} {}\r", quote_posix(python), quote_posix(file_path))
    } else if lower.ends_with(".sh") {
        match conda_path.filter(|path| !path.trim().is_empty()) {
            Some(path) => format!(
                "CONDA_PREFIX={} PATH={}:\"$PATH\" bash {}\r",
                quote_posix(path),
                quote_posix(&conda_bin_path(path, "sh")),
                quote_posix(file_path)
            ),
            None => format!("bash {}\r", quote_posix(file_path)),
        }
    } else {
        return RunnableFileCommand {
            command: None,
            unavailable_reason: Some("Only Python or shell scripts can be run.".to_string()),
        };
    };
    RunnableFileCommand {
        command: Some(command),
        unavailable_reason: None,
    }
}

fn build_local_runnable_file_command(
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
    shell_program: &str,
) -> Result<RunnableFileCommand, String> {
    let lower = file_path.to_ascii_lowercase();
    let (shell_kind, _) = shell_kind_and_label(shell_program);
    let command = if lower.ends_with(".py") {
        let python = conda_python_path
            .filter(|path| !path.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(local_python_program);
        match shell_kind.as_str() {
            "powershell" => format!(
                "& {} {}\r",
                quote_powershell(&python),
                quote_powershell(file_path)
            ),
            "cmd" => format!("{} {}\r", quote_cmd(&python)?, quote_cmd(file_path)?),
            _ => format!("{} {}\r", quote_posix(&python), quote_posix(file_path)),
        }
    } else if lower.ends_with(".sh") {
        #[cfg(windows)]
        let bash = detect_path("bash");
        #[cfg(not(windows))]
        let bash = {
            let detected = detect_path("bash");
            if detected.is_empty() {
                "bash".to_string()
            } else {
                detected
            }
        };
        if bash.is_empty() {
            return Ok(RunnableFileCommand {
                command: None,
                unavailable_reason: Some(
                    "Git Bash is required to run .sh files on Windows.".to_string(),
                ),
            });
        }
        match (
            shell_kind.as_str(),
            conda_path.filter(|path| !path.trim().is_empty()),
        ) {
            ("powershell", Some(path)) => format!(
                "$env:CONDA_PREFIX = {}; $env:PATH = {} + ';' + $env:PATH; & {} {}\r",
                quote_powershell(path),
                quote_powershell(&conda_bin_path(path, "powershell")),
                quote_powershell(&bash),
                quote_powershell(file_path)
            ),
            ("powershell", None) => format!(
                "& {} {}\r",
                quote_powershell(&bash),
                quote_powershell(file_path)
            ),
            ("cmd", Some(path)) => {
                validate_cmd_value(path, "Conda path")?;
                format!(
                    "set \"CONDA_PREFIX={path}\" && set \"PATH={};%PATH%\" && {} {}\r",
                    conda_bin_path(path, "cmd"),
                    quote_cmd(&bash)?,
                    quote_cmd(file_path)?
                )
            }
            ("cmd", None) => format!("{} {}\r", quote_cmd(&bash)?, quote_cmd(file_path)?),
            (_, Some(path)) => format!(
                "CONDA_PREFIX={} PATH={}:\"$PATH\" {} {}\r",
                quote_posix(path),
                quote_posix(&conda_bin_path(path, &shell_kind)),
                quote_posix(&bash),
                quote_posix(file_path)
            ),
            (_, None) => format!("{} {}\r", quote_posix(&bash), quote_posix(file_path)),
        }
    } else {
        return Ok(RunnableFileCommand {
            command: None,
            unavailable_reason: Some("Only Python or shell scripts can be run.".to_string()),
        });
    };
    Ok(RunnableFileCommand {
        command: Some(command),
        unavailable_reason: None,
    })
}

pub fn runnable_file_command(
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
    remote: bool,
    shell_program: &str,
) -> Result<RunnableFileCommand, String> {
    validate_command_value(file_path, "File path")?;
    if let Some(value) = conda_path {
        validate_command_value(value, "Conda path")?;
    }
    if let Some(value) = conda_python_path {
        validate_command_value(value, "Conda Python path")?;
    }
    if remote {
        return Ok(build_remote_runnable_file_command(
            file_path,
            conda_path,
            conda_python_path,
        ));
    }
    build_local_runnable_file_command(file_path, conda_path, conda_python_path, shell_program)
}

// ── P8-02：node_runtime 的纯探测半边上收（宿主 `node_runtime.rs::detect_node`
// 同源；`agent_scripts`/`agent_tools/node_bootstrap` 消费）。Windows 分支不删除。
// Windows 的一键安装命令面（winget/node_version）留宿主——tokio 子进程与 Tauri
// hooks 缓存是宿主职责。

/// Windows 的标准 Node 安装位置候选。PATH 由刚装完的安装器刷新不到，所以
/// 这些位置必须直接探测。
#[cfg(any(windows, test))]
pub(crate) fn windows_node_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for key in ["NODE_HOME", "NVM_SYMLINK"] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            directories.push(PathBuf::from(value));
        }
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            directories.push(PathBuf::from(value).join("nodejs"));
        }
    }
    if let Some(value) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        directories.push(PathBuf::from(value).join("Programs").join("nodejs"));
    }
    directories
}

/// Locate Node without relying exclusively on the process PATH. On Windows the
/// PATH inherited by Aeroric predates a just-completed installer, so standard
/// Node install locations must also be checked directly.
pub fn detect_node() -> Option<PathBuf> {
    let from_path = detect_path("node");
    if !from_path.is_empty() {
        let candidate = PathBuf::from(from_path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    #[cfg(any(windows, test))]
    {
        for directory in windows_node_directories() {
            let candidate = directory.join("node.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
