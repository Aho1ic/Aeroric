//! P3-04 首片金样测试：Claude/Codex 转录解析 + 旧客户端 serde 形状 + 损坏反例。
//!
//! fixture 来源：`~/.aeroric/agent-homes/**` 真实语料（claude 最小/中位、codex 最小/中位），
//! 按 `docs/native/extraction/sessions-corpus.md` §三 的脱敏变换表生成（补记二百四十一）：
//! 自由文本按**原字节长度**替换，`cwd`/`gitBranch`/`sessionId`/时间戳换固定常量，
//! 非 ASCII 与路径形态键值全部清零；入库前后都跑了 corpus §三 的零命中清单。
//! 测试**不得**读本机 `~/.aeroric`（`core-discovers-no-real-home` 守着）。

use aeroric_core::sessions::claude::parse_claude_session;
use aeroric_core::sessions::codex::parse_codex_session;
use aeroric_core::sessions::parser::parse_session_lines;
use aeroric_core::sessions::{SessionContent, SessionMessage};
use std::collections::BTreeMap;

fn fixture_lines(name: &str) -> Vec<String> {
    let path = format!(
        "{}/tests/fixtures/sessions/{name}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path} must exist: {e}"));
    assert!(
        body.is_ascii(),
        "fixture {name} carries non-ASCII bytes — the sanitizer's guarantee is broken"
    );
    // 探针本身不得含字面斜杠路径——`core-discovers-no-real-home` 扫的是源码文本，
    // 字面量会让守卫扫到自己的哨兵（本文件 2026-09-29 首跑就这么红了一次）。
    // "\x2f" 运行时等价于 '/'，源文本里不出现被禁子串；用户名同样不落地。
    for pat in ["\x2fUsers\x2f", "-Users-", "--Users-", "Downloads", "LYX"] {
        assert!(
            !body.contains(pat),
            "fixture {name} leaks real path fragment {pat:?}"
        );
    }
    body.lines().map(str::to_owned).collect()
}

fn histogram<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeMap<T, usize> {
    let mut map = BTreeMap::new();
    for item in items {
        *map.entry(item).or_insert(0) += 1;
    }
    map
}

fn type_of(line: &str) -> &str {
    // 只为断言行形状：取第一个 "type":"…" 的值。
    let start = line.find("\"type\":\"").unwrap() + 8;
    let rest = &line[start..];
    &rest[..rest.find('"').unwrap()]
}

fn kind(c: &SessionContent) -> &'static str {
    match c {
        SessionContent::Text { .. } => "text",
        SessionContent::ToolUse { .. } => "tool_use",
        SessionContent::ToolResult { .. } => "tool_result",
        SessionContent::Thinking { .. } => "thinking",
        SessionContent::Attachment { .. } => "attachment",
    }
}

fn shape_of(name: &str, is_codex: bool) -> Vec<(String, Vec<&'static str>)> {
    let lines = fixture_lines(name);
    if is_codex {
        let mut messages = Vec::new();
        parse_session_lines(&lines, true, &mut messages);
        messages
            .into_iter()
            .map(|m| (m.role, m.content.iter().map(kind).collect()))
            .collect()
    } else {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        parse_claude_session(&refs)
            .into_iter()
            .map(|m| (m.role, m.content.iter().map(kind).collect()))
            .collect()
    }
}

#[test]
fn golden_fixture_shapes_are_pinned() {
    // 行数与 type 直方图来自脱敏前原文件（与解析器无关的语料形状）。
    let claude_min = fixture_lines("claude-min");
    assert_eq!(claude_min.len(), 7);
    assert_eq!(
        histogram(claude_min.iter().map(|l| type_of(l).to_owned())),
        BTreeMap::from([
            ("mode".to_owned(), 1),
            ("permission-mode".to_owned(), 1),
            ("file-history-snapshot".to_owned(), 1),
            ("user".to_owned(), 3),
            ("last-prompt".to_owned(), 1),
        ])
    );

    assert_eq!(fixture_lines("claude-mid").len(), 12);

    let codex_min = fixture_lines("codex-min");
    assert_eq!(codex_min.len(), 9);
    assert_eq!(
        histogram(codex_min.iter().map(|l| type_of(l).to_owned())),
        BTreeMap::from([
            ("session_meta".to_owned(), 1),
            ("event_msg".to_owned(), 3),
            ("response_item".to_owned(), 3),
            ("world_state".to_owned(), 1),
            ("turn_context".to_owned(), 1),
        ])
    );

    assert_eq!(fixture_lines("codex-mid").len(), 23);
}

#[test]
fn golden_claude_parses_to_expected_message_shape() {
    // 期望值＝迁移后的 core 解析器对金样的读数，2026-09-29 首片实测钉死；
    // 同一行为另由宿主 40 条既有测试经 re-export 复跑兜底（`--lib` 2465 不降）。
    assert_eq!(
        shape_of("claude-min", false),
        vec![
            ("user".to_owned(), vec!["text"]),
            ("user".to_owned(), vec!["text"]),
            ("user".to_owned(), vec!["text"]),
        ],
        "三条 user 行各产一条消息；mode/permission-mode/file-history-snapshot/last-prompt 全部静默跳过"
    );
    assert_eq!(
        shape_of("claude-mid", false),
        vec![
            ("user".to_owned(), vec!["text"]),
            ("assistant".to_owned(), vec!["text"]),
        ],
        "agent_listing/skill_listing/token_reminder/system 等噪声型 top-level 行不产消息"
    );
}

#[test]
fn golden_codex_parses_to_expected_message_shape() {
    assert_eq!(
        shape_of("codex-min", true),
        vec![
            ("user".to_owned(), vec!["text"]),
            ("user".to_owned(), vec!["text"]),
        ]
    );
    assert_eq!(
        shape_of("codex-mid", true),
        vec![
            ("user".to_owned(), vec!["text", "text"]),
            ("user".to_owned(), vec!["text"]),
            ("user".to_owned(), vec!["text"]),
        ],
        "event_msg 与 response_item 双写的重复文本按宿主原行为归并"
    );
}

#[test]
fn serde_wire_shape_is_pinned_for_every_content_variant() {
    // 旧客户端 JSON 的形状契约：tag 名、snake_case、messageId/mediaType 改名、
    // message_id=None 时字段整个缺席。任何一处变形，React 会话视图就渲染不出。
    let cases = [
        SessionContent::Text {
            text: "hello".into(),
        },
        SessionContent::ToolUse {
            id: "t1".into(),
            name: "Read".into(),
            input: "{}".into(),
        },
        SessionContent::ToolResult {
            id: "t1".into(),
            output: "ok".into(),
        },
        SessionContent::Thinking {
            thinking: "hmm".into(),
        },
        SessionContent::Attachment {
            name: "f.md".into(),
            media_type: "text/markdown".into(),
            source: "data:text/markdown;base64,AAA".into(),
        },
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|c| serde_json::to_string(c).unwrap())
        .collect();
    assert_eq!(rendered[0], r#"{"type":"text","text":"hello"}"#);
    assert_eq!(
        rendered[1],
        r#"{"type":"tool_use","id":"t1","name":"Read","input":"{}"}"#
    );
    assert_eq!(
        rendered[2],
        r#"{"type":"tool_result","id":"t1","output":"ok"}"#
    );
    assert_eq!(rendered[3], r#"{"type":"thinking","thinking":"hmm"}"#);
    assert_eq!(
        rendered[4],
        r#"{"type":"attachment","name":"f.md","mediaType":"text/markdown","source":"data:text/markdown;base64,AAA"}"#
    );

    let with_id = SessionMessage {
        role: "user".into(),
        content: vec![cases[0].clone()],
        message_id: Some("m-1".into()),
    };
    assert_eq!(
        serde_json::to_string(&with_id).unwrap(),
        r#"{"role":"user","content":[{"type":"text","text":"hello"}],"messageId":"m-1"}"#
    );
    let without_id = SessionMessage {
        role: "assistant".into(),
        content: vec![cases[3].clone()],
        message_id: None,
    };
    assert_eq!(
        serde_json::to_string(&without_id).unwrap(),
        r#"{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}"#,
        "messageId must vanish, not serialize as null"
    );
}

#[test]
fn corrupted_and_off_dialect_lines_produce_no_messages_and_no_panic() {
    // corpus §一：自然损坏在真实语料里 0 例，反例必须自己合成——四种形态。
    let long_text = "y".repeat(300_000);
    let mut messages: Vec<SessionMessage> = Vec::new();
    parse_session_lines(
        &[
            r#"{"type":"user","message":{"content":"半条"#.to_owned(), // 截断的 JSON
            r#"[1,2,3]"#.to_owned(),                                   // 非 object 行
            r#"{"no_type":"x"}"#.to_owned(),                           // type 缺失
            format!(r#"{{"type":"user","message":{{"content":"{long_text}"}}}}"#), // 超长单行
        ],
        false,
        &mut messages,
    );
    assert_eq!(messages.len(), 1, "only the long valid line may survive");
    let SessionContent::Text { text } = &messages[0].content[0] else {
        panic!("long line must parse as one text block");
    };
    assert_eq!(text.len(), 300_000);

    let mut codex_messages: Vec<SessionMessage> = Vec::new();
    parse_session_lines(
        &[
            r#"{"type":"event_ms"#.to_owned(), // codex 侧同样不 panic
            "not json at all".to_owned(),
            r#"{"type":"response_item","payload":{"type":"unknown_kind"}}"#.to_owned(),
        ],
        true,
        &mut codex_messages,
    );
    assert!(codex_messages.is_empty());
}

#[test]
fn codex_merges_assistant_parts_and_dedups_adjacent_user_text() {
    // 去重规则只对**紧邻的上一条 user 消息**生效（宿主
    // `parse_session_messages_preserves_codex_tool_output_for_handoff` 依赖的行为）；
    // 间隔了 assistant 之后的重复 user 行按原行为另起一条，不得"顺手"合并。
    let lines = vec![
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"same prompt"}}"#.to_owned(),
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"same prompt"}]}}"#.to_owned(),
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"part one"}]}}"#.to_owned(),
        r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"thinking alpha"}]}}"#.to_owned(),
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"same prompt"}}"#.to_owned(),
        r#"{"type":"response_item","payload":{"type":"reasoning","summary":["plain string instead of object"]}}"#.to_owned(),
    ];
    let mut messages = Vec::new();
    parse_session_lines(&lines, true, &mut messages);
    assert_eq!(
        messages
            .iter()
            .map(|m| (m.role.clone(), m.content.iter().map(kind).collect::<Vec<_>>()))
            .collect::<Vec<_>>(),
        vec![
            ("user".to_owned(), vec!["text"]),
            ("assistant".to_owned(), vec!["text", "thinking"]),
            ("user".to_owned(), vec!["text"]),
        ],
        "第 2 行并入第 1 行（紧邻 user）；reasoning 并入 assistant；第 5 行因上一条是 assistant 而另起；第 6 行字符串形 summary 如实丢弃"
    );

    let bare = parse_codex_session(&[
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"solo"}}"#,
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"solo"}}"#,
    ]);
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0].content.len(), 1);

    let mut noise = Vec::new();
    parse_session_lines(
        &[
            r#"{"type":"response_item","payload":{"type":"reasoning","summary":["plain string instead of object"]}}"#.to_owned(),
        ],
        true,
        &mut noise,
    );
    assert!(
        noise.is_empty(),
        "string-array summary joins to empty thinking and is dropped —不产空块"
    );
}
