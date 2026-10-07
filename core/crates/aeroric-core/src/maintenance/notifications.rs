//! 通知簿记（自 `src-tauri/src/notification.rs` 上收，HTTP/进程替换留在宿主）。
//!
//! 这里只有"拿到远端发布列表之后怎么记账"这一半：抓取节流、读过的 id 怎么对账、
//! 一条 release 何时该显示、过期与版本区间怎么判。真正的 `reqwest` 抓取、下载与
//! 安装属于宿主（它才有网络出口与进程替换权，见 [`super::updates`]）。
//!
//! 三处判定写成收参数而不是读环境：
//!
//! - **节流**收 `now`（旧的 `Utc::now()` 让"1 小时"无法断言）。
//! - **过期**收 UTC 当日字符串（旧注释按 UTC 比 date，而文件里其它日期全是本地——
//!   这里把差异显式化成参数，宿主决定用哪套；P8-04 收口时按 UTC 保持字面兼容）。
//! - **装得上吗**收 `os`/`arch`/`linux_kind`（旧代码直接读 `std::env::consts`，
//!   于是"这台机器上有没有 dmg"没法在测试里问）。

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::updates::{
    compare_versions, select_release_installer_asset, validate_release_asset_filename,
};

/// 发布列表来源。旧值一字不改：读过的 id 与缓存以它为版本键，换 URL 等于换账本。
pub const RELEASES_URL: &str = "https://api.github.com/repos/Aho1ic/Aeroric/releases";

/// 服务端抓取节流（旧 `FETCH_INTERVAL_SECS`）。
pub const FETCH_INTERVAL_SECS: i64 = 3600;

/// 远端发布项（旧 `RemoteNotification`，`notifications.json` 的缓存形状）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNotification {
    pub id: String,
    pub level: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub body_zh: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub min_app_version: Option<String>,
    #[serde(default)]
    pub max_app_version: Option<String>,
    #[serde(default)]
    pub release_tag: Option<String>,
    #[serde(default)]
    pub newer_than_current: bool,
    #[serde(default)]
    pub update_install_supported: bool,
}

/// `notifications.json` 的持久形状（旧 `NotificationStore`）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStore {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub read_ids: Vec<String>,
    #[serde(default)]
    pub last_fetched_at: Option<String>,
    #[serde(default)]
    pub cached_notifications: Option<Vec<RemoteNotification>>,
}

impl NotificationStore {
    /// 读盘语义：来源对不上（换了 URL）或解析失败都从空账开始——旧实现同款，
    /// 因为它宁可重抓一次也不把上一个来源的已读状态带到新来源。
    pub fn from_json(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    /// `notifications.json` 的落点（`data_dir` 由调用方注入：G21）。
    pub fn path(data_dir: &std::path::Path) -> std::path::PathBuf {
        data_dir.join("notifications.json")
    }

    /// 读。`guard` 是调用方持有的记账互斥（load-modify-save 之间不能被插队），
    /// 这里只是为了让"读也发生在锁内"这件事在签名上看得见。
    pub fn load(data_dir: &std::path::Path, _guard: &std::sync::MutexGuard<'_, ()>) -> Self {
        std::fs::read_to_string(Self::path(data_dir))
            .map(|raw| Self::from_json(&raw))
            .unwrap_or_default()
    }

    /// 原子写（0600：里面是账号相关的发布缓存与已读状态）。
    pub fn save(
        &self,
        data_dir: &std::path::Path,
        _guard: &mut std::sync::MutexGuard<'_, ()>,
    ) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        crate::storage::atomic::write_atomic(
            &Self::path(data_dir),
            raw.as_bytes(),
            crate::storage::atomic::FileMode::Private,
        )
        .map_err(|error| error.to_string())
    }
}

/// 该不该去网络抓一次。`force` 是 UI 的显式刷新；节流只对自动轮询生效。
pub fn should_fetch(store: &NotificationStore, force: bool, now: DateTime<Utc>) -> bool {
    if force {
        return true;
    }
    let Some(last) = store.last_fetched_at.as_deref() else {
        return true;
    };
    let Ok(last) = DateTime::parse_from_rfc3339(last) else {
        // 时间戳读不出来时重抓：坏账本不能变成永久静音。
        return true;
    };
    store.cached_notifications.is_none()
        || now.signed_duration_since(last.with_timezone(&Utc))
            >= chrono::Duration::seconds(FETCH_INTERVAL_SECS)
}

/// 抓回来之后对账：已读 id 只保留远端还存在的（旧 `apply_fetched_notifications`），
/// 否则账本会随发布列表无限增长。
pub fn apply_fetched_notifications(
    store: &mut NotificationStore,
    remote: Vec<RemoteNotification>,
    now: DateTime<Utc>,
) {
    let remote_ids: HashSet<&str> = remote.iter().map(|item| item.id.as_str()).collect();
    store.read_ids.retain(|id| remote_ids.contains(id.as_str()));
    store.source = Some(RELEASES_URL.to_string());
    store.last_fetched_at = Some(now.to_rfc3339());
    store.cached_notifications = Some(remote);
}

/// 去掉控制字符（换行除外）并截断——远端文案是不可信输入。
pub fn sanitize_text(value: &str, max_len: usize) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .take(max_len)
        .collect()
}

/// 只放行 http(s)：`javascript:`/`data:` 这类不该进 UI 的链接直接丢。
pub fn sanitize_url(url: &Option<String>) -> Option<String> {
    url.as_ref().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
            Some(sanitize_text(trimmed, 2000))
        } else {
            None
        }
    })
}

/// 一条通知在当前版本/当日该不该显示。
///
/// `today_utc` 是 `YYYY-MM-DD`（旧实现按 UTC 取当日，这里保持字面兼容并把来源
/// 显式化——它和文件里其它本地日期不同源，是已知的不一致，改口径要用户点头）。
pub fn is_valid(notif: &RemoteNotification, app_version: &str, today_utc: &str) -> bool {
    if let Some(expires) = &notif.expires_at {
        if expires.as_str() < today_utc {
            return false;
        }
    }
    if let Some(min) = &notif.min_app_version {
        if compare_versions(app_version, min) == std::cmp::Ordering::Less {
            return false;
        }
    }
    if let Some(max) = &notif.max_app_version {
        if compare_versions(app_version, max) == std::cmp::Ordering::Greater {
            return false;
        }
    }
    true
}

/// GitHub release 的最小形状（只留判定要用的字段；`body` 缺省时给 "No release notes."）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitHubRelease {
    pub id: serde_json::Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub tag_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubReleaseAsset {
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
}

/// 一条 release → 一条通知。判定函数：`newer_than_current` 决定要不要提示升级，
/// `update_install_supported` 还要这台机器真的找得到可装资产。
///
/// `os`/`arch`/`linux_kind` 由宿主给（`std::env::consts::OS`、`ARCH` 与
/// `/etc/debian_version` 之类的探测结果是宿主的活）。
pub fn release_to_notification(
    release: &GitHubRelease,
    app_version: &str,
    os: &str,
    arch: &str,
    linux_kind: Option<&str>,
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
    let version = super::updates::release_version(&release.tag_name);
    let newer_than_current = compare_versions(&version, app_version) == std::cmp::Ordering::Greater;
    let update_install_supported = newer_than_current
        && select_release_installer_asset(&release.assets, os, arch, linux_kind).is_some();

    RemoteNotification {
        id: format!("release-{}", release_id_text(&release.id)),
        level: "info".to_string(),
        title: format!("{title}{suffix}"),
        body,
        body_zh: None,
        url: Some(release.html_url.clone()),
        created_at: release
            .published_at
            .clone()
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
        expires_at: None,
        min_app_version: None,
        max_app_version: None,
        release_tag: Some(release.tag_name.clone()),
        newer_than_current,
        update_install_supported,
    }
}

/// GitHub 的 release id 是数字，但形状保留成 `Value` 以免上游改成字符串时整个
/// 解析失败；数字按数字渲染，字符串脱引号——`release-123` 与 `release-abc` 都成立。
fn release_id_text(id: &serde_json::Value) -> String {
    match id {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// 上线给 UI 的通知项（旧 `NotificationItem`：字段名逐个显式，别改成 snake_case）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationResult {
    pub notifications: Vec<NotificationItem>,
    #[serde(rename = "unreadCount")]
    pub unread_count: usize,
}

/// 账本 → UI 结果：先按当日与版本筛，再按发布时间倒序（旧实现对 JSON 顺序的依赖
/// 在这里显式化，远端返回顺序不再影响界面），最后逐条打上已读标记。
pub fn build_result(
    store: &NotificationStore,
    app_version: &str,
    today_utc: &str,
) -> NotificationResult {
    let mut notifications: Vec<&RemoteNotification> = store
        .cached_notifications
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|item| is_valid(item, app_version, today_utc))
        .collect();
    notifications.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let items: Vec<NotificationItem> = notifications
        .into_iter()
        .map(|item| NotificationItem {
            id: item.id.clone(),
            level: item.level.clone(),
            title: item.title.clone(),
            body: item.body.clone(),
            body_zh: item.body_zh.clone(),
            url: sanitize_url(&item.url),
            created_at: item.created_at.clone(),
            is_read: store.read_ids.iter().any(|id| id == &item.id),
            release_tag: item.release_tag.clone(),
            newer_than_current: item.newer_than_current,
            update_install_supported: item.update_install_supported,
        })
        .collect();
    let unread_count = items.iter().filter(|item| !item.is_read).count();
    NotificationResult {
        notifications: items,
        unread_count,
    }
}

/// 资产文件名守卫（旧 `validate_release_asset_filename`）：发布资产名来自网络，
/// 拼进本地路径之前必须确认它只是一个文件名。
pub fn valid_asset_filename(name: &str) -> bool {
    validate_release_asset_filename(name).is_ok()
}
