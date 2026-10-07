//! 通知与更新的 GUI 适配层（Task P8-04 刀三）。
//!
//! 编排（要不要抓、抓回来怎么对账、准备阶段的顺序、周报循环）已上收
//! `aeroric_core::maintenance`。本文件只剩**宿主专属**的三块：
//!
//! 1. **网络**：`reqwest` 抓 GitHub Releases、流式下载资产（`fetch_remote` /
//!    `fetch_release_by_tag` / `download_asset`）。
//! 2. **平台替换**：`hdiutil`+`ditto`+`mv`（macOS）与 detached update helper
//!    （Windows/Linux）—— 它们替换**正在运行的这个 app**，只有持有 app 身份的进程能做。
//! 3. **Tauri 出口**：`authorize_app_restart` / `request_restart` / `exit`。
//!
//! 这一层实现 [`MaintenanceHost`] 端口并把 core 的 `MaintenanceService` 装起来，
//! 所以 `#[tauri::command]` 与宿主 UDS 服务走的是**同一份编排**（§3.1 #8）。
//! 平台权限面（`permission.*`）不在这里：那六个命令按 `command-map.json` 的
//! `rpc_method: null` 属于 `host_local_os_ui`，只由 GUI 适配器执行。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

#[cfg(target_os = "macos")]
use crate::authorize_app_restart;

use aeroric_core::maintenance::updates::TargetPlatform;
use aeroric_core::maintenance::{
    sanitize_text, validate_release_asset_filename, GitHubRelease, GitHubReleaseAsset,
    MaintenanceHost, MaintenanceService, ReleaseInstallResult, ReleaseUpdatePrepareResult,
    RELEASES_URL,
};

/// `updates/<safe_tag>`（core 的 `release_update_dir` 从 `maintenance` 根透出）。
use aeroric_core::maintenance::updates::release_update_dir;
use aeroric_core::tasks::cleanup::LocalCalendar;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024; // 1MB limit
const MAX_UPDATE_ASSET_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB
const MAX_CHECKSUM_ASSET_BYTES: u64 = 1024 * 1024; // 1 MiB
const REQUEST_TIMEOUT_SECS: u64 = 15;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const UPDATE_HELPER_FLAG: &str = "--aeroric-update-helper";
#[cfg(any(target_os = "windows", target_os = "linux"))]
const UPDATE_HELPER_RUNNING: &str = "running";
const UPDATE_HELPER_FAILED: &str = "failed";

/// 命令签名用的别名（core 类型是唯一真相；宿主只借名字让 `lib.rs` 的注册表读得顺）。
pub type NotificationResult = aeroric_core::maintenance::NotificationResult;

// ── 宿主端口：core 编排伸手要的东西 ─────────────────────────────────────────

/// 本地日历：`chrono::Local` 的语义，与旧前端 `new Date(y,m,d,h)` 一致。
struct SystemCalendar;

impl LocalCalendar for SystemCalendar {
    fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64) {
        use chrono::{Datelike, Local, TimeZone, Timelike};
        match Local.timestamp_millis_opt(now_ms).single() {
            Some(moment) => (
                moment.weekday().num_days_from_sunday(),
                moment.year() as i64,
                moment.month() as i64,
                moment.day() as i64,
                moment.hour() as i64,
            ),
            None => (0, 1970, 1, 1, 0),
        }
    }

    fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64 {
        use chrono::{Local, NaiveDate, TimeZone};
        // JS 会**归一化**越界分量（`new Date(2026, 8, -3, 20)` 是上个月末），
        // 这里用天数偏移复现同一语义：先把 (year, month-1, 1) 当基准再加 day-1 天。
        // 任何一步构造不出来都退 epoch/午夜——调度规则要的是"一个时刻"，不是 panic。
        let base = NaiveDate::from_ymd_opt(year as i32, month.clamp(1, 12) as u32, 1)
            .or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1))
            .unwrap_or_default();
        let mut shifted = base;
        let month_delta = month - month.clamp(1, 12);
        if month_delta != 0 {
            shifted = shift_months(shifted, month_delta);
        }
        let shifted = shifted + chrono::Duration::days(day - 1);
        let naive = shifted
            .and_hms_opt(hour.clamp(0, 23) as u32, 0, 0)
            .or_else(|| shifted.and_hms_opt(0, 0, 0))
            .unwrap_or_default();
        Local
            .from_local_datetime(&naive)
            .single()
            .map(|moment| moment.timestamp_millis())
            .unwrap_or_else(|| naive.and_utc().timestamp_millis())
    }
}

fn shift_months(date: chrono::NaiveDate, delta: i64) -> chrono::NaiveDate {
    use chrono::{Datelike, NaiveDate};
    let mut year = date.year() as i64;
    let mut month = date.month() as i64 + delta;
    while month > 12 {
        month -= 12;
        year += 1;
    }
    while month < 1 {
        month += 12;
        year -= 1;
    }
    NaiveDate::from_ymd_opt(year as i32, month as u32, date.day()).unwrap_or(date)
}

/// GUI 侧的维护面端口：网络＋平台探测＋摘要。
pub struct GuiMaintenanceHost;

#[async_trait::async_trait]
impl MaintenanceHost for GuiMaintenanceHost {
    fn app_version(&self) -> String {
        APP_VERSION.to_string()
    }

    fn platform(&self) -> TargetPlatform<'static> {
        TargetPlatform {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            linux_kind: linux_package_kind(),
        }
    }

    fn calendar(&self) -> Arc<dyn LocalCalendar> {
        Arc::new(SystemCalendar)
    }

    async fn fetch_releases(&self) -> Result<Vec<GitHubRelease>, String> {
        fetch_remote().await
    }

    async fn fetch_release(&self, tag: &str) -> Result<GitHubRelease, String> {
        fetch_release_by_tag(tag).await
    }

    async fn download_release_assets(
        &self,
        release: &GitHubRelease,
        asset_name: &str,
        tag_name: &str,
    ) -> Result<(String, String), String> {
        let asset = release
            .assets
            .iter()
            .find(|candidate| candidate.name == asset_name)
            .cloned()
            .ok_or_else(|| "Missing installer asset for prepared update.".to_string())?;
        let checksum_name =
            aeroric_core::maintenance::find_checksum_for_asset(&asset, &release.assets)?;
        let checksum_asset = release
            .assets
            .iter()
            .find(|candidate| candidate.name == checksum_name)
            .cloned()
            .ok_or_else(|| "Missing checksum asset for prepared update.".to_string())?;
        let update_dir = release_update_dir(&updates_dir()?, tag_name);
        let _ = tokio::fs::remove_dir_all(&update_dir).await;
        tokio::fs::create_dir_all(&update_dir)
            .await
            .map_err(|error| format!("Create update directory failed: {error}"))?;
        aeroric_core::maintenance::validate_release_asset_filename(&asset.name)?;
        aeroric_core::maintenance::validate_release_asset_filename(&checksum_asset.name)?;
        let installer_path = update_dir.join(&asset.name);
        let checksum_path = update_dir.join(&checksum_asset.name);
        download_asset(&asset, &installer_path, MAX_UPDATE_ASSET_BYTES).await?;
        download_asset(&checksum_asset, &checksum_path, MAX_CHECKSUM_ASSET_BYTES).await?;
        let checksum_text = tokio::fs::read_to_string(&checksum_path)
            .await
            .map_err(|error| format!("Read checksum failed: {error}"))?;
        let verify_path = installer_path.clone();
        let verify_name = asset.name.clone();
        tokio::task::spawn_blocking(move || {
            aeroric_core::maintenance::verify_downloaded_checksum(
                &verify_path,
                &verify_name,
                &checksum_text,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
        Ok((
            installer_path.to_string_lossy().into_owned(),
            checksum_path.to_string_lossy().into_owned(),
        ))
    }

    async fn summarize(
        &self,
        task: &aeroric_core::maintenance::WeeklyReportTask,
        locale: &str,
    ) -> Option<String> {
        let session_path = task.session_path.as_deref()?;
        if session_path.is_empty() {
            return None;
        }
        // 认不出的家族退 `claude`（core 的 `session_family()` 已归一），最坏结果只是
        // 这一条摘要降级成标题——旧 `report.rs` 同款。
        // 同一份 `AgentFamily` 只存在于 core（宿主 `app_settings` 是它的 re-export）。
        let family =
            crate::app_settings::AgentFamily::parse(task.session_family()).unwrap_or_default();
        crate::agent_assist::summarize_task_session(
            session_path,
            &task.project_path,
            family,
            locale,
        )
        .await
    }
}

/// 进程级维护面服务（Tauri 命令与宿主 UDS 服务共用同一份编排与同一份记账锁）。
pub fn service() -> &'static MaintenanceService {
    static SERVICE: std::sync::LazyLock<MaintenanceService> = std::sync::LazyLock::new(|| {
        MaintenanceService::new(
            Arc::new(GuiMaintenanceHost),
            crate::storage::aeroric_dir().unwrap_or_default(),
        )
    });
    &SERVICE
}

// ── 宿主专属：路径 ──────────────────────────────────────────────────────────

fn aeroric_dir() -> Result<PathBuf, String> {
    crate::storage::aeroric_dir()
}

fn updates_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("updates"))
}

fn update_helpers_dir() -> Result<PathBuf, String> {
    Ok(updates_dir()?.join("helpers"))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn update_helper_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Aeroric-update-helper.exe"
    } else {
        "Aeroric-update-helper"
    }
}

fn linux_package_kind() -> Option<&'static str> {
    if Path::new("/etc/debian_version").exists() {
        Some("deb")
    } else if Path::new("/etc/redhat-release").exists()
        || !crate::platform::detect_path("rpm").is_empty()
    {
        Some("rpm")
    } else {
        None
    }
}

async fn fetch_remote() -> Result<Vec<GitHubRelease>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none()) // no redirects to prevent domain bypass
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Aeroric")
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;

    // Verify response is from the expected domain (guard against redirect tricks)
    let final_url = resp.url().as_str();
    if !final_url.starts_with(RELEASES_URL) {
        return Err(format!("Unexpected response URL: {final_url}"));
    }

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    // Verify content-type is JSON
    if let Some(ct) = resp.headers().get("content-type") {
        let ct_str = ct.to_str().unwrap_or("");
        if !ct_str.contains("application/json") && !ct_str.contains("text/plain") {
            return Err(format!("Unexpected content-type: {ct_str}"));
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Read body failed: {e}"))?;

    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Response exceeds 1MB limit".to_string());
    }

    let releases: Vec<GitHubRelease> =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid JSON: {e}"))?;

    // Limit notification count to prevent memory abuse
    if releases.len() > 200 {
        return Err("Too many notifications".to_string());
    }

    // 只做"抓回来 + 解析 + 滤草稿"：判定（新旧、能不能装、过期、版本区间）
    // 全在 core 的 `release_to_notification` / `is_valid`。
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft)
        .collect())
}

async fn fetch_release_by_tag(tag_name: &str) -> Result<GitHubRelease, String> {
    let sanitized_tag = sanitize_text(tag_name.trim(), 80);
    if sanitized_tag.is_empty()
        || sanitized_tag.contains('/')
        || sanitized_tag.contains('\\')
        || sanitized_tag.contains("..")
    {
        return Err("Invalid release tag".to_string());
    }

    let url = format!("https://api.github.com/repos/Aho1ic/Aeroric/releases/tags/{sanitized_tag}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Aeroric")
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;

    if resp.url().as_str() != url {
        return Err(format!("Unexpected response URL: {}", resp.url()));
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Read body failed: {e}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Response exceeds 1MB limit".to_string());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid JSON: {e}"))
}

fn checked_download_total(current: u64, chunk_bytes: usize, max_bytes: u64) -> Result<u64, String> {
    let total = current
        .checked_add(chunk_bytes as u64)
        .ok_or_else(|| "Download size overflow".to_string())?;
    if total > max_bytes {
        return Err(format!("Download exceeds the {} byte limit", max_bytes));
    }
    Ok(total)
}

async fn download_asset(
    asset: &GitHubReleaseAsset,
    target: &Path,
    max_bytes: u64,
) -> Result<(), String> {
    let url = asset.browser_download_url.trim();
    if !url.starts_with("https://github.com/Aho1ic/Aeroric/releases/download/") {
        return Err("Unexpected asset download URL".to_string());
    }
    validate_release_asset_filename(&asset.name)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let response = client
        .get(url)
        .header("User-Agent", "Aeroric")
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download failed: {e}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(format!("Download exceeds the {} byte limit", max_bytes));
    }

    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid download target filename".to_string())?;
    let temporary = target.with_file_name(format!(".{target_name}.{}.part", uuid::Uuid::new_v4()));
    let result = async {
        let mut file = tokio::fs::File::create(&temporary)
            .await
            .map_err(|e| format!("Create download file failed: {e}"))?;
        let mut stream = response.bytes_stream();
        let mut total_bytes = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Read download failed: {e}"))?;
            total_bytes = checked_download_total(total_bytes, chunk.len(), max_bytes)?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write download failed: {e}"))?;
        }
        file.flush()
            .await
            .map_err(|e| format!("Flush download failed: {e}"))?;
        file.sync_all()
            .await
            .map_err(|e| format!("Sync download failed: {e}"))?;
        drop(file);
        tokio::fs::rename(&temporary, target)
            .await
            .map_err(|e| format!("Finalize download failed: {e}"))
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(target_os = "macos")]
async fn run_command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("Failed to run {program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{program} failed: {stderr}"))
    }
}

#[cfg(target_os = "macos")]
async fn install_macos_dmg(dmg_path: &Path) -> Result<String, String> {
    let dmg = dmg_path
        .to_str()
        .ok_or_else(|| "Invalid DMG path".to_string())?;
    let plist = run_command(
        "hdiutil",
        &["attach", dmg, "-nobrowse", "-readonly", "-plist"],
    )
    .await?;
    let mount_point = plist
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("<string>/Volumes/") {
                return None;
            }
            Some(
                trimmed
                    .trim_start_matches("<string>")
                    .trim_end_matches("</string>")
                    .to_string(),
            )
        })
        .ok_or_else(|| "Mounted DMG volume was not found".to_string())?;

    let source_app = Path::new(&mount_point).join("Aeroric.app");
    let source = source_app
        .to_str()
        .ok_or_else(|| "Invalid source app path".to_string())?;
    let destination = "/Applications/Aeroric.app";
    let update_id = uuid::Uuid::new_v4();
    let staging = format!("/Applications/Aeroric.app.update-{update_id}");
    let backup = format!("/Applications/Aeroric.app.previous-{update_id}");

    let copy_result = async {
        let _ = run_command("rm", &["-rf", &staging]).await;
        run_command("ditto", &[source, &staging]).await?;
        if Path::new(destination).exists() {
            run_command("mv", &[destination, &backup]).await?;
        }
        let install_result = run_command("mv", &[&staging, destination]).await;
        if install_result.is_err()
            && Path::new(&backup).exists()
            && !Path::new(destination).exists()
        {
            let _ = run_command("mv", &[&backup, destination]).await;
        }
        install_result?;
        let _ = run_command("rm", &["-rf", &backup]).await;
        Ok::<(), String>(())
    }
    .await;
    let _ = run_command("hdiutil", &["detach", &mount_point, "-quiet"]).await;
    copy_result?;
    Ok(destination.to_string())
}

#[cfg(any(target_os = "linux", test))]
fn linux_package_tool(asset_name: &str) -> Result<&'static str, String> {
    let lower = asset_name.to_ascii_lowercase();
    if lower.ends_with(".deb") {
        Ok("dpkg")
    } else if lower.ends_with(".rpm") {
        Ok("rpm")
    } else {
        Err("Unsupported Linux installer package.".to_string())
    }
}

#[cfg(target_os = "linux")]
fn ensure_linux_update_prerequisites(asset_name: &str) -> Result<(PathBuf, PathBuf), String> {
    let pkexec = crate::platform::detect_path("pkexec");
    if pkexec.is_empty() {
        return Err(
            "Automatic update requires pkexec. Install polkit before restarting to update."
                .to_string(),
        );
    }
    let tool_name = linux_package_tool(asset_name)?;
    let tool = crate::platform::detect_path(tool_name);
    if tool.is_empty() {
        return Err(format!(
            "Automatic update requires {tool_name}, but it was not found."
        ));
    }
    Ok((PathBuf::from(pkexec), PathBuf::from(tool)))
}

#[cfg(target_os = "linux")]
fn run_linux_package_installer(installer: &Path, asset_name: &str) -> Result<(), String> {
    let (pkexec, tool) = ensure_linux_update_prerequisites(asset_name)?;
    let mut command = Command::new(pkexec);
    command.arg(tool);
    if asset_name.to_ascii_lowercase().ends_with(".deb") {
        command.arg("-i");
    } else {
        command.args(["-U", "--replacepkgs"]);
    }
    let status = command
        .arg(installer)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("Failed to start privileged package installer: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Package installer exited with status {}.",
            status.code().unwrap_or(-1)
        ))
    }
}

#[cfg(target_os = "windows")]
fn run_windows_package_installer(installer: &Path) -> Result<(), String> {
    let status = Command::new(installer)
        .arg("/S")
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("Failed to start Windows installer: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Windows installer exited with status {}.",
            status.code().unwrap_or(-1)
        ))
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn parent_process_is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Signal 0 performs an existence/permission check without sending a signal.
        // SAFETY: `kill(pid, 0)` does not deliver a signal or mutate process
        // state; the cast is from the validated platform PID type to libc's
        // expected pid_t representation.
        return unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
    }
    #[cfg(target_os = "windows")]
    {
        let filter = format!("PID eq {pid}");
        return Command::new("tasklist")
            .args(["/FI", &filter, "/NH"])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .any(|token| token == pid.to_string())
            })
            .unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn wait_for_parent_exit(pid: u32) -> Result<(), String> {
    for _ in 0..1200 {
        if !parent_process_is_running(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err("Timed out waiting for Aeroric to exit before installing the update.".to_string())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn run_update_helper(parent_pid: u32, original_exe: &Path) -> Result<(), String> {
    let pending = load_pending_update()?;
    let installer = validate_pending_installer_path(
        &updates_dir()?,
        &pending.tag_name,
        &pending.asset_name,
        Path::new(&pending.installer_path),
    )?;
    verify_pending_update_files(&pending, &installer)?;
    wait_for_parent_exit(parent_pid)?;

    #[cfg(target_os = "windows")]
    run_windows_package_installer(&installer)?;
    #[cfg(target_os = "linux")]
    run_linux_package_installer(&installer, &pending.asset_name)?;
    Command::new(original_exe)
        .spawn()
        .map_err(|error| format!("Update installed, but Aeroric could not restart: {error}"))?;
    clear_pending_update()?;
    if let Some(update_dir) = installer.parent() {
        let _ = fs::remove_dir_all(update_dir);
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn run_update_helper(_parent_pid: u32, _original_exe: &Path) -> Result<(), String> {
    Err("The update helper is only supported on Windows and Linux.".to_string())
}

fn schedule_update_helper_cleanup() {
    let Ok(helpers_dir) = update_helpers_dir() else {
        return;
    };
    std::thread::spawn(move || {
        for _ in 0..20 {
            if !helpers_dir.exists() || fs::remove_dir_all(&helpers_dir).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// Runs before Tauri initializes. A copied Aeroric executable acts as the
/// detached updater so Windows can replace the installed executable after the
/// main process has fully released it.
pub fn try_run_update_helper() -> bool {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) != Some(UPDATE_HELPER_FLAG) {
        schedule_update_helper_cleanup();
        return false;
    }

    let parent_pid = args
        .get(2)
        .and_then(|arg| arg.to_str())
        .and_then(|arg| arg.parse::<u32>().ok());
    let original_exe = args.get(3).map(PathBuf::from);
    let result = match (parent_pid, original_exe) {
        (Some(parent_pid), Some(original_exe)) => run_update_helper(parent_pid, &original_exe),
        _ => Err("Invalid update helper arguments.".to_string()),
    };
    if let Err(error) = result {
        let _ = service().set_helper_status(UPDATE_HELPER_FAILED, Some(error));
        if let Some(original_exe) = args.get(3) {
            let _ = Command::new(original_exe).spawn();
        }
    }
    true
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn launch_update_helper() -> Result<(String, bool), String> {
    #[cfg(target_os = "linux")]
    {
        let pending = load_pending_update()?;
        ensure_linux_update_prerequisites(&pending.asset_name)?;
    }

    let original_exe = std::env::current_exe()
        .map_err(|error| format!("Find Aeroric executable failed: {error}"))?;
    let helper_dir = update_helpers_dir()?.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&helper_dir)
        .map_err(|error| format!("Create update helper directory failed: {error}"))?;
    let helper_path = helper_dir.join(update_helper_file_name());
    fs::copy(&original_exe, &helper_path)
        .map_err(|error| format!("Copy update helper failed: {error}"))?;

    let mut command = Command::new(&helper_path);
    command
        .arg(UPDATE_HELPER_FLAG)
        .arg(std::process::id().to_string())
        .arg(&original_exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
    update_pending_helper_status(UPDATE_HELPER_RUNNING, None)?;
    if let Err(error) = command.spawn() {
        let message = format!("Start update helper failed: {error}");
        let _ = update_pending_helper_status(UPDATE_HELPER_FAILED, Some(message.clone()));
        return Err(message);
    }
    Ok((original_exe.to_string_lossy().into_owned(), true))
}

// ── Tauri 命令（薄壳：全部委托 core 编排）───────────────────────────────────

#[tauri::command]
pub async fn get_notifications(
    app: AppHandle,
    force: Option<bool>,
) -> Result<NotificationResult, String> {
    let force = force.unwrap_or(false);
    let value = crate::host_dispatch::dispatch(
        &app,
        "maintenance.get_notifications",
        serde_json::json!({ "force": force }),
        async {
            let result = service().get_notifications(force).await?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn mark_notification_read(app: AppHandle, id: String) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "maintenance.mark_notification_read",
        serde_json::json!({ "id": id }),
        async {
            service()
                .mark_notification_read(&id)
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn mark_all_notifications_read(app: AppHandle) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "maintenance.mark_all_notifications_read",
        serde_json::Value::Null,
        async {
            service()
                .mark_all_notifications_read()
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn get_pending_release_update(
    app: AppHandle,
    tag_name: Option<String>,
) -> Result<Option<ReleaseUpdatePrepareResult>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "maintenance.get_pending_release_update",
        serde_json::json!({ "tagName": tag_name }),
        async {
            let pending = service().pending_update();
            let filtered = match (pending, tag_name.as_deref()) {
                (Some(pending), Some(tag)) if pending.tag_name != tag => None,
                (pending, _) => pending,
            };
            let mapped = filtered.map(|pending| ReleaseUpdatePrepareResult {
                tag_name: pending.tag_name,
                asset_name: pending.asset_name,
                installer_path: pending.installer_path,
                ready_to_restart: true,
                checksum_verified: pending.checksum_verified,
                helper_status: pending.helper_status,
                error: pending.error,
            });
            serde_json::to_value(mapped).map_err(|error| error.to_string())
        },
    )
    .await?;
    // 宿主与本地都回同一个形状：`null` 或一条待装记录。
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn prepare_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseUpdatePrepareResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "maintenance.prepare_release_update",
        serde_json::json!({ "tagName": tag_name }),
        async {
            let result = service().prepare_release_update(&tag_name).await?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// 替换正在运行的 app。**只有持有 app 身份的 GUI 适配器能做**——这正是 Action 5
/// 把"准备"与"替换"分开的原因：下载与校验可以在无头宿主里做，替换不行。
#[tauri::command]
pub async fn restart_and_install_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseInstallResult, String> {
    let service = service();
    let pending = service
        .pending_update()
        .ok_or_else(|| "No prepared update is pending.".to_string())?;
    if pending.tag_name != tag_name {
        return Err("Prepared update does not match the selected release.".to_string());
    }
    let updates_root = updates_dir()?;
    let installer_path = aeroric_core::maintenance::validate_pending_installer_path(
        &updates_root,
        &pending.tag_name,
        &pending.asset_name,
        Path::new(&pending.installer_path),
    )?;
    if !installer_path.exists() {
        return Err("Prepared installer was not found. Download the update again.".to_string());
    }
    aeroric_core::maintenance::verify_pending_update_files(&pending, &installer_path)?;

    #[cfg(target_os = "macos")]
    let (installed_app_path, restarted) = {
        let installed_app_path = install_macos_dmg(&installer_path).await?;
        let update_dir = installer_path.parent().map(Path::to_path_buf);
        let _ = service.clear_pending_update();
        if let Some(dir) = update_dir {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        authorize_app_restart(&app);
        app.request_restart();
        (installed_app_path, true)
    };
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let (installed_app_path, restarted) = {
        let result = launch_update_helper().inspect_err(|error| {
            let _ = service.set_helper_status(UPDATE_HELPER_FAILED, Some(error.clone()));
        })?;
        crate::authorize_app_exit(&app);
        app.exit(0);
        result
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let (installed_app_path, restarted) = {
        return Err("In-app release installation is not supported on this platform.".to_string());
    };

    Ok(ReleaseInstallResult {
        tag_name: pending.tag_name,
        asset_name: pending.asset_name,
        installed_app_path,
        restarted,
    })
}

/// 便捷口：准备＋替换。前端按两步走（`prepare` → 用户确认 → `restart_and_install`），
/// 这个命令保留给"一步到位"的调用方（CLI/脚本）；它一直是零调用方，P8-04 之后
/// 仍由上面的两步路径服务 UI。
#[tauri::command]
pub async fn install_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseInstallResult, String> {
    let prepared = prepare_release_update(app.clone(), tag_name).await?;
    restart_and_install_release_update(app, prepared.tag_name).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这些用例测的是**宿主留下的那一半**：平台包工具、下载上限、helper 开关。
    /// 判定/记账/资产选择的用例随业务体上收 core
    /// （`core/crates/aeroric-core/tests/maintenance.rs` 与 `src/maintenance/*` 单测），
    /// 在这里重写一遍只会得到两份会漂移的断言。

    #[test]
    fn chooses_native_linux_package_tools() {
        assert_eq!(
            linux_package_tool("Aeroric_1.2.3_amd64.deb").unwrap(),
            "dpkg"
        );
        assert_eq!(
            linux_package_tool("Aeroric-1.2.3-1.x86_64.rpm").unwrap(),
            "rpm"
        );
        assert!(linux_package_tool("Aeroric.AppImage").is_err());
    }

    #[test]
    fn the_download_ceiling_is_a_hard_stop() {
        assert_eq!(checked_download_total(0, 10, 100).unwrap(), 10);
        assert!(checked_download_total(95, 10, 100).is_err());
        assert!(checked_download_total(0, 0, 0).is_ok());
    }

    /// `--aeroric-update-helper` 是**独立进程的入口**：它必须在 Tauri 起来之前被识别，
    /// 所以这个字面量不能随手改（改了等于升级包永远装不上）。
    #[test]
    fn the_update_helper_flag_is_stable() {
        assert_eq!(UPDATE_HELPER_FLAG, "--aeroric-update-helper");
    }

    /// 平台探测只回答"这台机器上有没有 dmg/deb/rpm"；域外平台返回 `None`
    /// （而不是猜一个）。选择逻辑本身在 core，这里钉的是**输入**。
    #[test]
    fn the_platform_probe_reports_the_running_os() {
        let wiring = GuiMaintenanceHost.platform();
        assert_eq!(wiring.os, std::env::consts::OS);
        assert_eq!(wiring.arch, std::env::consts::ARCH);
    }
}
