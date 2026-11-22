use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;
use crate::paths::AppPaths;

const NOW: u64 = 1_800_000_000_000;
const HOUR: u64 = 60 * 60 * 1000;

fn data_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-health-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_json(path: &Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// Every file under `dir` with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

fn delivery(id: &str, task: &str, state: &str, reason: Option<&str>, updated: u64) -> String {
    let mut body = json!({"id": id, "taskId": task, "kind": "user", "mode": "now",
        "label": "你的消息", "text": "继续", "state": state, "attempts": 1,
        "createdMs": updated, "updatedMs": updated});
    if let Some(reason) = reason {
        body["reason"] = json!(reason);
    }
    // The shape the queue writes.
    serde_json::from_value::<Delivery>(body.clone()).expect("a delivery");
    body.to_string()
}

fn fixture() -> (PathBuf, CoreContext) {
    let dir = data_dir();
    let context = CoreContext::read_only(AppPaths::with_data_dir("/", Some(&dir)));
    write_json(
        &context.paths.projects_file(),
        json!([{"id": "p1", "name": "网关", "path": "/tmp/p1"}]),
    );
    let failed = |id: &str, reason: &str, at: u64| json!({"id": id, "status": "failed", "failureReason": reason, "createdAt": at - 1000, "completedAt": at});
    write_json(
        &context.paths.tasks_file("p1").unwrap(),
        json!([
            failed("t1", "codex exited with code 137 after 12s", NOW - HOUR),
            failed("t2", "codex exited with code 137 after 340s", NOW - 2 * HOUR),
            failed("t3", "登录过期", NOW - 3 * HOUR),
            failed("old", "codex exited with code 137 after 1s", NOW - 30 * 24 * HOUR),
            {"id": "t4", "status": "done", "createdAt": NOW - HOUR},
        ]),
    );
    let conn = rusqlite::Connection::open(dir.join("delivery.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE deliveries (id TEXT PRIMARY KEY, task_id TEXT NOT NULL, state TEXT NOT NULL,
            updated_ms INTEGER NOT NULL, body TEXT NOT NULL);",
    )
    .unwrap();
    for (id, task, state, reason, updated) in [
        ("d1", "t5", "failed", Some("Agent 没装"), NOW - HOUR),
        ("d2", "t6", "waiting", None, NOW - HOUR),
        ("d3", "t7", "waiting", None, NOW - 60_000),
        ("d4", "t8", "delivered", None, NOW - HOUR),
    ] {
        conn.execute(
            "INSERT INTO deliveries VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                task,
                state,
                updated as i64,
                delivery(id, task, state, reason, updated)
            ],
        )
        .unwrap();
    }
    drop(conn);
    write_json(
        &dir.join("schedules.json"),
        json!({"rules": [], "runs": [
            {"id": "r1", "ruleId": "s1", "dueMs": NOW - HOUR, "atMs": NOW - HOUR,
             "outcome": "started", "taskId": "t9", "resultExpected": true,
             "resultError": "结果不符合结构：$ 缺少字段 status"},
            {"id": "r2", "ruleId": "s1", "dueMs": NOW, "atMs": NOW - 2 * HOUR,
             "outcome": "failed", "detail": "Agent 没装"},
            {"id": "r3", "ruleId": "s1", "dueMs": NOW, "atMs": NOW - HOUR, "outcome": "started", "taskId": "t10"}
        ]}),
    );
    (dir, context)
}

/// What changes between occurrences is taken out of the signature.
#[test]
fn a_signature_drops_what_changes_between_occurrences() {
    assert_eq!(
        signature("codex exited with code 137 after 12s"),
        signature("codex  exited with code 137 after 340s")
    );
    assert_eq!(signature("task 3f9a2c7e1b4d5a6f failed"), "task … failed");
    assert_ne!(signature("登录过期"), signature("额度用完"));
    assert!(signature(&"长".repeat(500)).chars().count() <= SIGNATURE_CHARS + 1);
}

/// Failed tasks, stuck or failed deliveries and failed schedule runs are
/// grouped, a repeat first; old ones and healthy ones are left out.
#[test]
fn the_scan_groups_what_went_wrong_and_puts_repeats_first() {
    let (_dir, context) = fixture();
    let report = scan(&context, NOW - 7 * 24 * HOUR, NOW);
    assert_eq!(
        report.scanned,
        Scanned {
            projects: 1,
            tasks: 5,
            deliveries: 4,
            schedule_runs: 3
        }
    );
    assert!(report.unreadable.is_empty(), "{:?}", report.unreadable);
    let first = &report.groups[0];
    assert_eq!((first.kind, first.count), (Kind::FailedTask, 2));
    assert_eq!(first.examples, ["t1", "t2"]);
    assert!(first.sample.contains("12s"), "the newest in full");
    let lines: Vec<(Kind, &str)> = report
        .groups
        .iter()
        .map(|g| (g.kind, g.signature.as_str()))
        .collect();
    assert!(lines.contains(&(Kind::FailedTask, "登录过期")));
    assert!(lines.contains(&(Kind::StuckDelivery, "发送失败：Agent 没装")));
    assert!(lines.contains(&(Kind::StuckDelivery, "长时间没有进展")));
    assert!(lines.contains(&(Kind::FailedScheduleRun, "Agent 没装")));
    assert!(lines
        .iter()
        .any(|(k, s)| *k == Kind::FailedScheduleRun && s.contains("缺少字段 status")));
    assert_eq!(report.groups.len(), 6, "{lines:?}");
    let text = text(&report);
    assert!(text.contains("反复出现 · 2 次"), "{text}");
    assert!(text.contains("没有调用模型"));
}

/// The scan reads; every file is as it was, and nothing was created.
#[test]
fn the_scan_changes_nothing_in_the_data_directory() {
    let (dir, context) = fixture();
    let before = snapshot(&dir);
    let report = scan(&context, 0, NOW);
    assert!(!report.groups.is_empty());
    assert_eq!(snapshot(&dir), before);
}

/// An empty data directory is a clean report, not an error.
#[test]
fn an_empty_data_directory_reports_nothing_wrong() {
    let dir = data_dir();
    let context = CoreContext::read_only(AppPaths::with_data_dir("/", Some(&dir)));
    let report = scan(&context, 0, NOW);
    assert_eq!(report.groups, []);
    assert!(text(&report).contains("没有发现"));
    assert!(snapshot(&dir).is_empty());
}
