//! P3-04 第四片金样：分页／尾部窗口的读取层与格式探测／分派。
//!
//! 判据来自宿主 `session.rs` 的既有测试语义（经 re-export 仍在宿主跑，这里在
//! core API 上直接钉）与 `sessions.md` Action 1「保留既有路径校验与读取上限」：
//! - 反向字节游标分页：整文件按页拼回**逐字连续**、游标单调、`has_more` 收敛；
//! - 越界游标钳到 EOF、`cursor=0`／`limit=0`／空文件三条退化路径不读盘；
//! - 无尾换行的残行仍返回；空白行跳过；CRLF 剥 `\r`；非 UTF-8 报既有错误文案；
//! - 尾窗只留最后 `MAX_SESSION_LINES` 行，而 codex 探测结论**不随头行被弹出而丢**
//!   （这正是 `read_session_tail` 单独回传 `detected_codex` 的理由，注释原文）；
//! - `parse_session_messages_with_format` 的双解析兜底：首选族解析为空且与
//!   探测结论不一致时，按探测结论重解析一次。

use aeroric_core::sessions::history::{
    read_session_page_lines, read_session_tail, MAX_SESSION_LINES, SESSION_MESSAGE_PAGE_LINES,
};
use aeroric_core::sessions::parser::{
    is_codex_format, parse_session_messages_with_format, SESSION_FORMAT_DETECTION_LINES,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-history-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_lines(path: &Path, lines: &[String]) {
    let mut file = fs::File::create(path).unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
    file.flush().unwrap();
}

#[test]
fn pages_stitch_the_whole_file_back_without_gaps() {
    let dir = scratch("stitch");
    let path = dir.join("session.jsonl");
    let total = SESSION_MESSAGE_PAGE_LINES * 2 + 500;
    let lines: Vec<String> = (0..total)
        .map(|i| format!("{{\"i\":{i},\"text\":\"行-{i}\"}}"))
        .collect();
    write_lines(&path, &lines);

    let mut cursor: Option<u64> = None;
    let mut collected: Vec<String> = Vec::new();
    let mut page_count = 0usize;
    loop {
        let (page_lines, next_cursor, has_more) =
            read_session_page_lines(&path, cursor, SESSION_MESSAGE_PAGE_LINES).unwrap();
        assert!(page_lines.len() <= SESSION_MESSAGE_PAGE_LINES);
        page_count += 1;
        // 页内已按时间正序返回（函数注释原文），整页前插，不得逐行反转。
        collected.splice(0..0, page_lines);
        if !has_more {
            assert_eq!(next_cursor, None, "最后一页不得再发游标");
            break;
        }
        let next = next_cursor.expect("has_more 时必带游标");
        assert!(cursor.is_none_or(|prev| next < prev), "游标必须严格后退");
        cursor = Some(next);
        assert!(page_count < 10, "分页必须收敛");
    }
    assert_eq!(page_count, 3);
    assert_eq!(collected, lines, "页拼接必须逐字等于源文件（正序）");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cursor_degrades_on_eof_zero_limit_and_empty_file() {
    let dir = scratch("degrade");
    let path = dir.join("session.jsonl");
    write_lines(&path, &["{\"a\":1}".to_string(), "{\"b\":2}".to_string()]);
    let file_len = fs::metadata(&path).unwrap().len();

    // 越界游标钳到文件尾 ⇒ 与 None 完全一致。
    let (clamp, ..) = read_session_page_lines(&path, Some(file_len + 9_999), 10).unwrap();
    let (none, ..) = read_session_page_lines(&path, None, 10).unwrap();
    assert_eq!(clamp, none);

    // limit=0：空页、无游标、无更多，不触碰内容（None 与 0 两个游标都验）。
    for cursor in [Some(0u64), None] {
        let (lines, next, more) = read_session_page_lines(&path, cursor, 0).unwrap();
        assert!(
            lines.is_empty() && next.is_none() && !more,
            "limit=0 必须空页"
        );
    }
    // cursor=0：`end == 0` 退化分支。
    let (lines, next, more) = read_session_page_lines(&path, Some(0), 10).unwrap();
    assert!(
        lines.is_empty() && next.is_none() && !more,
        "cursor=0 必须空页"
    );

    let empty = dir.join("empty.jsonl");
    fs::write(&empty, b"").unwrap();
    let (lines, next, more) = read_session_page_lines(&empty, None, 10).unwrap();
    assert!(lines.is_empty() && next.is_none() && !more);

    assert!(read_session_page_lines(&dir.join("missing.jsonl"), None, 10).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ragged_tail_and_crlf_and_blank_lines_follow_host_semantics() {
    let dir = scratch("ragged");
    let path = dir.join("session.jsonl");
    {
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"{\"a\":1}\n\n   \n{\"b\":2}\r\n{\"tail\":\"no newline\"}")
            .unwrap();
    }
    let (lines, next, more) = read_session_page_lines(&path, None, 10).unwrap();
    assert_eq!(
        lines,
        vec![
            "{\"a\":1}".to_string(),
            "{\"b\":2}".to_string(),
            "{\"tail\":\"no newline\"}".to_string(),
        ],
        "空白行跳过、CRLF 剥 \\r、无尾换行的残行必须返回"
    );
    assert_eq!(next, None);
    assert!(!more);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn invalid_utf8_line_reports_the_pinned_wording() {
    let dir = scratch("utf8");
    let path = dir.join("session.jsonl");
    fs::write(&path, b"{\"x\":\"\xff\xfe\"}\n").unwrap();
    let error = read_session_page_lines(&path, None, 10).unwrap_err();
    assert!(
        error.starts_with("Session file is not valid UTF-8:"),
        "错误文案是旧客户端在网契约: {error}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tail_keeps_newest_lines_but_not_at_the_cost_of_detection() {
    let dir = scratch("tail");
    let path = dir.join("session.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    // 首行是 codex 的 session_meta：探测窗前；尾窗装 20 000 行时它必被弹出。
    writeln!(
        file,
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"c1\"}}}}"
    )
    .unwrap();
    for i in 0..(MAX_SESSION_LINES + 50) {
        writeln!(file, "{{\"type\":\"user\",\"i\":{i}}}").unwrap();
    }
    drop(file);
    let (lines, detected_codex) = read_session_tail(&path).unwrap();
    assert_eq!(lines.len(), MAX_SESSION_LINES, "尾窗必须钳在上限");
    assert!(detected_codex, "头行已被弹出，但探测结论必须单独回传");
    assert!(
        lines
            .first()
            .unwrap()
            .starts_with("{\"type\":\"user\",\"i\":50}"),
        "被弹出 51 行（meta＋i=0..49）后，窗首必须是 i=50：{:?}",
        lines.first()
    );
    assert!(
        lines.last().unwrap().contains("\"i\":20049"),
        "尾行不能丢: {:?}",
        lines.last()
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn format_probe_window_is_two_hundred_lines_only() {
    // 纯切片语义，不读盘；窗长 200 由 take() 决定。
    let mut lines: Vec<String> = (0..SESSION_FORMAT_DETECTION_LINES)
        .map(|i| format!("{{\"noise\":{i}}}"))
        .collect();
    assert!(!is_codex_format(&lines), "纯噪声窗内不得判成 codex");
    lines.insert(5, "{\"type\":\"event_msg\",\"payload\":{}}".to_string());
    assert!(is_codex_format(&lines), "窗内出现事件标记即判 codex");

    let mut late = vec!["{\"type\":\"session_meta\",\"payload\":{}}".to_string()];
    late.extend((0..SESSION_FORMAT_DETECTION_LINES).map(|i| format!("{{\"noise\":{i}}}")));
    assert!(
        is_codex_format(&late),
        "第 1 行在窗内——窗长 200 的边界由 take() 决定"
    );

    let beyond: Vec<String> = (0..(SESSION_FORMAT_DETECTION_LINES + 1))
        .map(|i| {
            if i == SESSION_FORMAT_DETECTION_LINES {
                "{\"type\":\"session_meta\",\"payload\":{}}".to_string()
            } else {
                format!("{{\"noise\":{i}}}")
            }
        })
        .collect();
    assert!(!is_codex_format(&beyond), "第 201 行（窗后）不得参与判定");
}

#[test]
fn preferred_family_falls_back_to_detection_when_primary_parses_empty() {
    // claude 形状的行；prefer_codex=true 时首选 codex 解析得空，必须按探测结论兜底。
    let lines = vec![
        "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"你好\"}]}}"
            .to_string(),
    ];
    let messages = parse_session_messages_with_format(&lines, true, false);
    assert_eq!(messages.len(), 1, "兜底重解析必须产出 claude 消息");
    assert_eq!(messages[0].role, "user");

    let direct = parse_session_messages_with_format(&lines, false, false);
    assert_eq!(direct, messages, "首选即正确时结果一致");
    // 兜底只在「首选族解析为空且两结论不一致」这一格发生；两结论一致时不重复解析。
}
