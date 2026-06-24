use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
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

// ── Path helpers ─────────────────────────────────────────────────────────────

fn aeroric_dir() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

fn store_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("notifications.json"))
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
    let update_install_supported = cfg!(target_os = "macos")
        && newer_than_current
        && select_macos_dmg_asset(&release.assets, arch).is_some();

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

#[cfg(not(target_os = "macos"))]
async fn install_macos_dmg(_dmg_path: &Path) -> Result<String, String> {
    Err("In-app release installation is currently supported on macOS only.".to_string())
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
pub async fn install_release_update(
    app: AppHandle,
    tag_name: String,
) -> Result<ReleaseInstallResult, String> {
    let release = fetch_release_by_tag(&tag_name).await?;
    let release_version = release_version(&release.tag_name);
    if compare_versions(&release_version, APP_VERSION) != std::cmp::Ordering::Greater {
        return Err("Selected release is not newer than the installed version.".to_string());
    }
    let asset = select_macos_dmg_asset(&release.assets, current_arch())
        .ok_or_else(|| "No compatible macOS DMG asset found for this release.".to_string())?
        .clone();
    let temp_dir = std::env::temp_dir().join(format!("aeroric-update-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("Create temp directory failed: {e}"))?;
    let dmg_path = temp_dir.join(&asset.name);

    download_asset(&asset, &dmg_path).await?;
    let installed_app_path = install_macos_dmg(&dmg_path).await?;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    app.request_restart();

    Ok(ReleaseInstallResult {
        tag_name: release.tag_name,
        asset_name: asset.name,
        installed_app_path,
        restarted: true,
    })
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
        assert!(notification.update_install_supported);
    }
}
