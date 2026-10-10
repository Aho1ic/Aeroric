//! 会话读取层（P3-04 第四片）：字节游标反向分页与尾部行数窗口。
//!
//! 逐字迁移自宿主 `src-tauri/src/session.rs` 的 `SessionMessagePage`、
//! `SESSION_MESSAGE_PAGE_LINES`、`SESSION_MESSAGE_READ_CHUNK_BYTES`、
//! `read_session_page_lines`、`MAX_SESSION_LINES`、`read_session_tail`，
//! 行为一行未改。两处刻意的形状变化只有：① 页类型的字段由私有改 `pub`
//! （宿主命令在迁移后仍要构造它，serde 输出 `camelCase` 逐字节不变）；
//! ② 可见性升为 `pub`。**路径校验留在宿主**——`validate_session_path_for`
//! 依赖宿主的 `AgentFamily` 与允许根（唯一真相，同第二片"家目录由调用方
//! 注入"的形状，补记二百四十二），core 拿到的是已过校验的 canonical 绝对路径。
//! 三个读取上限（页 1 000 行、反向块 64 KiB、尾部 20 000 行）随代码逐字迁移，
//! Action 1「保留既有路径校验与读取上限」的上限半边在本文件、校验半边在宿主闸门。

use crate::sessions::parser::{line_is_codex_format, SESSION_FORMAT_DETECTION_LINES};
use crate::sessions::SessionMessage;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessagePage {
    pub messages: Vec<SessionMessage>,
    pub next_cursor: Option<u64>,
    pub has_more: bool,
}

pub const SESSION_MESSAGE_PAGE_LINES: usize = 1_000;
const SESSION_MESSAGE_READ_CHUNK_BYTES: u64 = 64 * 1024;

/// 从 `cursor` 之前反向读取最多 `limit` 条完整 JSONL 记录。
/// 返回的行仍按时间正序排列，`next_cursor` 指向本页最早一行的字节起点。
pub fn read_session_page_lines(
    path: &Path,
    cursor: Option<u64>,
    limit: usize,
) -> Result<(Vec<String>, Option<u64>, bool), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let file_len = file.metadata().map_err(|error| error.to_string())?.len();
    let end = cursor.unwrap_or(file_len).min(file_len);
    if end == 0 || limit == 0 {
        return Ok((Vec::new(), None, false));
    }

    let mut start = end;
    let mut bytes = Vec::new();
    loop {
        let next_start = start.saturating_sub(SESSION_MESSAGE_READ_CHUNK_BYTES);
        let chunk_len = (start - next_start) as usize;
        let mut chunk = vec![0u8; chunk_len];
        file.seek(SeekFrom::Start(next_start))
            .map_err(|error| error.to_string())?;
        file.read_exact(&mut chunk)
            .map_err(|error| error.to_string())?;
        chunk.extend(bytes);
        bytes = chunk;
        start = next_start;

        let complete_line_count = bytes
            .split(|byte| *byte == b'\n')
            .enumerate()
            .filter(|(index, line)| {
                (start == 0 || *index > 0) && !line.iter().all(u8::is_ascii_whitespace)
            })
            .count();
        if complete_line_count >= limit || start == 0 {
            break;
        }
    }

    let mut ranges = Vec::new();
    let mut line_start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        if (start == 0 || line_start > 0)
            && !bytes[line_start..index].iter().all(u8::is_ascii_whitespace)
        {
            ranges.push((line_start, index));
        }
        line_start = index + 1;
    }
    if line_start < bytes.len()
        && (start == 0 || line_start > 0)
        && !bytes[line_start..].iter().all(u8::is_ascii_whitespace)
    {
        ranges.push((line_start, bytes.len()));
    }

    if ranges.is_empty() && start == 0 {
        return Ok((Vec::new(), None, false));
    }

    let first = ranges.len().saturating_sub(limit);
    let selected = &ranges[first..];
    let page_start = selected
        .first()
        .map(|(range_start, _)| start + *range_start as u64)
        .unwrap_or(end);
    let mut lines = Vec::with_capacity(selected.len());
    for (range_start, range_end) in selected {
        let line = std::str::from_utf8(&bytes[*range_start..*range_end])
            .map_err(|error| format!("Session file is not valid UTF-8: {error}"))?;
        lines.push(line.trim_end_matches('\r').to_string());
    }
    let has_more = page_start > 0;

    Ok((lines, has_more.then_some(page_start), has_more))
}

/// 单次读取会话记录时保留的最大行数(按尾部保留)。长会话的 JSONL 可达数百 MB,
/// 整体读进内存会让桌面进程 RSS 随会话线性膨胀;Agent 交接与 UI 回看都只需要最近的
/// 上下文,因此只保留尾部。取 20000 行:够覆盖一次交接所需的近期对话,又把峰值
/// 内存钳在可控范围。
pub const MAX_SESSION_LINES: usize = 20_000;

/// 流式读取 JSONL,只在内存里保留最后 `MAX_SESSION_LINES` 行。
///
/// 格式探测(`is_codex_format`)只看**开头**若干行,而尾部窗口可能已经把它们丢掉,
/// 所以这里在流式扫描时顺便对前 `SESSION_FORMAT_DETECTION_LINES` 行做探测,
/// 把结论单独返回,避免为了探测而全量驻留。
pub fn read_session_tail(path: &Path) -> Result<(Vec<String>, bool), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    // VecDeque 做固定容量环形窗口,超出容量时从头部弹出,保证尾部 N 行。
    let mut tail: VecDeque<String> = VecDeque::with_capacity(MAX_SESSION_LINES.min(1024));
    let mut scanned = 0usize;
    let mut detected_codex = false;

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if scanned < SESSION_FORMAT_DETECTION_LINES {
            scanned += 1;
            if !detected_codex && line_is_codex_format(&line) {
                detected_codex = true;
            }
        }
        if tail.len() == MAX_SESSION_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    Ok((Vec::from(tail), detected_codex))
}
