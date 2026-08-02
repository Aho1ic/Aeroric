use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::storage::atomic_write;

// ── Security: hardcoded allowed notification source ──────────────────────────

const RELEASES_URL: &str = "https://api.github.com/repos/Aho1ic/Aeroric/releases";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024; // 1MB limit
const FETCH_INTERVAL_SECS: i64 = 3600; // 1 hour
const REQUEST_TIMEOUT_SECS: u64 = 15;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

static NOTIFICATION_STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

// ── Remote JSON types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteNotification {
    id: String,
    level: String,
    title: String,
    body: String,
    body_zh: Option<String>,
    url: Option<String>,
    created_at: String,
    expires_at: Option<String>,
    min_app_version: Option<String>,
    max_app_version: Option<String>,
    release_tag: Option<String>,
    newer_than_current: bool,
    update_install_supported: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    id: u64,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

// ── Local storage types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationStore {
    source: Option<String>,
    read_ids: Vec<String>,
    last_fetched_at: Option<String>,
    cached_notifications: Option<Vec<RemoteNotification>>,
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self {
            source: Some(RELEASES_URL.to_string()),
            read_ids: vec![],
            last_fetched_at: None,
            cached_notifications: None,
        }
    }
}

// ── Frontend-facing types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NotificationItem {
    pub id: String,
    pub level: String,
    pub title: String,
    pub body: String,
    #[serde(rename = "bodyZh")]
    pub body_zh: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "isRead")]
    pub is_read: bool,
    #[serde(rename = "releaseTag")]
    pub release_tag: Option<String>,
    #[serde(rename = "newerThanCurrent")]
    pub newer_than_current: bool,
    #[serde(rename = "updateInstallSupported")]
    pub update_install_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationResult {
    pub notifications: Vec<NotificationItem>,
    #[serde(rename = "unreadCount")]
    pub unread_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInstallResult {
    #[serde(rename = "tagName")]
    pub tag_name: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
    #[serde(rename = "installedAppPath")]
    pub installed_app_path: String,
    pub restarted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseUpdatePrepareResult {
    #[serde(rename = "tagName")]
    pub tag_name: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
    #[serde(rename = "installerPath")]
    pub installer_path: String,
    #[serde(rename = "readyToRestart")]
    pub ready_to_restart: bool,
    #[serde(rename = "checksumVerified")]
    pub checksum_verified: bool,
    #[serde(rename = "helperStatus")]
    pub helper_status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingReleaseUpdate {
    #[serde(rename = "tagName")]
    tag_name: String,
    #[serde(rename = "assetName")]
    asset_name: String,
    #[serde(rename = "installerPath")]
    installer_path: String,
    #[serde(rename = "checksumPath")]
    checksum_path: String,
    #[serde(rename = "checksumVerified")]
    checksum_verified: bool,
    #[serde(rename = "helperStatus")]
    helper_status: String,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingReleaseUpdatePayload {
    #[serde(rename = "tagName")]
    tag_name: String,
    #[serde(rename = "assetName")]
    asset_name: String,
    #[serde(rename = "installerPath")]
    installer_path: String,
    #[serde(rename = "checksumPath", default)]
    checksum_path: String,
    #[serde(rename = "checksumVerified", default)]
    checksum_verified: bool,
    #[serde(rename = "helperStatus", default)]
    helper_status: String,
    #[serde(default)]
    error: Option<String>,
}

const UPDATE_HELPER_FLAG: &str = "--aeroric-update-helper";
const UPDATE_HELPER_READY: &str = "ready";
#[cfg(any(target_os = "windows", target_os = "linux"))]
const UPDATE_HELPER_RUNNING: &str = "running";
const UPDATE_HELPER_FAILED: &str = "failed";

// ── Path helpers ─────────────────────────────────────────────────────────────

fn aeroric_dir() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

fn store_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("notifications.json"))
}

fn updates_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("updates"))
}

fn pending_update_path() -> Result<PathBuf, String> {
    Ok(updates_dir()?.join("pending-release-update.json"))
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

// ── Storage I/O ──────────────────────────────────────────────────────────────

fn load_store() -> NotificationStore {
    let Ok(path) = store_path() else {
        return NotificationStore::default();
    };
    match fs::read_to_string(&path) {
        Ok(data) => {
            let store: NotificationStore = serde_json::from_str(&data).unwrap_or_default();
            if store.source.as_deref() == Some(RELEASES_URL) {
                store
            } else {
                NotificationStore::default()
            }
        }
        Err(_) => NotificationStore::default(),
    }
}

fn save_store(store: &NotificationStore) -> Result<(), String> {
    let path = store_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    atomic_write(&path, &json)
}

fn notification_store_mutex() -> &'static Mutex<()> {
    NOTIFICATION_STORE_MUTEX.get_or_init(|| Mutex::new(()))
}

fn update_store<T, F>(mutate: F) -> Result<T, String>
where
    F: FnOnce(&mut NotificationStore) -> Result<T, String>,
{
    let _guard = notification_store_mutex().lock();
    let mut store = load_store();
    let result = mutate(&mut store)?;
    save_store(&store)?;
    Ok(result)
}

// ── Utilities ────────────────────────────────────────────────────────────────

fn should_fetch(store: &NotificationStore, force: bool) -> bool {
    if force {
        return true;
    }
    if store.cached_notifications.is_none() {
        return true;
    }

    match &store.last_fetched_at {
        None => true,
        Some(ts) => match chrono::DateTime::parse_from_rfc3339(ts) {
            Ok(last) => {
                let elapsed = (Utc::now() - last.with_timezone(&Utc)).num_seconds();
                elapsed > FETCH_INTERVAL_SECS
            }
            Err(_) => true,
        },
    }
}

fn apply_fetched_notifications(store: &mut NotificationStore, remote: Vec<RemoteNotification>) {
    let remote_ids: HashSet<&str> = remote.iter().map(|n| n.id.as_str()).collect();
    store.read_ids.retain(|id| remote_ids.contains(id.as_str()));
    store.source = Some(RELEASES_URL.to_string());
    store.last_fetched_at = Some(Utc::now().to_rfc3339());
    store.cached_notifications = Some(remote);
}

/// Strip control characters (except newline) and limit length to prevent
/// oversized or crafted strings from reaching the UI.
fn sanitize_text(s: &str, max_len: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max_len)
        .collect()
}

/// Only allow http(s) URLs — reject `javascript:`, `data:`, etc.
fn sanitize_url(url: &Option<String>) -> Option<String> {
    url.as_ref().and_then(|u| {
        let trimmed = u.trim();
        if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
            Some(sanitize_text(trimmed, 2000))
        } else {
            None
        }
    })
}

fn release_version(tag: &str) -> String {
    tag.trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

/// Simple semver comparison (major.minor.patch).
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let max_len = va.len().max(vb.len());
    for i in 0..max_len {
        let a_part = va.get(i).copied().unwrap_or(0);
        let b_part = vb.get(i).copied().unwrap_or(0);
        match a_part.cmp(&b_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn current_arch() -> &'static str {
    std::env::consts::ARCH
}

fn select_macos_dmg_asset<'a>(
    assets: &'a [GitHubReleaseAsset],
    arch: &str,
) -> Option<&'a GitHubReleaseAsset> {
    let arch_token = match arch {
        "aarch64" | "arm64" => "_aarch64.dmg",
        "x86_64" | "x64" => "_x64.dmg",
        _ => ".dmg",
    };

    assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.starts_with("aeroric_") && name.ends_with(arch_token)
    })
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

fn select_release_installer_asset<'a>(
    assets: &'a [GitHubReleaseAsset],
    os: &str,
    arch: &str,
    linux_kind: Option<&str>,
) -> Option<&'a GitHubReleaseAsset> {
    match os {
        "macos" => select_macos_dmg_asset(assets, arch),
        "windows" => {
            let suffix = match arch {
                "aarch64" | "arm64" => "_arm64-setup.exe",
                "x86_64" | "x64" => "_x64-setup.exe",
                _ => return None,
            };
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.starts_with("aeroric_") && name.ends_with(suffix)
            })
        }
        "linux" if matches!(arch, "x86_64" | "x64") => {
            let suffix = match linux_kind {
                Some("deb") => "_amd64.deb",
                Some("rpm") => "-1.x86_64.rpm",
                _ => return None,
            };
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.starts_with("aeroric") && name.ends_with(suffix)
            })
        }
        _ => None,
    }
}

fn expected_release_digest_asset_name(asset_name: &str) -> Option<String> {
    [".dmg", ".exe", ".deb", ".rpm"]
        .iter()
        .any(|suffix| asset_name.to_ascii_lowercase().ends_with(suffix))
        .then(|| "SHA256SUMS.txt".to_string())
}

fn find_checksum_for_asset(
    asset: &GitHubReleaseAsset,
    assets: &[GitHubReleaseAsset],
) -> Result<String, String> {
    let expected_name = expected_release_digest_asset_name(&asset.name)
        .ok_or_else(|| "Unsupported installer asset name".to_string())?;
    if assets
        .iter()
        .any(|candidate| candidate.name == expected_name)
    {
        return Ok(expected_name);
    }
    Err(format!(
        "Missing checksum asset for {}. Expected a {expected_name} release asset.",
        asset.name
    ))
}

fn validate_pending_installer_path(
    updates_root: &Path,
    tag_name: &str,
    asset_name: &str,
    installer_path: &Path,
) -> Result<PathBuf, String> {
    let canonical_root = updates_root
        .canonicalize()
        .map_err(|e| format!("Resolve updates directory failed: {e}"))?;
    let canonical_installer = installer_path
        .canonicalize()
        .map_err(|e| format!("Resolve installer path failed: {e}"))?;
    let expected_dir = canonical_root.join(
        sanitize_text(tag_name, 80)
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>(),
    );

    if canonical_installer.parent() != Some(expected_dir.as_path()) {
        return Err("Prepared installer is outside the update directory.".to_string());
    }
    if canonical_installer.file_name() != Some(std::ffi::OsStr::new(asset_name)) {
        return Err("Prepared installer does not match the selected release asset.".to_string());
    }
    Ok(canonical_installer)
}

fn verify_pending_update_files(
    pending: &PendingReleaseUpdate,
    installer_path: &Path,
) -> Result<(), String> {
    if !pending.checksum_verified {
        return Err("Prepared update has not passed checksum verification.".to_string());
    }
    let checksum_path = Path::new(&pending.checksum_path)
        .canonicalize()
        .map_err(|e| format!("Resolve checksum path failed: {e}"))?;
    let installer_path = installer_path
        .canonicalize()
        .map_err(|e| format!("Resolve installer path failed: {e}"))?;
    if checksum_path.parent() != installer_path.parent()
        || checksum_path.file_name() != Some(std::ffi::OsStr::new("SHA256SUMS.txt"))
    {
        return Err("Prepared checksum is outside the selected update directory.".to_string());
    }
    let checksum_text =
        fs::read_to_string(&checksum_path).map_err(|e| format!("Read checksum failed: {e}"))?;
    verify_downloaded_checksum(&installer_path, &pending.asset_name, &checksum_text)
}

fn verify_downloaded_checksum(
    asset_path: &Path,
    expected_file_name: &str,
    checksum_text: &str,
) -> Result<(), String> {
    let expected = checksum_text
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.split_whitespace();
            let hash = parts.next()?;
            let file_name = parts.next()?;
            if file_name.ends_with(expected_file_name) {
                Some(hash.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "Checksum entry for the downloaded installer was not found.".to_string())?;
    let raw = fs::read(asset_path).map_err(|e| format!("Read installer failed: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(raw);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err("Downloaded installer checksum does not match.".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_pending_release_update(
    tag_name: Option<String>,
) -> Result<Option<ReleaseUpdatePrepareResult>, String> {
    let pending = tokio::task::spawn_blocking(load_pending_update)
        .await
        .map_err(|e| e.to_string())??;
    if let Some(expected_tag) = tag_name {
        if pending.tag_name != expected_tag {
            return Ok(None);
        }
    }

    let updates_root = updates_dir()?;
    let installer_path = validate_pending_installer_path(
        &updates_root,
        &pending.tag_name,
        &pending.asset_name,
        Path::new(&pending.installer_path),
    )?;
    if !installer_path.exists() {
        return Ok(None);
    }

    if verify_pending_update_files(&pending, &installer_path).is_err() {
        return Ok(None);
    }
    Ok(Some(ReleaseUpdatePrepareResult {
        tag_name: pending.tag_name,
        asset_name: pending.asset_name,
        installer_path: installer_path.to_string_lossy().into_owned(),
        ready_to_restart: true,
        checksum_verified: true,
        helper_status: pending.helper_status,
        error: pending.error,
    }))
}

fn release_update_dir(tag_name: &str) -> Result<PathBuf, String> {
    let safe_tag = sanitize_text(tag_name, 80)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    Ok(updates_dir()?.join(safe_tag))
}

fn save_pending_update(pending: &PendingReleaseUpdate) -> Result<(), String> {
    let path = pending_update_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create updates directory failed: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(pending)
        .map_err(|e| format!("Serialize pending update failed: {e}"))?;
    atomic_write(&path, &raw)
}

fn load_pending_update() -> Result<PendingReleaseUpdate, String> {
    let path = pending_update_path()?;
    let raw = fs::read_to_string(&path).map_err(|e| format!("Read pending update failed: {e}"))?;
    let pending: PendingReleaseUpdatePayload =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid pending update JSON: {e}"))?;
    Ok(PendingReleaseUpdate {
        tag_name: pending.tag_name,
        asset_name: pending.asset_name,
        installer_path: pending.installer_path,
        checksum_path: pending.checksum_path,
        checksum_verified: pending.checksum_verified,
        helper_status: if pending.helper_status.is_empty() {
            UPDATE_HELPER_READY.to_string()
        } else {
            pending.helper_status
        },
        error: pending.error,
    })
}

fn update_pending_helper_status(status: &str, error: Option<String>) -> Result<(), String> {
    let mut pending = load_pending_update()?;
    pending.helper_status = status.to_string();
    pending.error = error;
    save_pending_update(&pending)
}

fn clear_pending_update() -> Result<(), String> {
    let path = pending_update_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Remove pending update failed: {e}"))?;
    }
    Ok(())
}

fn release_to_notification(
    release: GitHubRelease,
    app_version: &str,
    arch: &str,
) -> RemoteNotification {
    let title = release
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(release.tag_name.as_str())
        .to_string();
    let body = release
        .body
        .as_deref()
        .filter(|body| !body.trim().is_empty())
        .unwrap_or("No release notes.")
        .to_string();
    let suffix = if release.prerelease {
        " · prerelease"
    } else {
        ""
    };
    let release_version = release_version(&release.tag_name);
    let newer_than_current =
        compare_versions(&release_version, app_version) == std::cmp::Ordering::Greater;
    let update_install_supported = newer_than_current
        && select_release_installer_asset(
            &release.assets,
            std::env::consts::OS,
            arch,
            linux_package_kind(),
        )
        .is_some();

    RemoteNotification {
        id: format!("release-{}", release.id),
        level: "info".to_string(),
        title: format!("{}{}", title, suffix),
        body,
        body_zh: None,
        url: Some(release.html_url),
        created_at: release
            .published_at
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
        expires_at: None,
        min_app_version: None,
        max_app_version: None,
        release_tag: Some(release.tag_name),
        newer_than_current,
        update_install_supported,
    }
}

/// Check if a notification should be shown for the current app version & date.
fn is_valid(notif: &RemoteNotification, app_version: &str) -> bool {
    // Check expiry
    if let Some(expires) = &notif.expires_at {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        if expires.as_str() < today.as_str() {
            return false;
        }
    }
    // Check min version
    if let Some(min_ver) = &notif.min_app_version {
        if compare_versions(app_version, min_ver) == std::cmp::Ordering::Less {
            return false;
        }
    }
    // Check max version
    if let Some(max_ver) = &notif.max_app_version {
        if compare_versions(app_version, max_ver) == std::cmp::Ordering::Greater {
            return false;
        }
    }
    true
}

// ── HTTP fetch (async, with strict guards) ───────────────────────────────────

async fn fetch_remote() -> Result<Vec<RemoteNotification>, String> {
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

    Ok(releases
        .into_iter()
        .filter(|release| !release.draft)
        .map(|release| release_to_notification(release, APP_VERSION, current_arch()))
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

async fn download_asset(asset: &GitHubReleaseAsset, target: &Path) -> Result<(), String> {
    let url = asset.browser_download_url.trim();
    if !url.starts_with("https://github.com/Aho1ic/Aeroric/releases/download/") {
        return Err("Unexpected asset download URL".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let bytes = client
        .get(url)
        .header("User-Agent", "Aeroric")
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Read download failed: {e}"))?;

    tokio::fs::write(target, bytes)
        .await
        .map_err(|e| format!("Write download failed: {e}"))
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
        let _ = update_pending_helper_status(UPDATE_HELPER_FAILED, Some(error));
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

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_notifications(force: Option<bool>) -> Result<NotificationResult, String> {
    let mut store = tokio::task::spawn_blocking(load_store)
        .await
        .map_err(|e| e.to_string())?;

    let notifications = if should_fetch(&store, force.unwrap_or(false)) {
        match fetch_remote().await {
            Ok(remote) => {
                let cached_remote = remote.clone();
                store = tokio::task::spawn_blocking(move || {
                    update_store(|store| {
                        apply_fetched_notifications(store, cached_remote);
                        Ok(store.clone())
                    })
                })
                .await
                .map_err(|e| e.to_string())??;

                remote
            }
            Err(err) => {
                if let Some(cached) = store.cached_notifications.clone() {
                    cached
                } else {
                    return Err(err);
                }
            }
        }
    } else {
        store.cached_notifications.clone().unwrap_or_default()
    };

    let read_set: HashSet<&str> = store.read_ids.iter().map(|s| s.as_str()).collect();

    let items: Vec<NotificationItem> = notifications
        .iter()
        .filter(|n| is_valid(n, APP_VERSION))
        .map(|n| NotificationItem {
            id: sanitize_text(&n.id, 100),
            level: sanitize_text(&n.level, 20),
            title: sanitize_text(&n.title, 200),
            body: sanitize_text(&n.body, 2000),
            body_zh: n.body_zh.as_ref().map(|b| sanitize_text(b, 2000)),
            url: sanitize_url(&n.url),
            created_at: sanitize_text(&n.created_at, 20),
            is_read: read_set.contains(n.id.as_str()),
            release_tag: n.release_tag.as_ref().map(|tag| sanitize_text(tag, 80)),
            newer_than_current: n.newer_than_current,
            update_install_supported: n.update_install_supported,
        })
        .collect();

    let unread_count = items.iter().filter(|n| !n.is_read).count();

    Ok(NotificationResult {
        notifications: items,
        unread_count,
    })
}

#[tauri::command]
pub async fn mark_notification_read(id: String) -> Result<(), String> {
    let sanitized_id = sanitize_text(&id, 100);
    tokio::task::spawn_blocking(move || {
        update_store(|store| {
            if !store.read_ids.contains(&sanitized_id) {
                store.read_ids.push(sanitized_id);
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mark_all_notifications_read() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        update_store(|store| {
            if let Some(cached) = store.cached_notifications.clone() {
                for n in cached {
                    if !store.read_ids.contains(&n.id) {
                        store.read_ids.push(n.id);
                    }
                }
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn prepare_release_update(
    tag_name: String,
) -> Result<ReleaseUpdatePrepareResult, String> {
    let release = fetch_release_by_tag(&tag_name).await?;
    let release_version = release_version(&release.tag_name);
    if compare_versions(&release_version, APP_VERSION) != std::cmp::Ordering::Greater {
        return Err("Selected release is not newer than the installed version.".to_string());
    }
    let asset = select_release_installer_asset(
        &release.assets,
        std::env::consts::OS,
        current_arch(),
        linux_package_kind(),
    )
    .ok_or_else(|| "No compatible installer asset found for this release.".to_string())?
    .clone();
    let checksum_asset_name = find_checksum_for_asset(&asset, &release.assets)?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|candidate| candidate.name == checksum_asset_name)
        .ok_or_else(|| "Missing checksum asset for prepared update.".to_string())?
        .clone();
    let update_dir = release_update_dir(&release.tag_name)?;
    let _ = tokio::fs::remove_dir_all(&update_dir).await;
    tokio::fs::create_dir_all(&update_dir)
        .await
        .map_err(|e| format!("Create update directory failed: {e}"))?;
    let installer_path_buf = update_dir.join(&asset.name);
    let checksum_path_buf = update_dir.join(&checksum_asset.name);

    download_asset(&asset, &installer_path_buf).await?;
    download_asset(&checksum_asset, &checksum_path_buf).await?;
    let checksum_text = tokio::fs::read_to_string(&checksum_path_buf)
        .await
        .map_err(|e| format!("Read checksum failed: {e}"))?;
    let verify_path = installer_path_buf.clone();
    let verify_name = asset.name.clone();
    tokio::task::spawn_blocking(move || {
        verify_downloaded_checksum(&verify_path, &verify_name, &checksum_text)
    })
    .await
    .map_err(|e| e.to_string())??;
    let installer_path = installer_path_buf
        .to_str()
        .ok_or_else(|| "Invalid installer path".to_string())?
        .to_string();
    let checksum_path = checksum_path_buf
        .to_str()
        .ok_or_else(|| "Invalid checksum path".to_string())?
        .to_string();
    let pending = PendingReleaseUpdate {
        tag_name: release.tag_name.clone(),
        asset_name: asset.name.clone(),
        installer_path: installer_path.clone(),
        checksum_path,
        checksum_verified: true,
        helper_status: UPDATE_HELPER_READY.to_string(),
        error: None,
    };
    tokio::task::spawn_blocking(move || save_pending_update(&pending))
        .await
        .map_err(|e| e.to_string())??;

    Ok(ReleaseUpdatePrepareResult {
        tag_name: release.tag_name,
        asset_name: asset.name,
        installer_path,
        ready_to_restart: true,
        checksum_verified: true,
        helper_status: UPDATE_HELPER_READY.to_string(),
        error: None,
    })
}

#[tauri::command]
pub async fn restart_and_install_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseInstallResult, String> {
    let pending = tokio::task::spawn_blocking(load_pending_update)
        .await
        .map_err(|e| e.to_string())??;
    if pending.tag_name != tag_name {
        return Err("Prepared update does not match the selected release.".to_string());
    }

    let updates_root = updates_dir()?;
    let installer_path = validate_pending_installer_path(
        &updates_root,
        &pending.tag_name,
        &pending.asset_name,
        Path::new(&pending.installer_path),
    )?;
    if !installer_path.exists() {
        return Err("Prepared installer was not found. Download the update again.".to_string());
    }
    verify_pending_update_files(&pending, &installer_path)?;

    #[cfg(target_os = "macos")]
    let (installed_app_path, restarted) = {
        let installed_app_path = install_macos_dmg(&installer_path).await?;
        let update_dir = installer_path.parent().map(Path::to_path_buf);
        let _ = tokio::task::spawn_blocking(clear_pending_update).await;
        if let Some(dir) = update_dir {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        app.request_restart();
        (installed_app_path, true)
    };
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let (installed_app_path, restarted) = {
        let result = launch_update_helper().inspect_err(|error| {
            let _ = update_pending_helper_status(UPDATE_HELPER_FAILED, Some(error.clone()));
        })?;
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

#[tauri::command]
pub async fn install_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseInstallResult, String> {
    let prepared = prepare_release_update(tag_name.clone()).await?;
    restart_and_install_release_update(app, prepared.tag_name).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str) -> RemoteNotification {
        RemoteNotification {
            id: id.to_string(),
            level: "info".to_string(),
            title: format!("title-{id}"),
            body: format!("body-{id}"),
            body_zh: None,
            url: None,
            created_at: "2026-01-01".to_string(),
            expires_at: None,
            min_app_version: None,
            max_app_version: None,
            release_tag: None,
            newer_than_current: false,
            update_install_supported: false,
        }
    }

    #[test]
    fn apply_fetched_notifications_keeps_only_existing_read_ids_in_remote() {
        let mut store = NotificationStore {
            source: Some(RELEASES_URL.to_string()),
            read_ids: vec!["keep".to_string(), "drop".to_string()],
            last_fetched_at: None,
            cached_notifications: None,
        };

        apply_fetched_notifications(&mut store, vec![notification("keep"), notification("new")]);

        assert_eq!(store.read_ids, vec!["keep".to_string()]);
        assert_eq!(store.cached_notifications.unwrap().len(), 2);
        assert!(store.last_fetched_at.is_some());
    }

    #[test]
    fn should_fetch_when_forced_even_with_fresh_cache() {
        let store = NotificationStore {
            source: Some(RELEASES_URL.to_string()),
            read_ids: vec![],
            last_fetched_at: Some(Utc::now().to_rfc3339()),
            cached_notifications: Some(vec![notification("cached")]),
        };

        assert!(should_fetch(&store, true));
        assert!(!should_fetch(&store, false));
    }

    #[test]
    fn selects_macos_dmg_asset_for_current_architecture() {
        let assets = vec![
            GitHubReleaseAsset {
                name: "Source code (zip)".to_string(),
                browser_download_url: "https://example.invalid/source.zip".to_string(),
            },
            GitHubReleaseAsset {
                name: "Aeroric_1.2.3_x64.dmg".to_string(),
                browser_download_url: "https://example.invalid/x64.dmg".to_string(),
            },
            GitHubReleaseAsset {
                name: "Aeroric_1.2.3_aarch64.dmg".to_string(),
                browser_download_url: "https://example.invalid/aarch64.dmg".to_string(),
            },
        ];

        let selected = select_macos_dmg_asset(&assets, "aarch64").unwrap();

        assert_eq!(selected.name, "Aeroric_1.2.3_aarch64.dmg");
    }

    #[test]
    fn selects_windows_and_linux_installers() {
        let assets = vec![
            GitHubReleaseAsset {
                name: "Aeroric_1.2.3_arm64-setup.exe".to_string(),
                browser_download_url: "https://example.invalid/arm64.exe".to_string(),
            },
            GitHubReleaseAsset {
                name: "Aeroric_1.2.3_x64-setup.exe".to_string(),
                browser_download_url: "https://example.invalid/x64.exe".to_string(),
            },
            GitHubReleaseAsset {
                name: "Aeroric_1.2.3_amd64.deb".to_string(),
                browser_download_url: "https://example.invalid/a.deb".to_string(),
            },
            GitHubReleaseAsset {
                name: "Aeroric-1.2.3-1.x86_64.rpm".to_string(),
                browser_download_url: "https://example.invalid/a.rpm".to_string(),
            },
        ];

        assert_eq!(
            select_release_installer_asset(&assets, "windows", "aarch64", None)
                .unwrap()
                .name,
            "Aeroric_1.2.3_arm64-setup.exe"
        );
        assert_eq!(
            select_release_installer_asset(&assets, "linux", "x86_64", Some("deb"))
                .unwrap()
                .name,
            "Aeroric_1.2.3_amd64.deb"
        );
        assert_eq!(
            select_release_installer_asset(&assets, "linux", "x86_64", Some("rpm"))
                .unwrap()
                .name,
            "Aeroric-1.2.3-1.x86_64.rpm"
        );
    }

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
    fn release_notifications_are_installable_only_for_newer_versions_with_dmg_asset() {
        let release = GitHubRelease {
            id: 1,
            tag_name: "v9.9.9".to_string(),
            name: Some("Aeroric v9.9.9".to_string()),
            body: Some("notes".to_string()),
            html_url: "https://github.com/Aho1ic/Aeroric/releases/tag/v9.9.9".to_string(),
            published_at: Some("2026-06-24T00:00:00Z".to_string()),
            draft: false,
            prerelease: false,
            assets: vec![GitHubReleaseAsset {
                name: "Aeroric_9.9.9_aarch64.dmg".to_string(),
                browser_download_url: "https://example.invalid/Aeroric_9.9.9_aarch64.dmg"
                    .to_string(),
            }],
        };

        let notification = release_to_notification(release, "1.1.4", "aarch64");

        assert_eq!(notification.release_tag.as_deref(), Some("v9.9.9"));
        assert_eq!(
            notification.update_install_supported,
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn validates_pending_installer_path_inside_expected_update_directory() {
        let root =
            std::env::temp_dir().join(format!("aeroric-update-validate-{}", uuid::Uuid::new_v4()));
        let update_dir = root.join("v9.9.9");
        fs::create_dir_all(&update_dir).unwrap();
        let dmg = update_dir.join("Aeroric_9.9.9_aarch64.dmg");
        fs::write(&dmg, "fake dmg").unwrap();

        let valid =
            validate_pending_installer_path(&root, "v9.9.9", "Aeroric_9.9.9_aarch64.dmg", &dmg)
                .unwrap();
        assert_eq!(valid, dmg.canonicalize().unwrap());

        let outside = root.join("outside.dmg");
        fs::write(&outside, "fake dmg").unwrap();
        let err =
            validate_pending_installer_path(&root, "v9.9.9", "Aeroric_9.9.9_aarch64.dmg", &outside)
                .unwrap_err();
        assert!(err.contains("outside the update directory"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn requires_checksum_for_installable_release_asset() {
        let dmg = GitHubReleaseAsset {
            name: "Aeroric_9.9.9_aarch64.dmg".to_string(),
            browser_download_url: "https://example.invalid/Aeroric_9.9.9_aarch64.dmg".to_string(),
        };
        let err = find_checksum_for_asset(&dmg, std::slice::from_ref(&dmg)).unwrap_err();

        assert!(err.contains("checksum"));
    }

    #[test]
    fn pending_updates_remain_ready_only_while_installer_checksum_matches() {
        let root =
            std::env::temp_dir().join(format!("aeroric-update-checksum-{}", uuid::Uuid::new_v4()));
        let update_dir = root.join("v9.9.9");
        fs::create_dir_all(&update_dir).unwrap();
        let asset_name = "Aeroric_9.9.9_x64-setup.exe";
        let installer = update_dir.join(asset_name);
        fs::write(&installer, b"verified installer").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"verified installer"));
        let checksum = update_dir.join("SHA256SUMS.txt");
        fs::write(&checksum, format!("{digest}  {asset_name}\n")).unwrap();
        let pending = PendingReleaseUpdate {
            tag_name: "v9.9.9".to_string(),
            asset_name: asset_name.to_string(),
            installer_path: installer.to_string_lossy().into_owned(),
            checksum_path: checksum.to_string_lossy().into_owned(),
            checksum_verified: true,
            helper_status: UPDATE_HELPER_READY.to_string(),
            error: None,
        };

        verify_pending_update_files(&pending, &installer).unwrap();
        fs::write(&installer, b"tampered installer").unwrap();
        assert!(verify_pending_update_files(&pending, &installer).is_err());

        let _ = fs::remove_dir_all(root);
    }
}
