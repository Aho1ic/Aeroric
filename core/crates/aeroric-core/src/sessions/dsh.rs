//! dsh 族的路径层（P3-04 第二片的 dsh 半边）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session_dsh.rs` 的常量、`dsh_project_key`、
//! `dsh_encode_segment`、`dsh_transcript_in`。目录布局与"两形态并存"的原因
//! 保留原文；读侧解压（`read_compressed_dsh_lines`）与转录解析属 dsh 片，
//! **core 今天不新增 zstd 依赖**——手写约束 8 要求先核对宿主用的是哪个 crate
//! 再决定收编，见补记二百四十二。

use std::path::{Path, PathBuf};

/// 支持的 dsh 会话日志格式版本(`SESSION_FORMAT_VERSION`,dev preview 期为 0)。
pub const DSH_SESSION_FORMAT_VERSION: u64 = 0;

/// 明文 transcript 文件名(`compression: none`)。
pub const DSH_TRANSCRIPT_RAW: &str = "session.jsonl";
/// 压缩 transcript 文件名(插件默认 `compression: zstd`)。
pub const DSH_TRANSCRIPT_ZSTD: &str = "session.jsonl.zstd";

/// dsh `projectKey` 的移植:分隔符(`/` `\` `:`)折叠为 `-`,安全字符
/// (ASCII 字母数字与 `._-`,不含 `~`)保留,其余按 UTF-16 code unit 转义为
/// `~XXXX`;去掉前导 `-`,空则用 `root`,截断到 251,再包上 `--`。
pub fn dsh_project_key(cwd: &str) -> String {
    let mut readable = String::new();
    let mut separator_run = false;
    for code in cwd.encode_utf16() {
        match char::from_u32(u32::from(code)) {
            Some('/' | '\\' | ':') => {
                if !separator_run {
                    readable.push('-');
                }
                separator_run = true;
            }
            Some(ch)
                if ch != '~' && (ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) =>
            {
                readable.push(ch);
                separator_run = false;
            }
            _ => {
                readable.push('~');
                readable.push_str(&format!("{code:04X}"));
                separator_run = false;
            }
        }
    }
    let slug = readable.trim_start_matches('-');
    let slug = if slug.is_empty() { "root" } else { slug };
    // readable 只含 ASCII(非安全字符已转义),按字节截断即等价于 JS 的 slice(0, 251)。
    let truncated = &slug[..slug.len().min(251)];
    format!("--{truncated}--")
}

/// dsh `encodeSegment` 的移植:会话 id → 安全目录名(`.`/`..` 特判,
/// 安全字符保留,其余 `~XXXX`)。
pub fn dsh_encode_segment(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if raw == "." {
        return "~002E".to_string();
    }
    if raw == ".." {
        return "~002E~002E".to_string();
    }
    let mut out = String::new();
    for code in raw.encode_utf16() {
        match char::from_u32(u32::from(code)) {
            Some(ch)
                if ch != '~' && (ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) =>
            {
                out.push(ch);
            }
            _ => {
                out.push('~');
                out.push_str(&format!("{code:04X}"));
            }
        }
    }
    out
}

/// 会话目录里实际存在的 transcript。
///
/// 一个 root 只属于一种编码(插件在发现阶段就会拒绝相反的后缀),但 Aeroric
/// 把 `compression` 从 zstd 切到 none 之后,同一 root 里旧的压缩产物仍然留在
/// 磁盘上,所以这里按"存在即唯一"逐会话判定:明文优先,其次压缩,都不在返回
/// `None`。
pub fn dsh_transcript_in(session_dir: &Path) -> Option<PathBuf> {
    let raw = session_dir.join(DSH_TRANSCRIPT_RAW);
    if raw.is_file() {
        return Some(raw);
    }
    let compressed = session_dir.join(DSH_TRANSCRIPT_ZSTD);
    compressed.is_file().then_some(compressed)
}

// ── 读侧与解析（第三片，2026-09-29 补记二百四十三）──────────────────────────
// 以下全部逐字迁移自宿主 `src-tauri/src/session_dsh.rs`，行为零改动。
// zstd 用与宿主同一个 crate（`zstd = \"0.13\"`，手写约束 8 的"不引入第二份实现"）。

use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

use crate::sessions::SessionContent;
use crate::sessions::SessionMessage;

/// 该 transcript 是否是 zstd 压缩产物。
pub fn is_zstd_transcript(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zstd"))
}

/// 读出 transcript 的全部逻辑行。
///
/// 压缩产物是"多个独立 zstd frame 串接"(header 一帧,之后每个持久化批次一帧)。
/// `zstd::stream::read::Decoder` 默认就会一直串接到 EOF(`single_frame()` 才是
/// 只解第一帧),正好对上这个物理格式——**不要**加 `single_frame()`,否则只能拿到
/// header 行、正文全丢。
pub fn read_dsh_session_lines(path: &Path) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    if !is_zstd_transcript(path) {
        return BufReader::new(file)
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
    }
    let mut text = String::new();
    zstd::stream::read::Decoder::new(file)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text.lines().map(str::to_owned).collect())
}

/// 压缩 transcript 的全部行;明文 transcript 返回 `None`,让调用方保留自己的
/// 按字节分页 / 尾部截断读法。
pub fn read_compressed_dsh_lines(path: &Path) -> Result<Option<Vec<String>>, String> {
    if !is_zstd_transcript(path) {
        return Ok(None);
    }
    read_dsh_session_lines(path).map(Some)
}

fn parse_json(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line).ok()
}

/// 单行是否是 dsh 会话 header(`type: \"session\"` 且带数值 version)。
pub fn line_is_dsh_header(line: &str) -> bool {
    parse_json(line)
        .map(|value| {
            value.get("type").and_then(Value::as_str) == Some("session")
                && value.get("version").and_then(Value::as_u64).is_some()
                && value.get("id").and_then(Value::as_str).is_some()
        })
        .unwrap_or(false)
}

/// header 版本校验:遇到未来版本给出"升级 Aeroric"的明确错误(fail-loud)。
fn check_dsh_header(value: &Value) -> Result<(), String> {
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Corrupt dsh session log: header has no format version".to_string())?;
    if version != DSH_SESSION_FORMAT_VERSION {
        return Err(format!(
            "This DeepSeek Harness session uses log format v{version}, which this version of Aeroric cannot read; please upgrade Aeroric"
        ));
    }
    Ok(())
}

/// 读取 header 行中的会话 id 与创建时间。
fn header_meta(value: &Value) -> Option<(String, i64)> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    let created_at = value.get("createdAt").and_then(Value::as_i64)?;
    Some((id, created_at))
}

/// 读 transcript 首行的 header。压缩产物的 header 单独占一帧,解码器会先吐出它,
/// 所以两种编码都只要第一行——只是压缩路径没法按行 seek,交给统一读取器。
pub fn read_dsh_session_header(path: &Path) -> Option<(String, i64)> {
    let first_line = if is_zstd_transcript(path) {
        read_dsh_session_lines(path).ok()?.into_iter().next()?
    } else {
        let mut line = String::new();
        BufReader::new(File::open(path).ok()?)
            .read_line(&mut line)
            .ok()?;
        line
    };
    let value = parse_json(first_line.trim())?;
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    header_meta(&value)
}

/// ContentBlock 数组 → SessionContent 列表。
/// text → Text、reasoning → Thinking、tool-call → ToolUse;image 以占位文本呈现
/// (dsh 的 image block 是 attachment service 的内容寻址引用,不是本地路径)。
fn map_content_blocks(blocks: &[Value]) -> Vec<SessionContent> {
    let mut parts = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                if !text.trim().is_empty() {
                    parts.push(SessionContent::Text {
                        text: text.to_string(),
                    });
                }
            }
            Some("reasoning") => {
                let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                if !text.trim().is_empty() {
                    parts.push(SessionContent::Thinking {
                        thinking: text.to_string(),
                    });
                }
            }
            Some("tool-call") => {
                parts.push(SessionContent::ToolUse {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    input: block
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                });
            }
            Some("image") => {
                parts.push(SessionContent::Text {
                    text: "[image attachment]".to_string(),
                });
            }
            _ => {}
        }
    }
    parts
}

/// tool-result block 的模型可见输出拼接为文本。
fn tool_result_output(content: &[Value]) -> String {
    content
        .iter()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_text_message(role: &str, text: String, messages: &mut Vec<SessionMessage>) {
    if text.trim().is_empty() {
        return;
    }
    messages.push(SessionMessage {
        role: role.to_string(),
        content: vec![SessionContent::Text { text }],
        message_id: None,
    });
}

fn value_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(value_text)
                        .collect::<Vec<_>>()
                        .join("")
                })
                .filter(|text| !text.is_empty())
        })
}

fn push_dsh_event(value: &Value, messages: &mut Vec<SessionMessage>) {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    let data = value.get("data").unwrap_or(&Value::Null);
    match kind {
        "user/message" => {
            let blocks = data
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let parts = map_content_blocks(&blocks);
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "user".to_string(),
                    content: parts,
                    message_id: None,
                });
            }
        }
        "assistant/message" => {
            let blocks = data
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let parts = map_content_blocks(&blocks);
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "assistant".to_string(),
                    content: parts,
                    message_id: data
                        .get("message")
                        .and_then(|message| message.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
            }
        }
        // 与 Claude 解析器约定一致:tool result 作为 user 角色消息呈现。
        // tool/call 事件跳过——assistant/message 的 content 已含 tool-call block。
        "tool/result" => {
            let Some(blocks) = data
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
            else {
                return;
            };
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("tool-result") {
                    continue;
                }
                let id = block
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let inner = block
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut output = tool_result_output(&inner);
                if block.get("isError").and_then(Value::as_bool) == Some(true) && !output.is_empty()
                {
                    output = format!("[error] {output}");
                }
                messages.push(SessionMessage {
                    role: "user".to_string(),
                    content: vec![SessionContent::ToolResult { id, output }],
                    message_id: None,
                });
            }
        }
        "command/run" => {
            let name = data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("command");
            let args = data.get("args").and_then(Value::as_str).unwrap_or("");
            push_text_message("user", format!("/{name}{args}"), messages);
        }
        "command/done" => {
            let text = value_text(data).unwrap_or_else(|| "Command completed".to_string());
            push_text_message("assistant", text, messages);
        }
        "compaction/summary" => {
            let text = value_text(data)
                .map(|summary| format!("[compaction summary]\n{summary}"))
                .unwrap_or_else(|| "[conversation compacted]".to_string());
            push_text_message("assistant", text, messages);
        }
        "compaction/start" | "compaction/end" | "compaction/prune" => {
            push_text_message("assistant", format!("[{kind}]"), messages);
        }
        "subagent/descriptor" => {
            let label = data
                .get("name")
                .or_else(|| data.get("agent"))
                .and_then(Value::as_str)
                .unwrap_or("subagent");
            push_text_message("assistant", format!("[subagent: {label}]"), messages);
        }
        "subagent/start" | "subagent/end" | "subagent/message" => {
            let text = value_text(data).unwrap_or_else(|| format!("[{kind}]"));
            push_text_message("assistant", text, messages);
        }
        "tool-workflow/run-start"
        | "tool-workflow/run-end"
        | "tool-workflow/agent-start"
        | "tool-workflow/agent-end" => {
            let text = value_text(data).unwrap_or_else(|| format!("[{kind}]"));
            push_text_message("assistant", text, messages);
        }
        // turn/step boundaries, chunks, todo projections and request headers
        // are either live-only UI state or already represented by messages.
        _ => {}
    }
}

/// 解析 dsh 会话 JSONL(可能含 header 行,也可能是尾部截断后的纯事件行)。
/// 遇到未来格式版本的 header 返回明确错误。
pub fn parse_dsh_session_lines<S: AsRef<str>>(lines: &[S]) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let Some(value) = parse_json(line) else {
            continue;
        };
        if index == 0 && value.get("type").and_then(Value::as_str) == Some("session") {
            check_dsh_header(&value)?;
            continue;
        }
        push_dsh_event(&value, &mut messages);
    }
    Ok(messages)
}

/// 在项目会话目录下寻找 `since_ms` 之后创建、可选跳过已认领的最新会话。
///
/// 宿主的两条入口（`newest_dsh_session_since` 不做认领过滤、watcher 的
/// `discover_dsh_session_since` 必须过滤）在这里收敛成一个 `claimed: Option<&HashSet<String>>`
/// 参数——判定顺序与原文逐字一致（`created_at < since_ms` 跳过、`created_at > *at` 才换主）。
pub fn newest_dsh_session_in_dir(
    dir: &Path,
    since_ms: i64,
    claimed: Option<&HashSet<String>>,
) -> Option<(String, PathBuf)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(i64, String, PathBuf)> = None;
    for entry in entries.flatten() {
        let Some(path) = dsh_transcript_in(&entry.path()) else {
            continue;
        };
        if claimed.is_some_and(|claimed| claimed.contains(path.to_string_lossy().as_ref())) {
            continue;
        }
        let Some((session_id, created_at)) = read_dsh_session_header(&path) else {
            continue;
        };
        if created_at < since_ms {
            continue;
        }
        if best
            .as_ref()
            .map(|(at, _, _)| created_at > *at)
            .unwrap_or(true)
        {
            best = Some((created_at, session_id, path));
        }
    }
    best.map(|(_, id, path)| (id, path))
}
