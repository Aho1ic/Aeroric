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
