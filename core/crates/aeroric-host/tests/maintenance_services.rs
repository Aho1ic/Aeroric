//! Task P8-04 刀三：维护面与用量族的 UDS 服务面。
//!
//! 这个套件要证明三件事，每一件都是"注册了但不可达"或"注册了但坏着"的反例：
//!
//! 1. **方法真的在集合里**：`usage.*` 六条与 `diagnostics.list_startup_degradations`
//!    无论哪种装配都在；`maintenance.*` 九条**只在给了 [`MaintenanceHost`] 时**在，
//!    没给就是 method-not-found（§3.1 #8 的反面：不许注册一个会假装工作的空壳）。
//! 2. **过了真 UDS 与真握手**：不是直接调函数——`HostServices::assemble` 被
//!    `serve` 装进 socket，客户端走 `handshake` → 请求 → 应答，形状与 CLI 一致。
//! 3. **`maintenance.*` 的编排真的落在注入的数据根上**：准备一份假的发布列表与
//!    下载落盘，断言 `updates/pending-release-update.json` 写在 data_dir 里，
//!    而"替换正在运行的 app"被明确拒绝（GUI 适配器的活）。

use std::sync::Arc;

use aeroric_core::maintenance::updates::TargetPlatform;
use aeroric_core::maintenance::{
    MaintenanceHost, MaintenanceService, NullProgress, ReportProgressSink, StartupDegradationLog,
    WeeklyReportProgress, WeeklyReportTask,
};
use aeroric_core::tasks::cleanup::LocalCalendar;
use aeroric_host::dispatcher::HostWiring;
use aeroric_protocol::envelope::Frame;
use serde_json::json;

mod host_harness;
use host_harness::Harness;

/// UTC+8 日历（与 core 侧用例同一个夹具；周报区间只关心本地日历分量）。
struct Utc8;

impl LocalCalendar for Utc8 {
    fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64) {
        let local = now_ms + 8 * 3_600_000;
        let day = local.div_euclid(86_400_000);
        let millis = local.rem_euclid(86_400_000);
        let (year, month, date) = civil_from_days(day);
        (
            (day + 4).rem_euclid(7) as u32,
            year,
            month,
            date,
            millis / 3_600_000,
        )
    }

    fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64 {
        days_from_civil(year, month, day) * 86_400_000 + hour * 3_600_000 - 8 * 3_600_000
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

/// 可控的宿主端口：发布列表、下载落盘、摘要都由用例给，网络与平台探测不参与。
struct FakeHost {
    data_dir: std::path::PathBuf,
    releases: Vec<aeroric_core::maintenance::GitHubRelease>,
    downloaded: std::sync::Mutex<Vec<String>>,
    assets: Vec<(String, Vec<u8>)>,
    summaries: std::sync::Mutex<usize>,
}

impl FakeHost {
    fn new(data_dir: &std::path::Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            releases: Vec::new(),
            downloaded: std::sync::Mutex::new(Vec::new()),
            assets: Vec::new(),
            summaries: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait::async_trait]
impl MaintenanceHost for FakeHost {
    fn app_version(&self) -> String {
        "1.0.0".to_string()
    }
    fn platform(&self) -> TargetPlatform<'static> {
        TargetPlatform {
            os: "macos",
            arch: "aarch64",
            linux_kind: None,
        }
    }
    fn calendar(&self) -> Arc<dyn LocalCalendar> {
        Arc::new(Utc8)
    }
    async fn fetch_releases(
        &self,
    ) -> Result<Vec<aeroric_core::maintenance::GitHubRelease>, String> {
        Ok(self.releases.clone())
    }
    async fn fetch_release(
        &self,
        tag: &str,
    ) -> Result<aeroric_core::maintenance::GitHubRelease, String> {
        self.releases
            .iter()
            .find(|release| release.tag_name == tag)
            .cloned()
            .ok_or_else(|| format!("no release {tag}"))
    }
    async fn download_release_assets(
        &self,
        _release: &aeroric_core::maintenance::GitHubRelease,
        asset_name: &str,
        tag_name: &str,
    ) -> Result<(String, String), String> {
        let dir = aeroric_core::maintenance::updates::release_update_dir(
            &self.data_dir.join("updates"),
            tag_name,
        );
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let mut installer = None;
        for (name, bytes) in &self.assets {
            let path = dir.join(name);
            std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
            if name == asset_name {
                installer = Some(path);
            }
        }
        self.downloaded.lock().unwrap().push(asset_name.to_string());
        let installer = installer.ok_or_else(|| format!("asset {asset_name} was not provided"))?;
        Ok((
            installer.to_string_lossy().into_owned(),
            dir.join("SHA256SUMS.txt").to_string_lossy().into_owned(),
        ))
    }
    async fn summarize(&self, task: &WeeklyReportTask, _locale: &str) -> Option<String> {
        if task.session_path.is_some() {
            *self.summaries.lock().unwrap() += 1;
            Some("host summary".to_string())
        } else {
            None
        }
    }
}

struct CountingProgress(std::sync::Mutex<usize>);

impl ReportProgressSink for CountingProgress {
    fn progress(&self, _update: &WeeklyReportProgress) {
        *self.0.lock().unwrap() += 1;
    }
}

/// 一份 sha256 与文件字节真实匹配的 SHA256SUMS.txt。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn release_fixture(data_dir: &std::path::Path) -> (FakeHost, Vec<u8>) {
    let installer: Vec<u8> = b"fake dmg payload".to_vec();
    let sums = format!("{}  Aeroric_9.9.9_aarch64.dmg\n", sha256_hex(&installer));
    let mut host = FakeHost::new(data_dir);
    host.assets = vec![
        ("Aeroric_9.9.9_aarch64.dmg".to_string(), installer.clone()),
        ("SHA256SUMS.txt".to_string(), sums.into_bytes()),
    ];
    host.releases = vec![aeroric_core::maintenance::GitHubRelease {
        id: json!(7),
        name: Some("Aeroric 9.9.9".into()),
        body: Some("notes".into()),
        tag_name: "v9.9.9".into(),
        html_url: "https://example.com/v9.9.9".into(),
        prerelease: false,
        published_at: Some("2026-09-08T00:00:00Z".into()),
        draft: false,
        assets: vec![
            aeroric_core::maintenance::GitHubReleaseAsset {
                name: "Aeroric_9.9.9_aarch64.dmg".into(),
                browser_download_url: "https://github.com/Aho1ic/Aeroric/releases/download/v9.9.9/Aeroric_9.9.9_aarch64.dmg".into(),
            },
            aeroric_core::maintenance::GitHubReleaseAsset {
                name: "SHA256SUMS.txt".into(),
                browser_download_url: "https://github.com/Aho1ic/Aeroric/releases/download/v9.9.9/SHA256SUMS.txt".into(),
            },
        ],
    }];
    (host, installer)
}

/// 起一个宿主（真 socket、真握手、真 serve 循环）；wiring 按数据根现造。
async fn hosted(tag: &str, wiring: impl FnOnce(&std::path::Path) -> HostWiring) -> Harness {
    Harness::start_with_wiring(tag, wiring).await
}

#[tokio::test]
async fn the_usage_and_diagnostics_methods_are_registered_without_any_wiring() {
    let harness = hosted("maint-default", |_dir| HostWiring::default()).await;
    for method in [
        "usage.read_session_metrics",
        "usage.read_usage_statistics",
        "usage.refresh_usage_statistics_index",
        "usage.record_agent_config_usage",
        "usage.load_agent_usage_snapshot",
        "usage.read_usage_snapshot",
        "diagnostics.list_startup_degradations",
    ] {
        assert!(
            harness.services.services.is_registered(method),
            "{method} 没注册"
        );
    }
    // 没给维护面端口 ⇒ 九条一个都不在（能力不存在的正确形状）。
    for method in [
        "maintenance.get_notifications",
        "maintenance.mark_notification_read",
        "maintenance.mark_all_notifications_read",
        "maintenance.get_pending_release_update",
        "maintenance.prepare_release_update",
        "maintenance.install_release_update",
        "maintenance.restart_and_install_release_update",
        "maintenance.generate_weekly_report",
    ] {
        assert!(
            !harness.services.services.is_registered(method),
            "{method} 不该在没有 MaintenanceHost 时注册"
        );
    }
}

#[tokio::test]
async fn the_agent_ledger_round_trips_over_the_socket() {
    let harness = hosted("maint-ledger", |_dir| HostWiring::default()).await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    let record = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "1",
            "usage.record_agent_config_usage",
            json!({"agent": "codex"}),
        ),
    )
    .await;
    let Frame::Success(success) = record else {
        panic!("record failed: {record:?}");
    };
    assert_eq!(success.result["agents"]["codex"]["totalCount"], json!(1));

    let snapshot = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("2", "usage.load_agent_usage_snapshot", json!({})),
    )
    .await;
    let Frame::Success(success) = snapshot else {
        panic!("snapshot failed: {snapshot:?}");
    };
    assert_eq!(success.result["agents"]["codex"]["recentCount"], json!(1));
    assert_eq!(success.result["windowDays"], json!(7));

    // 落点必须是这个宿主的 data dir（G21：不是进程里某个全局）。
    assert!(
        harness.data_dir.join("agent-usage.json").exists(),
        "账本没落在注入的数据根里：{:?}",
        harness.data_dir
    );

    // 空 id 是 invalid-argument，不是断连。
    let blank = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "3",
            "usage.record_agent_config_usage",
            json!({"agent": "  "}),
        ),
    )
    .await;
    assert!(matches!(blank, Frame::Failure(_)), "{blank:?}");
}

#[tokio::test]
async fn the_startup_degradation_log_is_readable_over_the_socket() {
    let log = Arc::new(StartupDegradationLog::new());
    log.record("dbx-state", "disk full", ":memory:");
    let harness = hosted("maint-degradations", |_dir| HostWiring {
        degradations: Arc::clone(&log),
        ..Default::default()
    })
    .await;
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;

    let reply = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("1", "diagnostics.list_startup_degradations", json!({})),
    )
    .await;
    let Frame::Success(success) = reply else {
        panic!("degradations failed: {reply:?}");
    };
    assert_eq!(success.result[0]["component"], json!("dbx-state"));
    assert_eq!(success.result[0]["fallback"], json!(":memory:"));
}

#[tokio::test]
async fn the_maintenance_methods_run_the_core_orchestration() {
    // 维护面端口要指向宿主自己的数据根，而数据根是 startup 的产物 ⇒ 用闭包现造。
    let harness = hosted("maint-hosted", |dir| {
        let (mut fake, _installer) = release_fixture(dir);
        fake.data_dir = dir.to_path_buf();
        HostWiring {
            maintenance: Some(Arc::new(fake)),
            ..Default::default()
        }
    })
    .await;

    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(harness
        .services
        .services
        .is_registered("maintenance.get_notifications"));

    // 1. 发布列表 → 通知项（新版本 + 有可装资产 ⇒ 两个标记都为真）。
    let notifications = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("1", "maintenance.get_notifications", json!({"force": true})),
    )
    .await;
    let Frame::Success(success) = notifications else {
        panic!("notifications failed: {notifications:?}");
    };
    assert_eq!(success.result["unreadCount"], json!(1), "{success:?}");
    assert_eq!(success.result["notifications"][0]["id"], json!("release-7"));
    assert_eq!(
        success.result["notifications"][0]["updateInstallSupported"],
        json!(true),
        "arm64 dmg 在这台机器上可装"
    );
    assert!(
        harness.data_dir.join("notifications.json").exists(),
        "账本没落在注入的数据根里"
    );

    // 2. 标记已读后未读数归零。
    let marked = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "2",
            "maintenance.mark_notification_read",
            json!({"id": "release-7"}),
        ),
    )
    .await;
    assert!(matches!(marked, Frame::Success(_)), "{marked:?}");
    let after = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("3", "maintenance.get_notifications", json!({})),
    )
    .await;
    let Frame::Success(success) = after else {
        panic!("notifications failed: {after:?}");
    };
    assert_eq!(success.result["unreadCount"], json!(0));

    // 3. 准备更新：下载 + 校验 + 记 pending，全落在注入的 data dir 里。
    let prepared = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "4",
            "maintenance.prepare_release_update",
            json!({"tagName": "v9.9.9"}),
        ),
    )
    .await;
    let Frame::Success(success) = prepared else {
        panic!("prepare failed: {prepared:?}");
    };
    assert_eq!(success.result["readyToRestart"], json!(true), "{success:?}");
    assert_eq!(success.result["checksumVerified"], json!(true));
    assert!(
        harness
            .data_dir
            .join("updates/pending-release-update.json")
            .exists(),
        "待装记录没落在注入的数据根里"
    );

    // 4. 待装查询能看到它；换个 tag 就看不到。
    let pending = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "5",
            "maintenance.get_pending_release_update",
            json!({"tagName": "v9.9.9"}),
        ),
    )
    .await;
    let Frame::Success(success) = pending else {
        panic!("pending failed: {pending:?}");
    };
    assert_eq!(success.result["tagName"], json!("v9.9.9"));
    let other = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "6",
            "maintenance.get_pending_release_update",
            json!({"tagName": "v1.0.0"}),
        ),
    )
    .await;
    let Frame::Success(success) = other else {
        panic!("pending failed: {other:?}");
    };
    assert_eq!(success.result, json!(null));

    // 5. 替换运行中的 app 是 GUI 适配器的活：无头宿主明确拒绝，且带上正确的错误码。
    for method in [
        "maintenance.install_release_update",
        "maintenance.restart_and_install_release_update",
    ] {
        let refused = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request("7", method, json!({"tagName": "v9.9.9"})),
        )
        .await;
        let Frame::Failure(failure) = refused else {
            panic!("{method} 应被拒绝: {refused:?}");
        };
        assert_eq!(
            failure.error.code,
            aeroric_protocol::error::ErrorCode::UnsupportedCapability
        );
    }

    // 6. 周报：请求体→文件，摘要走端口（core 不 spawn agent）。
    let window = aeroric_core::maintenance::week_window(&Utc8, 1_789_000_000_000, 1, 0);
    let target = harness.data_dir.join("weekly.md");
    let report = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "8",
            "maintenance.generate_weekly_report",
            json!({
                "request": {
                    "fromMs": window.from_ms,
                    "toMs": window.to_ms,
                    "weekLabel": "2026-W37",
                    "locale": "zh-CN",
                    "outputPath": target.to_string_lossy(),
                    "tasks": [{
                        "projectName": "Alpha",
                        "projectPath": "/repo",
                        "title": "Task 1",
                        "agent": "claude",
                        "status": "done",
                        "createdAt": 1_789_000_000_000i64,
                        "sessionPath": "/tmp/s1.jsonl"
                    }]
                }
            }),
        ),
    )
    .await;
    let Frame::Success(success) = report else {
        panic!("report failed: {report:?}");
    };
    // `validate_export_output_path` 会 canonicalize（macOS 上 `/tmp` → `/private/tmp`），
    // 所以比尾部而不是整串。
    let returned = success.result.as_str().expect("path");
    assert!(returned.ends_with("weekly.md"), "{returned}");
    let markdown = std::fs::read_to_string(&target).expect("report file");
    assert!(markdown.contains("host summary"), "{markdown}");
    assert!(markdown.contains("2026-W37"), "{markdown}");

    // 7. 坏请求是 invalid-argument，不是断连。
    let broken = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "9",
            "maintenance.generate_weekly_report",
            json!({"request": {"fromMs": "x"}}),
        ),
    )
    .await;
    assert!(matches!(broken, Frame::Failure(_)), "{broken:?}");
}

/// 直连编排层（不经 socket）也钉一遍：`NullProgress` 与真实 progress sink 的差别。
#[tokio::test]
async fn the_weekly_report_orchestration_is_usable_without_a_socket() {
    let root = std::path::PathBuf::from("/tmp").join(format!(
        "ah-maint-direct-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&root).expect("root");
    let (host, _installer) = release_fixture(&root);
    let service = MaintenanceService::new(Arc::new(host), root.clone());

    assert!(service.pending_update().is_none());
    let result = service
        .get_notifications(true)
        .await
        .expect("notifications");
    assert_eq!(result.unread_count, 1);

    // 摘要计数只在有 sessionPath 时递增：两条任务只有一条被摘要。
    let window = aeroric_core::maintenance::week_window(&Utc8, 1_789_000_000_000, 1, 0);
    let request = aeroric_core::maintenance::WeeklyReportRequest {
        from_ms: window.from_ms,
        to_ms: window.to_ms,
        week_label: String::new(),
        locale: "en".to_string(),
        output_path: root.join("report-en.md").to_string_lossy().into_owned(),
        tasks: vec![
            serde_json::from_value(json!({
                "projectName": "Alpha", "projectPath": "/repo", "title": "One",
                "agent": "claude", "status": "done", "createdAt": window.from_ms + 1000,
                "sessionPath": "/tmp/a.jsonl"
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "projectName": "Alpha", "projectPath": "/repo", "title": "Two",
                "agent": "claude", "status": "done", "createdAt": window.from_ms + 2000
            }))
            .unwrap(),
        ],
    };
    let progress = CountingProgress(std::sync::Mutex::new(0));
    let path = service
        .write_weekly_report(request, &progress)
        .await
        .expect("report");
    assert!(path.ends_with("report-en.md"), "{path}");
    let markdown = std::fs::read_to_string(&path).expect("markdown");
    // 空 week_label ⇒ 由区间算出标题。
    assert!(markdown.starts_with("# 2026-W"), "{markdown}");
    assert!(markdown.contains("host summary"), "{markdown}");
    assert_eq!(*progress.0.lock().unwrap(), 3, "两条任务 + 收尾一条进度");

    // 空任务清单是明确错误。
    let empty = aeroric_core::maintenance::WeeklyReportRequest {
        tasks: Vec::new(),
        ..serde_json::from_value::<aeroric_core::maintenance::WeeklyReportRequest>(json!({
            "fromMs": window.from_ms, "toMs": window.to_ms, "weekLabel": "2026-W37",
            "locale": "en", "outputPath": root.join("x.md").to_string_lossy(), "tasks": []
        }))
        .unwrap()
    };
    assert!(service
        .write_weekly_report(empty, &NullProgress)
        .await
        .is_err());

    std::fs::remove_dir_all(root).ok();
}
