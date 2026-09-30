//! DSH 终端 projection 的行生成原语（P3-05 第七片，Action 1 的 `projection` 前身）。
//!
//! 逐字迁自宿主 `src-tauri/src/dsh_webui/terminal_render.rs` 的无状态一半：折叠摘要、
//! 回显行、ANSI 剥离与宽度裁剪。**Action 2 的边界随迁言明：DSH terminal projection
//! 只是适配输出，不能借用到 OMP**——本文件服务 dsh Web-API 会话的 xterm 视图
//! （`convertEol: false`，每行显式 CRLF 收尾），不是通用终端模型。
//! 有状态的两台（`TerminalWrap` 软换行、`ReasoningFold` 折叠累加器）仍留宿主，
//! 它们随 executor 接线片再议；`ANSI_GREEN`／`ANSI_RED` 被宿主的
//! `render_tool_event_view` 消费，也留宿主。

use serde_json::Value;

pub const ANSI_RESET: &str = "\x1b[0m";
pub const ANSI_DIM: &str = "\x1b[2m";
pub const ANSI_BOLD: &str = "\x1b[1m";

/// Widest folded fragment the bridge prints. Rows stay well inside a narrow
/// split pane, so a folded summary never wraps into a second terminal line.
pub const FOLD_SUMMARY_CHARS: usize = 96;

/// Non-empty string field, matching how the harness treats a blank title.
pub fn view_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

pub fn push_line(out: &mut String, style: &str, text: &str) {
    out.push_str(style);
    out.push_str(text);
    if !style.is_empty() {
        out.push_str(ANSI_RESET);
    }
    out.push_str("\r\n");
}

/// 用户输入在终端里的回显行。Web API 会话没有 PTY,终端不会自动回显,不自己
/// 打一行的话发出去的内容在终端里完全看不到。
///
/// 斜杠命令由 `command/run` 事件负责回显,这里跳过,避免同一条输入出现两次。
pub fn user_prompt_echo(prompt: &str, image_count: usize) -> Option<String> {
    let text = prompt.trim();
    if text.starts_with('/') {
        return None;
    }
    if text.is_empty() && image_count == 0 {
        return None;
    }
    let attachment = match image_count {
        0 => String::new(),
        1 => format!(" {ANSI_DIM}· 1 image{ANSI_RESET}"),
        count => format!(" {ANSI_DIM}· {count} images{ANSI_RESET}"),
    };
    // 续行缩进两格,与首行的 "❯ " 对齐。
    let body = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n  ");
    let mut out = String::from("\r\n");
    push_line(
        &mut out,
        "",
        &format!("{ANSI_BOLD}❯{ANSI_RESET} {body}{attachment}"),
    );
    Some(out)
}

/// Drop ANSI escape sequences and control bytes from one line. Tool output is
/// frequently styled, and a folded row that carried a cursor move or a
/// half-open colour span would corrupt every row printed after it.
pub fn plain_single_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\x1b' {
            if character == '\t' {
                out.push(' ');
            } else if !character.is_control() {
                out.push(character);
            }
            continue;
        }
        match characters.peek() {
            // CSI: parameters and intermediates, terminated by a final byte.
            Some('[') => {
                let _ = characters.next();
                for next in characters.by_ref() {
                    if ('\x40'..='\x7e').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: terminated by BEL or by ST (ESC \).
            Some(']') => {
                let _ = characters.next();
                let mut saw_escape = false;
                for next in characters.by_ref() {
                    if saw_escape || next == '\x07' {
                        break;
                    }
                    saw_escape = next == '\x1b';
                }
            }
            _ => {
                let _ = characters.next();
            }
        }
    }
    out.trim().to_string()
}

pub fn clip_fragment(text: &str) -> String {
    if text.chars().count() <= FOLD_SUMMARY_CHARS {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(FOLD_SUMMARY_CHARS).collect();
    clipped.push('…');
    clipped
}

/// Reduce a possibly long, possibly styled body to the single line a folded row
/// can carry: the first line with content, stripped of control bytes and
/// clipped. `None` means the body had nothing to say.
pub fn fold_summary(text: &str) -> Option<String> {
    text.split('\n')
        .map(plain_single_line)
        .find(|line| !line.is_empty())
        .map(|line| clip_fragment(&line))
}

/// Lines with content, used as a folded row's size hint. Blank lines are the
/// padding of a body that is not being printed, so they are not counted.
pub fn count_content_lines(text: &str) -> usize {
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .count()
}

pub fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}
