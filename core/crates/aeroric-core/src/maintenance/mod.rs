//! 维护面（Task P8-04）：清理调度、周报、通知与更新簿记、启动降级账。
//!
//! 这一层的形状由三条边界决定：
//!
//! 1. **不读时钟。** 每个判定都收 `now_ms`；定时器是宿主的（旧实现是 React 的
//!    `setInterval`，见 `src/state/app/useAppMaintenance.ts:92`），核心只回答"到点了吗"。
//! 2. **不猜日历。** 周/本地日期一律经 [`crate::tasks::cleanup::LocalCalendar`] 端口
//!    （P2-04 为清理规则建的同一条缝，周报复用），核心不引入第二套时区实现。
//! 3. **不动手替换。** 更新分两段：准备（选资产、校验 sha256、记 pending）在这里；
//!    真正替换正在运行的 app、`tccutil reset`、发系统通知属于拥有当前 app 身份的
//!    GUI platform adapter（计划 Action 3/5），核心只给出可验证的判定。
//!
//! 旧实现的落点：`src-tauri/src/{report,notification,startup_diagnostics}.rs`、
//! `src-tauri/src/{analytics,agent_usage,usage_index}.rs`（用量族在
//! [`crate::metrics`]）、`src/taskCleanup.ts`（规则已在 [`crate::tasks::cleanup`]）。

pub mod cleanup;
pub mod diagnostics;
pub mod notifications;
pub mod reports;
pub mod service;
pub mod updates;

use thiserror::Error;

/// 维护面的失败。业务性"没到点""没有待装更新"不是错误，是返回值；
/// 这里的错误只表示调用方给的东西无法完成（路径不可写、json 损坏等）。
#[derive(Debug, Error)]
pub enum MaintenanceError {
    #[error("{0}")]
    Invalid(String),
    #[error("weekly report output path is invalid: {0}")]
    OutputPath(String),
    #[error("report write failed: {0}")]
    WriteFailed(String),
    #[error("no tasks in range")]
    EmptyReport,
}

pub use cleanup::{run_cleanup_once, CleanupRun, CLEANUP_TICK_MS};
pub use diagnostics::{StartupDegradation, StartupDegradationLog};
pub use notifications::{
    apply_fetched_notifications, build_result, is_valid, release_to_notification, sanitize_text,
    sanitize_url, should_fetch, GitHubRelease, GitHubReleaseAsset, NotificationItem,
    NotificationResult, NotificationStore, RemoteNotification, FETCH_INTERVAL_SECS, RELEASES_URL,
};
pub use reports::{
    generate_weekly_report, iso_week, local_ymd, render_report, tasks_in_window, week_label,
    week_window, weekly_report_file_name, write_weekly_report_markdown, NullProgress, ReportEntry,
    ReportProgressSink, ReportSummarizer, WeekWindow, WeeklyReportProgress, WeeklyReportRequest,
    WeeklyReportTask, MAX_SUMMARIZED_TASKS,
};
pub use service::{MaintenanceHost, MaintenanceService};
pub use updates::{
    compare_versions, expected_release_digest_asset_name, find_checksum_for_asset, release_version,
    safe_tag, select_macos_dmg_asset, select_release_installer_asset,
    validate_pending_installer_path, validate_release_asset_filename, verify_downloaded_checksum,
    verify_pending_update_files, PendingReleaseUpdate, PendingReleaseUpdatePayload,
    ReleaseInstallResult, ReleaseUpdatePrepareResult, UPDATE_HELPER_READY,
};
