//! 维护面的编排（旧 `notification.rs` / `report.rs` 的命令体上收）。
//!
//! 分成两半：**决策在这里**（要不要抓、抓回来怎么对账、准备阶段的顺序、周报的循环），
//! **做不了的事走 [`MaintenanceHost`] 端口**——网络抓取、下载、`hdiutil`/`dpkg` 这类
//! 替换正在运行的 app 的动作、以及"现在几点/什么平台"。
//!
//! 这一层的存在理由是 §3.1 #8：同一个能力必须同时可达 CLI 与 UI，所以命令体不能
//! 长在宿主命令宏里。GUI 命令与宿主 UDS 服务调用的是同一份函数。
//!
//! 进程内的记账互斥（旧 `notification_store_mutex`）跟着搬进来：两个写者同时
//! load-modify-save 会把已读状态丢掉，而门禁是并发的。

use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::tasks::cleanup::LocalCalendar;

use super::notifications::{
    apply_fetched_notifications, build_result, release_to_notification, should_fetch,
    GitHubRelease, NotificationResult, NotificationStore, RemoteNotification,
};
use super::reports::{
    generate_weekly_report, write_weekly_report_markdown, ReportProgressSink, ReportSummarizer,
    WeeklyReportRequest,
};
use super::updates::{
    compare_versions, find_checksum_for_asset, release_update_dir, release_version,
    select_release_installer_asset, PendingReleaseUpdate, ReleaseUpdatePrepareResult,
    TargetPlatform,
};
use super::MaintenanceError;

/// 维护面自己做不到的事。实现留在宿主：它有网络出口、有平台探测、有"正在运行的那个
/// app"——core 三样都没有，也不该有。
#[async_trait::async_trait]
pub trait MaintenanceHost: Send + Sync {
    /// 当前 app 版本（旧 `env!("CARGO_PKG_VERSION")` 的读取点统一在这里）。
    fn app_version(&self) -> String;
    /// 目标平台（旧 `std::env::consts::OS/ARCH` 与 `/etc/debian_version` 探测）。
    fn platform(&self) -> TargetPlatform<'static>;
    /// 本地日历（周报区间与清理调度共用同一条缝）。
    fn calendar(&self) -> Arc<dyn LocalCalendar>;
    /// 抓发布列表（旧 `fetch_remote`）。
    async fn fetch_releases(&self) -> Result<Vec<GitHubRelease>, String>;
    /// 按 tag 抓一条（旧 `fetch_release_by_tag`）。
    async fn fetch_release(&self, tag: &str) -> Result<GitHubRelease, String>;
    /// 下载安装包＋校验文件到 `updates/<tag>/` 并验 sha256。返回 `(installer, checksum)`。
    async fn download_release_assets(
        &self,
        release: &GitHubRelease,
        asset_name: &str,
        tag_name: &str,
    ) -> Result<(String, String), String>;
    /// 周报摘要（旧 `agent_assist::summarize_task_session`；core 不 spawn agent）。
    async fn summarize(
        &self,
        task: &super::reports::WeeklyReportTask,
        locale: &str,
    ) -> Option<String>;
}

/// 通知与更新的编排状态。`data_dir` 由调用方注入（G21）。
pub struct MaintenanceService {
    host: Arc<dyn MaintenanceHost>,
    data_dir: std::path::PathBuf,
    /// 记账互斥：load-modify-save 之间不能被另一个写者插进来。
    store_guard: Mutex<()>,
}

impl MaintenanceService {
    pub fn new(host: Arc<dyn MaintenanceHost>, data_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            host,
            data_dir: data_dir.into(),
            store_guard: Mutex::new(()),
        }
    }

    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// 记账临界区：锁 → 读 → 改 → 写，**全程同步**。
    ///
    /// 这个形状是必须的（core 规约：`parking_lot` guard 不得跨 `.await`）：
    /// 抓网那一段在外面，锁只在这几行里持有。`NotificationStore::load/save` 收
    /// guard 参数就是为了让"读也发生在锁内"在签名上看得见。
    fn with_store<T>(
        &self,
        mutate: impl FnOnce(&mut NotificationStore) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.store_guard.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = NotificationStore::load(&self.data_dir, &guard);
        let outcome = mutate(&mut store)?;
        store.save(&self.data_dir, &mut guard)?;
        Ok(outcome)
    }

    /// 发布列表 → 通知（旧 `get_notifications` 命令体）。
    ///
    /// 抓取失败时退到缓存；连缓存都没有才报错——离线不该让通知中心变空白。
    pub async fn get_notifications(&self, force: bool) -> Result<NotificationResult, String> {
        let app_version = self.host.app_version();
        let platform = self.host.platform();
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let store = self.read_store();

        let store = if should_fetch(&store, force, Utc::now()) {
            match self.host.fetch_releases().await {
                Ok(releases) => {
                    let remote: Vec<RemoteNotification> = releases
                        .iter()
                        .filter(|release| !release.draft)
                        .map(|release| {
                            release_to_notification(
                                release,
                                &app_version,
                                platform.os,
                                platform.arch,
                                platform.linux_kind,
                            )
                        })
                        .collect();
                    let now = Utc::now();
                    self.with_store(move |store| {
                        apply_fetched_notifications(store, remote, now);
                        Ok(store.clone())
                    })?
                }
                Err(error) => {
                    if store.cached_notifications.is_none() {
                        return Err(error);
                    }
                    store
                }
            }
        } else {
            store
        };

        Ok(build_result(&store, &app_version, &today))
    }

    /// 只读一份当前账本（锁在函数内取完就放，不跨 `.await`）。
    fn read_store(&self) -> NotificationStore {
        let guard = self.store_guard.lock().unwrap_or_else(|e| e.into_inner());
        NotificationStore::load(&self.data_dir, &guard)
    }

    /// 标记一条已读（旧 `mark_notification_read`）。
    pub fn mark_notification_read(&self, id: &str) -> Result<(), String> {
        let id = super::notifications::sanitize_text(id, 100);
        self.with_store(move |store| {
            if !store.read_ids.contains(&id) {
                store.read_ids.push(id);
            }
            Ok(())
        })
    }

    /// 全部标记已读——只标**当前缓存里存在的**那些（旧行为：已读集合随后由
    /// `apply_fetched_notifications` 按远端存活对账）。
    pub fn mark_all_notifications_read(&self) -> Result<(), String> {
        self.with_store(|store| {
            if let Some(cached) = store.cached_notifications.clone() {
                for item in cached {
                    if !store.read_ids.contains(&item.id) {
                        store.read_ids.push(item.id);
                    }
                }
            }
            Ok(())
        })
    }

    /// 待装更新（旧 `get_pending_release_update`）：只读盘上那份，不抓网。
    pub fn pending_update(&self) -> Option<PendingReleaseUpdate> {
        PendingReleaseUpdate::load(&self.data_dir).ok().flatten()
    }

    /// 准备更新（旧 `prepare_release_update`）：选资产 → 下载 → 校验 → 记 pending。
    ///
    /// 顺序是刻意的：**先确认"这台机器有可装资产"再下载**（旧实现同款），否则用户会
    /// 白等一次下载才知道装不了。`ready_to_restart` 只在校验真的过了之后才为真。
    pub async fn prepare_release_update(
        &self,
        tag_name: &str,
    ) -> Result<ReleaseUpdatePrepareResult, String> {
        let app_version = self.host.app_version();
        let platform = self.host.platform();
        let release = self.host.fetch_release(tag_name).await?;
        if compare_versions(&release_version(&release.tag_name), &app_version)
            != std::cmp::Ordering::Greater
        {
            return Err("Selected release is not newer than the installed version.".to_string());
        }
        let asset = select_release_installer_asset(
            &release.assets,
            platform.os,
            platform.arch,
            platform.linux_kind,
        )
        .ok_or_else(|| "No installer asset is available for this platform.".to_string())?;
        let checksum_name = find_checksum_for_asset(asset, &release.assets)?;
        let _ = checksum_name;
        let (installer_path, checksum_path) = self
            .host
            .download_release_assets(&release, &asset.name, &release.tag_name)
            .await?;

        let pending = PendingReleaseUpdate {
            tag_name: release.tag_name.clone(),
            asset_name: asset.name.clone(),
            installer_path: installer_path.clone(),
            checksum_path,
            checksum_verified: true,
            helper_status: super::updates::UPDATE_HELPER_READY.to_string(),
            error: None,
        };
        // 自校验：记录说"验过"不算数，装之前还要再对一次实读哈希。
        super::updates::verify_pending_update_files(
            &pending,
            std::path::Path::new(&installer_path),
        )?;
        pending.save(&self.data_dir)?;
        Ok(ReleaseUpdatePrepareResult {
            tag_name: pending.tag_name.clone(),
            asset_name: pending.asset_name.clone(),
            installer_path,
            ready_to_restart: true,
            checksum_verified: true,
            helper_status: pending.helper_status,
            error: None,
        })
    }

    /// 更新 helper 的进度回写（Windows/Linux 的 detached helper 会调它）。
    pub fn set_helper_status(&self, status: &str, error: Option<String>) -> Result<(), String> {
        let Some(mut pending) =
            PendingReleaseUpdate::load(&self.data_dir).map_err(|e| e.to_string())?
        else {
            return Err("No prepared update is pending.".to_string());
        };
        pending.helper_status = status.to_string();
        pending.error = error;
        pending.save(&self.data_dir)
    }

    /// 清掉待装记录与下载目录（安装成功后调用）。
    pub fn clear_pending_update(&self) -> Result<(), String> {
        PendingReleaseUpdate::clear(&self.data_dir).map_err(|error| error.to_string())
    }

    /// 待装安装包所在目录（宿主的平台安装器要在里面找 dmg/deb/exe）。
    pub fn update_dir(&self, tag_name: &str) -> std::path::PathBuf {
        release_update_dir(&self.data_dir.join("updates"), tag_name)
    }

    /// 周报（旧 `generate_weekly_report` 命令体＋`src/weeklyReport.ts` 的区间）。
    pub async fn generate_weekly_report(
        &self,
        mut request: WeeklyReportRequest,
        progress: &dyn ReportProgressSink,
    ) -> Result<String, MaintenanceError> {
        // 区间与标题由调用方给的 `now_ms` 决定；未给标题时按请求里的区间算。
        if request.week_label.is_empty() {
            request.week_label = super::reports::week_label(
                &*self.host.calendar(),
                &super::reports::WeekWindow {
                    from_ms: request.from_ms,
                    to_ms: request.to_ms,
                },
            );
        }
        let summarizer = HostSummarizer {
            host: Arc::clone(&self.host),
        };
        let markdown =
            generate_weekly_report(&*self.host.calendar(), &request, &summarizer, progress).await?;
        Ok(markdown)
    }

    /// 生成并落盘，返回实际写入的路径。
    pub async fn write_weekly_report(
        &self,
        request: WeeklyReportRequest,
        progress: &dyn ReportProgressSink,
    ) -> Result<String, MaintenanceError> {
        let output = request.output_path.clone();
        let markdown = self.generate_weekly_report(request, progress).await?;
        let target = write_weekly_report_markdown(&output, &markdown)?;
        Ok(target.to_string_lossy().into_owned())
    }
}

/// 宿主摘要器的薄包装：`WeeklyReportTask` → 宿主端口，逐条降级为 `None`。
struct HostSummarizer {
    host: Arc<dyn MaintenanceHost>,
}

#[async_trait::async_trait]
impl ReportSummarizer for HostSummarizer {
    async fn summarize_task(
        &self,
        task: &super::reports::WeeklyReportTask,
        locale: &str,
    ) -> Option<String> {
        self.host.summarize(task, locale).await
    }
}
