//! P7-02 Action 1：platform 模块（cfg 隔离实现——不删除 Windows 分支；与
//! `src-tauri/src/platform/` 同源语义上收；平台探测纯函数）。

use std::path::PathBuf;

use serde::Serialize;

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
