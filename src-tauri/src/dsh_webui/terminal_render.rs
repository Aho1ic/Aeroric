//! dsh 的 web 后端事件 → 终端文本。
//!
//! 从 `dsh_webui.rs` 拆出来的一整块:软换行状态机(`TerminalWrap`)、ANSI 切分与
//! 列宽计算、以及把工具调用 / 工具结果 / 推理块折成终端行的那批渲染器。
//!
//! 为什么能整块搬走:这 800 多行对 `AppHandle`、`DshApiClient`、
//! `DshWebUiManager` 全都没有引用 —— 输入是 `&str` / `&Value`,输出是 `String`。
//! 唯一碰到应用状态的是 `sync_wrap_cols`(要读 pty 尺寸),它留在父模块。
//!
//! 可见性:只有父模块真正用到的才 `pub(super)`,其余保持私有 —— 让编译器继续
//! 盯着"哪些只有测试在用"。

use serde_json::Value;

/// Minimal ANSI styling for the tool render-intent output. The dsh stream lands
/// in an xterm view configured with `convertEol: false`, so every line this
/// module writes terminates with an explicit CRLF.
pub(super) const ANSI_GREEN: &str = "\x1b[32m";
pub(super) const ANSI_RED: &str = "\x1b[31m";

// 行生成八件与 ANSI 三常量、折叠宽度已随 P3-05 第七片迁入 core
// `aeroric_core::agents::dsh_host::projection`（补记二百五十六）；Action 2 的边界
// 随注释迁走（DSH projection 只是适配输出、不得借用到 OMP）。re-export 保持
// `pub(super)` 原路径——`render_tool_event_view`、`dsh_webui.rs` 与下方测试模块
// 的引用点一字不动；有状态的 `TerminalWrap`/`ReasoningFold` 留宿主。
pub(super) use aeroric_core::agents::dsh_host::projection::{
    count_content_lines, fold_summary, plural, push_line, user_prompt_echo, view_str,
    ReasoningFold, TerminalWrap, ANSI_BOLD, ANSI_DIM, ANSI_RESET,
};
// lib 目标只剩测试引用这五个（渲染分支留在本文件、normalize/wrap 断言在下方测试模块）——
// lib/test 分流照 `dsh_protocol`/`DSH_WEB_OUTPUT_LIMIT` 先例，补记二百五十八。
#[cfg(test)]
pub(super) use aeroric_core::agents::dsh_host::projection::{
    normalize_terminal_text, terminal_char_width,
};

/// Split a single-file change into signed rows. `old_text` of `None` is the
/// harness's `oldText: null` — a create or an overwrite, with no before-image to
/// compare against, so every line reads as an addition.
pub(super) fn diff_rows(old_text: Option<&str>, new_text: &str) -> Vec<(char, String)> {
    let next: Vec<&str> = new_text.split('\n').collect();
    let Some(old_text) = old_text else {
        return next
            .iter()
            .map(|line| ('+', line.trim_end_matches('\r').to_string()))
            .collect();
    };
    let prev: Vec<&str> = old_text.split('\n').collect();
    // A common prefix/suffix trim keeps the counted hunk tight without pulling a
    // full LCS diff into the backend; results already arrive as focused hunks.
    let mut head = 0;
    while head < prev.len() && head < next.len() && prev[head] == next[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < prev.len() - head
        && tail < next.len() - head
        && prev[prev.len() - 1 - tail] == next[next.len() - 1 - tail]
    {
        tail += 1;
    }
    let mut rows = Vec::new();
    for line in &prev[head..prev.len() - tail] {
        rows.push(('-', line.trim_end_matches('\r').to_string()));
    }
    for line in &next[head..next.len() - tail] {
        rows.push(('+', line.trim_end_matches('\r').to_string()));
    }
    rows
}

/// Name the files a change touches without printing any of it: the exact path
/// when a change is single-file, a count otherwise. `None` means the harness
/// sent a `diff` view with nothing usable in it.
fn diff_paths_fragment(diffs: &[Value]) -> Option<String> {
    let paths: Vec<&str> = diffs
        .iter()
        .filter_map(|diff| view_str(diff, "path"))
        .collect();
    match paths.as_slice() {
        [] => None,
        [path] => {
            let created = diffs
                .first()
                .is_some_and(|diff| diff.get("oldText").is_some_and(Value::is_null));
            Some(if created {
                format!("{path} (new file)")
            } else {
                (*path).to_string()
            })
        }
        paths => Some(format!("{} files", paths.len())),
    }
}

/// Render a pending-call view as one folded row: the tool's title plus a
/// single-line hint of what it is about to do. dsh web shows the same header on
/// a collapsed card and keeps the body — the full command, the proposed hunks,
/// the raw input — behind a disclosure. A terminal has no disclosure, so the
/// body is left to the session record and the insights trajectory.
fn render_tool_call_view(view: &Value) -> Option<String> {
    let title = view_str(view, "title")?;
    let mut kind = String::new();
    let mut detail: Vec<String> = Vec::new();
    match view.get("card").and_then(Value::as_str)? {
        "terminal" => {
            if let Some(description) = view_str(view, "description").and_then(fold_summary) {
                detail.push(description);
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            if let Some(paths) = diff_paths_fragment(diffs) {
                detail.push(paths);
            }
        }
        "generic" => {
            // rawInput is deliberately not printed: it is the unparsed tool
            // input and can be large. The insights trajectory shows it in full.
            if let Some(label) = view_str(view, "kind") {
                kind = format!(" {ANSI_DIM}({label}){ANSI_RESET}");
            }
            if let Some(location) = view
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
            {
                if let Some(path) = view_str(location, "path") {
                    let line = location
                        .get("line")
                        .and_then(Value::as_i64)
                        .map(|line| format!(":{line}"))
                        .unwrap_or_default();
                    detail.push(format!("{path}{line}"));
                }
            }
        }
        _ => return None,
    }
    let mut row = format!("{ANSI_BOLD}▸ {title}{ANSI_RESET}{kind}");
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::from("\r\n");
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render a completed-call view as one folded row that closes the call row
/// printed just above it: a state glyph, the harness's replacement title when
/// it sends one, and a one-line verdict. Bodies are summarized rather than
/// printed — dsh web keeps them collapsed on the same row, and a terminal that
/// dumped every command's output, every hunk and every matched line drowned the
/// conversation it was supposed to frame.
fn render_tool_result_view(view: &Value) -> Option<String> {
    let card = view.get("card").and_then(Value::as_str)?;
    let mut failed = false;
    let mut detail: Vec<String> = Vec::new();
    match card {
        "terminal" => {
            // exitCode and signal are mutually exclusive; a signal is the
            // stronger statement about how the run ended, so it wins.
            if let Some(signal) = view_str(view, "signal") {
                failed = true;
                detail.push(signal.to_string());
            } else if let Some(code) = view
                .get("exitCode")
                .and_then(Value::as_i64)
                .filter(|code| *code != 0)
            {
                failed = true;
                detail.push(format!("exit {code}"));
            }
            let output = view
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match fold_summary(output) {
                Some(summary) => {
                    detail.push(summary);
                    let lines = count_content_lines(output);
                    if lines > 1 {
                        detail.push(format!("{lines} lines"));
                    }
                }
                None => detail.push("no output".to_string()),
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            let paths = diff_paths_fragment(diffs)?;
            let (mut added, mut removed) = (0usize, 0usize);
            for diff in diffs {
                let Some(new_text) = diff.get("newText").and_then(Value::as_str) else {
                    continue;
                };
                for (sign, _) in diff_rows(diff.get("oldText").and_then(Value::as_str), new_text) {
                    if sign == '+' {
                        added += 1;
                    } else {
                        removed += 1;
                    }
                }
            }
            detail.push(format!("{paths} +{added} -{removed}"));
        }
        "search" => match view.get("shape").and_then(Value::as_str)? {
            "matches" => {
                let files = view.get("files").and_then(Value::as_array)?;
                let hits: usize = files
                    .iter()
                    .map(|file| {
                        file.get("matches")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len)
                    })
                    .sum();
                let scope = match files.as_slice() {
                    [file] => view_str(file, "path").unwrap_or("1 file").to_string(),
                    files => format!("{} files", files.len()),
                };
                detail.push(format!(
                    "{hits} {} in {scope}",
                    plural(hits, "match", "matches")
                ));
            }
            "paths" => {
                let paths: Vec<&str> = view
                    .get("paths")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter_map(|path| path.as_str().filter(|path| !path.is_empty()))
                    .collect();
                detail.push(match paths.as_slice() {
                    [path] => (*path).to_string(),
                    paths => format!("{} paths", paths.len()),
                });
            }
            _ => return None,
        },
        "read" => {
            let path = view_str(view, "path")?;
            let lines = view.get("lines").and_then(Value::as_array)?;
            let total = view.get("totalLines").and_then(Value::as_i64).unwrap_or(0);
            let offset = view.get("offset").and_then(Value::as_i64).unwrap_or(1);
            detail.push(format!(
                "{path} — {} of {total} lines from {offset}",
                lines.len()
            ));
        }
        "web" => match view.get("kind").and_then(Value::as_str)? {
            "search" => {
                if let Some(answer) = view_str(view, "answer").and_then(fold_summary) {
                    detail.push(answer);
                }
                let sources = view.get("sources").and_then(Value::as_array)?;
                detail.push(format!(
                    "{} {}",
                    sources.len(),
                    plural(sources.len(), "source", "sources")
                ));
            }
            "fetch" => {
                let url = view_str(view, "url")?;
                let status = view.get("statusCode").and_then(Value::as_i64)?;
                failed = !(200..400).contains(&status);
                detail.push(format!("{status} {url}"));
            }
            _ => return None,
        },
        // A generic result view carries only a replacement title; without one it
        // says nothing the raw event does not already carry.
        "generic" => {
            view_str(view, "title")?;
        }
        _ => return None,
    }
    if view.get("truncated") == Some(&Value::Bool(true)) {
        detail.push("truncated by the harness".to_string());
    }
    let glyph = if failed {
        format!("{ANSI_RED}✖{ANSI_RESET}")
    } else {
        format!("{ANSI_GREEN}✔{ANSI_RESET}")
    };
    let mut row = format!("  {glyph}");
    if let Some(title) = view_str(view, "title") {
        row.push_str(&format!(" {title}"));
    }
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::new();
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render the host-computed render intent riding a `tool/call` or `tool/result`
/// delivery. dsh derives one `view` per delivery (never persisting it) and its
/// own web UI draws it as a card; the terminal gets the same information as
/// styled lines. `None` means the view was absent, addressed the other event
/// kind, or used a shape this renderer does not know — the caller then falls
/// back to the raw event, which is what dsh specifies for a UI without the
/// matching capability.
pub(super) fn render_tool_event_view(payload: &Value, expected: &str) -> Option<String> {
    let envelope = payload.get("view")?;
    if envelope.get("for").and_then(Value::as_str)? != expected {
        return None;
    }
    let view = envelope.get("view")?;
    match expected {
        "call" => render_tool_call_view(view),
        "result" => render_tool_result_view(view),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/call" }, "view": { "for": "call", "view": view } })
    }

    fn result_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/result" }, "view": { "for": "result", "view": view } })
    }

    #[test]
    fn normalizes_every_terminal_newline_without_touching_unicode_or_ansi() {
        assert_eq!(normalize_terminal_text("one\ntwo"), "one\r\ntwo");
        assert_eq!(normalize_terminal_text("one\r\ntwo"), "one\r\ntwo");
        assert_eq!(normalize_terminal_text("one\rtwo"), "one\r\ntwo");
        assert_eq!(
            normalize_terminal_text("\x1b[32m中文\x1b[0m\n第二行\r\n第三行\r末行"),
            "\x1b[32m中文\x1b[0m\r\n第二行\r\n第三行\r\n末行"
        );
    }

    #[test]
    fn soft_wraps_at_word_boundaries_instead_of_splitting_a_word() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(20);
        let mut out = wrap.push("The quick brown fox jumps over the lazy dog");
        out.push_str(&wrap.flush());
        assert_eq!(out, "The quick brown fox\r\njumps over the lazy\r\ndog");
        for row in out.split("\r\n") {
            assert!(
                row.chars().count() <= 20,
                "row wider than the terminal: {row}"
            );
        }
    }

    #[test]
    fn keeps_a_word_whole_across_streamed_chunks() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(20);
        let mut out = String::new();
        // A DSH answer arrives one text-delta at a time, so the row-end word must
        // survive the chunk boundary: "fo" + "x" is one word, not two.
        for delta in ["The quick brown fo", "x jumps"] {
            out.push_str(&wrap.push(delta));
        }
        out.push_str(&wrap.flush());
        assert_eq!(out, "The quick brown fox\r\njumps");
    }

    #[test]
    fn hard_folds_a_word_wider_than_the_row_at_the_row_edge() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(20);
        let mut out = wrap.push(&"a".repeat(25));
        out.push_str(&wrap.flush());
        assert_eq!(out, format!("{}\r\n{}", "a".repeat(20), "a".repeat(5)));
    }

    #[test]
    fn counts_cjk_glyphs_as_two_columns() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(20);
        let out = wrap.push("中文换行测试终端宽度对");
        assert_eq!(out, "中文换行测试终端宽度\r\n对");
        assert_eq!(terminal_char_width('中'), 2);
        assert_eq!(terminal_char_width('a'), 1);
        assert_eq!(terminal_char_width('\u{200b}'), 0);
    }

    #[test]
    fn moves_a_styled_word_to_the_next_row_with_its_escape_sequence() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(20);
        let mut out = wrap.push("0123456789012345 \x1b[32mgreen\x1b[0m");
        out.push_str(&wrap.flush());
        assert_eq!(out, "0123456789012345 \r\n\x1b[32mgreen\x1b[0m");
    }

    /// 推一段并立刻收尾:行尾未落地的单词也一起写出来,便于断言完整文本。
    fn push_and_flush(wrap: &mut TerminalWrap, text: &str) -> String {
        let mut out = wrap.push(text);
        out.push_str(&wrap.flush());
        out
    }

    #[test]
    fn collapses_runs_of_blank_lines_down_to_one() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        // 渲染分支各写一个 \r\n 前缀,markdown 正文自己又带空行,叠起来一屏
        // 白占好几行。
        assert_eq!(
            push_and_flush(&mut wrap, "first\n\n\n\n\nsecond"),
            "first\r\n\r\nsecond"
        );
    }

    #[test]
    fn drops_blank_lines_before_the_first_row() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        assert_eq!(push_and_flush(&mut wrap, "\n\n"), "");
        assert_eq!(push_and_flush(&mut wrap, "\r\nfirst"), "first");
        // 首屏之后的空行照常保留一个。
        assert_eq!(push_and_flush(&mut wrap, "\n\n\nsecond"), "\r\n\r\nsecond");
    }

    #[test]
    fn collapses_blank_lines_across_a_chunk_boundary() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        assert_eq!(push_and_flush(&mut wrap, "first\n"), "first\r\n");
        // 上一段已经以换行收尾,这一段开头的空行只能再补一个。
        assert_eq!(push_and_flush(&mut wrap, "\n\n\nsecond"), "\r\nsecond");
    }

    #[test]
    fn keeps_escape_sequences_from_a_dropped_blank_line() {
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        // 只含 SGR 的"空行"宽度为 0,但序列必须透传,否则颜色区间会被拆开。
        assert_eq!(
            push_and_flush(&mut wrap, "\x1b[32m\n\x1b[32mgreen\x1b[0m"),
            "\x1b[32m\x1b[32mgreen\x1b[0m"
        );
    }

    #[test]
    fn leaves_output_untouched_until_the_frontend_reports_a_width() {
        let mut wrap = TerminalWrap::default();
        let out = wrap.push("a very long line that nothing knows the width of\nsecond");
        assert_eq!(
            out,
            "a very long line that nothing knows the width of\r\nsecond"
        );
    }

    #[test]
    fn echoes_the_submitted_prompt_and_leaves_slash_commands_to_the_command_event() {
        assert_eq!(
            user_prompt_echo("hello\nworld", 0),
            Some("\r\n\x1b[1m❯\x1b[0m hello\r\n  world\r\n".to_string())
        );
        assert_eq!(user_prompt_echo("/permission read-only", 0), None);
        assert_eq!(user_prompt_echo("   ", 0), None);
        let with_images = user_prompt_echo("", 2).expect("an image-only prompt still echoes");
        assert!(with_images.contains("2 images"));
    }

    #[test]
    fn folds_a_terminal_result_into_one_verdict_row() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "title": "pnpm lint",
                "card": "terminal",
                "output": "2 problems\nsrc/a.ts:1 unused\n",
                "exitCode": 1,
            })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("✖"));
        assert!(rendered.contains("pnpm lint"));
        assert!(rendered.contains("exit 1"));
        // The first output line is the verdict's evidence; the rest of the body
        // stays collapsed and is only counted.
        assert!(rendered.contains("2 problems"));
        assert!(!rendered.contains("src/a.ts:1 unused"));
        assert!(rendered.contains("2 lines"));
        // One row, terminated with an explicit CR: the xterm view runs with
        // convertEol disabled, so a bare LF would staircase the output.
        assert_eq!(rendered.matches("\r\n").count(), 1);
        assert!(rendered.ends_with("\r\n"));
    }

    #[test]
    fn reads_a_terminal_result_with_both_fields_as_the_signal() {
        let rendered = render_tool_event_view(
            &result_payload(json!({ "card": "terminal", "exitCode": 0, "signal": "SIGKILL" })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("✖"));
        assert!(rendered.contains("SIGKILL"));
        assert!(!rendered.contains("exit"));
        assert!(rendered.contains("no output"));
    }

    #[test]
    fn folds_a_styled_command_body_without_leaking_its_escapes() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "terminal",
                "output": "\u{1b}[31mFAIL\u{1b}[0m src/a.test.ts\nstack…\n",
            })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("FAIL src/a.test.ts"));
        // A folded row must not carry the body's own colour spans or cursor
        // moves into the rows printed after it.
        assert_eq!(rendered.matches("\u{1b}[31m").count(), 0);
        assert_eq!(rendered.matches("\r\n").count(), 1);
    }

    #[test]
    fn folds_an_edit_into_a_path_and_a_line_count() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/a.ts", "oldText": "keep\nold\ntail", "newText": "keep\nnew\ntail" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/a.ts +1 -1"));
        // The hunk itself belongs to the session record, not to the terminal.
        assert!(!rendered.contains("new"));
        assert!(!rendered.contains("keep"));
    }

    #[test]
    fn folds_a_created_file_as_additions_only() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/new.ts", "oldText": null, "newText": "one\ntwo" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/new.ts (new file) +2 -0"));
    }

    #[test]
    fn folds_search_and_read_results_into_their_shape() {
        let matches = render_tool_event_view(
            &result_payload(json!({
                "card": "search",
                "shape": "matches",
                "truncated": true,
                "files": [{
                    "path": "src/a.ts",
                    "matches": [
                        { "lineNumber": 12, "line": "const x = 1;" },
                        { "lineNumber": 30, "line": "const y = 2;" },
                    ],
                }],
            })),
            "result",
        )
        .expect("search result renders");
        assert!(matches.contains("2 matches in src/a.ts"));
        assert!(!matches.contains("const x = 1;"));
        assert!(matches.contains("truncated by the harness"));
        assert_eq!(matches.matches("\r\n").count(), 1);

        let paths = render_tool_event_view(
            &result_payload(json!({
                "card": "search",
                "shape": "paths",
                "paths": ["src/a.ts", "src/b.ts"],
            })),
            "result",
        )
        .expect("search result renders");
        assert!(paths.contains("2 paths"));

        let read = render_tool_event_view(
            &result_payload(json!({
                "card": "read",
                "path": "src/a.ts",
                "offset": 40,
                "totalLines": 120,
                "lines": [{ "number": 40, "text": "line forty" }],
            })),
            "result",
        )
        .expect("read result renders");
        assert!(read.contains("src/a.ts — 1 of 120 lines from 40"));
        assert!(!read.contains("line forty"));
    }

    #[test]
    fn folds_web_search_sources_and_fetch_status() {
        let search = render_tool_event_view(
            &result_payload(json!({
                "card": "web",
                "kind": "search",
                "answer": "Yes.",
                "sources": [{ "url": "https://example.com/a", "title": "Example A" }],
            })),
            "result",
        )
        .expect("web search renders");
        assert!(search.contains("Yes."));
        assert!(search.contains("1 source"));
        assert!(!search.contains("https://example.com/a"));

        let fetch = render_tool_event_view(
            &result_payload(json!({ "card": "web", "kind": "fetch", "url": "https://example.com", "statusCode": 503 })),
            "result",
        )
        .expect("web fetch renders");
        assert!(fetch.contains("✖"));
        assert!(fetch.contains("503 https://example.com"));
    }

    #[test]
    fn folds_a_pending_call_into_one_header_row() {
        let terminal = render_tool_event_view(
            &call_payload(json!({
                "card": "terminal",
                "title": "Bash",
                "description": "pnpm lint\n--max-warnings 0",
                "cwd": "/repo",
            })),
            "call",
        )
        .expect("terminal call renders");
        assert!(terminal.contains("▸ Bash"));
        assert!(terminal.contains("pnpm lint"));
        // Only the first line of the command survives, and cwd belongs to the
        // expanded card dsh web keeps behind its disclosure.
        assert!(!terminal.contains("--max-warnings"));
        assert!(!terminal.contains("/repo"));
        // A call row opens with a blank separator and closes its own line.
        assert!(terminal.starts_with("\r\n"));
        assert_eq!(terminal.matches("\r\n").count(), 2);

        // The result repeats the change once applied, so a folded call row names
        // the files without printing any of the proposed hunk.
        let diff = render_tool_event_view(
            &call_payload(json!({
                "card": "diff",
                "title": "Edit a.ts",
                "diffs": [{ "path": "src/a.ts", "oldText": "old", "newText": "new" }],
            })),
            "call",
        )
        .expect("diff call renders");
        assert!(diff.contains("▸ Edit a.ts"));
        assert!(diff.contains("src/a.ts"));
        assert!(!diff.contains("+new"));
    }

    #[test]
    fn declines_a_view_that_addresses_the_other_event_kind_or_an_unknown_card() {
        assert!(
            render_tool_event_view(&result_payload(json!({ "card": "terminal" })), "call")
                .is_none()
        );
        assert!(render_tool_event_view(
            &call_payload(json!({ "card": "sparkline", "title": "x" })),
            "call"
        )
        .is_none());
        assert!(
            render_tool_event_view(&call_payload(json!({ "card": "terminal" })), "call").is_none()
        );
        // A generic result view with no replacement title says nothing the raw
        // event does not already carry.
        assert!(
            render_tool_event_view(&result_payload(json!({ "card": "generic" })), "result")
                .is_none()
        );
        assert!(
            render_tool_event_view(&json!({ "event": { "type": "tool/call" } }), "call").is_none()
        );
    }
}
