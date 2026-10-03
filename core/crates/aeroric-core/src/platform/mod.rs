//! P7-02 Action 1：platform 模块（cfg 隔离实现——不删除 Windows 分支；与
//! `src-tauri/src/platform/` 同源语义上收；平台探测纯函数）。

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
