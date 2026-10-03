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
#[allow(
    dead_code,
    reason = "刀五宿主接线后由 get_agent_latest_versions 消费（P8-02 types 上收）"
)]
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentLatestVersion {
    pub agent: String,
    pub version: String,
    pub error_code: Option<AgentInstallErrorCode>,
    pub error: String,
}

#[allow(
    dead_code,
    reason = "刀五宿主接线后由 install_agent_tools 消费（P8-02 types 上收）"
)]
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentInstallRequest {
    #[serde(default)]
    pub operation_id: String,
    pub agents: Vec<String>,
}
