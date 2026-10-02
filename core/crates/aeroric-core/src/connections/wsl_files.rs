//! P7-02 Action 1：WSL 文件操作（cfg 隔离；与 `src-tauri/src/wsl_fs.rs` 同源
//! 语义上收；linuxPath ↔ Windows 路径转换纯函数）。
use serde::{Deserialize, Serialize};

/// WSL 文件条目（与 Tauri wsl_fs 的 FsEntry 变体对齐）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// WSL 内路径 → Windows UNC 路径（纯函数；`\\wsl$\<distro>\<path>`）。
pub fn wsl_path_to_windows_unc(distribution: &str, linux_path: &str) -> String {
    let trimmed = linux_path.trim_start_matches('/');
    if trimmed.is_empty() {
        format!(r"\\wsl$\{distribution}")
    } else {
        format!(r"\\wsl$\{distribution}\{}", trimmed.replace('/', "\\"))
    }
}

/// 旧 WSL JSON 读写（**不丢 distribution/linuxPath**——settings roundtrip）。
pub fn settings_roundtrip(json: &str) -> Result<super::wsl::WslSettings, String> {
    serde_json::from_str(json).map_err(|e| format!("malformed WSL settings: {e}"))
}

/// 读模型（linuxPath 保留断言用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslProjectTarget {
    pub distribution: String,
    #[serde(rename = "linuxPath")]
    pub linux_path: String,
}
