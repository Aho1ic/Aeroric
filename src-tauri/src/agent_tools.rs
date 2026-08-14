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
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use uuid::Uuid;

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

fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    agent: BuiltInAgent,
    stage: AgentInstallStage,
    progress: u8,
    error_code: Option<AgentInstallErrorCode>,
    message: impl Into<String>,
) {
    let _ = app.emit(
        INSTALL_EVENT,
        AgentInstallProgress {
            operation_id: operation_id.to_string(),
            agent: agent.id().to_string(),
            stage,
            progress,
            error_code,
            message: message.into(),
        },
    );
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

struct DownloadProgress<'a> {
    app: &'a AppHandle,
    operation_id: &'a str,
    agent: BuiltInAgent,
    start: u8,
    end: u8,
    message: String,
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
                progress.agent,
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
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(InstallError::new(
                AgentInstallErrorCode::ArchiveInvalid,
                format!("Archive contains unsupported entry {}", path.display()),
            ));
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
    crate::subprocess::configure_background_tokio_command(&mut command);
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
            let _ = child.kill().await;
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
                let _ = child.kill().await;
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
        agent,
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
        agent,
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
            agent,
            start: 14,
            end: 66,
            message: format!("Downloading Claude Code {version}"),
        },
    )
    .await?;
    emit_progress(
        app,
        operation_id,
        agent,
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
        agent,
        AgentInstallStage::Installing,
        78,
        None,
        "Installing Claude Code",
    );
    emit_progress(
        app,
        operation_id,
        agent,
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
        agent,
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
        agent,
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
        agent,
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
            agent,
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
        agent,
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
            agent,
            start: 20,
            end: 66,
            message: format!("Downloading Codex {version}"),
        },
    )
    .await?;
    emit_progress(
        app,
        operation_id,
        agent,
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
        agent,
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
        agent,
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
        agent,
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
                agent,
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
                agent,
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
                agent,
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

fn status_for(agent: BuiltInAgent) -> AgentToolStatus {
    let settings = crate::app_settings::load_settings_internal();
    let launch = crate::app_settings::get_agent_launch_spec_from(&settings, agent.id());
    let version = crate::app_settings::detect_launch_version(&launch).unwrap_or_default();
    let configured = crate::app_settings::configured_agent_path(&settings, agent.id());
    let managed_path = managed_tool_path(agent);
    let managed = managed_path
        .as_ref()
        .is_some_and(|path| Path::new(&configured) == path);
    let unsupported = platform_support(agent).err();
    let channel = if version.is_empty() {
        String::new()
    } else if managed {
        "aeroric-managed-native".to_string()
    } else if launch.program.is_empty() {
        String::new()
    } else {
        crate::app_settings::upgrade_manager_for_path(&configured).to_string()
    };
    AgentToolStatus {
        agent: agent.id().to_string(),
        supported: unsupported.is_none(),
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        libc: current_libc_label(),
        installed: !version.is_empty(),
        version,
        path: configured,
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
    let channel = if version.is_empty() {
        String::new()
    } else {
        crate::app_settings::upgrade_manager_for_path(&configured).to_string()
    };
    AgentToolStatus {
        agent: "dsh".to_string(),
        supported: true,
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        libc: current_libc_label(),
        installed: !version.is_empty(),
        version,
        path: configured,
        channel,
        managed: false,
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
