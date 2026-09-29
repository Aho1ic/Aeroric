//! P3-04 第六片金样（验收名单 `tests/hook_reader.rs`）：
//! Hook 事件路径布局＋`HookEventReader` 读侧语义。
//!
//! 判据来自 `sessions.md` Action 3 与宿主原文注释：保留 partial JSONL、截断、
//! 轮转、重复 SessionStart、claimed path 排他——**本片上收的是读侧四语义**；
//! 重复 SessionStart 去重的 already 判定与 emit 节流留宿主 dispatch（AppHandle
//! 地盘），claimed 排他锁在 core TaskRegistry（手写约束 4：路径粒度是 ADR-020
//! §3 的例外，本测试不重开它）。错误文案 `Invalid task id` 与 pty 在网契约同源。

use aeroric_core::sessions::hooks::{
    events_dir_for, events_root, hook_script_path, hooks_dir, parse_events_and_offset,
    read_hook_events, HookEvent, HookEventReader,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-hook-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn ev(task: &str, event: &str) -> String {
    format!("{{\"task_id\":\"{task}\",\"event\":\"{event}\"}}")
}

#[test]
fn layout_anchors_are_home_injected_and_pinned() {
    let home = Path::new("/tmp/fixture-home");
    assert_eq!(
        hooks_dir(home),
        PathBuf::from("/tmp/fixture-home/.aeroric/hooks")
    );
    assert_eq!(
        hook_script_path(home),
        PathBuf::from("/tmp/fixture-home/.aeroric/hooks/aeroric-hook.mjs")
    );
    assert_eq!(
        events_root(home),
        PathBuf::from("/tmp/fixture-home/.aeroric/events")
    );
    assert_eq!(
        events_dir_for(home, "task-01_A").unwrap(),
        PathBuf::from("/tmp/fixture-home/.aeroric/events/task-01_A")
    );
}

#[test]
fn task_id_gate_rejects_the_whole_traversal_alphabet() {
    let home = Path::new("/tmp/fixture-home");
    for bad in [
        "", "   ", "../evil", "a/b", "a\\b", "a;b", "$(id)", "a b", ".", "..", "id.json", "项目",
        "id\u{0}",
    ] {
        assert_eq!(
            events_dir_for(home, bad).unwrap_err(),
            "Invalid task id",
            "拒绝 {bad:?} 的文案必须是 pty 在网契约"
        );
    }
}

#[test]
fn parse_advances_only_over_complete_lines() {
    let mut buf = format!("{}\n", ev("t1", "SessionStart")).into_bytes();
    let tail = format!("{}\n{}", ev("t2", "Stop"), "{\"task_id\":\"t3\"");
    buf.extend_from_slice(tail.as_bytes());
    let (events, offset) = parse_events_and_offset(&buf, 0);
    assert_eq!(
        events
            .iter()
            .map(|e| e.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["t1", "t2"],
        "残行不消费"
    );
    let complete_len = buf.len() - "{\"task_id\":\"t3\"".len();
    assert_eq!(offset, complete_len as u64, "offset 只到最后一个完整行末");

    // 非零 offset 的推进量是相对本次读到的字节（宿主 `offset + last_complete_end` 原文）。
    let (none, same) = parse_events_and_offset(b"", 77);
    assert!(none.is_empty());
    assert_eq!(same, 77);

    // 空行与纯空白行跳过但仍计入推进。
    let (events, offset) = parse_events_and_offset(b"\n  \n", 3);
    assert!(events.is_empty());
    assert_eq!(offset, 3 + 4, "「\\n  \\n」全长按 4 字节推进");
}

#[test]
fn invalid_utf8_line_is_skipped_without_stalling_the_stream() {
    let mut buf = format!("{}\n", ev("t1", "UserPromptSubmit")).into_bytes();
    buf.extend_from_slice(&[0xFF, 0xFE, b'\n']);
    buf.extend_from_slice(format!("{}\n", ev("t2", "Stop")).as_bytes());
    let (events, offset) = parse_events_and_offset(&buf, 0);
    assert_eq!(events.len(), 2);
    assert_eq!(offset, buf.len() as u64, "坏行照样推进");

    // 端到端：读文件路径（read_hook_events）同样不卡住。
    let dir = scratch("utf8");
    let file = dir.join("events.jsonl");
    fs::write(&file, &buf).unwrap();
    let (events, next) = read_hook_events(&file, 0).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(next, buf.len() as u64);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn file_growth_truncation_and_rotation_offsets_follow_host_semantics() {
    let dir = scratch("growth");
    let file = dir.join("events.jsonl");
    fs::write(&file, format!("{}\n", ev("t1", "SessionStart")).as_bytes()).unwrap();
    let (events, o1) = read_hook_events(&file, 0).unwrap();
    assert_eq!(events.len(), 1);

    // 未增长：返回旧 offset、零事件。
    let (events, same) = read_hook_events(&file, o1).unwrap();
    assert!(events.is_empty());
    assert_eq!(same, o1);

    // 追加：只有新行被交付一次。
    {
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(f, "{}", ev("t1", "Stop")).unwrap();
    }
    let (events, o2) = read_hook_events(&file, o1).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "Stop");
    assert!(o2 > o1);

    // 截断/轮转成更小的文件：offset **不回头**（size <= offset 原样返回）——
    // 这是逐字保留的既有语义；消失-再出现的重置由扫描器的 seen 清理承担。
    fs::write(&file, b"").unwrap();
    let (events, held) = read_hook_events(&file, o2).unwrap();
    assert!(events.is_empty());
    assert_eq!(held, o2);

    let missing = dir.join("gone.jsonl");
    assert!(
        read_hook_events(&missing, 0).is_none(),
        "读不到 ⇒ None，offset 不动"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scan_delivers_each_complete_line_exactly_once_across_tasks() {
    let home = scratch("scan");
    let root = events_root(&home);
    let a = root.join("task-a");
    let b = root.join("task-b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(
        a.join("events.jsonl"),
        format!(
            "{}\n{}\n",
            ev("task-a", "SessionStart"),
            ev("task-a", "Stop")
        ),
    )
    .unwrap();
    fs::write(
        b.join("events.jsonl"),
        format!("{}\n", ev("task-b", "Stop")),
    )
    .unwrap();
    // 非目录条目与没有 events.jsonl 的子目录都被跳过。
    fs::write(root.join("loose.jsonl"), b"{}\n").unwrap();
    fs::create_dir_all(root.join("empty-task")).unwrap();

    let mut reader = HookEventReader::new(root.clone());
    let mut first = reader.scan();
    assert_eq!(first.len(), 3);
    // 跨文件顺序不作承诺（read_dir 序），但文件内顺序必须保留。
    first.sort_by(|x, y| (&x.task_id, &x.event).cmp(&(&y.task_id, &y.event)));
    assert_eq!(first[0].task_id, "task-a");
    assert_eq!(first[0].event, "SessionStart", "文件内行序不得颠倒");
    assert_eq!(first[1].event, "Stop");
    assert_eq!(first[2].task_id, "task-b");

    assert!(reader.scan().is_empty(), "第二轮零重放");
    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(a.join("events.jsonl"))
            .unwrap();
        writeln!(f, "{}", ev("task-a", "UserPromptSubmit")).unwrap();
    }
    let third = reader.scan();
    assert_eq!(
        third.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(),
        vec!["UserPromptSubmit"],
        "只有新增完整行被交付"
    );

    // 消失 ⇒ offset 清除；同任务目录重建（轮转到新文件）从 0 重读。
    fs::remove_dir_all(&a).unwrap();
    assert!(reader.scan().iter().all(|e| e.task_id != "task-a"));
    fs::create_dir_all(&a).unwrap();
    fs::write(
        a.join("events.jsonl"),
        format!("{}\n", ev("task-a", "SessionStart")),
    )
    .unwrap();
    let revived = reader.scan();
    assert_eq!(revived.len(), 1);
    assert_eq!(revived[0].event, "SessionStart", "重建文件必须从 0 起读");

    // 读不到文件（read_dir 与 open 之间消失）不恐慌：留下旧 offset 语义由
    // is_file→None 分支覆盖，此处断言 scan 对损坏根目录返回空而不是崩。
    drop(fs::remove_dir_all(&root));
    assert!(reader.scan().is_empty());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn wire_fields_keep_the_snake_case_shape_with_codex_like_tristate() {
    let legacy: HookEvent = serde_json::from_str(r#"{"agent":"codex"}"#).unwrap();
    assert_eq!(legacy.task_id, "");
    assert_eq!(legacy.codex_like, None);
    let explicit: HookEvent =
        serde_json::from_str(r#"{"agent":"custom-codex-wrapper","codex_like":true}"#).unwrap();
    assert_eq!(explicit.codex_like, Some(true));
    let full: HookEvent = serde_json::from_str(
        r#"{"task_id":"t","agent":"a","event":"Stop","session_id":"s","transcript_path":"/p","tool_name":"bash","codex_like":false}"#,
    )
    .unwrap();
    assert_eq!(full.transcript_path, "/p");
    assert_eq!(full.tool_name, "bash");
    assert_eq!(full.codex_like, Some(false));
}
