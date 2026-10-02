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

/// detect_path（与 `platform::detect_path` 同语义；按 cfg 分支）。
pub fn detect_path(binary: &str) -> String {
    if binary.contains('/') {
        return binary.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        let candidates = [".exe", ".cmd", ".bat"];
        for ext in candidates {
            let candidate = format!("{binary}{ext}");
            if std::path::Path::new(&candidate).exists() {
                return candidate;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let candidate = format!("/usr/local/bin/{binary}");
        if std::path::Path::new(&candidate).exists() {
            return candidate;
        }
    }
    binary.to_string()
}
