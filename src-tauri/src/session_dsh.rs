//! DeepSeek Harness(dsh)会话解析与发现。
//!
//! 目录布局(dsh `session-persistence-jsonl` 后端,Aeroric 受管 patch 固定
//! `compression: none` + `packChunks: false`,因此 transcript 是"一行一事件"的
//! 明文 JSONL):
//!
//! ```text
//! <DSH_HOME>/sessions/--<项目路径 slug>--/<转义会话id>/session.jsonl
//! ```
//!
//! 首行为 header(`type: "session"`,带格式版本号);其后每行是
//! `{type, seq, time, data}` 事件。header 版本不等于本模块支持的版本时
//! **明确拒绝**(与 dsh 自身语义一致),而不是静默解析出错误内容。

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::session::{SessionContent, SessionMessage};
use crate::TaskManager;

/// 支持的 dsh 会话日志格式版本(`SESSION_FORMAT_VERSION`,dev preview 期为 0)。
pub(crate) const DSH_SESSION_FORMAT_VERSION: u64 = 0;

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
#[cfg(test)]
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

pub(crate) fn read_dsh_session_header(path: &Path) -> Option<(String, i64)> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;
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
                });
            }
        }
        // 与 Claude 解析器约定一致:tool result 作为 user 角色消息呈现。
        // tool/call 事件跳过——assistant/message 的 content 已含 tool-call block。
        "tool/result" => {
            let Some(block) = data
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
                .and_then(|content| content.first())
            else {
                return;
            };
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
            if block.get("isError").and_then(Value::as_bool) == Some(true) && !output.is_empty() {
                output = format!("[error] {output}");
            }
            messages.push(SessionMessage {
                role: "user".to_string(),
                content: vec![SessionContent::ToolResult { id, output }],
            });
        }
        // turn/step 边界、chunk、todo、request header 等事件不进入会话视图;
        // subagent/compaction 的专属展示在 Phase 6 接入。
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
        let path = entry.path().join("session.jsonl");
        if !path.is_file() {
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
        let path = entry.path().join("session.jsonl");
        if !path.is_file() {
            continue;
        }
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
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn detects_dsh_header_lines() {
        assert!(line_is_dsh_header(&header_line(0)));
        assert!(!line_is_dsh_header(r#"{"type":"session_meta"}"#));
        assert!(!line_is_dsh_header("not json"));
    }
}
