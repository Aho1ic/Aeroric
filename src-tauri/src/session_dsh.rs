//! DeepSeek Harness(dsh)会话解析与发现。
//!
//! 目录布局(dsh `session-persistence-jsonl` 后端):
//!
//! ```text
//! <DSH_HOME>/sessions/--<项目路径 slug>--/<转义会话id>/session.jsonl[.zstd]
//! ```
//!
//! 后缀由插件的 `compression` 决定:`none` 落明文 `session.jsonl`,默认的 `zstd`
//! 落 `session.jsonl.zstd`。Aeroric 受管 patch 请求 `compression: none`,但只有
//! headless 走 `--patch`、web 走 home 层 `cordis.patch.yml`,而**已经存在的会话
//! 不会被改写**——所以同一个 root 里明文与压缩产物长期并存,读取端必须逐会话按
//! 后缀判定,不能全局假设一种编码。
//!
//! 首行为 header(`type: "session"`,带格式版本号);其后每行是
//! `{type, seq, time, data}` 事件。header 版本不等于本模块支持的版本时
//! **明确拒绝**(与 dsh 自身语义一致),而不是静默解析出错误内容。

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::session::{SessionContent, SessionMessage};
use crate::TaskManager;

/// 支持的 dsh 会话日志格式版本(`SESSION_FORMAT_VERSION`,dev preview 期为 0)。
pub(crate) const DSH_SESSION_FORMAT_VERSION: u64 = 0;

/// 明文 transcript 文件名(`compression: none`)。
pub(crate) const DSH_TRANSCRIPT_RAW: &str = "session.jsonl";
/// 压缩 transcript 文件名(插件默认 `compression: zstd`)。
pub(crate) const DSH_TRANSCRIPT_ZSTD: &str = "session.jsonl.zstd";

#[derive(Clone)]
pub(crate) struct DshSessionInfo {
    pub(crate) session_id: String,
    pub(crate) session_path: String,
}

// ── 路径规范化(与 dsh format.ts 逐字符对齐) ────────────────────────────────

/// dsh `projectKey` 的移植:分隔符(`/` `\` `:`)折叠为 `-`,安全字符
/// (ASCII 字母数字与 `._-`,不含 `~`)保留,其余按 UTF-16 code unit 转义为
/// `~XXXX`;去掉前导 `-`,空则用 `root`,截断到 251,再包上 `--`。
pub(crate) fn dsh_project_key(cwd: &str) -> String {
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
pub(crate) fn dsh_encode_segment(raw: &str) -> String {
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
pub(crate) fn dsh_transcript_in(session_dir: &Path) -> Option<PathBuf> {
    let raw = session_dir.join(DSH_TRANSCRIPT_RAW);
    if raw.is_file() {
        return Some(raw);
    }
    let compressed = session_dir.join(DSH_TRANSCRIPT_ZSTD);
    compressed.is_file().then_some(compressed)
}

/// 该 transcript 是否是 zstd 压缩产物。
fn is_zstd_transcript(path: &Path) -> bool {
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
pub(crate) fn read_dsh_session_lines(path: &Path) -> Result<Vec<String>, String> {
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
pub(crate) fn read_compressed_dsh_lines(path: &Path) -> Result<Option<Vec<String>>, String> {
    if !is_zstd_transcript(path) {
        return Ok(None);
    }
    read_dsh_session_lines(path).map(Some)
}

/// Resolve the canonical JSONL path for a known session id.  The Web API
/// returns the id only; Aeroric must derive the same path used by the official
/// `session-persistence-jsonl` backend so history/export/resume all converge on
/// one file instead of falling back to a PTY transcript.
///
/// 落盘前(`create` 不写任何字节,首个 `append` 才物化)两个候选都不存在,此时
/// 返回明文路径:注册发生在会话创建时,路径要能先登记下来,和插件 `locate()`
/// 可以在文件存在之前就返回目标的约定一致。
pub(crate) fn dsh_session_path_for(
    agent: &str,
    project_path: &str,
    session_id: &str,
) -> Option<PathBuf> {
    let session_dir =
        dsh_project_sessions_dir_for(agent, project_path)?.join(dsh_encode_segment(session_id));
    Some(dsh_transcript_in(&session_dir).unwrap_or_else(|| session_dir.join(DSH_TRANSCRIPT_RAW)))
}

/// 任意 dsh 族 agent(内建/自定义档案)的会话根目录。
pub(crate) fn dsh_sessions_root_for(agent: &str) -> Option<PathBuf> {
    Some(crate::dsh_home::dsh_home_for(agent).ok()?.join("sessions"))
}

/// 会话路径校验的允许根:所有 `agent-homes/*/sessions`(内建与 dsh-like 自定义
/// 档案共用隔离目录布局;claude/codex-like 的 home 里不存在 dsh 布局的会话,
/// 放宽到全部 agent home 不引入越权读取面)。
pub(crate) fn dsh_session_allowed_roots() -> Vec<PathBuf> {
    let Some(root) =
        crate::platform::home_dir().map(|home| home.join(".aeroric").join("agent-homes"))
    else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("sessions"))
        .filter(|path| path.is_dir())
        .collect()
}

/// 某项目在指定 agent 的 dsh 会话根下的目录。
pub(crate) fn dsh_project_sessions_dir_for(agent: &str, project_path: &str) -> Option<PathBuf> {
    Some(dsh_sessions_root_for(agent)?.join(dsh_project_key(project_path)))
}

/// 反查持有某个会话的 dsh 族 agent。
///
/// 会话按 `<home>/sessions/<projectKey>/<转义会话id>/session.jsonl[.zstd]` 落盘,
/// 而 home 目录名就是 agent id,所以归属可以纯靠磁盘判定——不需要对应的 `dsh web`
/// 实例还在跑,也不需要任务还是活跃状态。会话详情正是在任务结束后才打开的,
/// 少了这一步请求就会打到内置实例上换回 "session not found"。
pub(crate) fn dsh_agent_owning_session(session_id: &str) -> Option<String> {
    let session_dir = dsh_encode_segment(session_id);
    if session_dir.is_empty() {
        return None;
    }
    let root = crate::platform::home_dir()?
        .join(".aeroric")
        .join("agent-homes");
    for home in std::fs::read_dir(root).ok()?.flatten() {
        let Some(agent) = home.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // 目录名必须原样回推出同一个 home(排除被 sanitize 改写的名字),
        // 而且只认 dsh 族配置:其它 agent 家族的 home 里没有这套布局。
        if crate::dsh_home::dsh_home_for(&agent).ok().as_deref() != Some(&home.path())
            || !crate::app_settings::is_dsh_agent(&agent)
        {
            continue;
        }
        let Ok(projects) = std::fs::read_dir(home.path().join("sessions")) else {
            continue;
        };
        for project in projects.flatten() {
            if dsh_transcript_in(&project.path().join(&session_dir)).is_some() {
                return Some(agent);
            }
        }
    }
    None
}

// ── Header 与格式探测 ────────────────────────────────────────────────────────

fn parse_json(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line).ok()
}

/// 单行是否是 dsh 会话 header(`type: "session"` 且带数值 version)。
pub(crate) fn line_is_dsh_header(line: &str) -> bool {
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
pub(crate) fn read_dsh_session_header(path: &Path) -> Option<(String, i64)> {
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

// ── 事件 → SessionMessage 映射 ───────────────────────────────────────────────

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
pub(crate) fn parse_dsh_session_lines<S: AsRef<str>>(
    lines: &[S],
) -> Result<Vec<SessionMessage>, String> {
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

// ── 会话发现与 watcher ───────────────────────────────────────────────────────

/// 在项目会话目录下寻找 `since_ms` 之后创建的最新会话(不做认领过滤)。
pub(crate) fn newest_dsh_session_since(
    agent: &str,
    project_path: &str,
    since_ms: i64,
) -> Option<(String, PathBuf)> {
    let dir = dsh_project_sessions_dir_for(agent, project_path)?;
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(i64, String, PathBuf)> = None;
    for entry in entries.flatten() {
        let Some(path) = dsh_transcript_in(&entry.path()) else {
            continue;
        };
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

/// 在项目会话目录下寻找 `since_ms` 之后创建、且未被其他任务认领的最新会话。
pub(crate) fn discover_dsh_session_since(
    app: &AppHandle,
    agent: &str,
    project_path: &str,
    since_ms: i64,
) -> Option<(String, PathBuf)> {
    let dir = dsh_project_sessions_dir_for(agent, project_path)?;
    let entries = std::fs::read_dir(&dir).ok()?;
    let claimed = {
        let tm = app.state::<TaskManager>();
        let claimed = tm.claimed_session_paths.lock();
        claimed.clone()
    };
    let mut best: Option<(i64, String, PathBuf)> = None;
    for entry in entries.flatten() {
        let Some(path) = dsh_transcript_in(&entry.path()) else {
            continue;
        };
        if claimed.contains(path.to_string_lossy().as_ref()) {
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

/// dsh 会话 watcher:headless 进程启动后轮询发现新会话文件,认领并注册到
/// TaskManager,向前端广播 `task-session`(family = "dsh")。任务状态由退出码
/// 驱动(见 pty.rs),headless 下审批 fail-closed,无 input_required 流转,
/// 因此注册完成后 watcher 即退出。
pub(crate) fn spawn_dsh_session_watcher(
    app: AppHandle,
    task_id: String,
    agent: String,
    project_path: String,
    since_ms: i64,
) {
    thread::spawn(move || {
        // 会话文件在进程启动后立即创建;宽限 2 分钟覆盖冷启动(npm/node 慢启动)。
        for _ in 0..240 {
            {
                let tm = app.state::<TaskManager>();
                let still_running = tm.child_handles.lock().contains_key(&task_id);
                let already = tm.dsh_sessions.lock().contains_key(&task_id);
                if already {
                    return;
                }
                if !still_running {
                    // 进程已退出:再做最后一次发现,短会话(秒退)也能补挂。
                    if let Some((session_id, path)) =
                        discover_dsh_session_since(&app, &agent, &project_path, since_ms)
                    {
                        register_dsh_session(&app, &task_id, &session_id, &path);
                    }
                    return;
                }
            }
            if let Some((session_id, path)) =
                discover_dsh_session_since(&app, &agent, &project_path, since_ms)
            {
                register_dsh_session(&app, &task_id, &session_id, &path);
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
}

pub(crate) fn register_dsh_session(
    app: &AppHandle,
    task_id: &str,
    session_id: &str,
    session_path: &Path,
) {
    register_dsh_session_with_preset(app, task_id, session_id, session_path, None);
}

pub(crate) fn register_dsh_session_with_preset(
    app: &AppHandle,
    task_id: &str,
    session_id: &str,
    session_path: &Path,
    agent_preset: Option<&str>,
) {
    let path_string = session_path.to_string_lossy().into_owned();
    {
        let tm = app.state::<TaskManager>();
        let mut claimed = tm.claimed_session_paths.lock();
        if !claimed.insert(path_string.clone()) {
            return;
        }
        tm.dsh_sessions.lock().insert(
            task_id.to_string(),
            DshSessionInfo {
                session_id: session_id.to_string(),
                session_path: path_string.clone(),
            },
        );
    }
    let _ = app.emit(
        "task-session",
        serde_json::json!({
            "task_id": task_id,
            "session_id": session_id,
            "session_path": path_string,
            "codex_like": false,
            "family": "dsh",
            "agent_preset": agent_preset,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn project_key_matches_dsh_layout() {
        assert_eq!(dsh_project_key("/tmp/demo"), "--tmp-demo--");
        assert_eq!(dsh_project_key("C:\\work\\repo"), "--C-work-repo--");
        // 连续分隔符折叠、前导分隔符去除。
        assert_eq!(dsh_project_key("//tmp//x"), "--tmp-x--");
        // 非安全字符按 UTF-16 code unit 转义(CJK 单元)。
        assert_eq!(dsh_project_key("/tmp/同步"), "--tmp-~540C~6B65--");
        assert_eq!(dsh_project_key("/tmp/a b"), "--tmp-a~0020b--");
        assert_eq!(dsh_project_key("///"), "--root--");
    }

    #[test]
    fn encode_segment_escapes_unsafe_ids() {
        assert_eq!(dsh_encode_segment("abc-123_x.y"), "abc-123_x.y");
        assert_eq!(dsh_encode_segment(".."), "~002E~002E");
        assert_eq!(dsh_encode_segment("a/b"), "a~002Fb");
        assert_eq!(dsh_encode_segment("~"), "~007E");
    }

    fn header_line(version: u64) -> String {
        format!(
            r#"{{"type":"session","version":{version},"id":"sess-1","createdAt":1755100000000,"cwd":"/tmp/demo","delegationDepth":0}}"#
        )
    }

    #[test]
    fn parses_user_assistant_tool_events() {
        let lines = vec![
            header_line(0),
            r#"{"type":"turn/start","seq":0,"time":1,"data":{"turn":0}}"#.to_string(),
            r#"{"type":"user/message","seq":1,"time":2,"data":{"id":"m1","role":"user","content":[{"type":"text","text":"hello dsh"}],"source":{"kind":"human"}},"surfaceOp":"append"}"#.to_string(),
            r#"{"type":"assistant/message","seq":2,"time":3,"data":{"turn":0,"step":0,"message":{"id":"m2","role":"assistant","content":[{"type":"reasoning","text":"think"},{"type":"text","text":"hi"},{"type":"tool-call","id":"call-1","name":"bash","arguments":"{\"cmd\":\"ls\"}"}]},"usage":{"inputTokens":10,"outputTokens":5}},"surfaceOp":"append"}"#.to_string(),
            r#"{"type":"tool/result","seq":3,"time":4,"data":{"turn":0,"step":0,"message":{"id":"m3","role":"user","content":[{"type":"tool-result","toolCallId":"call-1","content":[{"type":"text","text":"file.txt"}]}]}},"surfaceOp":"append"}"#.to_string(),
            r#"{"type":"turn/end","seq":4,"time":5,"data":{"turn":0,"reason":{"kind":"completed"}}}"#.to_string(),
        ];
        let messages = parse_dsh_session_lines(&lines).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert!(
            matches!(&messages[0].content[0], SessionContent::Text { text } if text == "hello dsh")
        );
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].message_id.as_deref(), Some("m2"));
        assert!(
            matches!(&messages[1].content[0], SessionContent::Thinking { thinking } if thinking == "think")
        );
        assert!(matches!(&messages[1].content[1], SessionContent::Text { text } if text == "hi"));
        assert!(
            matches!(&messages[1].content[2], SessionContent::ToolUse { name, .. } if name == "bash")
        );
        assert_eq!(messages[2].role, "user");
        assert!(
            matches!(&messages[2].content[0], SessionContent::ToolResult { id, output } if id == "call-1" && output == "file.txt")
        );
    }

    #[test]
    fn refuses_future_format_versions_loudly() {
        let lines = vec![header_line(3)];
        let error = parse_dsh_session_lines(&lines).unwrap_err();
        assert!(error.contains("v3"));
        assert!(error.contains("upgrade"));
    }

    #[test]
    fn skips_unknown_and_boundary_events() {
        let lines = vec![
            header_line(0),
            r#"{"type":"todo/write","seq":0,"time":1,"data":{"todos":[]}}"#.to_string(),
            r#"{"type":"assistant/chunk","seq":1,"time":2,"data":{"turn":0,"step":0,"chunk":{"type":"text-delta","index":0,"text":"x"}}}"#.to_string(),
            r#"{"type":"future/event","seq":2,"time":3,"data":{},"ignorable":true}"#.to_string(),
        ];
        assert_eq!(parse_dsh_session_lines(&lines).unwrap().len(), 0);
    }

    /// 按插件真实的落盘方式写压缩 transcript:每次 append 一个独立 zstd frame,
    /// 物理文件是多个 frame 串接,而不是一个大 frame。
    fn write_zstd_frames(path: &Path, batches: &[&[String]]) {
        use std::io::Write;
        let mut file = File::create(path).expect("transcript is writable");
        for batch in batches {
            let mut text = String::new();
            for line in *batch {
                text.push_str(line);
                text.push('\n');
            }
            let frame = zstd::encode_all(text.as_bytes(), 0).expect("frame encodes");
            file.write_all(&frame).expect("frame is writable");
        }
    }

    fn temp_session_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aeroric-dsh-transcript-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("session dir is creatable");
        dir
    }

    #[test]
    fn reads_every_zstd_frame_of_a_compressed_transcript() {
        let dir = temp_session_dir("frames");
        let body: Vec<String> = (0..40)
            .map(|seq| {
                format!(
                    r#"{{"type":"turn/start","seq":{seq},"time":{seq},"data":{{"turn":{seq}}}}}"#
                )
            })
            .collect();
        // header 单独一帧,正文分批追加——只解第一帧就只剩 header,正文全丢。
        write_zstd_frames(
            &dir.join(DSH_TRANSCRIPT_ZSTD),
            &[&[header_line(0)], &body[..20], &body[20..]],
        );
        let path = dsh_transcript_in(&dir).expect("compressed transcript is found");
        assert!(path.ends_with(DSH_TRANSCRIPT_ZSTD));
        let lines = read_dsh_session_lines(&path).expect("transcript decodes");
        assert_eq!(lines.len(), 41);
        assert!(line_is_dsh_header(&lines[0]));
        assert_eq!(lines[40], body[39]);
        assert_eq!(
            read_compressed_dsh_lines(&path).expect("decodes"),
            Some(lines)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_the_plaintext_transcript_and_reads_it_as_lines() {
        let dir = temp_session_dir("plaintext");
        let raw = dir.join(DSH_TRANSCRIPT_RAW);
        fs::write(&raw, format!("{}\n", header_line(0))).expect("raw transcript is writable");
        write_zstd_frames(&dir.join(DSH_TRANSCRIPT_ZSTD), &[&[header_line(0)]]);
        // 两种编码可以同时存在(改过 compression 的 home),明文优先。
        let path = dsh_transcript_in(&dir).expect("transcript is found");
        assert!(path.ends_with(DSH_TRANSCRIPT_RAW));
        assert_eq!(read_dsh_session_lines(&path).unwrap().len(), 1);
        // 明文不走解压分支,调用方据此回落到按字节分页的读取。
        assert_eq!(read_compressed_dsh_lines(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_no_transcript_before_the_first_append() {
        let dir = temp_session_dir("empty");
        assert_eq!(dsh_transcript_in(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_dsh_header_lines() {
        assert!(line_is_dsh_header(&header_line(0)));
        assert!(!line_is_dsh_header(r#"{"type":"session_meta"}"#));
        assert!(!line_is_dsh_header("not json"));
    }
}
