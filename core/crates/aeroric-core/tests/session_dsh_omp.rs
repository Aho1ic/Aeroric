//! P3-04 第三片金样：dsh／omp 方言的读侧与解析（含多帧 zstd 串接）。
//!
//! 期望值来自 2026-09-29 迁移时对宿主原实现同输入的真实读数（逐字搬运 ⇒ 两侧同源），
//! 结构照 `e3836e62:docs/native/extraction/sessions-corpus.md` §一 的方言表：
//! dsh 是斜杠式 `type`（`user/message`、`assistant/chunk`、`command/done`…），
//! omp 是 `{type:"message", message:{role…}, id}` 的 append-only entry 树。

use aeroric_core::sessions::dsh::{
    dsh_transcript_in, line_is_dsh_header, newest_dsh_session_in_dir, parse_dsh_session_lines,
    read_compressed_dsh_lines, read_dsh_session_header, read_dsh_session_lines, DSH_TRANSCRIPT_RAW,
    DSH_TRANSCRIPT_ZSTD,
};
use aeroric_core::sessions::omp::{
    newest_omp_session_in_dir, parse_omp_session_lines, read_omp_session_header,
};
use aeroric_core::sessions::SessionContent;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-dshomp-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn kinds(m: &aeroric_core::sessions::SessionMessage) -> Vec<&'static str> {
    m.content
        .iter()
        .map(|c| match c {
            SessionContent::Text { .. } => "text",
            SessionContent::ToolUse { .. } => "tool_use",
            SessionContent::ToolResult { .. } => "tool_result",
            SessionContent::Thinking { .. } => "thinking",
            SessionContent::Attachment { .. } => "attachment",
        })
        .collect()
}

fn dsh_header_line(version: u64, id: &str, created_at: i64) -> String {
    format!(r#"{{"type":"session","version":{version},"id":"{id}","createdAt":{created_at}}}"#)
}

#[test]
fn dsh_parses_slash_dialect_events_into_messages() {
    let lines = vec![
        dsh_header_line(0, "dsh-s1", 100),
        r#"{"type":"turn/start","seq":0,"time":1,"data":{"turn":0}}"#.to_string(),
        r#"{"type":"user/message","seq":1,"time":2,"data":{"content":[{"type":"text","text":"hello dsh"},{"type":"image"}]}}"#.to_string(),
        r#"{"type":"assistant/message","seq":2,"time":3,"data":{"message":{"id":"m2","content":[{"type":"reasoning","text":"think"},{"type":"text","text":"hi"},{"type":"tool-call","id":"call-1","name":"bash","arguments":"{\"cmd\":\"ls\"}"}]}}}"#.to_string(),
        r#"{"type":"tool/call","seq":3,"time":4,"data":{}}"#.to_string(),
        r#"{"type":"tool/result","seq":4,"time":5,"data":{"message":{"content":[{"type":"tool-result","toolCallId":"call-1","content":[{"type":"text","text":"file.txt"}]}]}}}"#.to_string(),
        r#"{"type":"tool/result","seq":5,"time":6,"data":{"message":{"content":[{"type":"tool-result","toolCallId":"call-2","isError":true,"content":[{"type":"text","text":"boom"}]}]}}}"#.to_string(),
        r#"{"type":"command/run","seq":6,"time":7,"data":{"name":"status","args":" --json"}}"#.to_string(),
        r#"{"type":"command/done","seq":7,"time":8,"data":{"text":"Command finished"}}"#.to_string(),
        r#"{"type":"compaction/summary","seq":8,"time":9,"data":{"text":"compact body"}}"#.to_string(),
        r#"{"type":"subagent/descriptor","seq":9,"time":10,"data":{"name":"worker"}}"#.to_string(),
        r#"{"type":"turn/end","seq":10,"time":11,"data":{"turn":0}}"#.to_string(),
        "not json at all".to_string(),
    ];
    let messages = parse_dsh_session_lines(&lines).expect("v0 header parses");
    let shape: Vec<(String, Vec<&'static str>)> = messages
        .iter()
        .map(|m| (m.role.clone(), kinds(m)))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("user".to_owned(), vec!["text", "text"]), // image → "[image attachment]" 占位文本
            (
                "assistant".to_owned(),
                vec!["thinking", "text", "tool_use"] // tool/call 事件跳过：assistant 已含 tool-call block
            ),
            ("user".to_owned(), vec!["tool_result"]),
            ("user".to_owned(), vec!["tool_result"]), // isError ⇒ "[error] " 前缀
            ("user".to_owned(), vec!["text"]),        // /command + args
            ("assistant".to_owned(), vec!["text"]),   // command/done
            ("assistant".to_owned(), vec!["text"]),   // compaction summary
            ("assistant".to_owned(), vec!["text"]),   // subagent descriptor
        ],
        "dsh 方言的斜杠式 type 全部按原映射落位"
    );
    assert_eq!(messages[1].message_id.as_deref(), Some("m2"));
    let SessionContent::ToolResult { output, .. } = &messages[3].content[0] else {
        panic!("expected tool result");
    };
    assert_eq!(output, "[error] boom");
    let SessionContent::Text { text } = &messages[4].content[0] else {
        panic!("expected command text");
    };
    assert_eq!(text, "/status --json");
    let SessionContent::Text { text } = &messages[6].content[0] else {
        panic!("expected summary text");
    };
    assert_eq!(text, "[compaction summary]\ncompact body");
}

#[test]
fn dsh_rejects_future_header_version_and_tolerates_truncated_tail() {
    let future = vec![dsh_header_line(1, "dsh-s2", 100)];
    let err = parse_dsh_session_lines(&future).unwrap_err();
    assert!(
        err.contains("v1") && err.contains("upgrade Aeroric"),
        "{err}"
    );
    // 尾部截断读取不带 header 时，纯事件行照常解析（宿主 read_session_messages 的截断路径）。
    let no_header = vec![
        r#"{"type":"user/message","data":{"content":[{"type":"text","text":"tail"}]}}"#.to_string(),
    ];
    assert_eq!(parse_dsh_session_lines(&no_header).unwrap().len(), 1);
}

/// 按插件真实落盘方式写压缩 transcript：header 一帧、每个持久化批次一帧，
/// 物理文件是多帧串接——不是一个大 frame。
fn write_zstd_frames(path: &Path, batches: &[&[String]]) {
    let mut file = fs::File::create(path).unwrap();
    for batch in batches {
        let mut text = String::new();
        for line in *batch {
            text.push_str(line);
            text.push('\n');
        }
        file.write_all(&zstd::encode_all(text.as_bytes(), 0).unwrap())
            .unwrap();
    }
}

#[test]
fn dsh_multiframe_zstd_is_streamed_to_eof_not_first_frame() {
    let dir = scratch("zstd-frames");
    let header = dsh_header_line(0, "dsh-s3", 12345);
    let body: Vec<String> = (0..40)
        .map(|i| format!(r#"{{"type":"command/done","seq":{i},"data":{{"text":"line {i}"}}}}"#))
        .collect();
    let compressed = dir.join(DSH_TRANSCRIPT_ZSTD);
    write_zstd_frames(
        &compressed,
        &[std::slice::from_ref(&header), &body[..20], &body[20..]],
    );
    // transcript_in：明文缺席时压缩产物被选中。
    assert_eq!(
        dsh_transcript_in(&dir).as_deref(),
        Some(compressed.as_path())
    );
    // 串接到 EOF：1 header + 40 body。只解第一帧就只剩 1 行——这条断言钉住不加 single_frame()。
    let lines = read_dsh_session_lines(&compressed).expect("decodes");
    assert_eq!(lines.len(), 41);
    assert!(line_is_dsh_header(&lines[0]));
    assert_eq!(lines[40], body[39]);
    assert_eq!(
        read_dsh_session_header(&compressed),
        Some(("dsh-s3".to_owned(), 12345))
    );
    // 明文路径：read_compressed 返回 None，让调用方保留按字节分页的读法。
    let raw = dir.join(DSH_TRANSCRIPT_RAW);
    fs::write(&raw, format!("{header}\n{}\n", body[0])).unwrap();
    assert_eq!(dsh_transcript_in(&dir).as_deref(), Some(raw.as_path()));
    assert_eq!(read_compressed_dsh_lines(&raw).unwrap(), None);
    assert_eq!(
        read_dsh_session_header(&raw),
        Some(("dsh-s3".to_owned(), 12345))
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dsh_dir_discovery_orders_by_created_at_and_skips_claimed() {
    let root = scratch("dsh-discover");
    let mk = |sid: &str, created: i64| -> PathBuf {
        let sdir = root.join(sid);
        fs::create_dir_all(&sdir).unwrap();
        let file = sdir.join(DSH_TRANSCRIPT_RAW);
        fs::write(&file, format!("{}\n", dsh_header_line(0, sid, created))).unwrap();
        file
    };
    let old = mk("s-old", 1_000);
    let mid = mk("s-mid", 2_000);
    let newest = mk("s-new", 3_000);
    // 不做认领过滤时取 created_at 最大。
    let (id, path) = newest_dsh_session_in_dir(&root, 0, None).unwrap();
    assert_eq!(
        (id.as_str(), path),
        ("s-new", root.join("s-new").join(DSH_TRANSCRIPT_RAW))
    );
    // 窗口语义照宿主原文 `created_at < since_ms` 跳过：created_at == since 保留、越过即空手。
    let (id, _) = newest_dsh_session_in_dir(&root, 3_000, None).unwrap();
    assert_eq!(id, "s-new");
    assert_eq!(
        newest_dsh_session_in_dir(&root, 3_001, None),
        None,
        "since 抬过最大 created_at 时不得返回任何会话"
    );
    // 认领过滤：s-new 被占 → 退到 s-mid；全认领 → None（第二任务不得抢第一个任务的会话）。
    let mut claimed = HashSet::new();
    claimed.insert(newest.to_string_lossy().to_string());
    let (id, path) = newest_dsh_session_in_dir(&root, 0, Some(&claimed)).unwrap();
    assert_eq!((id.as_str(), path), ("s-mid", mid.clone()));
    claimed.insert(mid.to_string_lossy().to_string());
    claimed.insert(old.to_string_lossy().to_string());
    assert_eq!(newest_dsh_session_in_dir(&root, 0, Some(&claimed)), None);
    let _ = fs::remove_dir_all(&root);
}

fn omp_message_line(id: &str, role: &str, content: &str) -> String {
    format!(r#"{{"type":"message","id":"{id}","message":{{"role":"{role}","content":{content}}}}}"#)
}

#[test]
fn omp_parses_entry_tree_flat_with_message_ids() {
    let lines = vec![
        r#"{"type":"session","id":"omp-s1","title":"t"}".to_string(),
        r#"{"type":"model_usage","data":{}}"#.to_string(),
        "".to_string(),
        omp_message_line("e1", "user", r#""plain prompt""#),
        omp_message_line(
            "e2",
            "user",
            r#"[{"type":"text","text":"see"},{"type":"image","mimeType":"image/jpeg"}]"#,
        ),
        omp_message_line(
            "e3",
            "assistant",
            r#"[{"type":"thinking","thinking":"deep"},{"type":"text","text":"answer"},{"type":"toolCall","id":"call-1","name":"bash","arguments":{"cmd":"ls"}}]"#,
        ),
        omp_message_line("e4", "developer", r#""system note""#),
        r#"{"type":"message","id":"e5","message":{"role":"toolResult","toolCallId":"call-9","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}}"#.to_string(),
        r#"{"type":"message","id":"e6","message":{"role":"toolResult","toolCallId":"call-x","content":[{"type":"text","text":"   "}]}}"#.to_string(),
        "}{ broken json".to_string(),
    ];
    let messages = parse_omp_session_lines(&lines).unwrap();
    let shape: Vec<(String, Vec<&'static str>)> = messages
        .iter()
        .map(|m| (m.role.clone(), kinds(m)))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("user".to_owned(), vec!["text"]),
            ("user".to_owned(), vec!["text", "attachment"]),
            ("assistant".to_owned(), vec!["thinking", "text", "tool_use"]),
            ("toolResult".to_owned(), vec!["tool_result"]),
        ],
        "header/model_usage/developer/空输出/坏行一律跳过，entry 线性平铺"
    );
    assert_eq!(messages[0].message_id.as_deref(), Some("e1"));
    let SessionContent::Attachment {
        name,
        media_type,
        source,
    } = &messages[1].content[1]
    else {
        panic!("image becomes placeholder attachment");
    };
    assert_eq!(
        (name.as_str(), media_type.as_str(), source.as_str()),
        ("image", "image/jpeg", "inline-image")
    );
    let SessionContent::ToolUse { input, .. } = &messages[2].content[2] else {
        panic!("toolCall");
    };
    assert_eq!(input, r#"{"cmd":"ls"}"#); // 对象 arguments → to_string 原文
    let SessionContent::ToolResult { output, id } = &messages[3].content[0] else {
        panic!("toolResult");
    };
    assert_eq!((output.as_str(), id.as_str()), ("line1\nline2", "call-9"));
}

#[test]
fn omp_header_read_is_bounded_to_first_four_lines() {
    let dir = scratch("omp-header");
    let path = dir.join("s.jsonl");
    // 首行 256 字节 title 槽（定长填充），第二行才是 header。
    let title = format!("{{\"pad\":\"{}\"}}", "x".repeat(256));
    fs::write(
        &path,
        format!("{title}\n{}\n", r#"{"type":"session","id":"omp-h1"}"#),
    )
    .unwrap();
    assert_eq!(read_omp_session_header(&path).as_deref(), Some("omp-h1"));
    // header 落在第 5 行 → 窗口外，返回 None（title 槽固定 256B 的假设被逐字保留）。
    let late = dir.join("late.jsonl");
    fs::write(
        &late,
        "1\n2\n3\n4\n{\"type\":\"session\",\"id\":\"late\"}\n",
    )
    .unwrap();
    assert_eq!(read_omp_session_header(&late), None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn omp_dir_discovery_uses_mtime_window_and_claimed_skip() {
    let dir = scratch("omp-discover");
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000);
    let mk = |sid: &str, offset: u64| -> PathBuf {
        let path = dir.join(format!("2026-09-06T10-30-00_{sid}.jsonl"));
        let file = fs::File::create(&path).unwrap();
        file.set_modified(base + Duration::from_secs(offset))
            .unwrap();
        path
    };
    let older = mk("0198older-0000-0000-0000-000000000000", 100);
    let newer = mk("0198newer-0000-0000-0000-000000000000", 200);
    let since = base
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let (id, path) = newest_omp_session_in_dir(&dir, since, None).unwrap();
    assert!(
        path == newer && id.starts_with("0198newer"),
        "取 mtime 最新"
    );
    // R7 核心局面：已认领的那条更新时，必须退到未认领的旧一条，而不是空手而归。
    let mut claimed = HashSet::new();
    claimed.insert(newer.to_string_lossy().to_string());
    let (id, path) = newest_omp_session_in_dir(&dir, since, Some(&claimed)).unwrap();
    assert!(path == older && id.starts_with("0198older"));
    // 边界：mtime 恰等于 since → `<=` 跳过（懒会话不得被窗口漏掉也不得被提前认领）。
    assert_eq!(newest_omp_session_in_dir(&dir, since + 201_000, None), None);
    // 非 .jsonl 条目忽略。
    fs::write(dir.join("notes.txt"), b"x").unwrap();
    let (id, _) = newest_omp_session_in_dir(&dir, since, Some(&claimed)).unwrap();
    assert!(id.starts_with("0198older"));
    let _ = fs::remove_dir_all(&dir);
}
