//! 周报（自 `src-tauri/src/report.rs` 上收；区间/筛选半边自 `src/weeklyReport.ts`）。
//!
//! 旧实现把区间与筛选留在 React、把摘要与拼装留在 Tauri。两边都进 core 之后，
//! 一条断言的边界重新划在**可测**这一侧：
//!
//! - 区间、筛选、标题（ISO 周）、文件名、markdown 拼装全是纯函数，日历经
//!   [`LocalCalendar`] 端口传入——"周日 20:00 算成周六"这类错误只有对着成品字符串
//!   才看得见，所以它们必须能被普通断言钉住，而不是靠真跑一遍。
//! - 模型摘要是端口（[`ReportSummarizer`]）：core 不 spawn agent。一条摘要失败
//!   （超时、文件被删、超限）降级成"只有元数据行"，不拖垮整份报告。
//! - 落盘经 [`write_weekly_report_markdown`]，正文含模型对会话内容的概括，一律
//!   0600（旧实现用 `atomic_write_private`，理由同款）。

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sessions::export::{format_timestamp_ms, sanitize_md_inline};
use crate::storage::atomic::FileMode;
use crate::storage::compat::Record;
use crate::tasks::cleanup::{LocalCalendar, DAY_MS};

use super::MaintenanceError;

/// 最多给多少条任务生成模型摘要。再往上走用户会以为程序卡死了，而每条摘要都要起
/// 一个 agent 进程（旧值 40，别改）。
pub const MAX_SUMMARIZED_TASKS: usize = 40;

/// 统计区间，两端都含：`from_ms` 是起始日 00:00:00.000，`to_ms` 是结束日 23:59:59.999。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeekWindow {
    pub from_ms: i64,
    pub to_ms: i64,
}

/// 包含 `now_ms` 的那个自定义周的本地边界（`src/weeklyReport.ts::weekWindow`）。
///
/// `week_start_day`/`week_end_day` 用 0=周日 .. 6=周六。区间长度
/// `((end - start + 7) % 7) + 1` 天，取值 1..7。`to` 用"起始日 + 天数 - 1 毫秒"，
/// 交给日历端口的进位，跨月/跨年不用单独处理。
pub fn week_window(
    calendar: &dyn LocalCalendar,
    now_ms: i64,
    week_start_day: u32,
    week_end_day: u32,
) -> WeekWindow {
    let span_days = ((week_end_day as i64 - week_start_day as i64 + 7) % 7) + 1;
    let (today, year, month, day, _) = calendar.parts(now_ms);
    let days_since_start = (today as i64 - week_start_day as i64 + 7) % 7;
    let from_ms = calendar.slot_ms(year, month, day - days_since_start, 0);
    let next_ms = calendar.slot_ms(year, month, day - days_since_start + span_days, 0);
    WeekWindow {
        from_ms,
        to_ms: next_ms - 1,
    }
}

/// 落在区间内的任务记录：创建时间**或**结束时间任一在区间内就算——只看一个会漏掉
/// 跨周的长任务，而那通常恰好是最值得写进周报的活。已归档照样统计。
pub fn tasks_in_window(records: &[Record], window: &WeekWindow) -> Vec<Record> {
    records
        .iter()
        .filter(|record| {
            let created = record.get("createdAt").and_then(|value| value.as_i64());
            let ended = record
                .get("completedAt")
                .and_then(|value| value.as_i64())
                .or(created);
            let created_in = created
                .map(|value| value >= window.from_ms && value <= window.to_ms)
                .unwrap_or(false);
            let ended_in = ended
                .map(|value| value >= window.from_ms && value <= window.to_ms)
                .unwrap_or(false);
            created_in || ended_in
        })
        .cloned()
        .collect()
}

/// `YYYY-Www`。取**区间中点**算 ISO 周：`iso_week` 是严格 ISO-8601（周一为周首），
/// 而区间周首可配；周日起始时起始日属于上一个 ISO 周，标题会比内容早一周。
/// 中点落在区间里占天数最多的那个 ISO 周上。
pub fn week_label(calendar: &dyn LocalCalendar, window: &WeekWindow) -> String {
    let midpoint = window.from_ms + (window.to_ms - window.from_ms) / 2;
    let (year, week) = iso_week(calendar, midpoint);
    format!("{year}-W{week:02}")
}

/// 本地 `YYYY-MM-DD`（区间行与文件名都用它）。
pub fn local_ymd(calendar: &dyn LocalCalendar, ms: i64) -> String {
    let (_, year, month, day, _) = calendar.parts(ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `aeroric-weekly-<from>_<to>.md`（与 `RunningView` 的导出命名同风格）。
pub fn weekly_report_file_name(calendar: &dyn LocalCalendar, window: &WeekWindow) -> String {
    format!(
        "aeroric-weekly-{}_{}.md",
        local_ymd(calendar, window.from_ms),
        local_ymd(calendar, window.to_ms)
    )
}

/// 严格 ISO-8601 周号（周一为周首，周四定周）。
pub fn iso_week(calendar: &dyn LocalCalendar, ms: i64) -> (i64, u32) {
    let (js_weekday, year, month, day, _) = calendar.parts(ms);
    // JS `getDay`：0=周日 ⇒ ISO 1..7。
    let iso_weekday = ((js_weekday as i64 + 6) % 7) + 1;
    let ordinal = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
    let mut week = (ordinal - iso_weekday + 10) / 7;
    if week < 1 {
        // 属于上一年的最后一周。
        let prev_year = year - 1;
        let weeks = weeks_in_iso_year(prev_year);
        return (prev_year, weeks);
    }
    let weeks = weeks_in_iso_year(year);
    if week > weeks as i64 {
        return (year + 1, 1);
    }
    if week < 1 {
        week = 1;
    }
    (year, week as u32)
}

fn weeks_in_iso_year(year: i64) -> u32 {
    let jan1 = iso_weekday_of(year, 1, 1);
    if jan1 == 4 || (jan1 == 3 && is_leap(year)) {
        53
    } else {
        52
    }
}

fn iso_weekday_of(year: i64, month: i64, day: i64) -> i64 {
    let days = days_from_civil(year, month, day);
    // 1970-01-01 是周四 ⇒ ISO weekday 4。
    ((days + 3).rem_euclid(7)) + 1
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Howard Hinnant 的 `days_from_civil`：公历日期 → 1970-01-01 起的天数。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 一条任务在报告里的输入形状（旧 `WeeklyReportTask`，字段名逐个显式）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportTask {
    pub project_name: String,
    pub project_path: String,
    pub title: String,
    pub agent: String,
    pub status: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    /// 会话 jsonl 绝对路径；缺失时这条任务直接降级成标题。
    #[serde(default)]
    pub session_path: Option<String>,
    /// 会话协议族（`claude`/`codex`/`dsh`/`omp`）。认不出的值退到 `claude`，
    /// 最坏只是这一条降级，不让整个请求反序列化失败（旧行为）。
    #[serde(default)]
    pub session_family: Option<String>,
    #[serde(default)]
    pub additions: Option<i64>,
    #[serde(default)]
    pub deletions: Option<i64>,
}

impl WeeklyReportTask {
    /// 交给摘要器的协议族；认不出的一律 `claude`。
    pub fn session_family(&self) -> &str {
        match self.session_family.as_deref() {
            Some("codex") => "codex",
            Some("dsh") => "dsh",
            Some("omp") => "omp",
            _ => "claude",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportRequest {
    pub from_ms: i64,
    pub to_ms: i64,
    /// `YYYY-Www`，由区间与日历算出（[`week_label`]）。
    pub week_label: String,
    pub locale: String,
    /// 目标 md 的路径。校验在 [`write_weekly_report_markdown`]（摘要要花
    /// 真金白银的 token，拿不到落脚点就别开始）。
    pub output_path: String,
    pub tasks: Vec<WeeklyReportTask>,
}

/// 进度事件体（旧 `WeeklyReportProgress`；v1 topic `weekly.report_progress` 的载荷）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportProgress {
    /// `week_label`。UI 可能同时开着两个区间的面板，不带 scope 会画到错的进度条上。
    pub scope: String,
    pub completed: usize,
    pub total: usize,
    pub current_title: String,
}

/// 模型摘要端口。实现留在宿主：它才知道怎么起 agent 进程（旧
/// `agent_assist::summarize_task_session`）。
#[async_trait::async_trait]
pub trait ReportSummarizer: Send + Sync {
    /// 拿不到摘要就返回 `None` —— 降级不是错误。
    async fn summarize_task(&self, task: &WeeklyReportTask, locale: &str) -> Option<String>;
}

/// 进度出口（宿主把它转成 emit；测试用 [`NullProgress`]）。
pub trait ReportProgressSink: Send + Sync {
    fn progress(&self, update: &WeeklyReportProgress);
}

/// 什么都不做的进度出口。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullProgress;

impl ReportProgressSink for NullProgress {
    fn progress(&self, _update: &WeeklyReportProgress) {}
}

/// 一条任务在报告里的最终形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportEntry {
    pub task: WeeklyReportTask,
    /// 模型摘要；`None` 表示降级——只写元数据行。
    pub summary: Option<String>,
}

const LABELS_ZH: ReportLabels = ReportLabels {
    heading: "工作报告",
    overview: "概览",
    range: "区间",
    task_count: "任务数",
    code_changes: "代码变更",
    progress: "进展",
    blockers: "阻塞与风险",
    status_done: "完成",
    status_failed: "失败",
    status_cancelled: "取消",
    not_summarized: "另有 {n} 条任务未生成摘要",
};

const LABELS_EN: ReportLabels = ReportLabels {
    heading: "Work report",
    overview: "Overview",
    range: "Range",
    task_count: "Tasks",
    code_changes: "Code changes",
    progress: "Progress",
    blockers: "Blockers and risks",
    status_done: "done",
    status_failed: "failed",
    status_cancelled: "cancelled",
    not_summarized: "{n} more task(s) without a summary",
};

/// 报告里的固定文案。md 是产物不是 UI：它会被存档、被别人打开，不该随读者当前的
/// 界面语言变，所以按 `request.locale` 选一套而不是走 i18n 表。
struct ReportLabels {
    heading: &'static str,
    overview: &'static str,
    range: &'static str,
    task_count: &'static str,
    code_changes: &'static str,
    progress: &'static str,
    blockers: &'static str,
    status_done: &'static str,
    status_failed: &'static str,
    status_cancelled: &'static str,
    not_summarized: &'static str,
}

fn labels_for(locale: &str) -> &'static ReportLabels {
    if locale.starts_with("zh") {
        &LABELS_ZH
    } else {
        &LABELS_EN
    }
}

fn status_label(status: &str, labels: &ReportLabels) -> String {
    match status {
        "done" => labels.status_done.to_string(),
        "failed" => labels.status_failed.to_string(),
        "cancelled" => labels.status_cancelled.to_string(),
        other => sanitize_md_inline(other),
    }
}

/// 拼出整份报告的 markdown。纯函数（不碰 IO、不碰宿主）是为了能直接断言输出文本。
pub fn render_report(
    calendar: &dyn LocalCalendar,
    request: &WeeklyReportRequest,
    entries: &[ReportEntry],
    skipped: usize,
) -> String {
    let labels = labels_for(&request.locale);
    let mut out = String::with_capacity(2048);

    let _ = writeln!(
        out,
        "# {} {}\n",
        sanitize_md_inline(&request.week_label),
        labels.heading
    );

    let done = entries
        .iter()
        .filter(|entry| entry.task.status == "done")
        .count();
    let failed = entries
        .iter()
        .filter(|entry| entry.task.status == "failed")
        .count();
    let cancelled = entries
        .iter()
        .filter(|entry| entry.task.status == "cancelled")
        .count();
    let additions: i64 = entries
        .iter()
        .filter_map(|entry| entry.task.additions)
        .sum();
    let deletions: i64 = entries
        .iter()
        .filter_map(|entry| entry.task.deletions)
        .sum();

    let _ = writeln!(out, "## {}\n", labels.overview);
    let _ = writeln!(
        out,
        "- {}: {} ~ {}",
        labels.range,
        local_ymd(calendar, request.from_ms),
        local_ymd(calendar, request.to_ms)
    );
    let _ = writeln!(
        out,
        "- {}: {} ({} {} / {} {} / {} {})",
        labels.task_count,
        entries.len(),
        done,
        labels.status_done,
        failed,
        labels.status_failed,
        cancelled,
        labels.status_cancelled
    );
    if additions != 0 || deletions != 0 {
        let _ = writeln!(
            out,
            "- {}: +{} / -{}",
            labels.code_changes, additions, deletions
        );
    }
    if skipped > 0 {
        let _ = writeln!(
            out,
            "- {}",
            labels.not_summarized.replace("{n}", &skipped.to_string())
        );
    }
    out.push('\n');

    let _ = writeln!(out, "## {}\n", labels.progress);
    // 按项目分组，保持 entries 的相对顺序（同一项目下仍是前端排好的时间顺序）。
    let mut seen_projects: Vec<&str> = Vec::new();
    for entry in entries {
        if !seen_projects.contains(&entry.task.project_name.as_str()) {
            seen_projects.push(&entry.task.project_name);
        }
    }
    for project in &seen_projects {
        let _ = writeln!(out, "### {}\n", sanitize_md_inline(project));
        for entry in entries
            .iter()
            .filter(|entry| entry.task.project_name.as_str() == *project)
        {
            let task = &entry.task;
            let mut meta = format!(
                "- **{}** · {} · {}",
                sanitize_md_inline(&task.title),
                sanitize_md_inline(&task.agent),
                status_label(&task.status, labels)
            );
            let _ = write!(
                meta,
                " · {}",
                format_timestamp_ms(task.completed_at.unwrap_or(task.created_at))
            );
            if let (Some(additions), Some(deletions)) = (task.additions, task.deletions) {
                let _ = write!(meta, " · +{}/-{}", additions, deletions);
            }
            let _ = writeln!(out, "{}", meta);
            if let Some(summary) = &entry.summary {
                // 缩进两格挂在 bullet 下面；摘要先过 sanitize，否则里面的换行会另起一块。
                let _ = writeln!(out, "  {}", sanitize_md_inline(summary));
            }
        }
        out.push('\n');
    }

    let failures: Vec<&ReportEntry> = entries
        .iter()
        .filter(|entry| entry.task.status == "failed")
        .collect();
    if !failures.is_empty() {
        let _ = writeln!(out, "## {}\n", labels.blockers);
        for entry in failures {
            let reason = entry
                .task
                .failure_reason
                .as_deref()
                .map(sanitize_md_inline)
                .filter(|value| !value.is_empty());
            match reason {
                Some(reason) => {
                    let _ = writeln!(
                        out,
                        "- **{}** · {} · {}",
                        sanitize_md_inline(&entry.task.title),
                        labels.status_failed,
                        reason
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "- **{}** · {}",
                        sanitize_md_inline(&entry.task.title),
                        labels.status_failed
                    );
                }
            }
        }
        out.push('\n');
    }

    out
}

/// 串行跑完摘要并渲染（旧 `generate_weekly_report` 的循环）。
///
/// **串行，不并发**：并发调 agent 会撞速率限制，而且进度条会乱跳；周报是手动触发的
/// 低频操作，慢一点可以接受。返回 markdown 字符串，落盘由调用方经
/// [`write_weekly_report_markdown`] 完成——这样"写了什么"能被断言，而"写到哪"仍受
/// 路径校验。
pub async fn generate_weekly_report(
    calendar: &dyn LocalCalendar,
    request: &WeeklyReportRequest,
    summarizer: &dyn ReportSummarizer,
    progress: &dyn ReportProgressSink,
) -> Result<String, MaintenanceError> {
    if request.tasks.is_empty() {
        return Err(MaintenanceError::EmptyReport);
    }

    let total = request.tasks.len().min(MAX_SUMMARIZED_TASKS);
    let skipped = request.tasks.len().saturating_sub(MAX_SUMMARIZED_TASKS);
    let mut entries: Vec<ReportEntry> = Vec::with_capacity(request.tasks.len());

    for (index, task) in request.tasks.iter().enumerate() {
        let summary = if index < MAX_SUMMARIZED_TASKS {
            progress.progress(&WeeklyReportProgress {
                scope: request.week_label.clone(),
                completed: index,
                total,
                current_title: task.title.clone(),
            });
            match &task.session_path {
                Some(path) if !path.is_empty() => {
                    summarizer.summarize_task(task, &request.locale).await
                }
                _ => None,
            }
        } else {
            None
        };
        entries.push(ReportEntry {
            task: task.clone(),
            summary,
        });
    }

    progress.progress(&WeeklyReportProgress {
        scope: request.week_label.clone(),
        completed: total,
        total,
        current_title: String::new(),
    });

    Ok(render_report(calendar, request, &entries, skipped))
}

/// 校验输出路径并原子写入（0600）。返回实际写入的绝对路径。
pub fn write_weekly_report_markdown(
    output_path: &str,
    markdown: &str,
) -> Result<PathBuf, MaintenanceError> {
    let target = crate::sessions::export::validate_export_output_path(output_path)
        .map_err(MaintenanceError::OutputPath)?;
    crate::storage::atomic::write_atomic(&target, markdown.as_bytes(), FileMode::Private)
        .map_err(|error| MaintenanceError::WriteFailed(error.to_string()))?;
    Ok(target)
}

/// 区间长度（天）。UI 与 CLI 的预览用它显示"这份周报覆盖几天"。
pub fn window_days(window: &WeekWindow) -> i64 {
    (window.to_ms - window.from_ms + 1).div_euclid(DAY_MS as i64)
}
