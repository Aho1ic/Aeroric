//! Task P8-04 acceptance: the maintenance plane's rules.
//!
//! Everything here is a pure rule or a local file operation, so the assertions are about
//! *arithmetic and bookkeeping*, not about a running app: which local week a timestamp
//! falls in, which tasks a retention window may delete, which release asset a machine can
//! actually install, and what happens to a checksum that no longer matches.
//!
//! The calendar is a fixture (`Utc8`) rather than the machine's zone: a rule whose answer
//! depends on where the test runner sits is not pinned by anything.

use std::future::Future;
use std::path::PathBuf;

use aeroric_core::maintenance::cleanup::{run_cleanup_once, CleanupRun, CLEANUP_TICK_MS};
use aeroric_core::maintenance::diagnostics::StartupDegradationLog;
use aeroric_core::maintenance::notifications::{
    apply_fetched_notifications, build_result, is_valid, release_to_notification, should_fetch,
    valid_asset_filename, GitHubRelease, GitHubReleaseAsset, NotificationStore, RemoteNotification,
    FETCH_INTERVAL_SECS,
};
use aeroric_core::maintenance::reports::{
    generate_weekly_report, iso_week, local_ymd, render_report, tasks_in_window, week_label,
    week_window, weekly_report_file_name, write_weekly_report_markdown, NullProgress, ReportEntry,
    ReportProgressSink, ReportSummarizer, WeekWindow, WeeklyReportProgress, WeeklyReportRequest,
    WeeklyReportTask, MAX_SUMMARIZED_TASKS,
};
use aeroric_core::maintenance::updates::{
    compare_versions, expected_release_digest_asset_name, find_checksum_for_asset,
    pending_update_result, release_version, safe_tag, select_release_installer_asset,
    validate_pending_installer_path, verify_downloaded_checksum, PendingReleaseUpdate,
    PendingReleaseUpdatePayload, ReleaseUpdatePrepareResult, UPDATE_HELPER_READY,
};
use aeroric_core::maintenance::MaintenanceError;
use aeroric_core::storage::compat::Record;
use aeroric_core::tasks::cleanup::{CleanupSchedule, LocalCalendar, Mode, ShouldRun};
use serde_json::json;

mod common;
use common::{build_service, cleanup, local_project, record, task, writer};

const DAY_MS: i64 = 86_400_000;

// ── 日历夹具：UTC+8，与旧前端 `new Date(y, m, d, h)` 的本地语义同形 ────────────────

struct Utc8;

impl LocalCalendar for Utc8 {
    fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64) {
        let local = now_ms + 8 * 3_600_000;
        let day = local.div_euclid(DAY_MS);
        let millis = local.rem_euclid(DAY_MS);
        let (year, month, date) = civil_from_days(day);
        // JS `getDay`：0=周日。
        (
            (day + 4).rem_euclid(7) as u32,
            year,
            month,
            date,
            millis / 3_600_000,
        )
    }

    fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64 {
        days_from_civil(year, month, day) * DAY_MS + hour * 3_600_000 - 8 * 3_600_000
    }
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 2026-09-09（周三）12:00 UTC+8。
fn wednesday() -> i64 {
    Utc8.slot_ms(2026, 9, 9, 12)
}

// ── 周报区间 ────────────────────────────────────────────────────────────────

#[test]
fn week_window_starts_on_the_configured_local_day() {
    let now = wednesday();
    // 周日起始（旧默认），2026-09-06 是周日。
    let sunday = week_window(&Utc8, now, 0, 6);
    assert_eq!(local_ymd(&Utc8, sunday.from_ms), "2026-09-06");
    assert_eq!(local_ymd(&Utc8, sunday.to_ms), "2026-09-12");
    assert_eq!(sunday.from_ms, Utc8.slot_ms(2026, 9, 6, 0));
    assert_eq!(sunday.to_ms, Utc8.slot_ms(2026, 9, 13, 0) - 1);

    // 周一起始（ISO 惯例）：同一个 now 落进本周一那一段。
    let monday = week_window(&Utc8, now, 1, 0);
    assert_eq!(local_ymd(&Utc8, monday.from_ms), "2026-09-07");
    assert_eq!(local_ymd(&Utc8, monday.to_ms), "2026-09-13");
    assert_eq!(window_days_span(&monday), 7);

    // 起止同一天 ⇒ 单日区间（`((end - start + 7) % 7) + 1`）。
    let single = week_window(&Utc8, now, 3, 3);
    assert_eq!(local_ymd(&Utc8, single.from_ms), "2026-09-09");
    assert_eq!(local_ymd(&Utc8, single.to_ms), "2026-09-09");
    assert_eq!(window_days_span(&single), 1);
}

fn window_days_span(window: &WeekWindow) -> i64 {
    (window.to_ms - window.from_ms + 1) / DAY_MS
}

#[test]
fn week_label_prefers_the_iso_week_holding_most_of_the_window() {
    let now = wednesday();
    // 周日→周六的区间有 6/7 天落在下一个 ISO 周；取中点才不会写出早一周的标题。
    let sunday = week_window(&Utc8, now, 0, 6);
    assert_eq!(week_label(&Utc8, &sunday), "2026-W37");
    let monday = week_window(&Utc8, now, 1, 0);
    assert_eq!(week_label(&Utc8, &monday), "2026-W37");
}

#[test]
fn iso_week_handles_year_boundaries() {
    // 2021-01-01 是周五，属于 2020 年的第 53 周。
    let new_year = Utc8.slot_ms(2021, 1, 1, 9);
    assert_eq!(iso_week(&Utc8, new_year), (2020, 53));
    // 2019-12-30 是周一，属于 2020 年的第 1 周。
    let week_one = Utc8.slot_ms(2019, 12, 30, 9);
    assert_eq!(iso_week(&Utc8, week_one), (2020, 1));
    // 2026-01-01 是周四，属于 2026 年的第 1 周。
    let start = Utc8.slot_ms(2026, 1, 1, 9);
    assert_eq!(iso_week(&Utc8, start), (2026, 1));
}

#[test]
fn tasks_in_window_matches_created_or_ended() {
    let window = week_window(&Utc8, wednesday(), 1, 0);
    let inside = record(json!({"id": "created-in", "createdAt": window.from_ms + 1000}));
    let ended = record(
        json!({"id": "ended-in", "createdAt": window.from_ms - 5 * DAY_MS, "completedAt": window.to_ms - 1000}),
    );
    let outside = record(
        json!({"id": "outside", "createdAt": window.from_ms - DAY_MS, "completedAt": window.from_ms - DAY_MS}),
    );
    let selected = tasks_in_window(&[inside.clone(), ended.clone(), outside], &window);
    let ids: Vec<&str> = selected.iter().filter_map(|row| row.id()).collect();
    assert_eq!(ids, vec!["created-in", "ended-in"]);
}

#[test]
fn weekly_report_file_name_uses_local_dates() {
    let window = week_window(&Utc8, wednesday(), 1, 0);
    assert_eq!(
        weekly_report_file_name(&Utc8, &window),
        "aeroric-weekly-2026-09-07_2026-09-13.md"
    );
}

// ── 周报渲染与生成 ──────────────────────────────────────────────────────────

fn report_task(
    id: &str,
    project: &str,
    status: &str,
    extra: serde_json::Value,
) -> WeeklyReportTask {
    let mut value = json!({
        "projectName": project,
        "projectPath": "/repo",
        "title": format!("Task {id}"),
        "agent": "claude",
        "status": status,
        "createdAt": 1_757_000_000_000i64,
        "completedAt": 1_757_100_000_000i64,
    });
    if let Some(object) = value.as_object_mut() {
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
    }
    serde_json::from_value(value).expect("a report task")
}

fn report_request(tasks: Vec<WeeklyReportTask>) -> WeeklyReportRequest {
    let window = week_window(&Utc8, wednesday(), 1, 0);
    WeeklyReportRequest {
        from_ms: window.from_ms,
        to_ms: window.to_ms,
        week_label: week_label(&Utc8, &window),
        locale: "zh-CN".to_string(),
        output_path: "/tmp/report.md".to_string(),
        tasks,
    }
}

#[test]
fn render_report_groups_by_project_and_lists_blockers() {
    let request = report_request(vec![
        report_task(
            "1",
            "Alpha",
            "done",
            json!({"additions": 12, "deletions": 3}),
        ),
        report_task(
            "2",
            "Alpha",
            "failed",
            json!({"failureReason": "agent exited 1"}),
        ),
        report_task("3", "Beta", "cancelled", json!({})),
    ]);
    let entries = vec![
        ReportEntry {
            task: request.tasks[0].clone(),
            summary: Some("did the thing".into()),
        },
        ReportEntry {
            task: request.tasks[1].clone(),
            summary: None,
        },
        ReportEntry {
            task: request.tasks[2].clone(),
            summary: None,
        },
    ];
    let markdown = render_report(&Utc8, &request, &entries, 0);
    assert!(markdown.starts_with("# 2026-W37 工作报告\n"), "{markdown}");
    assert!(markdown.contains("## 概览"), "{markdown}");
    assert!(
        markdown.contains("- 任务数: 3 (1 完成 / 1 失败 / 1 取消)"),
        "{markdown}"
    );
    assert!(markdown.contains("- 代码变更: +12 / -3"), "{markdown}");
    assert!(markdown.contains("### Alpha"), "{markdown}");
    assert!(markdown.contains("### Beta"), "{markdown}");
    // 项目分组保持记录顺序：Alpha 的两条都在 Beta 之前。
    let alpha = markdown.find("### Alpha").unwrap();
    let beta = markdown.find("### Beta").unwrap();
    assert!(alpha < beta, "{markdown}");
    assert!(
        markdown.contains("  did the thing"),
        "summary hangs under its bullet: {markdown}"
    );
    assert!(markdown.contains("## 阻塞与风险"), "{markdown}");
    assert!(markdown.contains("agent exited 1"), "{markdown}");
}

#[test]
fn render_report_sanitizes_newlines_and_reports_skipped() {
    let request = report_request(vec![report_task(
        "1",
        "Alpha",
        "done",
        json!({"title": "line one\nline two"}),
    )]);
    let entries = vec![ReportEntry {
        task: request.tasks[0].clone(),
        summary: Some("first\nsecond".into()),
    }];
    let markdown = render_report(&Utc8, &request, &entries, 5);
    assert!(!markdown.contains("line one\nline two"), "{markdown}");
    assert!(markdown.contains("line one line two"), "{markdown}");
    assert!(markdown.contains("  first second"), "{markdown}");
    assert!(markdown.contains("另有 5 条任务未生成摘要"), "{markdown}");
}

#[derive(Default)]
struct RecordingProgress {
    seen: parking_lot::Mutex<Vec<WeeklyReportProgress>>,
}

impl RecordingProgress {
    fn seen(&self) -> Vec<WeeklyReportProgress> {
        self.seen.lock().clone()
    }
}

impl ReportProgressSink for RecordingProgress {
    fn progress(&self, update: &WeeklyReportProgress) {
        self.seen.lock().push(update.clone());
    }
}

struct SummaryFor(&'static str);

#[async_trait::async_trait]
impl ReportSummarizer for SummaryFor {
    async fn summarize_task(&self, task: &WeeklyReportTask, _locale: &str) -> Option<String> {
        if task.session_path.is_some() {
            Some(self.0.to_string())
        } else {
            None
        }
    }
}

#[test]
fn generate_weekly_report_emits_progress_and_degrades_per_task() {
    let tasks = vec![
        report_task(
            "1",
            "Alpha",
            "done",
            json!({"sessionPath": "/tmp/s1.jsonl"}),
        ),
        report_task("2", "Alpha", "done", json!({})),
    ];
    let request = report_request(tasks);
    let progress = RecordingProgress::default();
    let markdown = block_on(generate_weekly_report(
        &Utc8,
        &request,
        &SummaryFor("summary text"),
        &progress,
    ))
    .expect("a report");
    assert!(markdown.contains("  summary text"), "{markdown}");
    let seen = progress.seen();
    // 每条被摘要的任务前一条进度，收尾一条 `completed == total`。
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen[0].completed, 0);
    assert_eq!(seen[1].completed, 1);
    assert_eq!(seen[2].completed, 2);
    assert_eq!(seen[2].total, 2);
    assert!(seen[2].current_title.is_empty(), "{seen:?}");
    assert!(seen.iter().all(|update| update.scope == "2026-W37"));

    let empty = report_request(vec![]);
    let failed = block_on(generate_weekly_report(
        &Utc8,
        &empty,
        &SummaryFor("x"),
        &NullProgress,
    ));
    assert!(
        matches!(failed, Err(MaintenanceError::EmptyReport)),
        "{failed:?}"
    );
}

#[test]
fn generate_weekly_report_summarizes_at_most_the_cap() {
    let tasks: Vec<WeeklyReportTask> = (0..(MAX_SUMMARIZED_TASKS + 3))
        .map(|index| {
            report_task(
                &index.to_string(),
                "Alpha",
                "done",
                json!({"sessionPath": format!("/tmp/s{index}.jsonl")}),
            )
        })
        .collect();
    let total = tasks.len();
    let request = report_request(tasks);
    let markdown = block_on(generate_weekly_report(
        &Utc8,
        &request,
        &SummaryFor("x"),
        &NullProgress,
    ))
    .expect("a report");
    assert!(
        markdown.contains(&format!(
            "另有 {} 条任务未生成摘要",
            total - MAX_SUMMARIZED_TASKS
        )),
        "{markdown}"
    );
    assert_eq!(markdown.matches("  x").count(), MAX_SUMMARIZED_TASKS);
}

/// 只为在这一个套件里 await：内核的其它测试都是同步的，不为一个调用引入运行时依赖。
fn block_on<F: Future>(future: F) -> F::Output {
    // `pollster` 不在这棵依赖树里，`tokio` 在——用一个最小单线程运行时，与 core 的
    // 其它 async 测试同款（不引入新依赖）。
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

#[test]
fn write_weekly_report_markdown_refuses_an_unusable_path() {
    let (root, _context) = writer("maintenance-report");
    let bad = write_weekly_report_markdown("relative/path.md", "# x");
    assert!(
        matches!(bad, Err(MaintenanceError::OutputPath(_))),
        "{bad:?}"
    );

    let target: PathBuf = root.join("report.md");
    let written =
        write_weekly_report_markdown(target.to_str().unwrap(), "# weekly\n").expect("write report");
    assert_eq!(
        written,
        std::fs::canonicalize(&target).unwrap_or(target.clone())
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "# weekly\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "report bodies quote session content");
    }
    cleanup(root);
}

// ── 清理执行半边 ────────────────────────────────────────────────────────────

fn schedule(mode: Mode, last_run_at: Option<u64>) -> CleanupSchedule {
    CleanupSchedule {
        enabled: true,
        mode,
        last_run_at,
        interval_days: 7,
        weekday: 0,
        hour: 20,
        retain_days: 30,
    }
}

fn seed_tasks(context: &aeroric_core::CoreContext, rows: &[Record]) {
    let path = context.paths.tasks_file("p1").expect("tasks path");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::Value::Array(
            rows.iter()
                .map(|row| serde_json::Value::Object(row.0.clone()))
                .collect(),
        ))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn cleanup_run_skips_when_disabled_or_not_due() {
    let (root, context, executor) = common::harness("maintenance-skip", &[]);
    let service = build_service(context, &executor);
    let now = wednesday();

    let mut disabled = schedule(Mode::Interval, Some(0));
    disabled.enabled = false;
    let run = run_cleanup_once(&service, &disabled, &Utc8, now);
    assert_eq!(run, CleanupRun::Skipped(ShouldRun::Disabled));

    // 刚打开开关：下一轮才删（第一条记录由调用方落 last_run_at）。
    let fresh = schedule(Mode::Interval, None);
    assert_eq!(
        run_cleanup_once(&service, &fresh, &Utc8, now),
        CleanupRun::Skipped(ShouldRun::NoPreviousRun)
    );

    // 距上次 6 天，7 天周期未到。
    let not_yet = schedule(Mode::Interval, Some((now - 6 * DAY_MS) as u64));
    assert_eq!(
        run_cleanup_once(&service, &not_yet, &Utc8, now),
        CleanupRun::Skipped(ShouldRun::NotYet)
    );
    cleanup(root);
}

#[test]
fn cleanup_run_deletes_only_expired_terminal_tasks() {
    let now = wednesday();
    let old = now - 40 * DAY_MS;
    let recent = now - 2 * DAY_MS;
    let (root, context, executor) = common::harness(
        "maintenance-delete",
        &[
            task("done-old", "p1", "done"),
            task("done-recent", "p1", "done"),
            task("interrupted-old", "p1", "interrupted"),
            task("todo-old", "p1", "todo"),
        ],
    );
    // `harness` 只写任务文件；清理从项目表出发，所以项目表必须存在。
    aeroric_core::storage::save_projects(&context, &[local_project("p1", "/repo")])
        .expect("projects");
    seed_tasks(
        &context,
        &[
            record(
                json!({"id": "done-old", "projectId": "p1", "status": "done", "createdAt": old, "completedAt": old}),
            ),
            record(
                json!({"id": "done-recent", "projectId": "p1", "status": "done", "createdAt": recent, "completedAt": recent}),
            ),
            // `interrupted` 还能续跑、`todo` 是用户写下的待办：两者都不在 terminal 集合里。
            record(
                json!({"id": "interrupted-old", "projectId": "p1", "status": "interrupted", "createdAt": old}),
            ),
            record(
                json!({"id": "todo-old", "projectId": "p1", "status": "todo", "createdAt": old}),
            ),
            // 未处理的 worktree 不删：删除会跟着丢未提交的代码。
            record(
                json!({"id": "wt-old", "projectId": "p1", "status": "done", "createdAt": old, "completedAt": old, "worktreePath": "/repo/.wt", "worktreeDiscarded": false}),
            ),
            // 收藏的不删（`TaskService::delete` 也会再过滤一次）。
            record(
                json!({"id": "starred-old", "projectId": "p1", "status": "done", "createdAt": old, "completedAt": old, "starred": true}),
            ),
        ],
    );
    let service = build_service(context, &executor);
    let run = run_cleanup_once(&service, &schedule(Mode::Interval, Some(0)), &Utc8, now);
    let CleanupRun::Ran {
        expired,
        removed,
        errors,
    } = run
    else {
        panic!("expected a run");
    };
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(expired, vec!["done-old".to_string()]);
    assert_eq!(removed, vec!["done-old".to_string()]);

    // 盘上确实只剩没被选中的那些。
    let remaining: Vec<String> = service
        .load("p1")
        .expect("reload")
        .records
        .iter()
        .filter_map(|row| row.id().map(str::to_string))
        .collect();
    assert!(
        remaining.contains(&"interrupted-old".to_string()),
        "{remaining:?}"
    );
    assert!(remaining.contains(&"todo-old".to_string()), "{remaining:?}");
    assert!(remaining.contains(&"wt-old".to_string()), "{remaining:?}");
    assert!(
        remaining.contains(&"starred-old".to_string()),
        "{remaining:?}"
    );
    assert!(
        !remaining.contains(&"done-old".to_string()),
        "{remaining:?}"
    );
    cleanup(root);
}

#[test]
fn cleanup_run_reports_a_broken_project_and_finishes_the_rest() {
    let now = wednesday();
    let (root, context, executor) = common::harness("maintenance-broken", &[]);
    // p1 的任务文件坏掉，p2 正常：一条坏项目不能吞掉其余项目。
    let broken = context.paths.tasks_file("p1").expect("path");
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    std::fs::write(&broken, b"{not json").unwrap();
    let good = context.paths.tasks_file("p2").expect("path");
    std::fs::create_dir_all(good.parent().unwrap()).unwrap();
    std::fs::write(
        &good,
        serde_json::to_vec_pretty(&serde_json::Value::Array(vec![json!({
            "id": "p2-old", "projectId": "p2", "status": "done",
            "createdAt": now - 40 * DAY_MS, "completedAt": now - 40 * DAY_MS
        })]))
        .unwrap(),
    )
    .unwrap();
    aeroric_core::storage::save_projects(
        &context,
        &[local_project("p1", "/repo"), local_project("p2", "/other")],
    )
    .expect("projects");

    let service = build_service(context, &executor);
    let run = run_cleanup_once(&service, &schedule(Mode::Interval, Some(0)), &Utc8, now);
    let CleanupRun::Ran {
        removed, errors, ..
    } = run
    else {
        panic!("expected a run");
    };
    assert_eq!(removed, vec!["p2-old".to_string()]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("p1:"), "{errors:?}");
    assert_eq!(CLEANUP_TICK_MS, 1_800_000, "旧的 React tick 是 30 分钟");
    cleanup(root);
}

// ── 通知簿记 ────────────────────────────────────────────────────────────────

fn notification(id: &str, created_at: &str) -> RemoteNotification {
    RemoteNotification {
        id: format!("release-{id}"),
        level: "info".into(),
        title: format!("Release {id}"),
        body: "notes".into(),
        body_zh: None,
        url: Some("https://example.com/release".into()),
        created_at: created_at.to_string(),
        expires_at: None,
        min_app_version: None,
        max_app_version: None,
        release_tag: Some(format!("v{id}")),
        newer_than_current: false,
        update_install_supported: false,
    }
}

#[test]
fn notification_throttle_uses_force_and_interval() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-09T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut store = NotificationStore::default();
    assert!(
        should_fetch(&store, false, now),
        "an empty ledger always fetches"
    );
    assert!(should_fetch(&store, true, now));

    apply_fetched_notifications(
        &mut store,
        vec![notification("1", "2026-09-08T00:00:00Z")],
        now,
    );
    assert!(!should_fetch(&store, false, now), "just fetched");
    assert!(
        should_fetch(&store, true, now),
        "force bypasses the throttle"
    );
    let later = now + chrono::Duration::seconds(FETCH_INTERVAL_SECS + 1);
    assert!(should_fetch(&store, false, later));

    // 坏时间戳不能变成永久静音。
    store.last_fetched_at = Some("not a timestamp".into());
    assert!(should_fetch(&store, false, now));
}

#[test]
fn apply_fetched_notifications_prunes_read_ids_of_gone_releases() {
    let now = chrono::Utc::now();
    let mut store = NotificationStore {
        read_ids: vec!["release-1".into(), "release-2".into()],
        ..Default::default()
    };
    apply_fetched_notifications(
        &mut store,
        vec![notification("1", "2026-09-08T00:00:00Z")],
        now,
    );
    assert_eq!(store.read_ids, vec!["release-1".to_string()]);
    assert_eq!(store.source.as_deref(), Some(aerobic_releases_url()));
}

fn aerobic_releases_url() -> &'static str {
    aeroric_core::maintenance::notifications::RELEASES_URL
}

#[test]
fn build_result_sorts_unread_and_sanitizes_urls() {
    let mut older = notification("1", "2026-09-01T00:00:00Z");
    older.url = Some("javascript:alert(1)".into());
    let newer = notification("2", "2026-09-08T00:00:00Z");
    let store = NotificationStore {
        read_ids: vec!["release-2".into()],
        cached_notifications: Some(vec![older, newer]),
        ..Default::default()
    };
    let result = build_result(&store, "1.0.0", "2026-09-09");
    assert_eq!(result.unread_count, 1);
    let ids: Vec<&str> = result
        .notifications
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(ids, vec!["release-2", "release-1"], "newest first");
    assert!(result.notifications[0].is_read);
    assert_eq!(
        result.notifications[1].url, None,
        "javascript: never reaches the UI"
    );
}

#[test]
fn is_valid_honours_expiry_and_version_window() {
    let mut item = notification("1", "2026-09-01T00:00:00Z");
    assert!(is_valid(&item, "1.0.0", "2026-09-09"));
    item.expires_at = Some("2026-09-08".into());
    assert!(!is_valid(&item, "1.0.0", "2026-09-09"));
    item.expires_at = None;
    item.min_app_version = Some("2.0.0".into());
    assert!(!is_valid(&item, "1.0.0", "2026-09-09"));
    item.min_app_version = None;
    item.max_app_version = Some("0.9.0".into());
    assert!(!is_valid(&item, "1.0.0", "2026-09-09"));
}

// ── 更新准备 ────────────────────────────────────────────────────────────────

fn asset(name: &str) -> GitHubReleaseAsset {
    GitHubReleaseAsset {
        name: name.to_string(),
        browser_download_url: format!(
            "https://github.com/Aho1ic/Aeroric/releases/download/v1/{name}"
        ),
    }
}

#[test]
fn release_to_notification_flags_newer_and_installability() {
    let release = GitHubRelease {
        id: json!("r1"),
        name: Some("Aeroric 9.9.9".into()),
        body: Some("notes".into()),
        tag_name: "v9.9.9".into(),
        html_url: "https://example.com".into(),
        prerelease: true,
        published_at: Some("2026-09-08T00:00:00Z".into()),
        draft: false,
        assets: vec![asset("Aeroric_9.9.9_aarch64.dmg"), asset("SHA256SUMS.txt")],
    };
    let item = release_to_notification(&release, "1.0.0", "macos", "aarch64", None);
    assert_eq!(item.id, "release-r1");
    assert!(item.title.ends_with(" · prerelease"), "{}", item.title);
    assert!(item.newer_than_current);
    assert!(
        item.update_install_supported,
        "arm64 dmg on an arm64 machine"
    );

    // 同一条 release 在没有 dmg 的机器上：能提示，但不能装。
    let unsupported = release_to_notification(&release, "1.0.0", "linux", "x86_64", Some("deb"));
    assert!(unsupported.newer_than_current);
    assert!(!unsupported.update_install_supported);

    // 不比当前新：两个标记都为假。
    let old = release_to_notification(&release, "9.9.9", "macos", "aarch64", None);
    assert!(!old.newer_than_current);
    assert!(!old.update_install_supported);
}

#[test]
fn compare_versions_and_release_version() {
    assert_eq!(release_version("v1.2.3"), "1.2.3");
    assert_eq!(release_version("V1.2.3"), "1.2.3");
    assert_eq!(release_version("1.2.3"), "1.2.3");
    assert_eq!(compare_versions("1.2.3", "1.2.4"), std::cmp::Ordering::Less);
    assert_eq!(compare_versions("1.2", "1.2.0"), std::cmp::Ordering::Equal);
    assert_eq!(
        compare_versions("2.0.0", "1.9.9"),
        std::cmp::Ordering::Greater
    );
    // 认不出的段按 0（旧行为：`ada.1` 不比 `0` 低）。
    assert_eq!(
        compare_versions("ada.1", "0.0"),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn select_release_installer_asset_by_platform() {
    let assets = vec![
        asset("Aeroric_1.0.0_aarch64.dmg"),
        asset("Aeroric_1.0.0_x64.dmg"),
        asset("Aeroric_1.0.0_x64-setup.exe"),
        asset("Aeroric_1.0.0_arm64-setup.exe"),
        asset("Aeroric_1.0.0_amd64.deb"),
        asset("Aeroric-1.0.0-1.x86_64.rpm"),
    ];
    let pick = |os, arch, kind| {
        select_release_installer_asset(&assets, os, arch, kind).map(|found| found.name.clone())
    };
    assert_eq!(
        pick("macos", "aarch64", None).as_deref(),
        Some("Aeroric_1.0.0_aarch64.dmg")
    );
    assert_eq!(
        pick("macos", "x86_64", None).as_deref(),
        Some("Aeroric_1.0.0_x64.dmg")
    );
    assert_eq!(
        pick("windows", "x86_64", None).as_deref(),
        Some("Aeroric_1.0.0_x64-setup.exe")
    );
    assert_eq!(
        pick("windows", "aarch64", None).as_deref(),
        Some("Aeroric_1.0.0_arm64-setup.exe")
    );
    assert_eq!(
        pick("linux", "x86_64", Some("deb")).as_deref(),
        Some("Aeroric_1.0.0_amd64.deb")
    );
    assert_eq!(
        pick("linux", "x86_64", Some("rpm")).as_deref(),
        Some("Aeroric-1.0.0-1.x86_64.rpm")
    );
    // 认不出的组合返回 None，而不是随便挑一个。
    assert_eq!(pick("linux", "aarch64", Some("deb")), None);
    assert_eq!(pick("freebsd", "x86_64", None), None);
}

#[test]
fn checksum_asset_must_be_present_and_named() {
    let installer = asset("Aeroric_1.0.0_x64.dmg");
    let with_sums = vec![installer.clone(), asset("SHA256SUMS.txt")];
    assert_eq!(
        find_checksum_for_asset(&installer, &with_sums).as_deref(),
        Ok("SHA256SUMS.txt")
    );
    let without = vec![installer.clone()];
    assert!(find_checksum_for_asset(&installer, &without).is_err());
    // 不认识的扩展名没有校验文件可言。
    assert_eq!(expected_release_digest_asset_name("notes.txt"), None);
    assert_eq!(
        expected_release_digest_asset_name("Aeroric_1.0.0_x64.dmg").as_deref(),
        Some("SHA256SUMS.txt")
    );

    assert!(valid_asset_filename("Aeroric_1.0.0_x64.dmg"));
    assert!(!valid_asset_filename("../escape.dmg"));
    assert!(!valid_asset_filename("/abs.dmg"));
    assert!(!valid_asset_filename(""));
}

#[test]
fn pending_update_result_preserves_the_public_reply_without_revalidation() {
    let pending = PendingReleaseUpdate {
        tag_name: "v1.2.3".into(),
        asset_name: "Aeroric_1.2.3_x64.dmg".into(),
        installer_path: "/fictional/updates/v1.2.3/Aeroric_1.2.3_x64.dmg".into(),
        checksum_path: "/fictional/updates/v1.2.3/SHA256SUMS.txt".into(),
        checksum_verified: false,
        helper_status: "failed".into(),
        error: Some("synthetic helper failure".into()),
    };

    for tag_name in [None, Some("v1.2.3")] {
        let result: ReleaseUpdatePrepareResult =
            pending_update_result(Some(pending.clone()), tag_name).expect("pending query result");
        assert_eq!(result.tag_name, pending.tag_name);
        assert_eq!(result.asset_name, pending.asset_name);
        assert_eq!(result.installer_path, pending.installer_path);
        // 查询的 ready 标记保留旧语义，不代表重新校验通过或获得安装许可。
        assert!(result.ready_to_restart);
        assert_eq!(result.checksum_verified, pending.checksum_verified);
        assert_eq!(result.helper_status, pending.helper_status);
        assert_eq!(result.error, pending.error);

        let value = serde_json::to_value(&result).expect("public pending query wire");
        assert_eq!(
            value,
            json!({
                "tagName": "v1.2.3",
                "assetName": "Aeroric_1.2.3_x64.dmg",
                "installerPath": "/fictional/updates/v1.2.3/Aeroric_1.2.3_x64.dmg",
                "readyToRestart": true,
                "checksumVerified": false,
                "helperStatus": "failed",
                "error": "synthetic helper failure",
            })
        );
        assert_eq!(value.as_object().unwrap().len(), 7);
        assert!(value.get("checksumPath").is_none());
    }
}

#[test]
fn pending_update_result_preserves_legacy_defaults_and_exact_optional_tags() {
    let payload: PendingReleaseUpdatePayload = serde_json::from_value(json!({
        "tagName": "v1.2.3",
        "assetName": "Aeroric_1.2.3_x64.dmg",
        "installerPath": "/fictional/updates/v1.2.3/Aeroric_1.2.3_x64.dmg",
    }))
    .expect("minimal legacy pending payload");
    let pending = payload.normalize();
    assert!(pending.checksum_path.is_empty());
    assert_eq!(pending.helper_status, "ready");
    assert!(!pending.checksum_verified);
    assert_eq!(pending.error, None);

    let expected = ReleaseUpdatePrepareResult {
        tag_name: pending.tag_name.clone(),
        asset_name: pending.asset_name.clone(),
        installer_path: pending.installer_path.clone(),
        ready_to_restart: true,
        checksum_verified: false,
        helper_status: "ready".into(),
        error: None,
    };
    for (tag_name, matches) in [
        (None, true),
        (Some("v1.2.3"), true),
        (Some("v9.9.9"), false),
        (Some(""), false),
        (Some("V1.2.3"), false),
        (Some(" v1.2.3"), false),
        (Some("v1.2.3 "), false),
        (Some(" v1.2.3 "), false),
    ] {
        assert_eq!(
            pending_update_result(Some(pending.clone()), tag_name),
            matches.then(|| expected.clone()),
            "tag must match exactly without trimming: {tag_name:?}"
        );
        assert!(
            pending_update_result(None, tag_name).is_none(),
            "no record must remain None for tag {tag_name:?}"
        );
    }
}

#[test]
fn validate_pending_installer_path_stays_inside_the_update_directory() {
    let (root, _context) = writer("maintenance-update");
    let updates = root.join("updates");
    let tag_dir = updates.join(safe_tag("v1.2.3"));
    std::fs::create_dir_all(&tag_dir).unwrap();
    let installer = tag_dir.join("Aeroric_1.0.0_x64.dmg");
    std::fs::write(&installer, b"dmg").unwrap();

    let resolved =
        validate_pending_installer_path(&updates, "v1.2.3", "Aeroric_1.0.0_x64.dmg", &installer)
            .expect("inside the update dir");
    assert_eq!(resolved.file_name().unwrap(), "Aeroric_1.0.0_x64.dmg");

    // 换个 tag 目录就不认。
    std::fs::create_dir_all(updates.join(safe_tag("v9.9.9"))).unwrap();
    let error =
        validate_pending_installer_path(&updates, "v9.9.9", "Aeroric_1.0.0_x64.dmg", &installer);
    assert!(error.is_err(), "{error:?}");

    // 资产名对不上也不认。
    let error = validate_pending_installer_path(&updates, "v1.2.3", "other.dmg", &installer);
    assert!(error.is_err(), "{error:?}");

    // tag 里的 `/` 与 `..` 被压成 `_`：目录名不可能穿出去。
    assert_eq!(safe_tag("../../etc"), ".._.._etc");
    cleanup(root);
}

#[test]
fn verify_downloaded_checksum_detects_tampering() {
    let (root, _context) = writer("maintenance-checksum");
    let installer = root.join("Aeroric_1.0.0_x64.dmg");
    std::fs::write(&installer, b"installer-bytes").unwrap();
    // "installer-bytes" 的 sha256。
    let sums =
        "b6b1a4b0a4e1d6a2c4c1d1e6f0b5a6e6f2c9d4a1b7e2c8f3a0d9e5b4c3a2f1e0  Aeroric_1.0.0_x64.dmg\n";

    let pending = PendingReleaseUpdate {
        tag_name: "v1.0.0".into(),
        asset_name: "Aeroric_1.0.0_x64.dmg".into(),
        installer_path: installer.to_string_lossy().into_owned(),
        checksum_path: root.join("SHA256SUMS.txt").to_string_lossy().into_owned(),
        checksum_verified: true,
        helper_status: UPDATE_HELPER_READY.into(),
        error: None,
    };
    // 记录说"验过"但文件哈希对不上：以实读为准。
    let error = verify_downloaded_checksum(&installer, &pending.asset_name, sums);
    assert!(error.is_err(), "{error:?}");

    let entries: Vec<String> = sums
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let _hash = parts.next().unwrap();
            parts.next().unwrap().to_string()
        })
        .collect();
    assert_eq!(entries, vec![pending.asset_name.clone()]);

    // 没有该资产的条目＝拒绝，而不是"跳过校验"。
    let error = verify_downloaded_checksum(&installer, "other.dmg", sums);
    assert!(error.is_err(), "{error:?}");

    // checksumVerified=false 的待装记录不能进安装路径。
    let mut unverified = pending.clone();
    unverified.checksum_verified = false;
    assert!(
        aeroric_core::maintenance::updates::verify_pending_update_files(&unverified, &installer)
            .is_err()
    );
    cleanup(root);
}

// ── 启动降级账 ──────────────────────────────────────────────────────────────

#[test]
fn startup_degradation_log_lists_what_was_recorded() {
    let log = StartupDegradationLog::new();
    assert!(log.is_empty());
    log.record("dbx-state", "disk full", ":memory:");
    log.record("local-router", "port busy", "/tmp/aeroric-fallback");
    let rows = log.list();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].component, "dbx-state");
    assert_eq!(rows[0].reason, "disk full");
    assert_eq!(rows[0].fallback, ":memory:");
    let json = serde_json::to_string(&rows[1]).unwrap();
    assert!(json.contains("\"component\""), "{json}");
    assert!(json.contains("\"fallback\""), "{json}");
}
