//! P7-02 Action 1：WSL 连接模块（cfg 隔离；不删除 Windows 分支；与
//! `src-tauri/src/wsl.rs` 同源语义上收；旧 WSL JSON 读写不丢 distribution/
//! linuxPath）。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// WSL 状态。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslStatus {
    pub supported: bool,
    pub installed: bool,
    pub distribution_count: usize,
    pub default_distribution: Option<String>,
    pub error: Option<String>,
}

/// WSL 发行版。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslDistribution {
    pub name: String,
    pub state: String,
    pub version: Option<u8>,
    pub is_default: bool,
}

/// WSL 设置（旧 WSL JSON 读写**不丢 distribution/linuxPath**）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslDistributionSettings {
    pub distribution: String,
    /// WSL 内的 Linux 路径（旧 JSON 字段；读写不丢）。
    #[serde(rename = "linuxPath", default)]
    pub linux_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslSettings {
    #[serde(default)]
    pub default_distribution: Option<String>,
    #[serde(default)]
    pub distributions: Vec<WslDistributionSettings>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

/// WSL 项目目标判定（Action 2：在 macOS 打开 WSL 项目时**禁止当作本地
/// path**——元数据保留、显示需要 Windows 主机）。
pub fn wsl_project_is_local(wsl: bool, client_platform: &str) -> bool {
    !wsl || client_platform == "windows"
}
