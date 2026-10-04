//! 安装/升级的共享 wire 类型（P8-02 自宿主 `agent_tools.rs` 上收）。
//!
//! `AgentInstallErrorCode`/`AgentInstallStage`/`AgentInstallResult`/
//! `AgentUpgradeResult` 是 `AgentOperationSnapshot` 的组成部分,而快照类型在
//! core `operations` 里——类型放 core 才能不留第二份 serde 形状。宿主
//! `agent_tools.rs` 经 `aeroric_core::operations::types::*` 消费同一份定义。

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallErrorCode {
    UnsupportedPlatform,
    InvalidAgent,
    OperationConflict,
    NetworkUnavailable,
    ProxyAuthenticationRequired,
    DownloadFailed,
    DownloadInterrupted,
    ResponseTooLarge,
    ChecksumFailed,
    ArchiveInvalid,
    PermissionDenied,
    DiskFull,
    ProcessBlocked,
    InstallFailed,
    VerificationFailed,
    Cancelled,
    Internal,
}

impl AgentInstallErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::InvalidAgent => "invalid_agent",
            Self::OperationConflict => "operation_conflict",
            Self::NetworkUnavailable => "network_unavailable",
            Self::ProxyAuthenticationRequired => "proxy_authentication_required",
            Self::DownloadFailed => "download_failed",
            Self::DownloadInterrupted => "download_interrupted",
            Self::ResponseTooLarge => "response_too_large",
            Self::ChecksumFailed => "checksum_failed",
            Self::ArchiveInvalid => "archive_invalid",
            Self::PermissionDenied => "permission_denied",
            Self::DiskFull => "disk_full",
            Self::ProcessBlocked => "process_blocked",
            Self::InstallFailed => "install_failed",
            Self::VerificationFailed => "verification_failed",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallStage {
    Detecting,
    PreparingEnvironment,
    Downloading,
    VerifyingDownload,
    Installing,
    VerifyingInstall,
    RefreshingHooks,
    #[default]
    Completed,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentInstallResult {
    pub operation_id: String,
    pub agent: String,
    pub success: bool,
    pub supported: bool,
    pub platform: String,
    pub architecture: String,
    pub libc: String,
    pub version: String,
    pub path: String,
    pub channel: String,
    pub managed: bool,
    pub stage: AgentInstallStage,
    pub progress: u8,
    pub login_command: String,
    pub error_code: Option<AgentInstallErrorCode>,
    pub message: String,
}

/// 单个内置 Agent 的最新可用版本。查询失败时只返回错误信息，不影响其它 Agent。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentLatestVersion {
    pub agent: String,
    pub version: String,
    pub error_code: Option<AgentInstallErrorCode>,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentInstallRequest {
    #[serde(default)]
    pub operation_id: String,
    pub agents: Vec<String>,
}

/// 旧通道 `agent-tool-install-progress` 的 wire 形状（P8-02 自宿主上收）。
/// 宿主 emit wrapper 双发同一份 payload：旧通道（React 客户端）＋ v1 topic。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentInstallProgress {
    pub operation_id: String,
    pub agent: String,
    pub stage: AgentInstallStage,
    pub progress: u8,
    pub error_code: Option<AgentInstallErrorCode>,
    pub message: String,
}

/// 安装进度事件的旧桌面通道名（React 客户端）。v1 topic 见
/// [`TOOL_INSTALL_PROGRESS_TOPIC`]；两个字面值是在网契约，改名须双侧同步。
pub const TOOL_INSTALL_PROGRESS_EVENT: &str = "agent-tool-install-progress";

/// 安装进度事件的 v1 topic 名（`event-topics.json` 的
/// `agent.tool_install_progress` 行，P8-02 mapped）。
pub const TOOL_INSTALL_PROGRESS_TOPIC: &str = "agent.tool_install_progress";
