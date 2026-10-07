//! 周报的宿主接线层（Task P8-04 刀三）。
//!
//! 分工：**区间、筛选、ISO 周标题、markdown 拼装、落盘在 core**
//! （`aeroric_core::maintenance::reports`，P8-04 刀一从 `src/weeklyReport.ts` +
//! 本文件的渲染器上收）；本文件只剩宿主独有的两件事——模型摘要（要起 agent 进程）
//! 与进度事件双发。
//!
//! 三条设计约束随业务体上收，core 侧的注释是唯一真相：一条坏会话不拖垮整份报告、
//! 摘要串行不并发、进 bullet 的文本先过 `sanitize_md_inline`。

use tauri::AppHandle;

use aeroric_core::maintenance::{ReportProgressSink, WeeklyReportProgress, WeeklyReportRequest};

/// 旧桌面通道名。前端 `listen` 它，原样保留。
const PROGRESS_EVENT: &str = "weekly-report-progress";
/// v1 事件 topic（点号命名空间；payload 与旧通道同形）。
const PROGRESS_TOPIC: &str = "weekly.report_progress";

/// 周报（宿主接线层；编排与渲染在 `aeroric_core::maintenance::reports`）。
///
/// 计划 Action 1 的"timer/policy 移出 React"与本刀的形状一致：区间、筛选、
/// ISO 周标题、markdown 拼装、落盘全在 core；宿主只做两件它独有的事——
/// **摘要**（要起 agent 进程）与**进度事件双发**（旧桌面通道
/// `weekly-report-progress` ＋ v1 topic `weekly.report_progress`）。
#[tauri::command]
pub async fn generate_weekly_report(
    app: AppHandle,
    request: WeeklyReportRequest,
) -> Result<String, String> {
    let progress = HostProgress { app };
    crate::notification::service()
        .write_weekly_report(request, &progress)
        .await
        .map_err(|error| error.to_string())
}

/// 进度出口：core 只认 [`ReportProgressSink`]；这里一次回调发两条通道。
struct HostProgress {
    app: AppHandle,
}

impl ReportProgressSink for HostProgress {
    fn progress(&self, update: &WeeklyReportProgress) {
        use tauri::Emitter;
        let payload = serde_json::json!({
            "scope": update.scope,
            "completed": update.completed,
            "total": update.total,
            "currentTitle": update.current_title,
        });
        // 发射失败不该让作业停下——窗口可能已经关了，而报告本身仍有价值。
        let _ = self.app.emit(PROGRESS_EVENT, payload.clone());
        let _ = self.app.emit(PROGRESS_TOPIC, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 渲染与区间判定的用例随业务体上收 core
    /// （`core/crates/aeroric-core/tests/maintenance.rs` 的 `render_report_*` 与
    /// `generate_weekly_report_*`）。这里只剩**通道契约**：两条通道的名字都被外部
    /// 读着——旧通道是 React 的 `listen` 字面量，v1 topic 是事件词表的一行。
    #[test]
    fn the_progress_channels_are_named_exactly_once_each() {
        assert_eq!(PROGRESS_EVENT, "weekly-report-progress");
        assert_eq!(PROGRESS_TOPIC, "weekly.report_progress");
        assert_ne!(PROGRESS_EVENT, PROGRESS_TOPIC);
    }
}
