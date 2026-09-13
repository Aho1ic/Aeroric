//! 周报生成。
//!
//! 分工:**区间与筛选在前端**(`src/weeklyReport.ts`,纯函数、可测),**读会话 + 调模型 +
//! 拼 md + 写盘在这里**(那套 md 写作器与路径校验已经在 `session::export` 里了)。
//! 于是这一层只收一份"已经筛好的任务清单",不需要知道"一周从哪天开始"。
//!
//! ## 三条设计约束
//!
//! 1. **一条坏会话不能拖垮整份报告。** 摘要拿不到就降级成任务标题,继续往下走。
//!    文件被删、超出体积上限、agent 超时都属于正常降级,不是错误。
//! 2. **串行,不并发。** 并发调 agent 会撞速率限制,而且进度条会乱跳。周报是手动触发的
//!    低频操作,慢一点可以接受。
//! 3. **凡进 bullet 的文本先过 `sanitize_md_inline`。** 模型摘要里的换行会把 md 结构撑破。
//!
//! ## 章节标题为什么不走 i18n
//!
//! md 是产物不是 UI:它会被存档、被别人打开,不该随读者当前的界面语言变。Rust 侧也读不到
//! 前端的 i18n 表。所以按 `request.locale` 硬编码中英两套。

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::session::{format_timestamp_ms, sanitize_md_inline, validate_export_output_path};

/// 进度事件名。前端 `listen` 这个 topic。
const PROGRESS_EVENT: &str = "weekly-report-progress";

/// 最多给多少条任务生成模型摘要。
///
/// 超出的只记元数据,并在概览里注明还剩几条 —— 一周 40 条已经是很重的一周,而每条摘要
/// 都要起一个 agent 进程,再往上走用户会以为程序卡死了。
const MAX_SUMMARIZED_TASKS: usize = 40;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportTask {
    pub project_name: String,
    pub project_path: String,
    pub title: String,
    pub agent: String,
    pub status: String,
    pub created_at: i64,
    #[serde(default)]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    /// 会话 jsonl 绝对路径。缺失时这条任务直接降级成标题。
    #[serde(default)]
    pub session_path: Option<String>,
    /// 会话协议族(`claude`/`codex`/`dsh`/`omp`),决定路径校验用哪套布局根。
    /// 存字符串而不是 `AgentFamily`:前端传的是 `ProtocolFamily` 字面量,解析失败时
    /// 该退到 claude 而不是让整个请求反序列化失败。
    #[serde(default)]
    pub session_family: Option<String>,
    #[serde(default)]
    pub additions: Option<i64>,
    #[serde(default)]
    pub deletions: Option<i64>,
}

impl WeeklyReportTask {
    /// 会话协议族。缺失或不认识的值退到 `Claude`:最坏结果只是这一条任务的摘要降级成
    /// 标题,不影响其余条目。
    fn session_family(&self) -> crate::app_settings::AgentFamily {
        self.session_family
            .as_deref()
            .and_then(crate::app_settings::AgentFamily::parse)
            .unwrap_or(crate::app_settings::AgentFamily::Claude)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportRequest {
    pub from_ms: i64,
    pub to_ms: i64,
    /// `YYYY-Www`,由前端按 ISO 周算好。
    pub week_label: String,
    pub locale: String,
    /// 目标 md 的绝对路径。校验交给 `validate_export_output_path`。
    pub output_path: String,
    pub tasks: Vec<WeeklyReportTask>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyReportProgress {
    /// `week_label`。前端可能同时开着两个区间的面板,不带 scope 会画到错的进度条上。
    scope: String,
    completed: usize,
    total: usize,
    current_title: String,
}

/// 一条任务在报告里的最终形态。
struct ReportEntry {
    task: WeeklyReportTask,
    /// 模型摘要。`None` 表示降级 —— 只写元数据行。
    summary: Option<String>,
}

/// 报告里的固定文案。按 locale 选一套,整份报告用同一套。
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

fn labels_for(locale: &str) -> &'static ReportLabels {
    if locale.starts_with("zh") {
        &LABELS_ZH
    } else {
        &LABELS_EN
    }
}

/// 本地日期 `YYYY-MM-DD`。区间行用它,与前端算区间时的时区语义一致。
fn local_ymd(milliseconds: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(milliseconds)
        .single()
        .map(|timestamp| timestamp.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| milliseconds.to_string())
}

fn status_label(status: &str, labels: &ReportLabels) -> String {
    match status {
        "done" => labels.status_done.to_string(),
        "failed" => labels.status_failed.to_string(),
        "cancelled" => labels.status_cancelled.to_string(),
        other => sanitize_md_inline(other),
    }
}

/// 拼出整份报告的 markdown。
///
/// 单独抽成纯函数(不碰 IO、不碰 Tauri)是为了能直接断言输出文本 —— md 结构被撑破这类
/// 问题只有对着成品字符串才看得见。
fn render_report(request: &WeeklyReportRequest, entries: &[ReportEntry], skipped: usize) -> String {
    let labels = labels_for(&request.locale);
    let mut out = String::with_capacity(2048);

    let _ = writeln!(
        out,
        "# {} {}\n",
        sanitize_md_inline(&request.week_label),
        labels.heading
    );

    let done = entries.iter().filter(|e| e.task.status == "done").count();
    let failed = entries.iter().filter(|e| e.task.status == "failed").count();
    let cancelled = entries
        .iter()
        .filter(|e| e.task.status == "cancelled")
        .count();
    let additions: i64 = entries.iter().filter_map(|e| e.task.additions).sum();
    let deletions: i64 = entries.iter().filter_map(|e| e.task.deletions).sum();

    let _ = writeln!(out, "## {}\n", labels.overview);
    let _ = writeln!(
        out,
        "- {}: {} ~ {}",
        labels.range,
        local_ymd(request.from_ms),
        local_ymd(request.to_ms)
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
    // 按项目分组。保持 entries 的相对顺序,于是同一项目下仍是前端排好的时间顺序。
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
            .filter(|e| e.task.project_name.as_str() == *project)
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
                // 缩进两格挂在 bullet 下面;摘要先过 sanitize,否则里面的换行会另起一个块。
                let _ = writeln!(out, "  {}", sanitize_md_inline(summary));
            }
        }
        out.push('\n');
    }

    let failures: Vec<&ReportEntry> = entries
        .iter()
        .filter(|e| e.task.status == "failed")
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

/// 生成周报并写盘,返回实际写入的绝对路径。
///
/// 区间与任务筛选由前端完成(`src/weeklyReport.ts`);这里只负责摘要 + 拼装 + 落盘。
#[tauri::command]
pub async fn generate_weekly_report(
    app: AppHandle,
    request: WeeklyReportRequest,
) -> Result<String, String> {
    if request.tasks.is_empty() {
        return Err("No tasks in range".to_string());
    }

    // 先校验输出路径:摘要要花几十秒和真金白银的 token,拿不到落脚点就别开始。
    let output_path: std::path::PathBuf = {
        let raw = request.output_path.clone();
        tokio::task::spawn_blocking(move || validate_export_output_path(&raw))
            .await
            .map_err(|error| format!("Output path validation failed: {}", error))??
    };

    let total = request.tasks.len().min(MAX_SUMMARIZED_TASKS);
    let skipped = request.tasks.len().saturating_sub(MAX_SUMMARIZED_TASKS);
    let mut entries: Vec<ReportEntry> = Vec::with_capacity(request.tasks.len());

    for (index, task) in request.tasks.iter().enumerate() {
        let summary = if index < MAX_SUMMARIZED_TASKS {
            // 发不出去不该让作业停下 —— 窗口可能已经关了,而报告本身仍有价值。
            let _ = app.emit(
                PROGRESS_EVENT,
                WeeklyReportProgress {
                    scope: request.week_label.clone(),
                    completed: index,
                    total,
                    current_title: task.title.clone(),
                },
            );
            match &task.session_path {
                Some(session_path) if !session_path.is_empty() => {
                    crate::agent_assist::summarize_task_session(
                        session_path,
                        &task.project_path,
                        task.session_family(),
                        &request.locale,
                    )
                    .await
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

    let _ = app.emit(
        PROGRESS_EVENT,
        WeeklyReportProgress {
            scope: request.week_label.clone(),
            completed: total,
            total,
            current_title: String::new(),
        },
    );

    let markdown = render_report(&request, &entries, skipped);
    let target = output_path.clone();
    // `atomic_write_private`(0o600)而不是 `atomic_write`:正文里每条摘要都是模型对会话
    // 正文的概括,可能带上仓库内容、路径与错误信息。仓库对承载此类内容的写入一律用私有版
    // (settings.json / ssh-passwords.json / mcp.json 同理),同机其它用户不该能读。
    // 收紧的是文件权限,不影响用户自己把它发给别人。
    tokio::task::spawn_blocking(move || crate::storage::atomic_write_private(&target, &markdown))
        .await
        .map_err(|error| format!("Report write thread failed: {}", error))??;

    Ok(output_path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: &str) -> WeeklyReportTask {
        WeeklyReportTask {
            project_name: "Alpha".to_string(),
            project_path: "/tmp/alpha".to_string(),
            title: format!("Task {}", id),
            agent: "codex".to_string(),
            status: status.to_string(),
            created_at: 1_757_000_000_000,
            completed_at: Some(1_757_100_000_000),
            failure_reason: None,
            session_path: None,
            session_family: Some("codex".to_string()),
            additions: None,
            deletions: None,
        }
    }

    fn request(locale: &str, tasks: Vec<WeeklyReportTask>) -> WeeklyReportRequest {
        WeeklyReportRequest {
            from_ms: 1_757_000_000_000,
            to_ms: 1_757_500_000_000,
            week_label: "2026-W37".to_string(),
            locale: locale.to_string(),
            output_path: "/tmp/report.md".to_string(),
            tasks,
        }
    }

    fn entry(task: WeeklyReportTask, summary: Option<&str>) -> ReportEntry {
        ReportEntry {
            task,
            summary: summary.map(|value| value.to_string()),
        }
    }

    #[test]
    fn overview_counts_match_the_entries() {
        let entries = vec![
            entry(task("a", "done"), None),
            entry(task("b", "done"), None),
            entry(task("c", "failed"), None),
            entry(task("d", "cancelled"), None),
        ];
        let markdown = render_report(&request("en", Vec::new()), &entries, 0);
        assert!(
            markdown.contains("- Tasks: 4 (2 done / 1 failed / 1 cancelled)"),
            "unexpected overview line:\n{markdown}"
        );
    }

    #[test]
    fn multiline_summary_stays_inside_one_bullet() {
        // 模型摘要里的换行是最容易撑破 md 结构的东西:一个裸 \n 就会把后半段变成
        // 顶级段落,项目分组从那里往下全乱。
        let entries = vec![entry(
            task("a", "done"),
            Some("第一行\n\n- 看起来像新 bullet\n第三行"),
        )];
        let markdown = render_report(&request("zh", Vec::new()), &entries, 0);
        let summary_lines: Vec<&str> = markdown
            .lines()
            .filter(|line| line.contains("第一行"))
            .collect();
        assert_eq!(summary_lines.len(), 1, "summary must occupy one line");
        assert_eq!(
            summary_lines[0], "  第一行 - 看起来像新 bullet 第三行",
            "newlines must collapse to spaces"
        );
    }

    #[test]
    fn failed_tasks_get_a_blockers_section_and_others_do_not() {
        let ok = vec![entry(task("a", "done"), None)];
        assert!(!render_report(&request("en", Vec::new()), &ok, 0).contains("Blockers and risks"));

        let mut failing = task("b", "failed");
        failing.failure_reason = Some("exit code 1".to_string());
        let bad = vec![entry(failing, None)];
        let markdown = render_report(&request("en", Vec::new()), &bad, 0);
        assert!(markdown.contains("## Blockers and risks"));
        assert!(markdown.contains("exit code 1"));
    }

    #[test]
    fn locale_picks_the_heading_set() {
        let entries = vec![entry(task("a", "done"), None)];
        let zh = render_report(&request("zh-CN", Vec::new()), &entries, 0);
        assert!(zh.contains("# 2026-W37 工作报告"));
        assert!(zh.contains("## 概览"));

        let en = render_report(&request("en", Vec::new()), &entries, 0);
        assert!(en.contains("# 2026-W37 Work report"));
        assert!(en.contains("## Overview"));
    }

    #[test]
    fn skipped_count_is_reported_only_when_nonzero() {
        let entries = vec![entry(task("a", "done"), None)];
        assert!(!render_report(&request("en", Vec::new()), &entries, 0).contains("more task"));
        assert!(
            render_report(&request("en", Vec::new()), &entries, 3).contains("3 more task(s)"),
            "the report must admit it did not summarize everything"
        );
    }

    #[test]
    fn projects_are_grouped_once_each() {
        // 同一项目出现两次说明分组按顺序去重失效,报告会出现两个同名 ### 段。
        let mut beta = task("b", "done");
        beta.project_name = "Beta".to_string();
        let entries = vec![
            entry(task("a", "done"), None),
            entry(beta, None),
            entry(task("c", "done"), None),
        ];
        let markdown = render_report(&request("en", Vec::new()), &entries, 0);
        assert_eq!(markdown.matches("### Alpha").count(), 1);
        assert_eq!(markdown.matches("### Beta").count(), 1);
    }

    #[test]
    fn code_changes_line_appears_only_with_diff_stats() {
        let plain = vec![entry(task("a", "done"), None)];
        assert!(!render_report(&request("en", Vec::new()), &plain, 0).contains("Code changes"));

        let mut with_diff = task("b", "done");
        with_diff.additions = Some(120);
        with_diff.deletions = Some(30);
        let entries = vec![entry(with_diff, None)];
        let markdown = render_report(&request("en", Vec::new()), &entries, 0);
        assert!(markdown.contains("- Code changes: +120 / -30"));
        assert!(markdown.contains("+120/-30"));
    }
}
