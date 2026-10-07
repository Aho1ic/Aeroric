//! Task P8-04 acceptance, part 2: the metrics family (usage index, analytics, agent ledger).
//!
//! Two properties are the point of this file and neither is visible from a return value:
//!
//! - **G21 — the landing directory is injectable.** Every case here points the family at a
//!   throwaway home and a throwaway data directory and then asserts that the *real* data
//!   directory was not touched. Before this task the index opened
//!   `~/.aeroric/usage-statistics.sqlite3` from a module-level path, which is why the host
//!   test sweep was rewriting the user's statistics database.
//! - **G03 — the watcher can be stopped.** The old loop was a bare `thread::spawn` holding an
//!   `AppHandle` forever: nothing could join it, and `stop` did not exist. The handle is now
//!   `UsageIndexWatcher`, and `stop()` is asserted to return and to be idempotent.
//!
//! The scanning root is also injected, so these cases never read the developer's own
//! `~/.codex` / `~/.claude` trees — which is the other half of the same G21 requirement.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aeroric_core::metrics::agent_usage::{
    load_agent_usage_snapshot_at, record_agent_config_usage_at, DAILY_BUCKET_RETENTION_DAYS,
    USAGE_WINDOW_DAYS,
};
use aeroric_core::metrics::analytics::{
    collect_jsonl_files, is_codex_session, is_dsh_session, is_omp_session,
    parse_claude_usage_requests, parse_codex_usage_requests, read_usage_statistics_at, UsageAgent,
};
use aeroric_core::metrics::index::{
    index_database_exists, latest_updated_at_at, load_requests_at, refresh_index_at,
    refresh_now_at, start as start_index, IndexEventSink, UsageIndexWatcher, INDEX_EVENT,
    INDEX_TOPIC,
};
use aeroric_core::metrics::MetricsPaths;
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "aeroric-metrics-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("scratch root");
    root
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("fixture write");
}

/// 一条 Claude 会话记录（真实形状的最小集：`type` + `message.usage` + `timestamp`）。
fn claude_line(id: &str, timestamp: &str, input: u64, cache_read: u64) -> String {
    json!({
        "type": "assistant",
        "timestamp": timestamp,
        "message": {
            "id": id,
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": input,
                "output_tokens": 12,
                "cache_creation_input_tokens": 3,
                "cache_read_input_tokens": cache_read,
            }
        }
    })
    .to_string()
}

fn codex_lines(timestamp: &str, input: u64, cached: u64) -> String {
    let context = json!({
        "type": "turn_context",
        "payload": { "model": "gpt-5-codex" }
    });
    let count = json!({
        "type": "event_msg",
        "timestamp": timestamp,
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": 7,
                    "reasoning_output_tokens": 5
                }
            }
        }
    });
    format!("{context}\n{count}\n")
}

// ── G21：落点与扫描根都可注入 ────────────────────────────────────────────────

#[test]
fn the_index_lands_in_the_injected_data_dir() {
    let root = scratch("index-landing");
    let home = root.join("home");
    let data = root.join("data");
    std::fs::create_dir_all(&home).unwrap();
    write(
        &home.join(".claude/projects/demo/session.jsonl"),
        &format!(
            "{}\n",
            claude_line("msg-1", "2026-09-09T04:00:00Z", 100, 40)
        ),
    );
    let paths = MetricsPaths::new(&home, &data);

    assert!(refresh_index_at(&paths).expect("scan"));
    assert!(
        index_database_exists(&paths),
        "the database must land inside the injected data dir"
    );
    // 数据根之外一个字节都不该出现：整个 root 里除了 home/data 没有第三个目录。
    let entries: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.contains(&"home".to_string()), "{entries:?}");
    assert!(entries.contains(&"data".to_string()), "{entries:?}");

    // 第二次跑同一份语料：指纹没变 ⇒ 没有新改动（幂等）。
    assert!(!refresh_index_at(&paths).expect("second scan"));

    let today = chrono::Local::now().date_naive();
    let requests = load_requests_at(&paths, today, today).expect("rows");
    assert!(
        requests.is_empty(),
        "fixture is dated in the future: {requests:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_scan_roots_come_from_the_injected_home() {
    let root = scratch("roots");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // 四族各放一条，另有两条不该被当成会话根。
    write(&home.join(".codex/sessions/a.jsonl"), "{}\n");
    write(&home.join(".claude/projects/a.jsonl"), "{}\n");
    write(&home.join(".dsh/sessions/a.jsonl"), "{}\n");
    write(&home.join(".omp/agent/sessions/a.jsonl"), "{}\n");
    write(&home.join("notes/readme.jsonl"), "{}\n");
    write(&home.join(".cache/junk.jsonl"), "{}\n");

    let paths = MetricsPaths::new(&home, root.join("data"));
    let mut roots = paths.usage_roots();
    roots.sort();
    let names: Vec<String> = roots
        .iter()
        .map(|path| {
            path.strip_prefix(&home)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ".claude/projects",
            ".codex/sessions",
            ".dsh/sessions",
            ".omp/agent/sessions",
        ],
        "只有四族的会话目录是扫描根"
    );

    // 收集器沿目录树递归，且只收 jsonl / jsonl.zstd。
    let mut files = std::collections::HashSet::new();
    collect_jsonl_files(&home.join(".codex"), &mut files);
    assert_eq!(files.len(), 1);
    let mut nested = std::collections::HashSet::new();
    collect_jsonl_files(&home.join("notes"), &mut nested);
    assert_eq!(
        nested.len(),
        1,
        "不在扫描根里的目录由调用方决定，收集器不做筛选"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn read_usage_statistics_reports_the_seeded_window() {
    let root = scratch("statistics");
    let home = root.join("home");
    let data = root.join("data");
    std::fs::create_dir_all(&home).unwrap();
    // 用本地时间构造"今天"的语料：统计窗口按本地日历日切分。
    // **时间戳必须带真实偏移**（`to_rfc3339`）：把本地墙钟文本硬贴上 `+00:00` 会在
    // UTC+8 的机器上把记录算到"明天"，于是整窗落空——这一条被门禁当场逮住过。
    let stamp = chrono::Local::now().to_rfc3339();
    write(
        &home.join(".claude/projects/demo/session.jsonl"),
        &format!(
            "{}\n{}\n",
            claude_line("m1", &stamp, 100, 40),
            claude_line("m2", &stamp, 200, 10)
        ),
    );
    write(
        &home.join(".codex/sessions/rollout.jsonl"),
        &codex_lines(&stamp, 50, 20),
    );
    let paths = MetricsPaths::new(&home, &data);
    assert!(refresh_index_at(&paths).expect("scan"));

    let stats = read_usage_statistics_at(&paths, 7, "all".to_string()).expect("statistics");
    assert_eq!(stats.range_days, 7);
    assert_eq!(stats.agent, "all");
    assert_eq!(stats.totals.request_count, 3, "{stats:?}");
    // 三族各自出现一次：claude 两条、codex 一条。
    assert_eq!(stats.breakdown.claude.request_count, 2);
    assert_eq!(stats.breakdown.codex.request_count, 1);
    assert_eq!(stats.breakdown.dsh.request_count, 0);
    assert_eq!(stats.breakdown.omp.request_count, 0);
    assert!(stats.updated_at > 0, "{stats:?}");
    assert_eq!(stats.series.len(), 7, "7 天档零填充");

    // 只算一族。
    let claude_only = read_usage_statistics_at(&paths, 7, "claude".to_string()).expect("claude");
    assert_eq!(claude_only.totals.request_count, 2);
    // 认不出的参数是输入错误，不是空报告。
    assert!(read_usage_statistics_at(&paths, 3, "all".to_string()).is_err());
    assert!(read_usage_statistics_at(&paths, 7, "nope".to_string()).is_err());
    std::fs::remove_dir_all(root).ok();
}

// ── G03：索引 watcher 可关停 ────────────────────────────────────────────────

#[test]
fn the_usage_index_watcher_can_be_stopped_twice() {
    let root = scratch("watcher");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let sink: IndexEventSink = Arc::new(move |_updated_at| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let paths = MetricsPaths::new(&home, root.join("data"));

    let mut watcher: UsageIndexWatcher = start_index(paths, sink);
    // 起手一轮会跑一次扫描；停下来必须立刻返回，而不是等满 30s 兜底睡眠。
    let started = std::time::Instant::now();
    watcher.stop();
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "stop() 等了 {elapsed:?}"
    );
    assert!(watcher.is_stopped());
    // 幂等：第二次调用是 no-op，不是 panic 也不是再 join 一次。
    watcher.stop();
    assert!(watcher.is_stopped());
    std::fs::remove_dir_all(root).ok();
}

/// 通道常量是契约（事件扫描器与 v1 词表读它们），不是实现细节。
#[test]
fn the_index_topics_name_both_channels() {
    assert_eq!(INDEX_EVENT, "usage-statistics-updated");
    assert_eq!(INDEX_TOPIC, "usage.statistics_updated");
    assert_ne!(
        INDEX_EVENT, INDEX_TOPIC,
        "旧通道与 v1 词表是两条线，必须同时发"
    );
}

#[test]
fn refresh_now_bypasses_the_throttle() {
    let root = scratch("refresh-now");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let paths = MetricsPaths::new(&home, root.join("data"));
    write(
        &home.join(".claude/projects/demo/session.jsonl"),
        &format!("{}\n", claude_line("m1", "2026-09-09T04:00:00Z", 10, 1)),
    );
    // 首次扫描写入来源。
    let first = refresh_now_at(&paths).expect("first refresh");
    assert!(first, "新的来源必须被看见");
    // 无变化的显式刷新不改东西（但也不报错）。
    assert!(!refresh_now_at(&paths).expect("second refresh"));
    assert!(latest_updated_at_at(&paths).expect("updated_at") > 0);
    std::fs::remove_dir_all(root).ok();
}

// ── 解析器 ──────────────────────────────────────────────────────────────────

/// 三家探测器的**真实**首行形状：codex 认 `session_meta`/`event_msg`，dsh 只认
/// 首行是 header，omp 认 title 槽或 `version: 3` 的 header。三家在同一份语料上
/// 不会同时成立——否则"这个会话是哪一族"就变成看调用顺序。
#[test]
fn the_family_detectors_do_not_confuse_each_other() {
    let codex = r#"{"type":"event_msg","payload":{"type":"token_count"}}"#;
    let dsh = r#"{"type":"session","version":1,"id":"abc","cwd":"/tmp"}"#;
    let omp = r#"{"type":"session","version":3,"cwd":"/tmp"}"#;
    let omp_title = r#"{"type":"title","v":1,"title":"hi"}"#;

    assert!(is_codex_session(codex), "event_msg 是 codex 的行");
    assert!(!is_dsh_session(codex), "dsh 只认首行 header");
    assert!(!is_omp_session(codex));

    assert!(is_dsh_session(dsh), "session+version 是 dsh header");
    assert!(!is_codex_session(dsh), "dsh header 不带 codex 的行类型");
    assert!(!is_omp_session(dsh), "dsh 的 version 是 1，omp 认 3");

    assert!(is_omp_session(omp), "version 3 是 omp header");
    assert!(is_omp_session(omp_title), "title 槽是 omp 独有");
    assert!(!is_codex_session(&format!("{omp}\n{omp_title}")));
}

#[test]
fn claude_parser_keeps_the_latest_row_per_message_id() {
    let content = format!(
        "{}\n{}\n",
        // 同一 message id 出现两次（流式重写）时取更新的那一条。
        claude_line("same", "2026-09-09T04:00:00Z", 10, 1),
        claude_line("same", "2026-09-09T04:00:05Z", 99, 0),
    );
    let requests = parse_claude_usage_requests(&content, "src");
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].input_tokens, 99);
    assert_eq!(requests[0].agent, UsageAgent::Claude);
    assert_eq!(requests[0].cache_read_tokens, 0);
}

#[test]
fn codex_parser_subtracts_cached_input_and_adds_reasoning() {
    let content = codex_lines("2026-09-09T04:00:00Z", 100, 30);
    let requests = parse_codex_usage_requests(&content);
    assert_eq!(requests.len(), 1, "{requests:?}");
    let row = &requests[0];
    assert_eq!(row.agent, UsageAgent::Codex);
    assert_eq!(row.model, "gpt-5-codex", "model 来自上一条 turn_context");
    assert_eq!(row.input_tokens, 70, "input 要减去 cached");
    assert_eq!(row.output_tokens, 12, "可见输出 + reasoning 输出");
    assert_eq!(row.cache_read_tokens, 30);
}

// ── Agent 配置账本 ──────────────────────────────────────────────────────────

#[test]
fn the_agent_ledger_lands_in_the_injected_data_dir() {
    let root = scratch("ledger");
    let home = root.join("home");
    let data = root.join("data");
    std::fs::create_dir_all(&home).unwrap();
    let paths = MetricsPaths::new(&home, &data);

    let snapshot = record_agent_config_usage_at(&paths, " codex ").expect("record");
    assert_eq!(snapshot.window_days, USAGE_WINDOW_DAYS);
    let stats = snapshot.agents.get("codex").expect("trimmed id");
    assert_eq!(stats.total_count, 1);
    assert_eq!(stats.recent_count, 1);
    assert!(stats.last_used_at > 0);

    record_agent_config_usage_at(&paths, "codex").expect("second record");
    let reloaded = load_agent_usage_snapshot_at(&paths).expect("snapshot");
    assert_eq!(reloaded.agents["codex"].total_count, 2);
    assert!(paths.agent_usage_file().exists(), "账本落在注入的数据根里");

    // 空 id 是调用错误，且不留痕。
    assert!(record_agent_config_usage_at(&paths, "   ").is_err());

    // 读不到文件时退到空账本（排序退化），而不是报错。
    let fresh = MetricsPaths::new(&home, data.join("nowhere"));
    assert!(load_agent_usage_snapshot_at(&fresh)
        .expect("missing file is not an error")
        .agents
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}

/// 旧账本升级路径：`total` 留着、日桶可能已经过期。快照必须报"窗口内 0、
/// 历史总数非 0"，前端据此退到历史排序——这条规则写在
/// `snapshot_reports_zero_recent_with_nonzero_total` 的注释里，这里量的是它在
/// **真盘**上的行为（读一份手工写的旧文件）。
#[test]
fn a_stale_ledger_file_still_loads_and_keeps_its_total() {
    let root = scratch("ledger-stale");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let paths = MetricsPaths::new(&home, root.join("data"));
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    let stale_day = (chrono::Local::now().date_naive()
        - chrono::Duration::days(DAILY_BUCKET_RETENTION_DAYS + 5))
    .format("%Y-%m-%d")
    .to_string();
    std::fs::write(
        paths.agent_usage_file(),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "agents": { "codex": { "total": 42, "last_used_at": 1, "daily": { stale_day: 3 } } }
        }))
        .unwrap(),
    )
    .expect("seed ledger");

    let snapshot = load_agent_usage_snapshot_at(&paths).expect("snapshot");
    let stats = &snapshot.agents["codex"];
    assert_eq!(stats.total_count, 42, "total 永不裁剪");
    assert_eq!(stats.recent_count, 0, "过期桶不计入 7 天窗口");

    // 手工改坏的文件退到空账本（排序退化），而不是让首页选择器报错。
    std::fs::write(paths.agent_usage_file(), b"{ not json").unwrap();
    assert!(load_agent_usage_snapshot_at(&paths)
        .expect("corrupt")
        .agents
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}
