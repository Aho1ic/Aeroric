use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::redirect::{Attempt, Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::process::Command;
use uuid::Uuid;

mod dsh;
mod node_bootstrap;

const MAX_METADATA_BYTES: u64 = 5 * 1024 * 1024;
const MAX_CLAUDE_BINARY_BYTES: u64 = 384 * 1024 * 1024;
const MAX_CODEX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 1024 * 1024 * 1024;
const INSTALL_EVENT: &str = "agent-tool-install-progress";
const CODEX_RELEASE_API: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const CODEX_CHECKSUM_ASSET: &str = "codex-package_SHA256SUMS";

#[derive(Default)]
struct InstallState {
    cancellations: HashMap<String, Arc<AtomicBool>>,
    agent_operations: HashMap<BuiltInAgent, String>,
}

static INSTALL_STATE: OnceLock<Mutex<InstallState>> = OnceLock::new();

fn install_state() -> &'static Mutex<InstallState> {
    INSTALL_STATE.get_or_init(|| Mutex::new(InstallState::default()))
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum BuiltInAgent {
    Claude,
    Codex,
}

impl BuiltInAgent {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinuxLibc {
    Glibc,
    Musl,
}

impl LinuxLibc {
    fn label(self) -> &'static str {
        match self {
            Self::Glibc => "glibc",
            Self::Musl => "musl",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentToolStatus {
    pub agent: String,
    pub supported: bool,
    pub platform: String,
    pub architecture: String,
    pub libc: String,
    pub installed: bool,
    pub version: String,
    pub path: String,
    pub channel: String,
    pub managed: bool,
    pub error_code: Option<AgentInstallErrorCode>,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentInstallRequest {
    #[serde(default)]
    pub operation_id: String,
    pub agents: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

impl Default for AgentInstallResult {
    fn default() -> Self {
        Self {
            operation_id: String::new(),
            agent: String::new(),
            success: false,
            supported: false,
            platform: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            libc: current_libc_label(),
            version: String::new(),
            path: String::new(),
            channel: String::new(),
            managed: false,
            stage: AgentInstallStage::Failed,
            progress: 100,
            login_command: String::new(),
            error_code: None,
            message: String::new(),
        }
    }
}

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
    fn as_str(&self) -> &'static str {
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallStage {
    Detecting,
    PreparingEnvironment,
    Downloading,
    VerifyingDownload,
    Installing,
    VerifyingInstall,
    RefreshingHooks,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentInstallProgress {
    pub operation_id: String,
    pub agent: String,
    pub stage: AgentInstallStage,
    pub progress: u8,
    pub error_code: Option<AgentInstallErrorCode>,
    pub message: String,
}

#[derive(Debug)]
struct InstallError {
    code: AgentInstallErrorCode,
    message: String,
}

type InstallResult<T> = Result<T, InstallError>;

impl InstallError {
    fn new(code: AgentInstallErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn from_io(error: std::io::Error, context: &str) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::PermissionDenied => AgentInstallErrorCode::PermissionDenied,
            std::io::ErrorKind::StorageFull => AgentInstallErrorCode::DiskFull,
            _ => AgentInstallErrorCode::Internal,
        };
        Self::new(code, format!("{context}: {error}"))
    }

    fn from_archive_io(error: std::io::Error, context: &str) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::PermissionDenied => AgentInstallErrorCode::PermissionDenied,
            std::io::ErrorKind::StorageFull => AgentInstallErrorCode::DiskFull,
            _ => AgentInstallErrorCode::ArchiveInvalid,
        };
        Self::new(code, format!("{context}: {error}"))
    }
}

#[derive(Deserialize)]
struct ClaudeManifest {
    platforms: HashMap<String, ClaudePlatform>,
}

#[derive(Deserialize)]
struct ClaudePlatform {
    binary: String,
    checksum: String,
    size: u64,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    size: u64,
    digest: Option<String>,
}

impl GitHubReleaseAsset {
    fn sha256(&self) -> InstallResult<&str> {
        let digest = self.digest.as_deref().unwrap_or_default();
        let Some(value) = digest.strip_prefix("sha256:") else {
            return Err(InstallError::new(
                AgentInstallErrorCode::ChecksumFailed,
                format!("Release asset {} has no SHA-256 digest", self.name),
            ));
        };
        validate_sha256(value)?;
        Ok(value)
    }
}

#[derive(Clone)]
struct InstalledTool {
    version: String,
    path: PathBuf,
    channel: &'static str,
}

struct CleanupDir(PathBuf);

impl CleanupDir {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for CleanupDir {
    fn drop(&mut self) {
        let _ = remove_dir_all_with_retry(&self.0);
    }
}

fn with_windows_fs_retry<T>(
    mut operation: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let attempts = if cfg!(windows) { 8 } else { 1 };
    let mut last_error = None;
    for attempt in 0..attempts {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retryable = matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Other
                );
                if !cfg!(windows) || !retryable || attempt + 1 == attempts {
                    return Err(error);
                }
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(75 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("filesystem retry failed")))
}

fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    with_windows_fs_retry(|| fs::rename(from, to))
}

fn remove_dir_all_with_retry(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    with_windows_fs_retry(|| fs::remove_dir_all(path))
}

struct ActivatedDir {
    target: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}

impl ActivatedDir {
    fn activate(staged: &Path, target: &Path) -> InstallResult<Self> {
        let parent = target.parent().ok_or_else(|| {
            InstallError::new(
                AgentInstallErrorCode::Internal,
                "Install target has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| InstallError::from_io(error, "Create install directory failed"))?;
        let backup = target.exists().then(|| {
            parent.join(format!(
                ".{}.backup-{}",
                target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("tool"),
                Uuid::new_v4()
            ))
        });
        if let Some(backup) = backup.as_ref() {
            rename_with_retry(target, backup)
                .map_err(|error| InstallError::from_io(error, "Backup existing tool failed"))?;
        }
        if let Err(error) = rename_with_retry(staged, target) {
            if let Some(backup) = backup.as_ref() {
                let _ = rename_with_retry(backup, target);
            }
            return Err(InstallError::from_io(
                error,
                "Activate installed tool failed",
            ));
        }
        Ok(Self {
            target: target.to_path_buf(),
            backup,
            committed: false,
        })
    }

    fn commit(mut self) {
        self.committed = true;
        if let Some(backup) = self.backup.take() {
            let _ = remove_dir_all_with_retry(&backup);
        }
    }
}

impl Drop for ActivatedDir {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = remove_dir_all_with_retry(&self.target);
        if let Some(backup) = self.backup.as_ref() {
            let _ = rename_with_retry(backup, &self.target);
        }
    }
}

struct OperationGuard {
    operation_id: String,
    agents: Vec<BuiltInAgent>,
}

impl OperationGuard {
    fn begin(
        operation_id: String,
        agents: Vec<BuiltInAgent>,
    ) -> InstallResult<(Self, Arc<AtomicBool>)> {
        let mut state = install_state().lock();
        if state.cancellations.contains_key(&operation_id) {
            return Err(InstallError::new(
                AgentInstallErrorCode::OperationConflict,
                format!("Installation operation {operation_id} is already running"),
            ));
        }
        if let Some((agent, active_operation)) = agents.iter().find_map(|agent| {
            state
                .agent_operations
                .get(agent)
                .map(|operation| (*agent, operation.clone()))
        }) {
            return Err(InstallError::new(
                AgentInstallErrorCode::OperationConflict,
                format!(
                    "{} is already being installed by operation {active_operation}",
                    agent.id()
                ),
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        state
            .cancellations
            .insert(operation_id.clone(), cancelled.clone());
        for agent in &agents {
            state.agent_operations.insert(*agent, operation_id.clone());
        }
        Ok((
            Self {
                operation_id,
                agents,
            },
            cancelled,
        ))
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        let mut state = install_state().lock();
        state.cancellations.remove(&self.operation_id);
        for agent in &self.agents {
            if state
                .agent_operations
                .get(agent)
                .is_some_and(|operation| operation == &self.operation_id)
            {
                state.agent_operations.remove(agent);
            }
        }
    }
}

fn tools_dir() -> InstallResult<PathBuf> {
    crate::platform::home_dir()
        .map(|home| home.join(".aeroric").join("tools"))
        .ok_or_else(|| {
            InstallError::new(
                AgentInstallErrorCode::PermissionDenied,
                "Cannot determine the user home directory",
            )
        })
}

fn managed_tool_path(agent: BuiltInAgent) -> Option<PathBuf> {
    let file_name = match agent {
        BuiltInAgent::Claude if cfg!(windows) => "claude.exe",
        BuiltInAgent::Claude => "claude",
        BuiltInAgent::Codex if cfg!(windows) => "codex.exe",
        BuiltInAgent::Codex => "codex",
    };
    tools_dir().ok().map(|root| match agent {
        BuiltInAgent::Claude => root.join("claude").join("current").join(file_name),
        BuiltInAgent::Codex => root
            .join("codex")
            .join("current")
            .join("bin")
            .join(file_name),
    })
}

fn detect_linux_libc() -> LinuxLibc {
    if Path::new("/etc/alpine-release").exists()
        || ["/lib/ld-musl-x86_64.so.1", "/lib/ld-musl-aarch64.so.1"]
            .iter()
            .any(|path| Path::new(path).exists())
    {
        return LinuxLibc::Musl;
    }
    let output = std::process::Command::new("ldd").arg("--version").output();
    if output.is_ok_and(|output| {
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        text.to_ascii_lowercase().contains("musl")
    }) {
        LinuxLibc::Musl
    } else {
        LinuxLibc::Glibc
    }
}

fn current_linux_libc() -> LinuxLibc {
    static LIBC: OnceLock<LinuxLibc> = OnceLock::new();
    *LIBC.get_or_init(detect_linux_libc)
}

fn current_libc_label() -> String {
    if cfg!(target_os = "linux") {
        current_linux_libc().label().to_string()
    } else {
        String::new()
    }
}

fn claude_platform_for(os: &str, arch: &str, linux_libc: LinuxLibc) -> InstallResult<&'static str> {
    match (os, arch, linux_libc) {
        ("macos", "aarch64", _) => Ok("darwin-arm64"),
        ("macos", "x86_64", _) => Ok("darwin-x64"),
        ("windows", "aarch64", _) => Ok("win32-arm64"),
        ("windows", "x86_64", _) => Ok("win32-x64"),
        ("linux", "aarch64", LinuxLibc::Glibc) => Ok("linux-arm64"),
        ("linux", "x86_64", LinuxLibc::Glibc) => Ok("linux-x64"),
        ("linux", "aarch64", LinuxLibc::Musl) => Ok("linux-arm64-musl"),
        ("linux", "x86_64", LinuxLibc::Musl) => Ok("linux-x64-musl"),
        _ => Err(InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            format!("Claude Code is not available for {os}/{arch}"),
        )),
    }
}

fn claude_platform() -> InstallResult<&'static str> {
    claude_platform_for(
        std::env::consts::OS,
        std::env::consts::ARCH,
        current_linux_libc(),
    )
}

fn codex_target_for(os: &str, arch: &str) -> InstallResult<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        _ => Err(InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            format!("Codex is not available for {os}/{arch}"),
        )),
    }
}

fn codex_target() -> InstallResult<&'static str> {
    codex_target_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn platform_support(agent: BuiltInAgent) -> InstallResult<()> {
    match agent {
        BuiltInAgent::Claude => claude_platform().map(|_| ()),
        BuiltInAgent::Codex => codex_target().map(|_| ()),
    }
}

/// 进度事件对任意 agent 键通用(dsh 走的是新管线,不在 `BuiltInAgent` 里)。
/// 同时写入后端操作注册表,这样退出设置页再进来仍能看到当前阶段。
fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    agent: &str,
    stage: AgentInstallStage,
    progress: u8,
    error_code: Option<AgentInstallErrorCode>,
    message: impl Into<String>,
) {
    let message = message.into();
    crate::agent_ops::report_progress(app, agent, operation_id, stage.clone(), progress, &message);
    let _ = app.emit(
        INSTALL_EVENT,
        AgentInstallProgress {
            operation_id: operation_id.to_string(),
            agent: agent.to_string(),
            stage,
            progress,
            error_code,
            message,
        },
    );
}

/// 把"往哪个操作报进度"打包起来,免得每个阶段都重复传四个参数。
pub(crate) struct ProgressSink<'a> {
    pub(crate) app: &'a AppHandle,
    pub(crate) operation_id: &'a str,
    pub(crate) agent: &'a str,
}

impl ProgressSink<'_> {
    pub(crate) fn emit(&self, stage: AgentInstallStage, progress: u8, message: impl Into<String>) {
        emit_progress(
            self.app,
            self.operation_id,
            self.agent,
            stage,
            progress,
            None,
            message,
        );
    }
}

/// 子进程 stdout+stderr 合并成一段可读日志,并限长避免把整个 npm 输出塞进 UI。
fn combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = [stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if detail.chars().count() > 4000 {
        format!("{}...", detail.chars().take(4000).collect::<String>())
    } else {
        detail
    }
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> InstallResult<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(InstallError::new(
            AgentInstallErrorCode::Cancelled,
            "Installation cancelled",
        ))
    } else {
        Ok(())
    }
}

async fn wait_until_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn allowed_redirect(hosts: &'static [&'static str]) -> Policy {
    Policy::custom(move |attempt: Attempt<'_>| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match attempt.url().host_str() {
            Some(host) if hosts.contains(&host) => attempt.follow(),
            _ => attempt.error("redirected to an untrusted host"),
        }
    })
}

fn http_client(allowed_hosts: &'static [&'static str]) -> InstallResult<reqwest::Client> {
    let settings = crate::app_settings::load_settings_internal();
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .redirect(allowed_redirect(allowed_hosts))
        .user_agent(format!("Aeroric/{}", env!("CARGO_PKG_VERSION")));
    let proxy_url = settings.proxy_settings.url.trim();
    if !proxy_url.is_empty() {
        let mut proxy = reqwest::Proxy::all(proxy_url).map_err(|error| {
            InstallError::new(
                AgentInstallErrorCode::DownloadFailed,
                format!("Invalid proxy configuration: {error}"),
            )
        })?;
        if !settings.proxy_settings.username.is_empty() {
            proxy = proxy.basic_auth(
                &settings.proxy_settings.username,
                &settings.proxy_settings.password,
            );
        }
        if !settings.proxy_settings.no_proxy.trim().is_empty() {
            proxy = proxy.no_proxy(reqwest::NoProxy::from_string(
                settings.proxy_settings.no_proxy.trim(),
            ));
        }
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Cannot create HTTP client: {error}"),
        )
    })
}

fn classify_request_error(error: reqwest::Error, context: &str, interrupted: bool) -> InstallError {
    let code = if error.is_timeout() || error.is_connect() {
        AgentInstallErrorCode::NetworkUnavailable
    } else if interrupted {
        AgentInstallErrorCode::DownloadInterrupted
    } else {
        AgentInstallErrorCode::DownloadFailed
    };
    InstallError::new(code, format!("{context}: {error}"))
}

fn check_response(response: &reqwest::Response, max_bytes: u64) -> InstallResult<()> {
    if response.status() == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        return Err(InstallError::new(
            AgentInstallErrorCode::ProxyAuthenticationRequired,
            "The configured proxy requires authentication",
        ));
    }
    if !response.status().is_success() {
        return Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Download returned HTTP {}", response.status()),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(InstallError::new(
            AgentInstallErrorCode::ResponseTooLarge,
            "Download exceeds the allowed size",
        ));
    }
    Ok(())
}

async fn send_request(
    client: &reqwest::Client,
    url: &str,
    cancelled: &AtomicBool,
) -> InstallResult<reqwest::Response> {
    ensure_not_cancelled(cancelled)?;
    tokio::select! {
        result = client.get(url).send() => {
            result.map_err(|error| classify_request_error(error, "Download failed", false))
        }
        _ = wait_until_cancelled(cancelled) => {
            Err(InstallError::new(AgentInstallErrorCode::Cancelled, "Installation cancelled"))
        }
    }
}

async fn download_small_bytes(
    client: &reqwest::Client,
    url: &str,
    max_bytes: u64,
    cancelled: &AtomicBool,
) -> InstallResult<Vec<u8>> {
    let mut response = send_request(client, url, cancelled).await?;
    check_response(&response, max_bytes)?;
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            result = response.chunk() => {
                result.map_err(|error| classify_request_error(error, "Download interrupted", true))?
            }
            _ = wait_until_cancelled(cancelled) => {
                return Err(InstallError::new(AgentInstallErrorCode::Cancelled, "Installation cancelled"));
            }
        };
        let Some(chunk) = chunk else {
            break;
        };
        if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(InstallError::new(
                AgentInstallErrorCode::ResponseTooLarge,
                "Download exceeds the allowed size",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

struct DownloadedFile {
    size: u64,
    sha256: String,
}

pub(crate) struct DownloadProgress<'a> {
    pub(crate) app: &'a AppHandle,
    pub(crate) operation_id: &'a str,
    pub(crate) agent: String,
    pub(crate) start: u8,
    pub(crate) end: u8,
    pub(crate) message: String,
}

async fn download_to_file(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    expected_size: Option<u64>,
    max_bytes: u64,
    cancelled: &AtomicBool,
    progress: &DownloadProgress<'_>,
) -> InstallResult<DownloadedFile> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| InstallError::from_io(error, "Create download directory failed"))?;
    }
    let mut response = send_request(client, url, cancelled).await?;
    check_response(&response, max_bytes)?;
    if expected_size.is_some_and(|size| size > max_bytes) {
        return Err(InstallError::new(
            AgentInstallErrorCode::ResponseTooLarge,
            format!("Expected download size exceeds {max_bytes} bytes"),
        ));
    }
    if let (Some(expected), Some(actual)) = (expected_size, response.content_length()) {
        if expected != actual {
            return Err(InstallError::new(
                AgentInstallErrorCode::DownloadInterrupted,
                format!("Expected {expected} bytes but server reported {actual}"),
            ));
        }
    }

    let mut file = fs::File::create(target)
        .map_err(|error| InstallError::from_io(error, "Create download file failed"))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let denominator = expected_size.or_else(|| response.content_length());
    loop {
        let chunk = tokio::select! {
            result = response.chunk() => {
                result.map_err(|error| classify_request_error(error, "Download interrupted", true))?
            }
            _ = wait_until_cancelled(cancelled) => {
                return Err(InstallError::new(AgentInstallErrorCode::Cancelled, "Installation cancelled"));
            }
        };
        let Some(chunk) = chunk else {
            break;
        };
        ensure_not_cancelled(cancelled)?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > max_bytes {
            return Err(InstallError::new(
                AgentInstallErrorCode::ResponseTooLarge,
                "Download exceeds the allowed size",
            ));
        }
        file.write_all(&chunk)
            .map_err(|error| InstallError::from_io(error, "Write download failed"))?;
        hasher.update(&chunk);
        if let Some(total) = denominator.filter(|total| *total > 0) {
            let span = progress.end.saturating_sub(progress.start) as u64;
            let offset = downloaded.min(total).saturating_mul(span) / total;
            emit_progress(
                progress.app,
                progress.operation_id,
                &progress.agent,
                AgentInstallStage::Downloading,
                progress.start.saturating_add(offset as u8),
                None,
                progress.message.clone(),
            );
        }
    }
    file.flush()
        .map_err(|error| InstallError::from_io(error, "Flush download failed"))?;
    if let Some(expected) = expected_size {
        if downloaded != expected {
            return Err(InstallError::new(
                AgentInstallErrorCode::DownloadInterrupted,
                format!("Expected {expected} bytes but downloaded {downloaded}"),
            ));
        }
    }
    Ok(DownloadedFile {
        size: downloaded,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn validate_sha256(value: &str) -> InstallResult<()> {
    if value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(InstallError::new(
            AgentInstallErrorCode::ChecksumFailed,
            "Release metadata contains an invalid SHA-256 digest",
        ))
    }
}

fn verify_sha256(actual: &str, expected: &str) -> InstallResult<()> {
    validate_sha256(expected)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(InstallError::new(
            AgentInstallErrorCode::ChecksumFailed,
            format!("SHA-256 mismatch: expected {expected}, got {actual}"),
        ))
    }
}

fn safe_archive_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// 符号链接的 target 落在解包目录内吗?
///
/// 不能直接套 [`safe_archive_path`]:官方 Node 发行包里的 `bin/npm` 指向
/// `../lib/node_modules/npm/bin/npm-cli.js`,合法的相对链接**本来就带 `..`**,
/// 一律拒掉等于拒掉整个压缩包(见 `extract_tar_gz` 上的注释)。
///
/// 判据换成「按 link 所在目录逐段结算深度,过程中不许降到 0 以下」:`..` 减一、
/// 普通段加一。深度为 0 时再遇到 `..` 就是逃出解包根,拒掉。绝对路径直接拒。
fn safe_symlink_target(link_path: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    // link 自身所在目录的深度:`bin/npm` 的父目录是 `bin`,深度 1。
    let mut depth = link_path
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter(|component| matches!(component, Component::Normal(_)))
                .count()
        })
        .unwrap_or(0);
    for component in target.components() {
        match component {
            Component::ParentDir => {
                // 已经在解包根上,再往上就出界了。
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            // 前缀 / 根:绝对路径的形态,上面已经拦过,这里兜底。
            Component::Prefix(_) | Component::RootDir => return false,
        }
    }
    true
}

fn extract_tar_gz(archive_path: &Path, output: &Path) -> InstallResult<()> {
    fs::create_dir_all(output)
        .map_err(|error| InstallError::from_io(error, "Create extraction directory failed"))?;
    let file = fs::File::open(archive_path)
        .map_err(|error| InstallError::from_io(error, "Open archive failed"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut extracted_bytes = 0u64;
    for entry in archive
        .entries()
        .map_err(|error| InstallError::from_archive_io(error, "Read archive failed"))?
    {
        let mut entry = entry
            .map_err(|error| InstallError::from_archive_io(error, "Read archive entry failed"))?;
        let path = entry
            .path()
            .map_err(|error| InstallError::from_archive_io(error, "Read archive path failed"))?;
        if !safe_archive_path(&path) {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                "Archive contains an unsafe path",
            ));
        }
        let entry_type = entry.header().entry_type();
        // 符号链接必须放行:官方 Node 发行包里有 `bin/npm`、`bin/npx`、`bin/corepack`
        // 三条相对链接,一律拒掉会让托管 Node 安装在 macOS / Linux 上必然失败
        // ——而那条路径恰恰只在「机器上没有 Node」时才走到,没有退路。
        // 硬链接仍然拒:Node 发行包里没有,放行只是白扩攻击面。
        if !(entry_type.is_file() || entry_type.is_dir() || entry_type.is_symlink()) {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                format!("Archive contains unsupported entry {}", path.display()),
            ));
        }
        if entry_type.is_symlink() {
            // target 只允许指向解包目录内部:否则后续经这条链接写入就能落到目录外。
            let target = entry.link_name().ok().flatten().ok_or_else(|| {
                InstallError::new(
                    AgentInstallErrorCode::ArchiveInvalid,
                    format!("Archive symlink {} has no target", path.display()),
                )
            })?;
            if !safe_symlink_target(&path, &target) {
                return Err(InstallError::new(
                    AgentInstallErrorCode::ArchiveInvalid,
                    format!(
                        "Archive symlink {} escapes the target directory",
                        path.display()
                    ),
                ));
            }
        }
        if entry_type.is_file() {
            extracted_bytes = extracted_bytes.saturating_add(entry.size());
            if extracted_bytes > MAX_EXTRACTED_BYTES {
                return Err(InstallError::new(
                    AgentInstallErrorCode::ResponseTooLarge,
                    "Extracted archive exceeds the allowed size",
                ));
            }
        }
        let unpacked = entry
            .unpack_in(output)
            .map_err(|error| InstallError::from_archive_io(error, "Extract archive failed"))?;
        if !unpacked {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                "Archive contains a path outside the target directory",
            ));
        }
    }
    Ok(())
}

/// Windows 的 Node 发行包是 zip,其余平台是 tar.gz。安全性检查与 tar 路径一致:
/// 拒绝绝对路径、`..` 逃逸与超限解包体积。
fn extract_zip(archive_path: &Path, output: &Path) -> InstallResult<()> {
    fs::create_dir_all(output)
        .map_err(|error| InstallError::from_io(error, "Create extraction directory failed"))?;
    let file = fs::File::open(archive_path)
        .map_err(|error| InstallError::from_io(error, "Open archive failed"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::ArchiveInvalid,
            format!("Invalid archive: {error}"),
        )
    })?;
    let mut extracted_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                format!("Read archive entry failed: {error}"),
            )
        })?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                "Archive contains an unsafe path",
            ));
        };
        if !safe_archive_path(&relative) {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                "Archive contains an unsafe path",
            ));
        }
        let target = output.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| InstallError::from_io(error, "Create archive directory failed"))?;
            continue;
        }
        extracted_bytes = extracted_bytes.saturating_add(entry.size());
        if extracted_bytes > MAX_EXTRACTED_BYTES {
            return Err(InstallError::new(
                AgentInstallErrorCode::ResponseTooLarge,
                "Extracted archive exceeds the allowed size",
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| InstallError::from_io(error, "Create archive directory failed"))?;
        }
        let mut file = fs::File::create(&target)
            .map_err(|error| InstallError::from_io(error, "Write archive entry failed"))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| InstallError::from_archive_io(error, "Extract archive failed"))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// 按扩展名分派解压方式。
fn extract_archive(archive_path: &Path, output: &Path) -> InstallResult<()> {
    let name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive_path, output)
    } else {
        extract_tar_gz(archive_path, output)
    }
}

fn current_libc_is_musl() -> bool {
    cfg!(target_os = "linux") && current_linux_libc() == LinuxLibc::Musl
}

#[cfg(unix)]
fn make_executable(path: &Path) -> InstallResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|error| InstallError::from_io(error, "Read executable metadata failed"))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| InstallError::from_io(error, "Set executable permissions failed"))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> InstallResult<()> {
    Ok(())
}

async fn command_output(
    mut command: Command,
    cancelled: &AtomicBool,
    context: &str,
) -> InstallResult<std::process::Output> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);
    crate::subprocess::configure_terminable_tokio_process_tree(&mut command);
    let mut child = command.spawn().map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
            AgentInstallErrorCode::ProcessBlocked
        } else {
            AgentInstallErrorCode::InstallFailed
        };
        InstallError::new(code, format!("{context}: {error}"))
    })?;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = crate::subprocess::terminate_tokio_process_tree(&mut child).await;
            return Err(InstallError::new(
                AgentInstallErrorCode::Cancelled,
                "Installation cancelled",
            ));
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().await.map_err(|error| {
                    InstallError::new(
                        AgentInstallErrorCode::InstallFailed,
                        format!("{context}: {error}"),
                    )
                });
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(error) => {
                let _ = crate::subprocess::terminate_tokio_process_tree(&mut child).await;
                return Err(InstallError::new(
                    AgentInstallErrorCode::InstallFailed,
                    format!("{context}: {error}"),
                ));
            }
        }
    }
}

async fn detect_version(program: &Path, cancelled: &AtomicBool) -> InstallResult<String> {
    let mut command = Command::new(program);
    crate::subprocess::configure_terminable_tokio_process_tree(&mut command);
    command.arg("--version");
    let output = command_output(command, cancelled, "Version verification failed").await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            if stderr.is_empty() {
                format!("Version command exited with {}", output.status)
            } else {
                stderr
            },
        ));
    }
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::app_settings::extract_version(&combined).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            "The installed CLI did not report a valid version",
        )
    })
}

async fn install_claude(
    app: &AppHandle,
    operation_id: &str,
    cancelled: &AtomicBool,
) -> InstallResult<InstalledTool> {
    let agent = BuiltInAgent::Claude;
    let platform = claude_platform()?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::PreparingEnvironment,
        8,
        None,
        format!("Preparing Claude Code for {platform}"),
    );
    let client = http_client(&["downloads.claude.ai"])?;
    let base = "https://downloads.claude.ai/claude-code-releases";
    let version_bytes = download_small_bytes(
        &client,
        &format!("{base}/latest"),
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let version = String::from_utf8(version_bytes)
        .map_err(|error| {
            InstallError::new(
                AgentInstallErrorCode::DownloadFailed,
                format!("Invalid Claude release version: {error}"),
            )
        })?
        .trim()
        .to_string();
    if crate::app_settings::extract_version(&version).as_deref() != Some(version.as_str()) {
        return Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            "Claude release service returned an invalid version",
        ));
    }
    let manifest_bytes = download_small_bytes(
        &client,
        &format!("{base}/{version}/manifest.json"),
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let manifest: ClaudeManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Invalid Claude release manifest: {error}"),
        )
    })?;
    let artifact = manifest.platforms.get(platform).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            format!("Claude release does not contain {platform}"),
        )
    })?;
    validate_sha256(&artifact.checksum)?;
    if artifact.size > MAX_CLAUDE_BINARY_BYTES {
        return Err(InstallError::new(
            AgentInstallErrorCode::ResponseTooLarge,
            format!(
                "Claude binary is unexpectedly large: {} bytes",
                artifact.size
            ),
        ));
    }
    let expected_file_name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    if artifact.binary != expected_file_name {
        return Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!(
                "Claude manifest contains unexpected binary name {}",
                artifact.binary
            ),
        ));
    }

    let root = tools_dir()?.join("claude");
    let temp = root.join(format!(".install-{}", Uuid::new_v4()));
    let _cleanup = CleanupDir::new(temp.clone());
    let staged_current = temp.join("current");
    let staged_binary = staged_current.join(expected_file_name);
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Downloading,
        14,
        None,
        format!("Downloading Claude Code {version}"),
    );
    let download = download_to_file(
        &client,
        &format!("{base}/{version}/{platform}/{expected_file_name}"),
        &staged_binary,
        Some(artifact.size),
        MAX_CLAUDE_BINARY_BYTES,
        cancelled,
        &DownloadProgress {
            app,
            operation_id,
            agent: agent.id().to_string(),
            start: 14,
            end: 66,
            message: format!("Downloading Claude Code {version}"),
        },
    )
    .await?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::VerifyingDownload,
        70,
        None,
        "Verifying Claude Code download",
    );
    verify_sha256(&download.sha256, &artifact.checksum)?;
    make_executable(&staged_binary)?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Installing,
        78,
        None,
        "Installing Claude Code",
    );
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Installing,
        82,
        None,
        "Activating Claude Code",
    );
    ensure_not_cancelled(cancelled)?;
    let installed_path = root.join("current").join(expected_file_name);
    let activation = ActivatedDir::activate(&staged_current, &root.join("current"))?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::VerifyingInstall,
        86,
        None,
        "Verifying Claude Code",
    );
    let detected = detect_version(&installed_path, cancelled).await?;
    if detected != version {
        return Err(InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            format!("Expected Claude Code {version}, but detected {detected}"),
        ));
    }
    ensure_not_cancelled(cancelled)?;
    crate::app_settings::save_managed_agent_path(agent.id(), &installed_path)
        .map_err(|message| InstallError::new(AgentInstallErrorCode::Internal, message))?;
    activation.commit();
    Ok(InstalledTool {
        version: detected,
        path: installed_path,
        channel: "aeroric-managed-native",
    })
}

fn codex_asset_url(tag: &str, asset: &str) -> InstallResult<String> {
    if !tag.starts_with("rust-v")
        || tag.contains('/')
        || asset.contains('/')
        || asset.contains('\\')
        || asset.contains("..")
    {
        return Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            "Codex release metadata contains an unsafe asset name",
        ));
    }
    Ok(format!(
        "https://github.com/openai/codex/releases/download/{tag}/{asset}"
    ))
}

fn codex_version_from_tag(tag: &str) -> InstallResult<String> {
    let version = tag.strip_prefix("rust-v").unwrap_or_default();
    if crate::app_settings::extract_version(version).as_deref() == Some(version) {
        Ok(version.to_string())
    } else {
        Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Codex release has invalid tag {tag}"),
        ))
    }
}

fn checksum_for_asset(manifest: &[u8], asset_name: &str) -> InstallResult<String> {
    let text = std::str::from_utf8(manifest).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::ChecksumFailed,
            format!("Codex checksum manifest is not UTF-8: {error}"),
        )
    })?;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(checksum) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if name == asset_name && fields.next().is_none() {
            validate_sha256(checksum)?;
            return Ok(checksum.to_ascii_lowercase());
        }
    }
    Err(InstallError::new(
        AgentInstallErrorCode::ChecksumFailed,
        format!("Codex checksum manifest does not contain {asset_name}"),
    ))
}

fn verify_codex_layout(root: &Path) -> InstallResult<PathBuf> {
    let executable = root
        .join("bin")
        .join(if cfg!(windows) { "codex.exe" } else { "codex" });
    let ripgrep = root
        .join("codex-path")
        .join(if cfg!(windows) { "rg.exe" } else { "rg" });
    if !root.join("codex-package.json").is_file() || !executable.is_file() || !ripgrep.is_file() {
        return Err(InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            "Codex package is missing required files",
        ));
    }
    make_executable(&executable)?;
    make_executable(&ripgrep)?;
    for relative in [
        "bin/codex-code-mode-host",
        "codex-resources/bwrap",
        "codex-resources/codex-command-runner",
        "codex-resources/codex-windows-sandbox-setup",
    ] {
        let mut path = root.join(relative);
        if cfg!(windows) {
            path.set_extension("exe");
        }
        if path.is_file() {
            make_executable(&path)?;
        }
    }
    Ok(executable)
}

async fn install_codex(
    app: &AppHandle,
    operation_id: &str,
    cancelled: &AtomicBool,
) -> InstallResult<InstalledTool> {
    let agent = BuiltInAgent::Codex;
    let target = codex_target()?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::PreparingEnvironment,
        8,
        None,
        format!("Preparing Codex for {target}"),
    );
    let metadata_client = http_client(&["api.github.com"])?;
    let release_bytes = download_small_bytes(
        &metadata_client,
        CODEX_RELEASE_API,
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let release: GitHubRelease = serde_json::from_slice(&release_bytes).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Invalid Codex release metadata: {error}"),
        )
    })?;
    let version = codex_version_from_tag(&release.tag_name)?;
    let archive_name = format!("codex-package-{target}.tar.gz");
    let archive_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == archive_name)
        .ok_or_else(|| {
            InstallError::new(
                AgentInstallErrorCode::UnsupportedPlatform,
                format!("Codex release does not contain {archive_name}"),
            )
        })?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == CODEX_CHECKSUM_ASSET)
        .ok_or_else(|| {
            InstallError::new(
                AgentInstallErrorCode::ChecksumFailed,
                "Codex release does not contain its SHA-256 manifest",
            )
        })?;
    if archive_asset.size > MAX_CODEX_ARCHIVE_BYTES {
        return Err(InstallError::new(
            AgentInstallErrorCode::ResponseTooLarge,
            format!(
                "Codex archive is unexpectedly large: {} bytes",
                archive_asset.size
            ),
        ));
    }

    let root = tools_dir()?.join("codex");
    let temp = root.join(format!(".install-{}", Uuid::new_v4()));
    let _cleanup = CleanupDir::new(temp.clone());
    let staged_current = temp.join("current");
    let archive_path = temp.join(&archive_name);
    let checksum_path = temp.join(CODEX_CHECKSUM_ASSET);
    let download_client = http_client(&[
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
    ])?;

    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Downloading,
        14,
        None,
        "Downloading Codex checksum manifest",
    );
    let checksum_download = download_to_file(
        &download_client,
        &codex_asset_url(&release.tag_name, CODEX_CHECKSUM_ASSET)?,
        &checksum_path,
        Some(checksum_asset.size),
        MAX_METADATA_BYTES,
        cancelled,
        &DownloadProgress {
            app,
            operation_id,
            agent: agent.id().to_string(),
            start: 14,
            end: 18,
            message: "Downloading Codex checksum manifest".to_string(),
        },
    )
    .await?;
    verify_sha256(&checksum_download.sha256, checksum_asset.sha256()?)?;
    let mut checksum_bytes = Vec::with_capacity(checksum_download.size as usize);
    fs::File::open(&checksum_path)
        .and_then(|mut file| file.read_to_end(&mut checksum_bytes))
        .map_err(|error| InstallError::from_io(error, "Read Codex checksum manifest failed"))?;
    let expected_archive_sha = checksum_for_asset(&checksum_bytes, &archive_name)?;

    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Downloading,
        20,
        None,
        format!("Downloading Codex {version}"),
    );
    let archive_download = download_to_file(
        &download_client,
        &codex_asset_url(&release.tag_name, &archive_name)?,
        &archive_path,
        Some(archive_asset.size),
        MAX_CODEX_ARCHIVE_BYTES,
        cancelled,
        &DownloadProgress {
            app,
            operation_id,
            agent: agent.id().to_string(),
            start: 20,
            end: 66,
            message: format!("Downloading Codex {version}"),
        },
    )
    .await?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::VerifyingDownload,
        70,
        None,
        "Verifying Codex download",
    );
    verify_sha256(&archive_download.sha256, &expected_archive_sha)?;
    ensure_not_cancelled(cancelled)?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Installing,
        78,
        None,
        "Extracting Codex package",
    );
    extract_tar_gz(&archive_path, &staged_current)?;
    ensure_not_cancelled(cancelled)?;
    verify_codex_layout(&staged_current)?;
    ensure_not_cancelled(cancelled)?;
    let installed_path =
        root.join("current")
            .join("bin")
            .join(if cfg!(windows) { "codex.exe" } else { "codex" });
    let activation = ActivatedDir::activate(&staged_current, &root.join("current"))?;
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::VerifyingInstall,
        86,
        None,
        "Verifying Codex",
    );
    let detected = detect_version(&installed_path, cancelled).await?;
    if detected != version {
        return Err(InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            format!("Expected Codex {version}, but detected {detected}"),
        ));
    }
    ensure_not_cancelled(cancelled)?;
    crate::app_settings::save_managed_agent_path(agent.id(), &installed_path)
        .map_err(|message| InstallError::new(AgentInstallErrorCode::Internal, message))?;
    activation.commit();
    Ok(InstalledTool {
        version: detected,
        path: installed_path,
        channel: "aeroric-managed-native",
    })
}

async fn install_one(
    app: &AppHandle,
    operation_id: &str,
    agent: BuiltInAgent,
    cancelled: &AtomicBool,
) -> AgentInstallResult {
    emit_progress(
        app,
        operation_id,
        agent.id(),
        AgentInstallStage::Detecting,
        2,
        None,
        "Checking the current installation",
    );
    let support = platform_support(agent);
    let outcome = match support {
        Ok(()) => match agent {
            BuiltInAgent::Claude => install_claude(app, operation_id, cancelled).await,
            BuiltInAgent::Codex => install_codex(app, operation_id, cancelled).await,
        },
        Err(error) => Err(error),
    };
    match outcome {
        Ok(installed) => {
            emit_progress(
                app,
                operation_id,
                agent.id(),
                AgentInstallStage::RefreshingHooks,
                94,
                None,
                "Refreshing hook integration",
            );
            let hook_status = crate::hooks::ensure_installed();
            crate::hooks::cache_status(hook_status);
            emit_progress(
                app,
                operation_id,
                agent.id(),
                AgentInstallStage::Completed,
                100,
                None,
                "Installation complete",
            );
            AgentInstallResult {
                operation_id: operation_id.to_string(),
                agent: agent.id().to_string(),
                success: true,
                supported: true,
                version: installed.version,
                path: installed.path.to_string_lossy().into_owned(),
                channel: installed.channel.to_string(),
                managed: true,
                stage: AgentInstallStage::Completed,
                progress: 100,
                login_command: agent.id().to_string(),
                message: "installed".to_string(),
                ..Default::default()
            }
        }
        Err(error) => {
            let was_cancelled = error.code == AgentInstallErrorCode::Cancelled;
            let stage = if was_cancelled {
                AgentInstallStage::Cancelled
            } else {
                AgentInstallStage::Failed
            };
            emit_progress(
                app,
                operation_id,
                agent.id(),
                stage.clone(),
                100,
                Some(error.code.clone()),
                error.message.clone(),
            );
            AgentInstallResult {
                operation_id: operation_id.to_string(),
                agent: agent.id().to_string(),
                supported: error.code != AgentInstallErrorCode::UnsupportedPlatform,
                stage,
                error_code: Some(error.code),
                message: error.message,
                ..Default::default()
            }
        }
    }
}

/// 该 agent 当前是否已装好(能报出版本号就算装好)。
pub(crate) fn agent_is_installed(agent: &str) -> bool {
    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, agent);
    crate::app_settings::detect_launch_version(&launch)
        .is_some_and(|version| !version.trim().is_empty())
}

/// dsh 的安装/升级实现。三条策略都在这里收口,任何一条走不通都会落到托管安装,
/// 而不是像以前那样直接抛 "detected channel: standalone"。
async fn run_dsh_operation(
    app: &AppHandle,
    operation_id: &str,
    kind: crate::agent_ops::AgentOperationKind,
    cancelled: &AtomicBool,
    expected_version: Option<&str>,
) -> crate::agent_ops::OperationOutcome {
    use crate::agent_ops::OperationOutcome;

    let sink = ProgressSink {
        app,
        operation_id,
        agent: "dsh",
    };
    sink.emit(
        AgentInstallStage::Detecting,
        2,
        "Checking the current DeepSeek Harness installation",
    );

    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, "dsh");
    let configured = crate::app_settings::configured_agent_path(&settings, "dsh");
    let active_program = if configured.trim().is_empty() {
        launch.program.clone()
    } else {
        configured.clone()
    };
    let previous_version = crate::app_settings::detect_launch_version(&launch).unwrap_or_default();

    // 升级要替换文件,先挂起 DSH 运行时并在结束后恢复会话。
    let webui = app.state::<crate::dsh_webui::DshWebUiManager>();
    let suspended = match webui.suspend_for_upgrade("dsh").await {
        Ok(suspended) => Some(suspended),
        Err(error) => {
            return OperationOutcome::Upgrade(crate::app_settings::AgentUpgradeResult {
                agent: "dsh".to_string(),
                success: false,
                previous_version: previous_version.clone(),
                current_version: previous_version,
                message: format!("runtime-recovery: {error}"),
                channels: vec![crate::app_settings::AgentUpgradeChannel {
                    channel: "runtime-recovery".to_string(),
                    success: false,
                    message: error.clone(),
                }],
                runtime_recovery: Some(crate::dsh_webui::DshRuntimeRecovery {
                    errors: vec![error],
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    };

    let outcome = run_dsh_strategy(
        &sink,
        &settings,
        &configured,
        &active_program,
        expected_version,
        cancelled,
    )
    .await;

    // 无论成败都要把运行时恢复回来;`resume_after_upgrade` 自己会跳过原本没在跑的实例。
    let runtime_recovery = match suspended {
        Some(suspended) => Some(webui.resume_after_upgrade(app, suspended).await),
        None => None,
    };

    let (channels, channel, installed_launcher) = match outcome {
        Ok(success) => (
            success.channels,
            success.channel,
            success.installed_launcher,
        ),
        Err(error) => (
            vec![crate::app_settings::AgentUpgradeChannel {
                channel: "detection".to_string(),
                success: false,
                message: error.message,
            }],
            "detection".to_string(),
            None,
        ),
    };

    crate::app_settings::clear_cached_agent_versions();
    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, "dsh");
    let current_version = crate::app_settings::detect_launch_version(&launch).unwrap_or_default();

    let mut channels = channels;
    if let Some(recovery) = runtime_recovery.as_ref() {
        if !recovery.errors.is_empty() {
            channels.push(crate::app_settings::AgentUpgradeChannel {
                channel: "runtime-recovery".to_string(),
                success: false,
                message: recovery.errors.join("\n"),
            });
        } else if recovery.restarted {
            channels.push(crate::app_settings::AgentUpgradeChannel {
                channel: "runtime-recovery".to_string(),
                success: true,
                message: format!(
                    "restarted; reconnected {} session(s); cancelled {} running turn(s)",
                    recovery.reconnected_sessions, recovery.cancelled_turns
                ),
            });
        }
    }
    crate::app_settings::append_agent_upgrade_verification(
        &mut channels,
        &active_program,
        &previous_version,
        &current_version,
        expected_version,
    );

    let success = channels.iter().all(|entry| entry.success);
    let message = channels
        .iter()
        .map(|entry| format!("{}: {}", entry.channel, entry.message))
        .collect::<Vec<_>>()
        .join("\n");
    let managed = dsh::is_managed_program(&crate::app_settings::configured_agent_path(
        &settings, "dsh",
    ));

    // 首次安装要回 Install:前端的安装路径与登录命令都只从 `install_result` 取,
    // 回 Upgrade 会让 `kind == "install"` 的卡片拿不到这两项,只剩一句「安装完成」。
    if kind == crate::agent_ops::AgentOperationKind::Install {
        return OperationOutcome::Install(dsh_install_result(
            operation_id,
            success,
            cancelled.load(Ordering::Relaxed),
            current_version,
            installed_launcher
                .map(|launcher| launcher.to_string_lossy().into_owned())
                .unwrap_or_else(|| crate::app_settings::configured_agent_path(&settings, "dsh")),
            channel,
            managed,
            message,
        ));
    }

    OperationOutcome::Upgrade(crate::app_settings::AgentUpgradeResult {
        agent: "dsh".to_string(),
        success,
        previous_version,
        current_version,
        message,
        channels,
        channel,
        managed,
        runtime_recovery,
    })
}

/// 把一次 DSH 首装的结果装成 `AgentInstallResult`。
///
/// 拆成独立函数是为了能不起 Tauri 就测:前端的安装路径与登录命令只从
/// `install_result` 读,所以这里的 `path` / `login_command` 一错,卡片上就只剩
/// 一句「安装完成」。
#[allow(clippy::too_many_arguments)]
fn dsh_install_result(
    operation_id: &str,
    success: bool,
    cancelled: bool,
    version: String,
    path: String,
    channel: String,
    managed: bool,
    message: String,
) -> AgentInstallResult {
    AgentInstallResult {
        operation_id: operation_id.to_string(),
        agent: "dsh".to_string(),
        success,
        supported: true,
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        version,
        path,
        channel,
        managed,
        // 取消要如实报成 Cancelled:`start_agent_operation` 的 Install 分支用
        // `stage == Cancelled` 判定这次操作是被取消还是真失败,一律回 Failed
        // 会把用户自己点的取消显示成安装失败。
        stage: if success {
            AgentInstallStage::Completed
        } else if cancelled {
            AgentInstallStage::Cancelled
        } else {
            AgentInstallStage::Failed
        },
        progress: 100,
        // Claude/Codex 那条管线用 agent id 作登录命令,dsh 同理。
        login_command: "dsh".to_string(),
        error_code: if success {
            None
        } else if cancelled {
            Some(AgentInstallErrorCode::Cancelled)
        } else {
            Some(AgentInstallErrorCode::InstallFailed)
        },
        message,
        ..Default::default()
    }
}

struct DshStrategyOutcome {
    channels: Vec<crate::app_settings::AgentUpgradeChannel>,
    channel: String,
    /// 托管安装落地的启动器路径。只有 `Managed` 策略给得出来;源码 checkout 与包
    /// 管理器都是原地升级,没有「新装到哪」这回事。首次安装要靠它在卡片上显示
    /// 安装路径 —— 缺了就只有一句「安装完成」。
    installed_launcher: Option<PathBuf>,
}

async fn run_dsh_strategy(
    sink: &ProgressSink<'_>,
    settings: &crate::app_settings::AppSettings,
    configured: &str,
    active_program: &str,
    expected_version: Option<&str>,
    cancelled: &AtomicBool,
) -> InstallResult<DshStrategyOutcome> {
    let strategy = dsh::resolve_strategy(configured, active_program);
    let mut notes = Vec::new();

    let strategy = match strategy {
        dsh::DshStrategy::SourceCheckout { root } => {
            sink.emit(
                AgentInstallStage::PreparingEnvironment,
                8,
                "Checking the DSH source checkout",
            );
            match dsh::source_upgrade_block(&root, cancelled).await? {
                None => match dsh::upgrade_source_checkout(sink, &root, cancelled).await {
                    Ok(()) => {
                        sink.emit(
                            AgentInstallStage::RefreshingHooks,
                            94,
                            "Refreshing hook integration",
                        );
                        crate::hooks::cache_status(crate::hooks::ensure_installed());
                        sink.emit(AgentInstallStage::Completed, 100, "Upgrade complete");
                        return Ok(DshStrategyOutcome {
                            channels: vec![crate::app_settings::AgentUpgradeChannel {
                                channel: "source".to_string(),
                                success: true,
                                message: format!(
                                    "upgraded the DSH source checkout at {} in place",
                                    root.display()
                                ),
                            }],
                            channel: "source".to_string(),
                            installed_launcher: None,
                        });
                    }
                    // 构建失败也不报死,降级到托管安装,保证"一定有结果"。
                    Err(error) if error.code != AgentInstallErrorCode::Cancelled => {
                        notes.push(format!(
                            "in-place source upgrade failed, falling back to an Aeroric-managed install: {}",
                            error.message
                        ));
                        dsh::DshStrategy::Managed
                    }
                    Err(error) => return Err(error),
                },
                Some(block) => {
                    notes.push(format!(
                        "using an Aeroric-managed install because {}",
                        block.reason()
                    ));
                    dsh::DshStrategy::Managed
                }
            }
        }
        other => other,
    };

    match strategy {
        dsh::DshStrategy::PackageManager => {
            sink.emit(
                AgentInstallStage::Installing,
                45,
                "Running the package manager upgrade",
            );
            let program = active_program.to_string();
            let target = expected_version.map(str::to_string);
            let channels = tokio::task::spawn_blocking(move || {
                crate::app_settings::run_dsh_package_manager_upgrade(&program, target.as_deref())
            })
            .await
            .map_err(|error| {
                InstallError::new(
                    AgentInstallErrorCode::Internal,
                    format!("The upgrade worker failed: {error}"),
                )
            })?;
            sink.emit(
                AgentInstallStage::RefreshingHooks,
                94,
                "Refreshing hook integration",
            );
            crate::hooks::cache_status(crate::hooks::ensure_installed());
            sink.emit(AgentInstallStage::Completed, 100, "Upgrade complete");
            Ok(DshStrategyOutcome {
                channels,
                channel: "npm".to_string(),
                installed_launcher: None,
            })
        }
        dsh::DshStrategy::Managed | dsh::DshStrategy::SourceCheckout { .. } => {
            let target_version = match expected_version {
                Some(version) => version.to_string(),
                None => {
                    sink.emit(
                        AgentInstallStage::Detecting,
                        5,
                        "Resolving the latest DeepSeek Harness version",
                    );
                    latest_dsh_version(cancelled).await?
                }
            };
            let installed = dsh::install_managed(sink, &target_version, cancelled).await?;
            sink.emit(
                AgentInstallStage::RefreshingHooks,
                94,
                "Refreshing hook integration",
            );
            // 托管副本装好后把 dsh_path 指过去,否则下次启动还会用旧的活动安装。
            let launcher = installed.launcher.to_string_lossy().into_owned();
            if crate::app_settings::configured_agent_path(settings, "dsh") != launcher {
                crate::app_settings::set_configured_dsh_path(&launcher).map_err(|error| {
                    InstallError::new(
                        AgentInstallErrorCode::Internal,
                        format!("Cannot point dsh_path at the managed install: {error}"),
                    )
                })?;
                notes.push(format!(
                    "dsh_path now points at the managed install {launcher}"
                ));
            }
            if installed.node_managed {
                notes.push(
                    "downloaded a private Node.js runtime into ~/.aeroric/tools/node".to_string(),
                );
            }
            crate::hooks::cache_status(crate::hooks::ensure_installed());
            sink.emit(AgentInstallStage::Completed, 100, "Installation complete");
            let mut message = format!(
                "installed {}@{} into {}",
                dsh::DSH_NPM_PACKAGE,
                installed.version,
                installed.launcher.display()
            );
            if !notes.is_empty() {
                message.push('\n');
                message.push_str(&notes.join("\n"));
            }
            Ok(DshStrategyOutcome {
                channels: vec![crate::app_settings::AgentUpgradeChannel {
                    channel: "managed".to_string(),
                    success: true,
                    message,
                }],
                channel: "managed".to_string(),
                installed_launcher: Some(installed.launcher),
            })
        }
    }
}

/// 统一入口:内置 Claude/Codex 走原生安装管线,dsh 走上面那套策略,自定义 Agent
/// 归并到它的二进制。
pub(crate) async fn run_agent_operation(
    app: &AppHandle,
    operation_id: &str,
    binary_agent: &str,
    _requested_agent: &str,
    kind: crate::agent_ops::AgentOperationKind,
    expected_version: Option<&str>,
    cancelled: &Arc<AtomicBool>,
) -> crate::agent_ops::OperationOutcome {
    use crate::agent_ops::{AgentOperationKind, OperationOutcome};

    if binary_agent == "dsh" {
        return run_dsh_operation(app, operation_id, kind, cancelled, expected_version).await;
    }
    let Some(agent) = BuiltInAgent::parse(binary_agent) else {
        return OperationOutcome::Error {
            code: AgentInstallErrorCode::InvalidAgent,
            message: format!("Unknown agent {binary_agent}"),
        };
    };

    // 未安装、或已是 Aeroric 托管副本时用原生安装管线;否则走包管理器升级。
    let managed = status_for(agent).managed;
    if kind == AgentOperationKind::Install || managed {
        let (_guard, guard_cancelled) =
            match OperationGuard::begin(operation_id.to_string(), vec![agent]) {
                Ok(guard) => guard,
                Err(error) => {
                    return OperationOutcome::Error {
                        code: error.code,
                        message: error.message,
                    }
                }
            };
        // 注册表的取消标记与 install 管线自己的标记要联动。转发任务的生命周期绑在
        // 这个 guard 上,install 一结束就 abort —— 否则安装成功时两个 flag 都不会
        // 被置位,轮询循环会永远跑下去,每次操作泄漏一个任务。
        let _bridge = CancellationBridge::spawn(cancelled.clone(), guard_cancelled.clone());
        let result = install_one(app, operation_id, agent, &guard_cancelled).await;
        return OperationOutcome::Install(result);
    }

    let sink = ProgressSink {
        app,
        operation_id,
        agent: binary_agent,
    };
    sink.emit(
        AgentInstallStage::Installing,
        30,
        "Running the package manager upgrade",
    );
    let agent_id = binary_agent.to_string();
    let target = expected_version.map(str::to_string);
    let outcome = tokio::task::spawn_blocking(move || {
        crate::app_settings::run_builtin_agent_upgrade(&agent_id, target.as_deref())
    })
    .await;
    match outcome {
        Ok(Ok(result)) => {
            sink.emit(
                AgentInstallStage::RefreshingHooks,
                94,
                "Refreshing hook integration",
            );
            crate::hooks::cache_status(crate::hooks::ensure_installed());
            sink.emit(AgentInstallStage::Completed, 100, "Upgrade complete");
            OperationOutcome::Upgrade(result)
        }
        Ok(Err(message)) => OperationOutcome::Error {
            code: AgentInstallErrorCode::InstallFailed,
            message,
        },
        Err(error) => OperationOutcome::Error {
            code: AgentInstallErrorCode::Internal,
            message: format!("The upgrade worker failed: {error}"),
        },
    }
}

/// 把注册表的取消标记转发进 install 管线自己的标记。
///
/// 为什么还需要它:`cancel_agent_operation` 会直接按 operation_id 调
/// `cancel_agent_tool_install`,正常路径上不依赖转发。但那条路径要求 guard 已经
/// 登记完毕,所以「取消比 guard 注册更早到」这个窗口只能靠转发兜住。
///
/// 生命周期必须绑在调用方的作用域上:装完了就 abort。早先的实现把任务 detach 掉,
/// 循环条件又只看两个 flag —— 安装**成功**时两者都不会被置位(`OperationGuard::drop`
/// 只清 map 条目,不动 bool,而任务自己还攥着一份 Arc),于是每次操作都留下一个
/// 永远以 100ms 轮询的任务。
struct CancellationBridge(tokio::task::JoinHandle<()>);

impl CancellationBridge {
    fn spawn(outer: Arc<AtomicBool>, inner: Arc<AtomicBool>) -> Self {
        // 已经取消了就不必等第一个 tick,立刻同步过去。
        if outer.load(Ordering::Relaxed) {
            inner.store(true, Ordering::Relaxed);
        }
        Self(tokio::spawn(async move {
            while !inner.load(Ordering::Relaxed) {
                if outer.load(Ordering::Relaxed) {
                    inner.store(true, Ordering::Relaxed);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }))
    }
}

impl Drop for CancellationBridge {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn status_for(agent: BuiltInAgent) -> AgentToolStatus {
    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, agent.id());
    let version = crate::app_settings::detect_launch_version(&launch).unwrap_or_default();
    let configured = crate::app_settings::configured_agent_path(&settings, agent.id());
    // 启动规格包含最终真正执行的 PATH/Homebrew/npm shim 程序；升级渠道和状态
    // 必须基于这个有效路径，否则 Homebrew 安装会被误判为 standalone。
    let effective_program = if launch.program.trim().is_empty() {
        configured.clone()
    } else {
        launch.program.clone()
    };
    let managed_path = managed_tool_path(agent);
    let managed = managed_path
        .as_ref()
        .is_some_and(|path| Path::new(&effective_program) == path);
    let unsupported = platform_support(agent).err();
    let channel = if version.is_empty() {
        String::new()
    } else if managed {
        "aeroric-managed-native".to_string()
    } else if launch.program.is_empty() {
        String::new()
    } else {
        crate::app_settings::upgrade_manager_for_path(&effective_program).to_string()
    };
    AgentToolStatus {
        agent: agent.id().to_string(),
        supported: unsupported.is_none(),
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        libc: current_libc_label(),
        installed: !version.is_empty(),
        version,
        path: effective_program,
        channel,
        managed,
        error_code: unsupported.as_ref().map(|error| error.code.clone()),
        error: unsupported.map(|error| error.message).unwrap_or_default(),
    }
}

/// dsh 的安装状态:npm 分发、无 tools_dir 托管概念,platform 无限制。
fn dsh_status() -> AgentToolStatus {
    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, "dsh");
    let version = crate::app_settings::detect_launch_version(&launch).unwrap_or_default();
    let configured = crate::app_settings::configured_agent_path(&settings, "dsh");
    let effective_program = if launch.program.trim().is_empty() {
        configured.clone()
    } else {
        launch.program.clone()
    };
    // 托管副本要先判,再落到包管理器检测:托管 shim 经 symlink 会命中
    // `/node_modules/`,被 `detected_upgrade_manager` 误报成 npm。
    let managed =
        dsh::is_managed_program(&configured) || dsh::is_managed_program(&effective_program);
    let channel = if version.is_empty() {
        String::new()
    } else if managed {
        "managed".to_string()
    } else {
        crate::app_settings::upgrade_manager_for_path(&effective_program).to_string()
    };
    AgentToolStatus {
        agent: "dsh".to_string(),
        supported: true,
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        libc: current_libc_label(),
        installed: !version.is_empty(),
        version,
        path: effective_program,
        channel,
        managed,
        error_code: None,
        error: String::new(),
    }
}

#[tauri::command]
pub async fn get_agent_tool_status() -> Result<Vec<AgentToolStatus>, String> {
    tokio::task::spawn_blocking(|| {
        vec![
            status_for(BuiltInAgent::Claude),
            status_for(BuiltInAgent::Codex),
            dsh_status(),
        ]
    })
    .await
    .map_err(|error| error.to_string())
}

/// 查询 Claude Code 官方发布服务的最新版本号（不下载安装包）。
async fn latest_claude_version(cancelled: &AtomicBool) -> InstallResult<String> {
    claude_platform()?;
    let client = http_client(&["downloads.claude.ai"])?;
    let version_bytes = download_small_bytes(
        &client,
        "https://downloads.claude.ai/claude-code-releases/latest",
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let version = String::from_utf8(version_bytes)
        .map_err(|error| {
            InstallError::new(
                AgentInstallErrorCode::DownloadFailed,
                format!("Invalid Claude release version: {error}"),
            )
        })?
        .trim()
        .to_string();
    if crate::app_settings::extract_version(&version).as_deref() != Some(version.as_str()) {
        return Err(InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            "Claude release service returned an invalid version",
        ));
    }
    Ok(version)
}

/// 查询 Codex GitHub Release 的最新版本号（只读取 release metadata）。
async fn latest_codex_version(cancelled: &AtomicBool) -> InstallResult<String> {
    codex_target()?;
    let client = http_client(&["api.github.com"])?;
    let release_bytes =
        download_small_bytes(&client, CODEX_RELEASE_API, MAX_METADATA_BYTES, cancelled).await?;
    let release: GitHubRelease = serde_json::from_slice(&release_bytes).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Invalid Codex release metadata: {error}"),
        )
    })?;
    codex_version_from_tag(&release.tag_name)
}

/// npm registry 查询 dsh 最新版本(`@deepseek-ai/dsh`,dev preview 期版本形如
/// 0.1.0-rc.6,保留完整预发布后缀用于"当前 vs 最新"比较)。
async fn latest_dsh_version(cancelled: &AtomicBool) -> InstallResult<String> {
    let client = http_client(&["registry.npmjs.org"])?;
    let bytes = download_small_bytes(
        &client,
        "https://registry.npmjs.org/@deepseek-ai/dsh/latest",
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let metadata: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Invalid dsh registry metadata: {error}"),
        )
    })?;
    let version = metadata
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| {
            InstallError::new(
                AgentInstallErrorCode::DownloadFailed,
                "dsh registry metadata has no version",
            )
        })?;
    Ok(version.to_string())
}

/// 单个内置 Agent 的最新可用版本。查询失败时只返回错误信息，不影响其它 Agent。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentLatestVersion {
    pub agent: String,
    pub version: String,
    pub error_code: Option<AgentInstallErrorCode>,
    pub error: String,
}

#[tauri::command]
pub async fn get_agent_latest_versions() -> Result<Vec<AgentLatestVersion>, String> {
    let cancelled = AtomicBool::new(false);
    let mut results = Vec::with_capacity(2);
    for agent in [BuiltInAgent::Claude, BuiltInAgent::Codex] {
        let outcome = match agent {
            BuiltInAgent::Claude => latest_claude_version(&cancelled).await,
            BuiltInAgent::Codex => latest_codex_version(&cancelled).await,
        };
        results.push(match outcome {
            Ok(version) => AgentLatestVersion {
                agent: agent.id().to_string(),
                version,
                error_code: None,
                error: String::new(),
            },
            Err(error) => AgentLatestVersion {
                agent: agent.id().to_string(),
                version: String::new(),
                error_code: Some(error.code),
                error: error.message,
            },
        });
    }
    // dsh:npm 分发,无 tools_dir 原生安装机制;仅提供最新版本查询,
    // 安装/升级走 upgrade_agent_tool 的 npm 通道。
    results.push(match latest_dsh_version(&cancelled).await {
        Ok(version) => AgentLatestVersion {
            agent: "dsh".to_string(),
            version,
            error_code: None,
            error: String::new(),
        },
        Err(error) => AgentLatestVersion {
            agent: "dsh".to_string(),
            version: String::new(),
            error_code: Some(error.code),
            error: error.message,
        },
    });
    Ok(results)
}

#[tauri::command]
pub async fn install_agent_tools(
    app: AppHandle,
    request: AgentInstallRequest,
) -> Result<Vec<AgentInstallResult>, String> {
    let operation_id = if request.operation_id.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        request.operation_id.trim().to_string()
    };
    let mut requested = Vec::new();
    for value in request.agents {
        let Some(agent) = BuiltInAgent::parse(value.trim()) else {
            return Err(format!(
                "invalid_agent: only built-in Claude Code and Codex can be installed: {value}"
            ));
        };
        if !requested.contains(&agent) {
            requested.push(agent);
        }
    }
    if requested.is_empty() {
        return Err("invalid_agent: select at least one Agent to install".to_string());
    }
    let (_guard, cancelled) = OperationGuard::begin(operation_id.clone(), requested.clone())
        .map_err(|error| format!("{}: {}", error.code.as_str(), error.message))?;
    let mut results = Vec::with_capacity(requested.len());
    for agent in requested {
        results.push(install_one(&app, &operation_id, agent, &cancelled).await);
    }
    Ok(results)
}

#[tauri::command]
pub fn cancel_agent_tool_install(operation_id: String) -> Result<(), String> {
    let Some(cancelled) = install_state()
        .lock()
        .cancellations
        .get(operation_id.trim())
        .cloned()
    else {
        return Ok(());
    };
    cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_matrix_covers_all_requested_native_targets() {
        for arch in ["x86_64", "aarch64"] {
            assert!(claude_platform_for("macos", arch, LinuxLibc::Glibc).is_ok());
            assert!(claude_platform_for("windows", arch, LinuxLibc::Glibc).is_ok());
            assert!(claude_platform_for("linux", arch, LinuxLibc::Glibc).is_ok());
            assert!(claude_platform_for("linux", arch, LinuxLibc::Musl).is_ok());
            assert!(codex_target_for("macos", arch).is_ok());
            assert!(codex_target_for("windows", arch).is_ok());
            assert!(codex_target_for("linux", arch).is_ok());
        }
    }

    #[test]
    fn sha256_verification_rejects_mismatch() {
        let error = verify_sha256(
            &format!("{:x}", Sha256::digest(b"aeroric")),
            &"0".repeat(64),
        )
        .unwrap_err();
        assert_eq!(error.code, AgentInstallErrorCode::ChecksumFailed);
    }

    #[test]
    fn only_builtin_agents_are_installable() {
        assert_eq!(BuiltInAgent::parse("claude"), Some(BuiltInAgent::Claude));
        assert_eq!(BuiltInAgent::parse("codex"), Some(BuiltInAgent::Codex));
        assert_eq!(BuiltInAgent::parse("custom"), None);
    }

    #[test]
    fn codex_checksum_manifest_requires_exact_asset_name() {
        let manifest = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  codex-package-x86_64-apple-darwin.tar.gz\n";
        assert_eq!(
            checksum_for_asset(manifest, "codex-package-x86_64-apple-darwin.tar.gz").unwrap(),
            "a".repeat(64)
        );
        assert!(checksum_for_asset(manifest, "codex-package-other.tar.gz").is_err());
    }

    #[test]
    fn unsafe_archive_paths_are_rejected() {
        assert!(safe_archive_path(Path::new("bin/codex")));
        assert!(!safe_archive_path(Path::new("../outside")));
        assert!(!safe_archive_path(Path::new("/absolute")));
    }

    /// 官方 Node 发行包的 `bin/npm -> ../lib/node_modules/npm/bin/npm-cli.js`
    /// 必须放行,同时不能给出逃出解包目录的口子。
    #[test]
    fn symlink_targets_may_climb_out_of_their_own_directory_but_not_the_archive() {
        // Node 发行包里真实存在的三条。
        assert!(safe_symlink_target(
            Path::new("bin/npm"),
            Path::new("../lib/node_modules/npm/bin/npm-cli.js")
        ));
        assert!(safe_symlink_target(
            Path::new("bin/npx"),
            Path::new("../lib/node_modules/npm/bin/npx-cli.js")
        ));
        assert!(safe_symlink_target(
            Path::new("bin/corepack"),
            Path::new("../lib/node_modules/corepack/dist/corepack.js")
        ));
        // 同目录内的链接。
        assert!(safe_symlink_target(
            Path::new("bin/node"),
            Path::new("node-24")
        ));

        // 逃逸:深度归零后再往上。
        assert!(!safe_symlink_target(
            Path::new("bin/evil"),
            Path::new("../../etc/passwd")
        ));
        assert!(!safe_symlink_target(
            Path::new("evil"),
            Path::new("../outside")
        ));
        assert!(!safe_symlink_target(
            Path::new("a/b/evil"),
            Path::new("../../../outside")
        ));
        // 绝对 target。
        assert!(!safe_symlink_target(
            Path::new("bin/evil"),
            Path::new("/etc/passwd")
        ));
        // 中途出界,即使末尾又走回来也不行。
        assert!(!safe_symlink_target(
            Path::new("evil"),
            Path::new("../../tmp/x")
        ));
    }

    fn tar_gz_with_symlink(archive: &Path, link_target: &str) {
        let file = fs::File::create(archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);

        let payload = b"console.log('npm')\n";
        let mut header = tar::Header::new_gnu();
        header
            .set_path("lib/node_modules/npm/bin/npm-cli.js")
            .unwrap();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append(&header, &payload[..]).unwrap();

        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_path("bin/npm").unwrap();
        link.set_link_name(link_target).unwrap();
        link.set_cksum();
        builder.append(&link, &[][..]).unwrap();

        builder.into_inner().unwrap().finish().unwrap();
    }

    /// 回归:整条托管 Node 安装都卡在这里 —— 拒掉符号链接等于拒掉整个 Node 发行包。
    #[test]
    fn extracting_a_node_shaped_archive_keeps_the_bin_symlinks() {
        let root = std::env::temp_dir().join(format!("aeroric-symlink-ok-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _cleanup = CleanupDir::new(root.clone());
        let archive = root.join("node.tar.gz");
        tar_gz_with_symlink(&archive, "../lib/node_modules/npm/bin/npm-cli.js");

        let output = root.join("extracted");
        extract_tar_gz(&archive, &output).expect("a Node-shaped archive extracts");

        let link = output.join("bin").join("npm");
        assert!(
            fs::symlink_metadata(&link).is_ok(),
            "bin/npm should exist as a link"
        );
        // 链接要真的能解到那个文件,否则 npm 起不来。
        assert_eq!(
            fs::read_to_string(&link).unwrap().trim(),
            "console.log('npm')"
        );
    }

    #[test]
    fn extracting_an_archive_whose_symlink_escapes_is_refused() {
        let root = std::env::temp_dir().join(format!("aeroric-symlink-bad-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _cleanup = CleanupDir::new(root.clone());
        let archive = root.join("evil.tar.gz");
        tar_gz_with_symlink(&archive, "../../../../etc/passwd");

        let error = extract_tar_gz(&archive, &root.join("extracted"))
            .expect_err("an escaping symlink is rejected");
        assert_eq!(error.code, AgentInstallErrorCode::ArchiveInvalid);
        assert!(error.message.contains("escapes"));
    }

    /// 取消标记要能从注册表转发进 install 管线自己的标记。
    #[tokio::test]
    async fn the_cancellation_bridge_forwards_the_registry_flag() {
        let outer = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(AtomicBool::new(false));
        let bridge = CancellationBridge::spawn(outer.clone(), inner.clone());

        outer.store(true, Ordering::Relaxed);
        // 轮询间隔 100ms,给它几轮的余量。
        for _ in 0..40 {
            if inner.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(inner.load(Ordering::Relaxed), "the flag is forwarded");
        drop(bridge);
    }

    /// 已经取消了就不该等第一个 tick。
    #[tokio::test]
    async fn the_cancellation_bridge_syncs_an_already_cancelled_flag_immediately() {
        let outer = Arc::new(AtomicBool::new(true));
        let inner = Arc::new(AtomicBool::new(false));
        let _bridge = CancellationBridge::spawn(outer, inner.clone());
        assert!(inner.load(Ordering::Relaxed));
    }

    /// 安装成功时两个 flag 都不会被置位,所以转发任务只能靠 drop 收掉 ——
    /// 否则每次操作都留下一个永远 100ms 轮询的任务。
    #[tokio::test]
    async fn dropping_the_cancellation_bridge_aborts_the_forwarding_task() {
        let outer = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(AtomicBool::new(false));
        let bridge = CancellationBridge::spawn(outer, inner);
        let handle = bridge.0.abort_handle();
        assert!(!handle.is_finished());
        drop(bridge);
        // abort 是异步生效的,让出一次给运行时收尾。
        for _ in 0..40 {
            if handle.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(handle.is_finished(), "the task is aborted on drop");
    }

    /// 首装的卡片全靠 `install_result` 里的 path / login_command 显示「装到哪、
    /// 怎么登录」。托管安装要用真正落地的启动器路径,不是设置里那条旧路径。
    #[test]
    fn a_managed_first_install_reports_the_launcher_it_landed_on() {
        let result = dsh_install_result(
            "op-1",
            true,
            false,
            "1.2.3".to_string(),
            "/managed/bin/dsh".to_string(),
            "managed".to_string(),
            true,
            "managed: installed".to_string(),
        );
        assert!(result.success);
        assert_eq!(result.path, "/managed/bin/dsh");
        assert_eq!(result.login_command, "dsh");
        assert_eq!(result.stage, AgentInstallStage::Completed);
        assert_eq!(result.progress, 100);
        assert!(result.managed);
        assert!(result.error_code.is_none());
    }

    /// 用户自己点的取消不能显示成安装失败:`start_agent_operation` 只看
    /// `stage == Cancelled` 来区分这两者。
    #[test]
    fn a_cancelled_first_install_reports_cancelled_not_failed() {
        let cancelled = dsh_install_result(
            "op-2",
            false,
            true,
            String::new(),
            "/usr/local/bin/dsh".to_string(),
            "managed".to_string(),
            false,
            "cancelled".to_string(),
        );
        assert_eq!(cancelled.stage, AgentInstallStage::Cancelled);
        assert_eq!(cancelled.error_code, Some(AgentInstallErrorCode::Cancelled));

        let failed = dsh_install_result(
            "op-3",
            false,
            false,
            String::new(),
            "/usr/local/bin/dsh".to_string(),
            "npm".to_string(),
            false,
            "npm: exited 1".to_string(),
        );
        assert_eq!(failed.stage, AgentInstallStage::Failed);
        assert_eq!(
            failed.error_code,
            Some(AgentInstallErrorCode::InstallFailed)
        );
    }

    #[test]
    fn dropping_uncommitted_activation_restores_previous_directory() {
        let root = std::env::temp_dir().join(format!("aeroric-rollback-test-{}", Uuid::new_v4()));
        let target = root.join("current");
        let staged = root.join("staged");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(target.join("version"), "old").unwrap();
        fs::write(staged.join("version"), "new").unwrap();

        let activation = ActivatedDir::activate(&staged, &target).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
        drop(activation);

        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn version_verification_executes_from_the_activated_final_path() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-final-path-verification-{}",
            Uuid::new_v4()
        ));
        let staged = root.join("staged");
        let target = root.join("current");
        fs::create_dir_all(&staged).unwrap();
        let staged_binary = staged.join("tool");
        fs::write(
            &staged_binary,
            "#!/bin/sh\ncase \"$0\" in */current/tool) echo 1.2.3 ;; *) exit 5 ;; esac\n",
        )
        .unwrap();
        make_executable(&staged_binary).unwrap();

        let activation = ActivatedDir::activate(&staged, &target).unwrap();
        let cancelled = AtomicBool::new(false);
        let detected = detect_version(&target.join("tool"), &cancelled)
            .await
            .unwrap();

        assert_eq!(detected, "1.2.3");
        activation.commit();
        let _ = fs::remove_dir_all(root);
    }
}
