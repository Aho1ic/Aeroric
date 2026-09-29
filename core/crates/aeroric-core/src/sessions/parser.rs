//! 一行文本 → `SessionMessage` 的分派，与两族共用的附件抽取。
//!
//! 迁移自宿主 `src-tauri/src/session/parse.rs`（`parse_session_lines`／
//! `parse_session_line`／`attachment_from_value` 三个函数逐字搬运；
//! Claude 面见 [`crate::sessions::claude`]，Codex 面见 [`crate::sessions::codex`]）。

use crate::sessions::claude::parse_claude_session_line;
use crate::sessions::codex::parse_codex_session_line;
use crate::sessions::SessionContent;
use crate::sessions::SessionMessage;

/// 按 `is_codex` 分派逐行解析。签名与宿主的 `pub(crate) fn parse_session_lines` 一致。
pub fn parse_session_lines(lines: &[String], is_codex: bool, messages: &mut Vec<SessionMessage>) {
    for line in lines {
        parse_session_line(line, is_codex, messages);
    }
}

pub fn parse_session_line(line: &str, is_codex: bool, messages: &mut Vec<SessionMessage>) {
    if is_codex {
        parse_codex_session_line(line, messages);
    } else {
        parse_claude_session_line(line, messages);
    }
}

/// 从附件 JSON 值抽取可展示的 `Attachment` 块。两族的形状不同，键名候选集照宿主原文。
pub fn attachment_from_value(value: &serde_json::Value) -> Option<SessionContent> {
    let source_value = value.get("source").unwrap_or(value);
    let media_type = value
        .get("media_type")
        .or_else(|| value.get("mediaType"))
        .or_else(|| source_value.get("media_type"))
        .or_else(|| source_value.get("mediaType"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let name = value
        .get("name")
        .or_else(|| value.get("file_name"))
        .or_else(|| value.get("filename"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("attachment")
        .to_string();
    let source = source_value
        .get("url")
        .or_else(|| source_value.get("image_url"))
        .or_else(|| source_value.get("path"))
        .or_else(|| source_value.get("file_path"))
        .or_else(|| source_value.get("filePath"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            source_value
                .get("data")
                .and_then(serde_json::Value::as_str)
                .map(|data| format!("data:{media_type};base64,{data}"))
        })?;
    Some(SessionContent::Attachment {
        name,
        media_type,
        source,
    })
}

/// 格式探测只看文件**开头**这么多行（宿主 `session.rs` 的同名常量逐字迁移，第四片）。
pub const SESSION_FORMAT_DETECTION_LINES: usize = 200;

/// 单行是否带 Codex 专有的事件标记。供整段探测与流式探测共用。
pub fn line_is_codex_format(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|val| {
            val.get("type")
                .and_then(|v| v.as_str())
                .map(|kind| matches!(kind, "session_meta" | "event_msg"))
        })
        .unwrap_or(false)
}

/// 在探测窗内找得到 Codex 事件标记，就算 Codex 格式。
pub fn is_codex_format<S: AsRef<str>>(lines: &[S]) -> bool {
    lines
        .iter()
        .take(SESSION_FORMAT_DETECTION_LINES)
        .any(|line| line_is_codex_format(line.as_ref()))
}

pub fn parse_session_messages(lines: &[String], prefer_codex: bool) -> Vec<SessionMessage> {
    parse_session_messages_with_format(lines, prefer_codex, is_codex_format(lines))
}

/// 与 `parse_session_messages` 相同,但允许调用方给出已经算好的格式探测结论。
/// 尾部截断读取时,开头的 `session_meta` 行可能已不在 `lines` 里,必须由流式扫描
/// 阶段的探测结果补上,否则 Codex 会话会被误判成 Claude 格式而解析为空。
pub fn parse_session_messages_with_format(
    lines: &[String],
    prefer_codex: bool,
    detected_codex: bool,
) -> Vec<SessionMessage> {
    let primary_is_codex = detected_codex || prefer_codex;
    let mut messages = Vec::new();
    parse_session_lines(lines, primary_is_codex, &mut messages);

    if messages.is_empty() && primary_is_codex != detected_codex {
        parse_session_lines(lines, detected_codex, &mut messages);
    }

    messages
}
