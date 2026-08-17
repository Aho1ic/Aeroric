use std::collections::{HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

use crate::TaskManager;

mod export;

use export::export_session_markdown_inner;
pub use export::ExportTaskMeta;
#[cfg(test)]
use export::{validate_export_output_path, write_export_markdown};

#[derive(Clone)]
pub(crate) struct CodexSessionInfo {
    pub(crate) session_id: String,
    pub(crate) session_path: String,
}

#[derive(Clone)]
pub(crate) struct ClaudeSessionInfo {
    pub(crate) session_id: String,
    pub(crate) session_path: String,
    /// true 表示 lazy attach 预先注入的占位条目（jsonl 还未落盘），
    /// `is_task_active` 和 `finalize_task_exit::had_agent_session` 都应跳过它。
    pub(crate) is_placeholder: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredSession {
    session_id: String,
    session_path: String,
}

// ── 公共辅助函数 ──────────────────────────────────────────────────────────────

pub(crate) fn emit_task_status(app: &AppHandle, task_id: &str, status: &str) {
    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": status }),
    );
}

fn emit_active_task_status(app: &AppHandle, task_id: &str, status: &str) {
    if is_task_active(app, task_id) {
        emit_task_status(app, task_id, status);
    }
}

pub(crate) fn is_task_active(app: &AppHandle, task_id: &str) -> bool {
    let tm = app.state::<TaskManager>();
    if tm.child_handles.lock().contains_key(task_id) {
        return true;
    }

    let has_codex_session = tm
        .codex_sessions
        .lock()
        .get(task_id)
        .map(|info| !info.session_id.is_empty() && !info.session_path.is_empty())
        .unwrap_or(false);

    if has_codex_session {
        return true;
    }

    let has_claude_session = tm
        .claude_sessions
        .lock()
        .get(task_id)
        .map(|info| {
            !info.session_id.is_empty() && !info.session_path.is_empty() && !info.is_placeholder
        })
        .unwrap_or(false);

    has_claude_session
}

fn claim_session_path(app: &AppHandle, path: &str) -> bool {
    let tm = app.state::<TaskManager>();
    let mut claimed = tm.claimed_session_paths.lock();
    if claimed.contains(path) {
        return false;
    }
    claimed.insert(path.to_string());
    true
}

fn read_session_lines_since(
    session_path: &Path,
    offset: &mut u64,
    partial: &mut String,
) -> Result<Vec<String>, std::io::Error> {
    let mut file = File::open(session_path)?;
    file.seek(SeekFrom::Start(*offset))?;

    let mut chunk = String::new();
    file.read_to_string(&mut chunk)?;
    *offset += chunk.len() as u64;

    if chunk.is_empty() {
        return Ok(Vec::new());
    }

    partial.push_str(&chunk);
    let complete_len = if partial.ends_with('\n') {
        partial.len()
    } else {
        partial.rfind('\n').map(|idx| idx + 1).unwrap_or(0)
    };

    if complete_len == 0 {
        return Ok(Vec::new());
    }

    let completed = partial[..complete_len].to_string();
    let remaining = partial[complete_len..].to_string();
    *partial = remaining;

    Ok(completed.lines().map(|line| line.to_string()).collect())
}

fn session_modified_at(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

// ── Codex 会话监视器 ──────────────────────────────────────────────────────────

fn codex_sessions_roots(project_path: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(project_path).join(".codex").join("sessions")];
    if let Some(home) = crate::platform::home_dir() {
        let home_root = home.join(".codex").join("sessions");
        if !roots.iter().any(|root| root == &home_root) {
            roots.push(home_root);
        }
        append_custom_agent_session_roots(&mut roots, &home, project_path, true);
    }
    roots
}

fn append_custom_agent_session_roots(
    roots: &mut Vec<PathBuf>,
    home: &Path,
    project_path: &str,
    is_codex: bool,
) {
    let agent_homes = home.join(".aeroric").join("agent-homes");
    let Ok(entries) = fs::read_dir(agent_homes) else {
        return;
    };
    let encoded_project = encode_claude_project_path(project_path);
    for entry in entries.flatten() {
        let agent_home = entry.path();
        if !agent_home.is_dir() {
            continue;
        }
        let root = if is_codex {
            agent_home.join("sessions")
        } else {
            agent_home.join("projects").join(&encoded_project)
        };
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
}

fn collect_session_files_from_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        collect_session_files(root, &mut files);
    }
    files
}

fn collect_session_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, out);
            continue;
        }

        let is_rollout_jsonl = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
            .unwrap_or(false);

        if is_rollout_jsonl {
            out.push(path);
        }
    }
}

fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn watch_codex_session(
    app: AppHandle,
    task_id: String,
    session_path: PathBuf,
    project_path: PathBuf,
) {
    use notify::{RecursiveMode, Watcher};

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher_opt = notify::RecommendedWatcher::new(tx, notify::Config::default())
        .ok()
        .and_then(|mut w| {
            w.watch(&session_path, RecursiveMode::NonRecursive).ok()?;
            Some(w)
        });

    let mut offset = 0u64;
    let mut partial = String::new();
    let mut waiting_for_user = false;
    let mut pending_confirmation_calls = HashSet::new();
    let mut awaiting_user_reply = false;

    while is_task_active(&app, &task_id) {
        if let Ok(lines) = read_session_lines_since(&session_path, &mut offset, &mut partial) {
            // 手机远程:同批新行解析为结构化消息推送(无在线设备时零成本)
            crate::remote::publish_session_appended(&app, &task_id, &lines, true);
            for line in &lines {
                process_codex_session_line(
                    &app,
                    &task_id,
                    line,
                    &project_path,
                    &mut waiting_for_user,
                    &mut pending_confirmation_calls,
                    &mut awaiting_user_reply,
                );
            }
        }

        if watcher_opt.is_some() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => watcher_opt = None,
            }
        } else {
            thread::sleep(Duration::from_millis(400));
        }
    }
}

fn process_codex_session_line(
    app: &AppHandle,
    task_id: &str,
    line: &str,
    project_path: &Path,
    waiting_for_user: &mut bool,
    pending_confirmation_calls: &mut HashSet<String>,
    awaiting_user_reply: &mut bool,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };

    let event_type = value.get("type").and_then(serde_json::Value::as_str);
    let payload = value.get("payload");

    match event_type {
        Some("response_item") => {
            let payload_type = payload
                .and_then(|item| item.get("type"))
                .and_then(serde_json::Value::as_str);

            match payload_type {
                Some("function_call") => {
                    let name = payload
                        .and_then(|item| item.get("name"))
                        .and_then(serde_json::Value::as_str);
                    let call_id = payload
                        .and_then(|item| item.get("call_id"))
                        .and_then(serde_json::Value::as_str);
                    let arguments = payload
                        .and_then(|item| item.get("arguments"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");

                    if name == Some("request_user_input") {
                        *awaiting_user_reply = true;
                    } else if name
                        .map(|tool| tool_call_requires_confirmation(tool, arguments, project_path))
                        .unwrap_or(false)
                    {
                        if let Some(call_id) = call_id {
                            pending_confirmation_calls.insert(call_id.to_string());
                        } else {
                            *awaiting_user_reply = true;
                        }
                    }
                    sync_waiting_for_user(
                        app,
                        task_id,
                        waiting_for_user,
                        pending_confirmation_calls,
                        *awaiting_user_reply,
                    );
                }
                Some("function_call_output") => {
                    if let Some(call_id) = payload
                        .and_then(|item| item.get("call_id"))
                        .and_then(serde_json::Value::as_str)
                    {
                        pending_confirmation_calls.remove(call_id);
                    }

                    let output = payload
                        .and_then(|item| item.get("output"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if output.starts_with("aborted by user after") {
                        *awaiting_user_reply = true;
                    }
                    sync_waiting_for_user(
                        app,
                        task_id,
                        waiting_for_user,
                        pending_confirmation_calls,
                        *awaiting_user_reply,
                    );
                }
                Some("custom_tool_call") => {
                    let status = payload
                        .and_then(|item| item.get("status"))
                        .and_then(serde_json::Value::as_str);
                    let call_id = payload
                        .and_then(|item| item.get("call_id"))
                        .and_then(serde_json::Value::as_str);

                    if matches!(status, Some("completed") | Some("failed")) {
                        if let Some(call_id) = call_id {
                            pending_confirmation_calls.remove(call_id);
                        }
                        sync_waiting_for_user(
                            app,
                            task_id,
                            waiting_for_user,
                            pending_confirmation_calls,
                            *awaiting_user_reply,
                        );
                    }
                }
                Some("message") => {
                    let role = payload
                        .and_then(|item| item.get("role"))
                        .and_then(serde_json::Value::as_str);
                    if role == Some("user") {
                        *awaiting_user_reply = false;
                    } else if role == Some("assistant")
                        && assistant_message_requests_user_input(payload)
                    {
                        *awaiting_user_reply = true;
                    }
                    sync_waiting_for_user(
                        app,
                        task_id,
                        waiting_for_user,
                        pending_confirmation_calls,
                        *awaiting_user_reply,
                    );
                }
                _ => {}
            }
        }
        Some("event_msg") => {
            let payload_type = payload
                .and_then(|item| item.get("type"))
                .and_then(serde_json::Value::as_str);
            if payload_type == Some("user_message") {
                *awaiting_user_reply = false;
                sync_waiting_for_user(
                    app,
                    task_id,
                    waiting_for_user,
                    pending_confirmation_calls,
                    *awaiting_user_reply,
                );
            }
        }
        _ => {}
    }
}

fn sync_waiting_for_user(
    app: &AppHandle,
    task_id: &str,
    waiting_for_user: &mut bool,
    pending_confirmation_calls: &HashSet<String>,
    awaiting_user_reply: bool,
) {
    let next_waiting = awaiting_user_reply || !pending_confirmation_calls.is_empty();
    if *waiting_for_user == next_waiting {
        return;
    }

    *waiting_for_user = next_waiting;
    emit_active_task_status(
        app,
        task_id,
        if next_waiting {
            "input_required"
        } else {
            "running"
        },
    );
}

// ── 权限判断 ──────────────────────────────────────────────────────────────────

fn tool_call_requires_confirmation(name: &str, arguments: &str, project_path: &Path) -> bool {
    match name {
        "exec_command" => exec_command_requires_confirmation(arguments),
        "apply_patch" => apply_patch_requires_confirmation(arguments, project_path),
        _ => false,
    }
}

fn exec_command_requires_confirmation(arguments: &str) -> bool {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return false;
    };

    if args
        .get("sandbox_permissions")
        .and_then(serde_json::Value::as_str)
        == Some("require_escalated")
    {
        return true;
    }

    let Some(cmd) = args.get("cmd").and_then(serde_json::Value::as_str) else {
        return false;
    };

    !looks_like_read_only_command(cmd)
}

fn looks_like_read_only_command(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    if trimmed.is_empty() || contains_shell_redirection(trimmed) {
        return false;
    }

    trimmed
        .split([';', '|', '&', '\n'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .all(is_read_only_segment)
}

fn contains_shell_redirection(cmd: &str) -> bool {
    cmd.contains(" >")
        || cmd.contains(">>")
        || cmd.contains("<<")
        || cmd.contains(" 2>")
        || cmd.starts_with('>')
        || cmd.contains("| tee")
}

fn is_read_only_segment(segment: &str) -> bool {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let Some(command) = tokens.first().copied() else {
        return true;
    };

    match command {
        "pwd" | "ls" | "rg" | "grep" | "cat" | "head" | "tail" | "wc" | "stat" | "which"
        | "type" | "uname" | "date" | "ps" | "env" | "printenv" | "echo" | "printf"
        | "Get-Location" | "Get-ChildItem" | "Get-Content" | "Select-String" | "Get-Process"
        | "Get-Date" | "Get-Command" | "Test-Path" | "Resolve-Path" | "Where-Object"
        | "Measure-Object" | "Sort-Object" | "Select-Object" => true,
        "sed" => tokens.contains(&"-n") && !tokens.iter().any(|token| token.starts_with("-i")),
        "find" => !tokens
            .iter()
            .any(|token| matches!(*token, "-delete" | "-exec" | "-ok")),
        "git.exe" => matches!(
            tokens.get(1).copied(),
            Some("status")
                | Some("diff")
                | Some("show")
                | Some("log")
                | Some("branch")
                | Some("rev-parse")
                | Some("remote")
        ),
        "git" => matches!(
            tokens.get(1).copied(),
            Some("status")
                | Some("diff")
                | Some("show")
                | Some("log")
                | Some("branch")
                | Some("rev-parse")
                | Some("remote")
        ),
        _ => false,
    }
}

fn apply_patch_requires_confirmation(arguments: &str, project_path: &Path) -> bool {
    arguments.lines().any(|line| {
        extract_patch_path(line)
            .map(|path| patch_target_requires_confirmation(path, project_path))
            .unwrap_or(false)
    })
}

fn extract_patch_path(line: &str) -> Option<&str> {
    line.strip_prefix("*** Add File: ")
        .or_else(|| line.strip_prefix("*** Update File: "))
        .or_else(|| line.strip_prefix("*** Delete File: "))
        .or_else(|| line.strip_prefix("*** Move to: "))
        .map(str::trim)
}

fn patch_target_requires_confirmation(path: &str, project_path: &Path) -> bool {
    let target = Path::new(path);
    if !target.is_absolute() {
        return false;
    }

    let temp_dir = std::env::temp_dir();
    !target.starts_with(project_path) && !target.starts_with(&temp_dir)
}

fn assistant_message_requests_user_input(payload: Option<&serde_json::Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };

    let phase = payload.get("phase").and_then(serde_json::Value::as_str);
    if !matches!(phase, Some("final") | Some("final_answer")) {
        return false;
    }

    let Some(content) = payload.get("content").and_then(serde_json::Value::as_array) else {
        return false;
    };

    let text = content
        .iter()
        .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
        .collect::<String>();
    let text = text.trim();

    text.ends_with('?') || text.ends_with('？')
}

// ── Claude Code 会话监视器 ────────────────────────────────────────────────────

fn encode_claude_project_path(project_path: &str) -> String {
    project_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn claude_sessions_dirs_for_project(project_path: &str) -> Vec<PathBuf> {
    let Some(home) = crate::platform::home_dir() else {
        return Vec::new();
    };
    let mut roots = vec![home
        .join(".claude")
        .join("projects")
        .join(encode_claude_project_path(project_path))];
    append_custom_agent_session_roots(&mut roots, &home, project_path, false);
    roots
}

/// 某个具体 Agent 写 transcript 的目录。
///
/// 内建 claude/claude_gpt55 用 `~/.claude/projects/<encoded>`;自定义 claude-like Agent 的
/// 启动脚本会 `export CLAUDE_CONFIG_DIR="$HOME/.aeroric/agent-homes/{id}"`(见
/// `app_settings::agent_scripts`),因此写到 `<agent-home>/projects/<encoded>`。
///
/// 早期实现统一取 `claude_sessions_dirs_for_project()` 的第一项(永远是 `~/.claude`),
/// 导致自定义 Agent 的预置 session 路径指向一个永不存在的文件:它会被立刻广播并持久化,
/// 之后读会话就报 `Cannot resolve session path`。
fn claude_sessions_dir_for_agent_in(
    home: &Path,
    agent: &str,
    project_path: &str,
) -> Option<PathBuf> {
    let encoded_project = encode_claude_project_path(project_path);
    let config_root = match crate::app_settings::custom_agent_home_dir_name(agent) {
        Some(name) => home.join(".aeroric").join("agent-homes").join(name),
        None => home.join(".claude"),
    };
    Some(config_root.join("projects").join(encoded_project))
}

fn claude_sessions_dir_for_agent(agent: &str, project_path: &str) -> Option<PathBuf> {
    let home = crate::platform::home_dir()?;
    claude_sessions_dir_for_agent_in(&home, agent, project_path)
}

fn watch_claude_session(app: AppHandle, task_id: String, session_path: PathBuf) {
    use notify::{RecursiveMode, Watcher};

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher_opt = notify::RecommendedWatcher::new(tx, notify::Config::default())
        .ok()
        .and_then(|mut w| {
            w.watch(&session_path, RecursiveMode::NonRecursive).ok()?;
            Some(w)
        });

    let mut offset = 0u64;
    let mut partial = String::new();
    let mut waiting_for_user = false;

    while is_task_active(&app, &task_id) {
        if let Ok(lines) = read_session_lines_since(&session_path, &mut offset, &mut partial) {
            // 手机远程:同批新行解析为结构化消息推送(无在线设备时零成本)
            crate::remote::publish_session_appended(&app, &task_id, &lines, false);
            for line in &lines {
                process_claude_session_line(&app, &task_id, line, &mut waiting_for_user);
            }
        }

        if watcher_opt.is_some() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => watcher_opt = None,
            }
        } else {
            thread::sleep(Duration::from_millis(400));
        }
    }
}

fn process_claude_session_line(
    app: &AppHandle,
    task_id: &str,
    line: &str,
    waiting_for_user: &mut bool,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };

    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("assistant") => {
            // stop_reason == "tool_use" 是 Claude 暂停等待用户批准或拒绝工具调用的明确信号
            let stop_reason = value
                .get("message")
                .and_then(|m| m.get("stop_reason"))
                .and_then(serde_json::Value::as_str);

            if stop_reason == Some("tool_use") && !*waiting_for_user {
                *waiting_for_user = true;
                emit_active_task_status(app, task_id, "input_required");
            }
        }
        Some("user") => {
            // tool_result 条目表示用户已执行操作（批准或拒绝）
            let has_tool_result = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(serde_json::Value::as_array)
                .map(|content| {
                    content.iter().any(|item| {
                        item.get("type").and_then(serde_json::Value::as_str) == Some("tool_result")
                    })
                })
                .unwrap_or(false);

            if has_tool_result && *waiting_for_user {
                *waiting_for_user = false;
                emit_active_task_status(app, task_id, "running");
            }
        }
        _ => {}
    }
}

// ── Session messages (for conversation view) ──────────────────────────────────

#[derive(serde::Serialize, Clone, Debug)]
pub(crate) struct SessionMessage {
    pub(crate) role: String,
    pub(crate) content: Vec<SessionContent>,
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub(crate) message_id: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessagePage {
    messages: Vec<SessionMessage>,
    next_cursor: Option<u64>,
    has_more: bool,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SessionContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        id: String,
        output: String,
    },
    Thinking {
        thinking: String,
    },
    Attachment {
        name: String,
        #[serde(rename = "mediaType")]
        media_type: String,
        source: String,
    },
}

const SESSION_MESSAGE_PAGE_LINES: usize = 1_000;
const SESSION_MESSAGE_READ_CHUNK_BYTES: u64 = 64 * 1024;

/// 从 `cursor` 之前反向读取最多 `limit` 条完整 JSONL 记录。
/// 返回的行仍按时间正序排列，`next_cursor` 指向本页最早一行的字节起点。
fn read_session_page_lines(
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
const MAX_SESSION_LINES: usize = 20_000;

/// 流式读取 JSONL,只在内存里保留最后 `MAX_SESSION_LINES` 行。
///
/// 格式探测(`is_codex_format`)只看**开头**若干行,而尾部窗口可能已经把它们丢掉,
/// 所以这里在流式扫描时顺便对前 `SESSION_FORMAT_DETECTION_LINES` 行做探测,
/// 把结论单独返回,避免为了探测而全量驻留。
fn read_session_tail(path: &Path) -> Result<(Vec<String>, bool), String> {
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

#[tauri::command]
pub async fn read_session_messages(
    session_path: String,
    project_path: String,
    is_codex: bool,
    family: Option<String>,
) -> Result<Vec<SessionMessage>, String> {
    let family = crate::app_settings::resolve_family_param(family.as_deref(), is_codex);
    tauri::async_runtime::spawn_blocking(move || {
        let canonical = validate_session_path_for(&session_path, &project_path, family)?;
        if family == crate::app_settings::AgentFamily::Dsh {
            let (lines, _) = read_session_tail(&canonical)?;
            return crate::session_dsh::parse_dsh_session_lines(&lines);
        }
        let (lines, detected_codex) = read_session_tail(&canonical)?;
        Ok(parse_session_messages_with_format(
            &lines,
            is_codex,
            detected_codex,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_session_message_page(
    session_path: String,
    project_path: String,
    is_codex: bool,
    family: Option<String>,
    cursor: Option<u64>,
) -> Result<SessionMessagePage, String> {
    let family = crate::app_settings::resolve_family_param(family.as_deref(), is_codex);
    tauri::async_runtime::spawn_blocking(move || {
        let canonical = validate_session_path_for(&session_path, &project_path, family)?;
        let (lines, next_cursor, has_more) =
            read_session_page_lines(&canonical, cursor, SESSION_MESSAGE_PAGE_LINES)?;
        let messages = if family == crate::app_settings::AgentFamily::Dsh {
            crate::session_dsh::parse_dsh_session_lines(&lines)?
        } else {
            parse_session_messages_with_format(&lines, is_codex, is_codex)
        };
        Ok(SessionMessagePage {
            messages,
            next_cursor,
            has_more,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_session_id(
    session_path: String,
    project_path: String,
    is_codex: bool,
    family: Option<String>,
) -> Result<Option<String>, String> {
    let family = crate::app_settings::resolve_family_param(family.as_deref(), is_codex);
    tauri::async_runtime::spawn_blocking(move || {
        let canonical = validate_session_path_for(&session_path, &project_path, family)?;
        if family == crate::app_settings::AgentFamily::Dsh {
            return Ok(crate::session_dsh::read_dsh_session_header(&canonical).map(|(id, _)| id));
        }
        Ok(resolve_session_id_from_file(&canonical, is_codex))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn recover_task_session(
    project_path: String,
    prompt: String,
    created_at: i64,
    is_codex: bool,
    family: Option<String>,
    agent: Option<String>,
) -> Result<Option<RecoveredSession>, String> {
    let family = crate::app_settings::resolve_family_param(family.as_deref(), is_codex);
    tauri::async_runtime::spawn_blocking(move || {
        if family == crate::app_settings::AgentFamily::Dsh {
            // dsh 无法按 prompt 匹配(transcript 首条 user/message 含前缀拼接),
            // 按创建时间(留 10s 裕量)取该项目最新会话。
            return Ok(crate::session_dsh::newest_dsh_session_since(
                agent.as_deref().unwrap_or("dsh"),
                &project_path,
                created_at - 10_000,
            )
            .map(|(session_id, path)| RecoveredSession {
                session_id,
                session_path: path.to_string_lossy().into_owned(),
            }));
        }
        Ok(recover_session(
            &project_path,
            &prompt,
            created_at,
            is_codex,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

const SESSION_FORMAT_DETECTION_LINES: usize = 200;

/// 单行是否带 Codex 专有的事件标记。供整段探测与流式探测共用。
fn line_is_codex_format(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|val| {
            val.get("type")
                .and_then(|v| v.as_str())
                .map(|kind| matches!(kind, "session_meta" | "event_msg"))
        })
        .unwrap_or(false)
}

fn is_codex_format<S: AsRef<str>>(lines: &[S]) -> bool {
    lines
        .iter()
        .take(SESSION_FORMAT_DETECTION_LINES)
        .any(|line| line_is_codex_format(line.as_ref()))
}

fn parse_session_messages(lines: &[String], prefer_codex: bool) -> Vec<SessionMessage> {
    parse_session_messages_with_format(lines, prefer_codex, is_codex_format(lines))
}

/// 与 `parse_session_messages` 相同,但允许调用方给出已经算好的格式探测结论。
/// 尾部截断读取时,开头的 `session_meta` 行可能已不在 `lines` 里,必须由流式扫描
/// 阶段的探测结果补上,否则 Codex 会话会被误判成 Claude 格式而解析为空。
fn parse_session_messages_with_format(
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

fn resolve_session_id_from_file(path: &Path, is_codex: bool) -> Option<String> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if is_codex {
            let event_type = value.get("type").and_then(|v| v.as_str());
            if event_type == Some("session_meta") {
                if let Some(id) = value
                    .get("payload")
                    .and_then(|p| p.get("id"))
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.trim().is_empty())
                {
                    return Some(id.to_string());
                }
            }
        } else if let Some(id) = value
            .get("sessionId")
            .or_else(|| value.get("session_id"))
            .and_then(|v| v.as_str())
            .filter(|id| !id.trim().is_empty())
        {
            return Some(id.to_string());
        }
    }

    let stem = path.file_stem().and_then(|name| name.to_str())?;
    if !is_codex && is_uuid_like(stem) {
        return Some(stem.to_string());
    }

    if is_codex {
        if let Some(rest) = stem.strip_prefix("rollout-") {
            if let Some(candidate) = rest.get(20..) {
                if is_uuid_like(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        if is_uuid_like(stem) {
            return Some(stem.to_string());
        }
    }

    None
}

fn session_started_at_ms(path: &Path) -> Option<i64> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let timestamp = value
            .get("timestamp")
            .or_else(|| {
                value
                    .get("payload")
                    .and_then(|payload| payload.get("timestamp"))
            })
            .and_then(serde_json::Value::as_str);
        if let Some(timestamp) = timestamp {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) {
                return Some(parsed.timestamp_millis());
            }
        }
    }

    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
}

fn session_matches_project(path: &Path, project_path: &str, is_codex: bool) -> bool {
    if !is_codex {
        return true;
    }
    let expected = Path::new(project_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_path));
    let Ok(file) = File::open(path) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .filter_map(|value| {
            value
                .get("payload")
                .and_then(|payload| payload.get("cwd"))
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        })
        .any(|cwd| cwd.canonicalize().unwrap_or(cwd) == expected)
}

fn first_user_message(path: &Path, is_codex: bool) -> Option<String> {
    let lines = BufReader::new(File::open(path).ok()?)
        .lines()
        .map_while(Result::ok)
        .take(2_000)
        .collect::<Vec<_>>();
    parse_session_messages(&lines, is_codex)
        .into_iter()
        .find(|message| message.role == "user")
        .and_then(|message| {
            message
                .content
                .into_iter()
                .find_map(|content| match content {
                    SessionContent::Text { text } if !text.trim().is_empty() => Some(text),
                    _ => None,
                })
        })
}

fn recover_session(
    project_path: &str,
    prompt: &str,
    created_at: i64,
    is_codex: bool,
) -> Option<RecoveredSession> {
    let mut files = if is_codex {
        collect_session_files_from_roots(&codex_sessions_roots(project_path))
    } else {
        let mut files = Vec::new();
        for root in claude_sessions_dirs_for_project(project_path) {
            collect_jsonl_files(&root, &mut files);
        }
        files
    };
    let normalized_prompt = prompt.trim();
    files.retain(|path| session_matches_project(path, project_path, is_codex));

    files
        .into_iter()
        .filter_map(|path| {
            let started_at = session_started_at_ms(&path)?;
            let delta = started_at.abs_diff(created_at);
            if delta > 15 * 60 * 1_000 {
                return None;
            }
            let prompt_match = if normalized_prompt.is_empty() {
                true
            } else {
                first_user_message(&path, is_codex)
                    .map(|message| message.trim().contains(normalized_prompt))
                    .unwrap_or(false)
            };
            if !prompt_match {
                return None;
            }
            let session_id = resolve_session_id_from_file(&path, is_codex)?;
            Some((
                delta,
                RecoveredSession {
                    session_id,
                    session_path: path.to_string_lossy().into_owned(),
                },
            ))
        })
        .min_by_key(|(delta, _)| *delta)
        .map(|(_, recovered)| recovered)
}

pub(crate) fn parse_session_lines(
    lines: &[String],
    is_codex: bool,
    messages: &mut Vec<SessionMessage>,
) {
    for line in lines {
        parse_session_line(line, is_codex, messages);
    }
}

fn parse_session_line(line: &str, is_codex: bool, messages: &mut Vec<SessionMessage>) {
    if is_codex {
        parse_codex_session_line(line, messages);
    } else {
        parse_claude_session_line(line, messages);
    }
}

fn parse_claude_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages = Vec::new();

    for line in lines {
        parse_claude_session_line(line, &mut messages);
    }

    messages
}

fn parse_claude_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if msg_type == "attachment" {
        if let Some(content) = attachment_from_value(
            val.get("attachment")
                .or_else(|| val.get("payload"))
                .unwrap_or(&val),
        ) {
            messages.push(SessionMessage {
                role: "user".to_string(),
                content: vec![content],
                message_id: None,
            });
        }
        return;
    }
    let Some(message) = val.get("message") else {
        return;
    };

    match msg_type {
        "user" => {
            let parts = claude_user_content(message.get("content"));
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "user".to_string(),
                    content: parts,
                    message_id: None,
                });
            }
        }
        "assistant" => {
            let parts = message
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| claude_assistant_blocks(arr))
                .unwrap_or_default();
            if !parts.is_empty() {
                messages.push(SessionMessage {
                    role: "assistant".to_string(),
                    content: parts,
                    message_id: None,
                });
            }
        }
        _ => {}
    }
}

fn claude_user_content(content: Option<&serde_json::Value>) -> Vec<SessionContent> {
    match content {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
            vec![SessionContent::Text { text: s.clone() }]
        }
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(
                |block| match block.get("type").and_then(|value| value.as_str()) {
                    Some("text") => {
                        let text = block
                            .get("text")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        (!text.trim().is_empty()).then(|| SessionContent::Text {
                            text: text.to_string(),
                        })
                    }
                    Some("tool_result") => {
                        let id = block
                            .get("tool_use_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string();
                        let output = match block.get("content") {
                            Some(serde_json::Value::String(text)) => text.clone(),
                            Some(serde_json::Value::Array(items)) => items
                                .iter()
                                .filter_map(|item| {
                                    item.get("text")
                                        .and_then(|value| value.as_str())
                                        .map(ToOwned::to_owned)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
                            None => String::new(),
                        };
                        (!output.trim().is_empty())
                            .then_some(SessionContent::ToolResult { id, output })
                    }
                    Some("image") | Some("document") | Some("attachment") => {
                        attachment_from_value(block)
                    }
                    _ => None,
                },
            )
            .collect(),
        _ => Vec::new(),
    }
}

fn claude_assistant_blocks(blocks: &[serde_json::Value]) -> Vec<SessionContent> {
    let mut parts = Vec::new();
    for block in blocks {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        parts.push(SessionContent::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block
                    .get("input")
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_default();
                parts.push(SessionContent::ToolUse { id, name, input });
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|v| v.as_str()) {
                    if !thinking.trim().is_empty() {
                        parts.push(SessionContent::Thinking {
                            thinking: thinking.to_string(),
                        });
                    }
                }
            }
            Some("image") | Some("document") | Some("attachment") => {
                if let Some(attachment) = attachment_from_value(block) {
                    parts.push(attachment);
                }
            }
            _ => {}
        }
    }
    parts
}

fn json_value_to_display(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
        None => String::new(),
    }
}

fn attachment_from_value(value: &serde_json::Value) -> Option<SessionContent> {
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

fn append_assistant_content(messages: &mut Vec<SessionMessage>, part: SessionContent) {
    if let Some(last) = messages
        .last_mut()
        .filter(|message| message.role == "assistant")
    {
        last.content.push(part);
    } else {
        messages.push(SessionMessage {
            role: "assistant".to_string(),
            content: vec![part],
            message_id: None,
        });
    }
}

fn append_codex_user_message(messages: &mut Vec<SessionMessage>, mut content: Vec<SessionContent>) {
    let duplicate_text = messages
        .last()
        .filter(|message| message.role == "user")
        .map(|message| {
            content.iter().any(|candidate| {
                let SessionContent::Text {
                    text: candidate_text,
                } = candidate
                else {
                    return false;
                };
                message.content.iter().any(
                    |existing| matches!(existing, SessionContent::Text { text } if text == candidate_text),
                )
            })
        })
        .unwrap_or(false);

    if duplicate_text {
        if let Some(last) = messages.last_mut() {
            content.retain(|candidate| match candidate {
                SessionContent::Text { text: candidate } => !last
                    .content
                    .iter()
                    .any(|existing| matches!(existing, SessionContent::Text { text } if text == candidate)),
                _ => true,
            });
            last.content.extend(content);
        }
        return;
    }
    messages.push(SessionMessage {
        role: "user".to_string(),
        content,
        message_id: None,
    });
}

fn parse_codex_session(lines: &[&str]) -> Vec<SessionMessage> {
    let mut messages: Vec<SessionMessage> = Vec::new();

    for line in lines {
        parse_codex_session_line(line, &mut messages);
    }

    messages
}

fn parse_codex_session_line(line: &str, messages: &mut Vec<SessionMessage>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let payload = val.get("payload");

    match event_type {
        "event_msg" => {
            let payload_type = payload
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if payload_type == "user_message" {
                let text = payload
                    .and_then(|p| p.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    append_codex_user_message(
                        messages,
                        vec![SessionContent::Text {
                            text: text.to_string(),
                        }],
                    );
                }
            }
        }
        "response_item" => {
            let payload_type = payload
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match payload_type {
                "message" => {
                    let role = payload
                        .and_then(|p| p.get("role"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !matches!(role, "assistant" | "user") {
                        return;
                    }
                    let parts: Vec<SessionContent> = payload
                        .and_then(|p| p.get("content"))
                        .and_then(|v| v.as_array())
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(|b| {
                                    let t = b.get("type").and_then(|v| v.as_str())?;
                                    if matches!(t, "output_text" | "input_text" | "text") {
                                        let text = b.get("text").and_then(|v| v.as_str())?;
                                        if !text.trim().is_empty() {
                                            return Some(SessionContent::Text {
                                                text: text.to_string(),
                                            });
                                        }
                                    }
                                    if matches!(t, "input_image" | "image" | "attachment") {
                                        return attachment_from_value(b);
                                    }
                                    None
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if !parts.is_empty() {
                        if role == "assistant" {
                            if let Some(last) =
                                messages.last_mut().filter(|m| m.role == "assistant")
                            {
                                last.content.extend(parts);
                            } else {
                                messages.push(SessionMessage {
                                    role: "assistant".to_string(),
                                    content: parts,
                                    message_id: None,
                                });
                            }
                        } else {
                            append_codex_user_message(messages, parts);
                        }
                    }
                }
                "reasoning" => {
                    let thinking = payload
                        .and_then(|item| item.get("summary").or_else(|| item.get("content")))
                        .map(|summary| match summary {
                            serde_json::Value::String(text) => text.clone(),
                            serde_json::Value::Array(items) => items
                                .iter()
                                .filter_map(|item| {
                                    item.get("text").and_then(serde_json::Value::as_str)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            other => serde_json::to_string_pretty(other).unwrap_or_default(),
                        })
                        .unwrap_or_default();
                    if !thinking.trim().is_empty() {
                        append_assistant_content(messages, SessionContent::Thinking { thinking });
                    }
                }
                "agent_message" => {
                    let text = payload
                        .and_then(|item| item.get("message").or_else(|| item.get("text")))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if !text.trim().is_empty() {
                        append_assistant_content(
                            messages,
                            SessionContent::Text {
                                text: text.to_string(),
                            },
                        );
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let call_id = payload
                        .and_then(|p| p.get("call_id").or_else(|| p.get("id")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = payload
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(if payload_type == "custom_tool_call" {
                            "custom_tool"
                        } else {
                            "tool"
                        })
                        .to_string();
                    let raw_value = payload.and_then(|p| {
                        p.get("arguments")
                            .or_else(|| p.get("input"))
                            .or_else(|| p.get("payload"))
                    });
                    let input = match raw_value {
                        Some(serde_json::Value::String(raw)) => {
                            serde_json::from_str::<serde_json::Value>(raw)
                                .ok()
                                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                                .unwrap_or_else(|| raw.clone())
                        }
                        value => json_value_to_display(value),
                    };
                    append_assistant_content(
                        messages,
                        SessionContent::ToolUse {
                            id: call_id,
                            name,
                            input,
                        },
                    );
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let id = payload
                        .and_then(|item| item.get("call_id").or_else(|| item.get("id")))
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let output = json_value_to_display(payload.and_then(|item| {
                        item.get("output")
                            .or_else(|| item.get("result"))
                            .or_else(|| item.get("content"))
                    }));
                    if !output.trim().is_empty() {
                        messages.push(SessionMessage {
                            role: "user".to_string(),
                            content: vec![SessionContent::ToolResult { id, output }],
                            message_id: None,
                        });
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ── 会话摘要提取（供任务命名等上下文感知功能复用） ─────────────────────────────

/// 摘要提取允许读取的最大会话文件尺寸。超过该值直接返回 None，
/// 防止 100MB+ 长会话把整文件载入内存。
const MAX_SESSION_BYTES_FOR_SUMMARY: u64 = 50 * 1024 * 1024;

/// 摘要提取允许处理的最大行数。超过后只取头/尾各 `MAX_SESSION_LINES_FOR_SUMMARY / 2` 行，
/// 中间整段丢弃，避免 50MB 文件×几 MB JSON 行导致解析阶段峰值内存爆炸。
const MAX_SESSION_LINES_FOR_SUMMARY: usize = 20_000;

/// 校验前端传入的 session_path 是否合法：
/// - 必须绝对路径且文件存在
/// - canonicalize 后必须位于该 agent 允许的 session 根目录之内
///   （Claude: `~/.claude/projects/<encoded-project>/`；
///   Codex: `<project_path>/.codex/sessions/` 或 `~/.codex/sessions/`）
///
/// 这一关把死路径遍历——任意 `*.jsonl` 文件都不能被读取。
pub(crate) fn validate_session_path(
    session_path: &str,
    project_path: &str,
    is_codex: bool,
) -> Result<PathBuf, String> {
    validate_session_path_for(
        session_path,
        project_path,
        crate::app_settings::AgentFamily::from_codex_like(is_codex),
    )
}

/// family 版校验:dsh 的允许根为托管 DSH_HOME 的 sessions 目录。
pub(crate) fn validate_session_path_for(
    session_path: &str,
    project_path: &str,
    family: crate::app_settings::AgentFamily,
) -> Result<PathBuf, String> {
    let path = Path::new(session_path);
    if !path.is_absolute() {
        return Err("Session path must be absolute".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve session path: {}", e))?;
    if !canonical.is_file() {
        return Err("Session path is not a regular file".into());
    }

    let allowed_roots: Vec<PathBuf> = match family {
        crate::app_settings::AgentFamily::Codex => codex_sessions_roots(project_path)
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect(),
        crate::app_settings::AgentFamily::Claude => claude_sessions_dirs_for_project(project_path)
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect(),
        crate::app_settings::AgentFamily::Dsh => crate::session_dsh::dsh_session_allowed_roots()
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect(),
    };

    if allowed_roots.is_empty() {
        return Err("No allowed session roots are available for this agent".into());
    }
    if allowed_roots.iter().any(|root| canonical.starts_with(root)) {
        Ok(canonical)
    } else {
        Err(format!(
            "Session path is outside allowed session roots: {}",
            canonical.display()
        ))
    }
}

/// 读取并解析会话 JSONL，输出一段紧凑的纯文本摘要供 LLM 二次处理。
/// 超出 `budget_bytes` 时按"头 + 中间省略 + 尾"裁剪。
/// 当文件超过 `MAX_SESSION_BYTES_FOR_SUMMARY` 时返回 `None`，由调用方回退到仅 prompt 模式。
pub(crate) fn extract_session_summary_text(
    session_path: &str,
    budget_bytes: usize,
) -> Option<String> {
    let metadata = std::fs::metadata(session_path).ok()?;
    if metadata.len() > MAX_SESSION_BYTES_FOR_SUMMARY {
        return None;
    }

    // 使用 BufReader 流式读取；hard cap 行数，超过则丢弃中间段（仅留首尾各一半）。
    use std::io::BufRead;
    let file = File::open(session_path).ok()?;
    let reader = BufReader::new(file);
    let mut head: Vec<String> = Vec::new();
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let half = MAX_SESSION_LINES_FOR_SUMMARY / 2;
    for line in reader
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
    {
        if head.len() < half {
            head.push(line);
        } else {
            tail.push_back(line);
            if tail.len() > half {
                tail.pop_front();
            }
        }
    }
    head.extend(tail);
    let lines = head;
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();

    let messages = if is_codex_format(&line_refs) {
        parse_codex_session(&line_refs)
    } else {
        parse_claude_session(&line_refs)
    };

    let formatted: Vec<String> = messages
        .iter()
        .filter_map(format_message_for_summary)
        .collect();
    if formatted.is_empty() {
        return None;
    }

    let total: usize = formatted.iter().map(|s| s.len() + 1).sum();
    if total <= budget_bytes {
        return Some(formatted.join("\n"));
    }

    // 头 + 尾切片
    let half = budget_bytes / 2;
    let mut head_msgs: Vec<&str> = Vec::new();
    let mut head_size = 0usize;
    for msg in &formatted {
        if head_size + msg.len() + 1 > half {
            break;
        }
        head_size += msg.len() + 1;
        head_msgs.push(msg.as_str());
    }

    let mut tail_msgs: Vec<&str> = Vec::new();
    let mut tail_size = 0usize;
    let head_count = head_msgs.len();
    for msg in formatted.iter().rev() {
        if tail_msgs.len() + head_count >= formatted.len() {
            break;
        }
        if tail_size + msg.len() + 1 > half {
            break;
        }
        tail_size += msg.len() + 1;
        tail_msgs.push(msg.as_str());
    }
    tail_msgs.reverse();

    let omitted = formatted.len() - head_count - tail_msgs.len();
    let head_text = head_msgs.join("\n");
    let tail_text = tail_msgs.join("\n");
    if omitted == 0 {
        Some(format!("{}\n{}", head_text, tail_text))
    } else {
        Some(format!(
            "{}\n... [{} messages omitted] ...\n{}",
            head_text, omitted, tail_text
        ))
    }
}

/// 仅保留 user / assistant 的纯文本块。tool_use 和 thinking 都丢弃：
/// - tool_use：长任务里能凑出几十上百次 Read/Bash，很容易把预算挤爆，
///   把真正有信号的对话文本挤到尾部裁剪窗口外。
/// - thinking：不属于"实际成果"，模型自言自语对命名无价值。
/// - tool_result：上游 parse_codex_session / parse_claude_session 已不会输出。
fn format_message_for_summary(msg: &SessionMessage) -> Option<String> {
    let role = match msg.role.as_str() {
        "user" => "[user]",
        "assistant" => "[assistant]",
        _ => return None,
    };

    let mut parts: Vec<String> = Vec::new();
    for block in &msg.content {
        if let SessionContent::Text { text } = block {
            let cleaned = truncate_summary_chars(text, 400);
            if !cleaned.is_empty() {
                parts.push(cleaned);
            }
        }
    }

    if parts.is_empty() {
        return None;
    }
    Some(format!("{} {}", role, parts.join(" ")))
}

fn truncate_summary_chars(s: &str, max_chars: usize) -> String {
    let collapsed: String = s
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = collapsed.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars).collect();
    out.push('…');
    out
}

// ── 会话文件工具函数 ──────────────────────────────────────────────────────────

/// Strip ANSI escape sequences so we can do plain-text matching.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some(&'[') => {
                    chars.next(); // consume '['
                                  // consume until a byte that terminates a CSI sequence (ASCII letter)
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        if c2.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next(); // skip the char after bare ESC
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn is_uuid_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn find_claude_session_file(session_id: &str, project_path: &str) -> Option<PathBuf> {
    for sessions_dir in claude_sessions_dirs_for_project(project_path) {
        if is_uuid_like(session_id) {
            let file = sessions_dir.join(format!("{}.jsonl", session_id));
            if file.exists() {
                return Some(file);
            }
        }

        let Ok(entries) = std::fs::read_dir(&sessions_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if slug_matches_session_file(&path, session_id) {
                return Some(path);
            }
        }
    }
    None
}

/// Returns true if `path` is a Claude session JSONL that contains a
/// `custom-title` or `agent-name` record matching `slug`.
fn slug_matches_session_file(path: &Path, slug: &str) -> bool {
    use std::io::BufRead;
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let type_str = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if matches!(type_str, "custom-title" | "agent-name") {
            let name = v
                .get("customTitle")
                .or_else(|| v.get("agentName"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            if name == slug {
                return true;
            }
        }
    }
    false
}

fn find_codex_session_file(session_id: &str, project_path: &str) -> Option<PathBuf> {
    let suffix = format!("-{}.jsonl", session_id);
    let files = collect_session_files_from_roots(&codex_sessions_roots(project_path));
    files
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(&suffix))
                .unwrap_or(false)
        })
        .max_by_key(|p| session_modified_at(p))
}

// ── 跨配置会话接管 ───────────────────────────────────────────────────────────

/// 目标 Agent 读取 transcript 的根目录。内建 Agent 用 `~/.claude` / `~/.codex`,
/// 自定义 Agent 用各自的隔离 home(`~/.aeroric/agent-homes/{id}`)。
fn session_home_for_agent(agent: &str, is_codex: bool) -> Result<PathBuf, String> {
    match crate::app_settings::custom_agent_home_dir_name(agent) {
        Some(_) => crate::app_settings::custom_agent_home(agent),
        None if is_codex => crate::hooks::codex_home(),
        None => crate::platform::home_dir()
            .map(|home| home.join(".claude"))
            .ok_or_else(|| "Cannot resolve the home directory".to_string()),
    }
}

/// 目标 Agent 接管一个会话文件后的落盘路径。
///
/// Claude 按 `<home>/projects/<encoded-project>/<session-id>.jsonl` 定位;Codex 按
/// `<home>/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl` 递归搜索,所以沿用源文件的
/// 相对布局(年月日目录 + 文件名)即可被 `codex resume <id>` 找到。
fn adopted_session_target_path(
    source: &Path,
    agent: &str,
    is_codex: bool,
    project_path: &str,
) -> Result<PathBuf, String> {
    let home = session_home_for_agent(agent, is_codex)?;
    let file_name = source
        .file_name()
        .ok_or_else(|| "Session path has no file name".to_string())?;
    if !is_codex {
        return Ok(home
            .join("projects")
            .join(encode_claude_project_path(project_path))
            .join(file_name));
    }

    // 尽量保留 `sessions/<yyyy>/<mm>/<dd>/` 这层结构,Codex 的 picker 与索引都按它扫描。
    let date_parts: Vec<_> = source
        .parent()
        .map(|parent| {
            parent
                .components()
                .rev()
                .take(3)
                .filter_map(|component| component.as_os_str().to_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut target = home.join("sessions");
    if date_parts.len() == 3
        && date_parts
            .iter()
            .all(|part| part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit()))
    {
        for part in date_parts.iter().rev() {
            target = target.join(part);
        }
    }
    Ok(target.join(file_name))
}

/// 把一个会话 transcript 复制进目标 Agent 的 home,让目标配置可以原生 resume。
///
/// 两个 Agent 各有隔离 home 时,`claude --resume <id>` / `codex resume <id>` 只会在自己
/// 的 home 里查找,跨配置切换因此必然落到「把上下文塞进 prompt」的降级路径——那条路径
/// 依赖终端回放文本,既丢结构化工具调用又容易带入乱码。先接管文件,原生 resume 就能直接
/// 复用完整对话树。
///
/// 目标已存在同名文件时直接复用(重复切换是幂等的),不覆盖已有 transcript。
#[tauri::command]
pub async fn adopt_session_for_agent(
    session_path: String,
    project_path: String,
    is_codex: bool,
    target_agent: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !crate::app_settings::is_known_agent(&target_agent) {
            return Err(format!("Unknown agent: {target_agent}"));
        }
        let canonical = validate_session_path(&session_path, &project_path, is_codex)?;
        let target =
            adopted_session_target_path(&canonical, &target_agent, is_codex, &project_path)?;
        if let Ok(existing) = target.canonicalize() {
            if existing == canonical {
                return Ok(canonical.to_string_lossy().into_owned());
            }
        }
        if target.exists() {
            return Ok(target.to_string_lossy().into_owned());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Cannot create the target session directory: {error}"))?;
            crate::storage::ensure_private_dir(parent)?;
        }
        std::fs::copy(&canonical, &target)
            .map_err(|error| format!("Cannot adopt the session transcript: {error}"))?;
        crate::storage::ensure_private_file_permissions(&target)?;
        Ok(target.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| error.to_string())?
}

// ── /status-based session discovery ──────────────────────────────────────────

/// 从 Claude Code 的 `/status` 输出中提取 Session ID。
/// 输出示例: "Session ID: 1aee0948-e0f2-4ad1-b710-ba236fab378a"
fn extract_claude_status_session_id(output: &str) -> Option<String> {
    let clean = strip_ansi(output);
    // Use find() instead of line-by-line matching because Claude Code renders /status
    // using cursor-positioning escape sequences, which collapse multiple lines into one
    // after ANSI stripping (no \r\n between positioned text fragments).
    let pos = clean.find("Session ID:")?;
    let after = clean[pos + "Session ID:".len()..].trim_start();
    let id: String = after
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    if is_uuid_like(&id) {
        Some(id)
    } else {
        None
    }
}

/// 从 Codex 的 `/status` 输出中提取 Session ID。
/// 输出示例: "│  Session:                     019d247a-2a83-76f3-b5c6-e4a59955af3f  │"
///
/// Codex renders /status using cursor-positioning escape sequences, which collapse
/// multiple lines into one after ANSI stripping (same issue as Claude Code).
/// Use find() instead of line-by-line matching to handle both cases.
fn extract_codex_status_session_id(output: &str) -> Option<String> {
    let clean = strip_ansi(output);
    // 先过滤掉盒状边框字符，再用 find() 搜索 "Session:" 关键词，
    // 避免光标定位序列导致多行塌缩成一行后 lines() 无法匹配的问题
    let stripped: String = clean
        .chars()
        .filter(|c| !matches!(*c, '│' | '╭' | '╰' | '─' | '╮' | '╯' | '├' | '┤'))
        .collect();
    let pos = stripped.find("Session:")?;
    let after = stripped[pos + "Session:".len()..].trim_start();
    let id: String = after
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    if is_uuid_like(&id) {
        Some(id)
    } else {
        None
    }
}

/// 轮询最多 5 秒，直到会话文件出现。
fn wait_for_session_file(session_id: &str, project_path: &str, is_codex: bool) -> Option<PathBuf> {
    for _ in 0..50 {
        let path = if is_codex {
            find_codex_session_file(session_id, project_path)
        } else {
            find_claude_session_file(session_id, project_path)
        };
        if path.is_some() {
            return path;
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

/// 在 Session ID 确认后注册会话信息，并开始监视文件。
pub(crate) fn register_and_watch_session(
    app: &AppHandle,
    task_id: &str,
    session_id: &str,
    project_path: &str,
    is_codex: bool,
) {
    let path = match wait_for_session_file(session_id, project_path, is_codex) {
        Some(p) => p,
        None => return,
    };
    let path_string = path.to_string_lossy().into_owned();

    if !claim_session_path(app, &path_string) {
        return;
    }

    if is_codex {
        let tm = app.state::<TaskManager>();
        tm.codex_sessions.lock().insert(
            task_id.to_string(),
            CodexSessionInfo {
                session_id: session_id.to_string(),
                session_path: path_string.clone(),
            },
        );
    } else {
        let tm = app.state::<TaskManager>();
        tm.claude_sessions.lock().insert(
            task_id.to_string(),
            ClaudeSessionInfo {
                session_id: session_id.to_string(),
                session_path: path_string.clone(),
                is_placeholder: false,
            },
        );
    }

    let _ = app.emit(
        "task-session",
        serde_json::json!({
            "task_id": task_id,
            "session_id": session_id,
            "session_path": path_string,
            "codex_like": is_codex,
        }),
    );

    let app_clone = app.clone();
    let tid = task_id.to_string();
    if is_codex {
        let pp = PathBuf::from(project_path);
        thread::spawn(move || watch_codex_session(app_clone, tid, path, pp));
    } else {
        thread::spawn(move || watch_claude_session(app_clone, tid, path));
    }
}

/// 监听 PTY 输出流，通过 `/status` 响应获取 Session ID。
/// Claude 启动后 1.5 秒发送 `/status`；Codex 则在收到首个输出后再等待 1 秒，
/// 避免 session 尚未创建时过早查询。
fn should_send_status_command(
    status_sent: bool,
    is_codex: bool,
    start_elapsed: Duration,
    first_output_elapsed: Option<Duration>,
) -> bool {
    if status_sent {
        return false;
    }

    if is_codex {
        first_output_elapsed
            .map(|elapsed| elapsed >= Duration::from_secs(1))
            .unwrap_or(false)
            // 兜底：若 Codex 长时间无输出，也不要无限等待
            || start_elapsed >= Duration::from_secs(8)
    } else {
        start_elapsed >= Duration::from_millis(1500)
    }
}

pub(crate) fn should_start_status_session_watcher(
    use_hooks: bool,
    is_codex: bool,
    prompt_empty: bool,
) -> bool {
    if is_codex {
        // Codex 有无 prompt 的启动期优先走文件发现，避免把 /status 写入
        // trust/review 界面；hook 不可用时保留旧的 /status 兜底。
        return !use_hooks && !prompt_empty;
    }
    if use_hooks {
        return false;
    }

    !prompt_empty
}

/// Codex 没有公开的启动参数用于预置 session id。带首条 prompt 的任务启动后，
/// 通过新生成的 session 文件按项目、提示词和创建时间匹配真实 session，避免向
/// 正在显示启动确认界面的 PTY 注入 `/status`。
pub(crate) fn spawn_codex_session_recovery(
    app: AppHandle,
    task_id: String,
    project_path: String,
    prompt: String,
    created_at: i64,
) {
    thread::spawn(move || {
        // 30 秒覆盖 Codex 启动、trust/review 和首条消息落盘的常见延迟；任务
        // 退出后仍继续短暂扫描，确保手动完成不会抢在 session 事件之前清理状态。
        for _ in 0..150 {
            let registered = {
                let tm = app.state::<TaskManager>();
                let registered = tm.codex_sessions.lock().contains_key(&task_id);
                registered
            };
            if registered {
                return;
            }

            if let Some(recovered) = recover_session(&project_path, &prompt, created_at, true) {
                register_and_watch_session(
                    &app,
                    &task_id,
                    &recovered.session_id,
                    &project_path,
                    true,
                );
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
    });
}

fn send_status_command(app: &AppHandle, task_id: &str, is_codex: bool) {
    fn write_to_pty(app: &AppHandle, task_id: &str, bytes: &[u8]) {
        let writer = {
            let tm = app.state::<TaskManager>();
            let writer = tm.pty_writers.lock().get(task_id).cloned();
            writer
        };
        if let Some(writer) = writer {
            let mut writer = writer.lock();
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    if is_codex {
        // Codex 有自动补全菜单，需先输入 /status 触发菜单，
        // 再延迟发送 \r 选中执行；两次写入之间释放锁，避免长时间持锁
        write_to_pty(app, task_id, b"/status");
        thread::sleep(Duration::from_millis(100));
        write_to_pty(app, task_id, b"\r");
    } else {
        write_to_pty(app, task_id, b"/status\r");
    }
}

/// Claude 启动专用：立刻按预置 UUID 注册并广播 session id，后台等真实 jsonl
/// 文件出现后再 attach 监听。
/// 最长 2 分钟或任务结束，避免线程长期挂着。
///
/// 注入的 `claude_sessions` 条目以 `is_placeholder: true` 标记，`is_task_active`
/// 和 `finalize_task_exit::had_agent_session` 都会跳过；文件出现后升级为真，
/// 任何退出路径都会清理占位条目和 claimed 路径。
/// Claude 已经通过 `--session-id` 预先确定会话 ID 时，立即广播并持久化该 ID，
/// 同时后台等待 transcript 文件出现后再升级为可监视的真实会话。
pub(crate) fn spawn_claude_lazy_session_attach(
    app: AppHandle,
    task_id: String,
    session_id: String,
    project_path: String,
    agent: String,
) {
    thread::spawn(move || {
        let Some(sessions_dir) = claude_sessions_dir_for_agent(&agent, &project_path) else {
            return;
        };
        let expected = sessions_dir.join(format!("{}.jsonl", session_id));
        let path_string = expected.to_string_lossy().into_owned();

        // Hook 可能比 lazy attach 更早写入真实会话；此时直接退出，避免重复
        // claim 和启动第二个 transcript watcher。
        {
            let tm = app.state::<TaskManager>();
            if tm
                .claude_sessions
                .lock()
                .get(&task_id)
                .map(|info| info.session_id == session_id && !info.is_placeholder)
                .unwrap_or(false)
            {
                return;
            }
        }

        if !claim_session_path(&app, &path_string) {
            return;
        }

        {
            let tm = app.state::<TaskManager>();
            tm.claude_sessions.lock().insert(
                task_id.clone(),
                ClaudeSessionInfo {
                    session_id: session_id.clone(),
                    session_path: path_string.clone(),
                    is_placeholder: true,
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
            }),
        );

        // 后台等文件真正出现（500ms × 240 = 2 分钟，或任务结束）。
        let mut attached = false;
        for _ in 0..240 {
            // child_handles 是判断进程存活的唯一可靠信号；这里不能用 is_task_active，
            // 因为我们刚注入的占位条目会被它跳过（设计如此），改成直接看进程在不在更准确。
            let alive = {
                let tm = app.state::<TaskManager>();
                let handles = tm.child_handles.lock();
                handles.contains_key(&task_id)
            };
            if !alive {
                break;
            }
            // 预期路径优先，同时兜底搜索全部已知 session root：Agent 的 CLAUDE_CONFIG_DIR
            // 可能被用户脚本改到别处，那时预期路径永远不出现，而广播出去的路径已经被
            // 前端持久化。搜到真实文件就地纠正，避免留下读不了的死路径。
            let found = if expected.exists() {
                Some(expected.clone())
            } else {
                find_claude_session_file(&session_id, &project_path)
            };
            let Some(actual) = found else {
                thread::sleep(Duration::from_millis(500));
                continue;
            };
            let actual_string = actual.to_string_lossy().into_owned();
            let corrected = actual_string != path_string;

            // 升级为真：placeholder 翻成 false，让 is_task_active / had_agent_session
            // 重新识别为有效会话。
            let should_start_watcher = {
                let tm = app.state::<TaskManager>();
                let mut sessions = tm.claude_sessions.lock();
                match sessions.get_mut(&task_id) {
                    Some(info) if info.is_placeholder => {
                        info.is_placeholder = false;
                        info.session_path = actual_string.clone();
                        true
                    }
                    _ => false,
                }
            };

            if should_start_watcher && corrected {
                // 换 claim：旧的猜测路径不再持有，新路径若已被别的任务占用则放弃接管。
                let claimed_actual = {
                    let tm = app.state::<TaskManager>();
                    let mut claimed = tm.claimed_session_paths.lock();
                    claimed.remove(&path_string);
                    claimed.insert(actual_string.clone())
                };
                if !claimed_actual {
                    // 已被其他任务监听：还原占位标记，交给下方清理分支收尾。
                    let tm = app.state::<TaskManager>();
                    if let Some(info) = tm.claude_sessions.lock().get_mut(&task_id) {
                        info.is_placeholder = true;
                        info.session_path = path_string.clone();
                    }
                    break;
                }
                // 让前端把先前持久化的错误路径替换为真实路径。
                let _ = app.emit(
                    "task-session",
                    serde_json::json!({
                        "task_id": task_id,
                        "session_id": session_id,
                        "session_path": actual_string,
                        "codex_like": false,
                    }),
                );
            }

            attached = true;
            if should_start_watcher {
                watch_claude_session(app.clone(), task_id.clone(), actual);
            }
            break;
        }

        // 文件出现 → watch_claude_session 已接管，claude_sessions 条目交给
        // finalize_task_exit 在任务退出时清理。
        if attached {
            return;
        }

        // 否则（超时 / 任务退出）：清理占位条目和 claimed 路径，避免泄漏。
        let tm = app.state::<TaskManager>();
        let removed = tm.claude_sessions.lock().remove(&task_id);
        if let Some(info) = removed {
            if info.is_placeholder {
                tm.claimed_session_paths.lock().remove(&info.session_path);
            } else {
                // 极少数情况：placeholder 已被外部翻成 false 但 attached 仍为 false
                // （例如 watch 启动失败）。把它还原回 sessions，避免吞掉真实条目。
                tm.claude_sessions.lock().insert(task_id.clone(), info);
            }
        }
    });
}

/// 监听 PTY 输出流，通过 `/status` 响应获取 Session ID。
/// 仅非空 prompt 的任务会启动该 watcher；交互式 REPL 启动不自动输入任何命令。
/// Claude 启动后 1.5 秒发送 `/status`；Codex 则在收到首个输出后再等待 1 秒，
/// 避免 session 尚未创建时过早查询。
///
/// 当 `pre_session_id` 为 `Some` 时（Claude >= 2.1.87），跳过 `/status` 发现，
/// 直接使用预置 session id 注册会话文件。若文件在超时内未出现，自动回退到 `/status` 流程。
pub(crate) fn spawn_status_session_watcher(
    app: AppHandle,
    task_id: String,
    project_path: String,
    agent: String,
    is_codex: bool,
    rx: mpsc::Receiver<String>,
    pre_session_id: Option<String>,
    prompt_empty: bool,
) {
    // ── Claude 空 prompt 快速路径：session id 已知，文件 lazy 等 ──
    // 空 prompt 启动时 Claude 进入 REPL，要等用户实际发出首条消息才落盘 session 文件，
    // 走标准路径 wait_for_session_file 必然超时；这里直接用预生成 UUID 立刻广播，
    // 后台再无限等文件出现后 attach 监听。
    if let Some(ref sid) = pre_session_id {
        if !is_codex && prompt_empty {
            spawn_claude_lazy_session_attach(app, task_id, sid.clone(), project_path, agent);
            return;
        }
    }

    // ── Claude >= 2.1.87 快速路径：预置 session id，不发 /status ──
    if let Some(ref sid) = pre_session_id {
        if !is_codex {
            // ID 已经在 run_task 启动前确定，并由 spawn_claude_lazy_session_attach
            // 立即发给前端；这里不再等待文件或重复启动 watcher。
            spawn_claude_lazy_session_attach(app, task_id, sid.clone(), project_path, agent);
            return;
        }
    }

    // ── 原始路径：Codex 或 Claude < 2.1.87 ──
    thread::spawn(move || {
        run_status_session_watcher(app, task_id, project_path, is_codex, rx);
    });
}

/// 旧的 /status 轮询流程：非空 prompt 且无法使用 hook/预置 session 的任务走此路径。
fn run_status_session_watcher(
    app: AppHandle,
    task_id: String,
    project_path: String,
    is_codex: bool,
    rx: mpsc::Receiver<String>,
) {
    let start_time = Instant::now();
    let mut status_sent = false;
    let mut status_sent_at: Option<Instant> = None;
    let mut status_send_count: u32 = 0;
    let mut first_output_at = None;
    let mut accumulated = String::new();
    // 发送 /status 后的独立缓冲，避免大量输出将 /status 响应挤出裁剪窗口
    let mut status_response_buf = String::new();
    let mut collecting_response = false;

    loop {
        if !is_task_active(&app, &task_id) {
            break;
        }

        let should_send_status = should_send_status_command(
            status_sent,
            is_codex,
            start_time.elapsed(),
            first_output_at.map(|instant: Instant| instant.elapsed()),
        );

        // 首次发送或重试：若已发送但 3 秒内未提取到 Session ID，则再发一次。
        // Codex 在 session 创建前 /status 不含 Session 字段，需要在任务真正开始后重试。
        // 最多重试 5 次（含首次发送），避免对长时间无法解析的任务持续干扰 PTY 输入流。
        let should_retry = status_sent
            && status_send_count < 5
            && status_sent_at
                .map(|t| t.elapsed() >= Duration::from_secs(3))
                .unwrap_or(false);

        if should_send_status || should_retry {
            status_sent = true;
            status_send_count += 1;
            status_sent_at = Some(Instant::now());
            collecting_response = true;
            status_response_buf.clear();
            send_status_command(&app, &task_id, is_codex);
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                if is_codex && first_output_at.is_none() {
                    first_output_at = Some(Instant::now());
                }
                accumulated.push_str(&chunk);
                // 限制缓冲区大小，防止内存占用过大
                if accumulated.len() > 65536 {
                    let trim = accumulated.len() - 32768;
                    accumulated.drain(..trim);
                }

                // /status 发送后，额外收集响应到独立缓冲（最多 8KB），
                // 避免主缓冲裁剪把 Session ID 丢掉
                if collecting_response {
                    status_response_buf.push_str(&chunk);
                    if status_response_buf.len() > 8192 {
                        collecting_response = false;
                    }
                }

                let session_id = if is_codex {
                    extract_codex_status_session_id(&status_response_buf)
                        .or_else(|| extract_codex_status_session_id(&accumulated))
                } else {
                    extract_claude_status_session_id(&status_response_buf)
                        .or_else(|| extract_claude_status_session_id(&accumulated))
                };

                if let Some(sid) = session_id {
                    register_and_watch_session(&app, &task_id, &sid, &project_path, is_codex);
                    // Claude Code 的 /status 以全屏面板形式展示，需发送 ESC 关闭；
                    // Codex 无此面板，无需处理
                    if !is_codex {
                        let writer = {
                            let tm = app.state::<TaskManager>();
                            let writer = tm.pty_writers.lock().get(&task_id).cloned();
                            writer
                        };
                        if let Some(writer) = writer {
                            let mut writer = writer.lock();
                            let _ = writer.write_all(b"\x1b");
                            let _ = writer.flush();
                        }
                    }
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// 供 `resume_task` 使用：根据已知的 session_id 查找会话文件并开始监视。
pub(crate) fn spawn_resume_session_watcher(
    app: AppHandle,
    task_id: String,
    project_path: String,
    session_id: String,
    is_codex: bool,
) {
    thread::spawn(move || {
        register_and_watch_session(&app, &task_id, &session_id, &project_path, is_codex);
    });
}

// ── Markdown export ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_session_markdown(
    session_path: String,
    project_path: String,
    is_codex: bool,
    family: Option<String>,
    output_path: String,
    task_meta: ExportTaskMeta,
) -> Result<(), String> {
    let family = crate::app_settings::resolve_family_param(family.as_deref(), is_codex);
    tokio::task::spawn_blocking(move || {
        export_session_markdown_inner(
            &session_path,
            &project_path,
            family,
            &output_path,
            &task_meta,
        )
    })
    .await
    .map_err(|e| format!("Join error: {}", e))?
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_user_line(text: &str) -> String {
        serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": text
            }
        })
        .to_string()
    }

    fn codex_assistant_line(text: &str) -> String {
        serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    { "type": "output_text", "text": text }
                ]
            }
        })
        .to_string()
    }

    fn codex_tool_output_line(call_id: &str, output: &str) -> String {
        serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": output
            }
        })
        .to_string()
    }

    fn ignored_codex_like_line(idx: usize) -> String {
        serde_json::json!({
            "type": "turn_context",
            "payload": {
                "cwd": format!("/tmp/project-{idx}")
            }
        })
        .to_string()
    }

    #[test]
    fn parse_session_messages_detects_codex_after_initial_noise() {
        let mut lines: Vec<String> = (0..12).map(ignored_codex_like_line).collect();
        lines.push(codex_user_line("first visible user message"));
        lines.push(codex_assistant_line("assistant reply"));

        let messages = parse_session_messages(&lines, false);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert!(matches!(
            &messages[0].content[0],
            SessionContent::Text { text } if text == "first visible user message"
        ));
        assert_eq!(messages[1].role, "assistant");
        assert!(matches!(
            &messages[1].content[0],
            SessionContent::Text { text } if text == "assistant reply"
        ));
    }

    #[test]
    fn read_session_tail_keeps_only_the_newest_lines() {
        let path = std::env::temp_dir().join(format!(
            "aeroric-session-tail-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        // 头部放 session_meta(Codex 标记),再写超过窗口上限的行数,
        // 使最早的行必然被挤出尾部窗口。
        let mut raw = String::from(r#"{"type":"session_meta","payload":{"id":"abc"}}"#);
        raw.push('\n');
        let total = MAX_SESSION_LINES + 50;
        for idx in 0..total {
            raw.push_str(&codex_user_line(&format!("message-{idx}")));
            raw.push('\n');
        }
        std::fs::write(&path, &raw).expect("write session file");

        let (lines, detected_codex) = read_session_tail(&path).expect("read tail");
        let _ = std::fs::remove_file(&path);

        // 窗口被钳制,且保留的是最新的行。
        assert_eq!(lines.len(), MAX_SESSION_LINES);
        assert!(lines
            .last()
            .expect("last line")
            .contains(&format!("message-{}", total - 1)));
        assert!(!lines.iter().any(|line| line.contains("\"message-0\"")));
        // 关键:session_meta 已被挤出窗口,格式探测仍须在流式扫描阶段命中,
        // 否则 Codex 会话会被当成 Claude 格式解析成空列表。
        assert!(
            detected_codex,
            "codex format must be detected while streaming"
        );
        let messages = parse_session_messages_with_format(&lines, false, detected_codex);
        assert!(
            !messages.is_empty(),
            "truncated codex tail must still parse"
        );
    }

    #[test]
    fn session_message_pages_cover_more_than_twenty_thousand_lines_without_gaps() {
        let path = std::env::temp_dir().join(format!(
            "aeroric-session-pages-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let total = 20_137usize;
        let raw = (0..total)
            .map(|index| format!("记录-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, raw).expect("write paged session fixture");

        let mut cursor = None;
        let mut pages = Vec::new();
        loop {
            let (lines, next_cursor, has_more) =
                read_session_page_lines(&path, cursor, SESSION_MESSAGE_PAGE_LINES)
                    .expect("read session page");
            assert!(lines.len() <= SESSION_MESSAGE_PAGE_LINES);
            pages.push(lines);
            if !has_more {
                break;
            }
            let next = next_cursor.expect("cursor while more pages remain");
            if let Some(previous) = cursor {
                assert!(next < previous, "cursor must move toward the file start");
            }
            cursor = Some(next);
        }
        let _ = fs::remove_file(&path);

        let restored = pages.into_iter().rev().flatten().collect::<Vec<_>>();
        assert_eq!(restored.len(), total);
        for (index, line) in restored.iter().enumerate() {
            assert_eq!(line, &format!("记录-{index}"));
        }
    }

    #[test]
    fn parse_codex_custom_tools_reasoning_and_attachments() {
        let lines = vec![
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "reasoning summary"}]
                }
            })
            .to_string(),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "custom-1",
                    "name": "image_lookup",
                    "input": {"query": "diagram"}
                }
            })
            .to_string(),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call_output",
                    "call_id": "custom-1",
                    "output": {"status": "ok", "items": [1, 2]}
                }
            })
            .to_string(),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_image",
                        "name": "reference.png",
                        "media_type": "image/png",
                        "url": "https://example.test/reference.png"
                    }]
                }
            })
            .to_string(),
        ];

        let messages = parse_session_messages(&lines, true);
        assert!(messages.iter().flat_map(|message| &message.content).any(|content| {
            matches!(content, SessionContent::Thinking { thinking } if thinking == "reasoning summary")
        }));
        assert!(messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|content| {
                matches!(content, SessionContent::ToolUse { id, name, input }
                if id == "custom-1" && name == "image_lookup" && input.contains("diagram"))
            }));
        assert!(messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|content| {
                matches!(content, SessionContent::ToolResult { id, output }
                if id == "custom-1" && output.contains("items"))
            }));
        assert!(messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|content| {
                matches!(content, SessionContent::Attachment { name, media_type, source }
                if name == "reference.png"
                    && media_type == "image/png"
                    && source == "https://example.test/reference.png")
            }));
    }

    #[test]
    fn parse_session_messages_preserves_codex_tool_output_for_handoff() {
        let lines = vec![
            codex_user_line("inspect the file"),
            codex_tool_output_line("call-1", "file contents"),
        ];

        let messages = parse_session_messages(&lines, true);

        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[1].content[0],
            SessionContent::ToolResult { id, output }
                if id == "call-1" && output == "file contents"
        ));
    }

    #[test]
    fn parse_session_messages_preserves_claude_tool_result_for_handoff() {
        let lines = vec![
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "id": "tool-1",
                        "name": "Read",
                        "input": {"file_path": "src/main.rs"}
                    }]
                }
            })
            .to_string(),
            serde_json::json!({
                "type": "user",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "content": "source text"
                    }]
                }
            })
            .to_string(),
        ];

        let messages = parse_session_messages(&lines, false);

        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[1].content[0],
            SessionContent::ToolResult { id, output }
                if id == "tool-1" && output == "source text"
        ));
    }

    #[test]
    fn parse_session_messages_prefers_known_codex_agent_when_marker_is_absent() {
        let lines = vec![codex_assistant_line("assistant-only restored output")];

        let messages = parse_session_messages(&lines, true);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert!(matches!(
            &messages[0].content[0],
            SessionContent::Text { text } if text == "assistant-only restored output"
        ));
    }

    #[test]
    fn parse_session_messages_keeps_full_long_history() {
        let lines: Vec<String> = (0..260)
            .flat_map(|idx| {
                [
                    codex_user_line(&format!("user line {idx}")),
                    codex_assistant_line(&format!("assistant line {idx}")),
                ]
            })
            .collect();

        let messages = parse_session_messages(&lines, true);

        assert_eq!(messages.len(), 520);
        assert!(matches!(
            &messages.first().unwrap().content[0],
            SessionContent::Text { text } if text == "user line 0"
        ));
        assert!(matches!(
            &messages.last().unwrap().content[0],
            SessionContent::Text { text } if text == "assistant line 259"
        ));
    }

    #[test]
    fn resolve_session_id_reads_codex_meta_and_filename_fallback() {
        let dir = std::env::temp_dir();
        let meta_path =
            dir.join("rollout-2026-07-07T12-00-00-019f39d7-aaaa-7bbb-8ccc-9ddddddddddd.jsonl");
        fs::write(
            &meta_path,
            serde_json::json!({
                "type": "session_meta",
                "payload": { "id": "019f39d7-1111-2222-3333-444444444444" }
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            resolve_session_id_from_file(&meta_path, true),
            Some("019f39d7-1111-2222-3333-444444444444".to_string())
        );
        let _ = fs::remove_file(&meta_path);

        let fallback_path =
            dir.join("rollout-2026-07-07T12-00-00-019f39d7-aaaa-7bbb-8ccc-9ddddddddddd.jsonl");
        fs::write(&fallback_path, "{}\n").unwrap();
        assert_eq!(
            resolve_session_id_from_file(&fallback_path, true),
            Some("019f39d7-aaaa-7bbb-8ccc-9ddddddddddd".to_string())
        );
        let _ = fs::remove_file(&fallback_path);
    }

    #[test]
    fn recover_session_matches_project_prompt_and_creation_time() {
        let root = std::env::temp_dir().join(format!("aeroric-recover-{}", uuid::Uuid::new_v4()));
        let sessions = root.join(".codex/sessions/2026/07/10");
        fs::create_dir_all(&sessions).unwrap();
        let session_id = "019f39d7-1111-2222-3333-444444444444";
        let path = sessions.join(format!("rollout-2026-07-10T06-00-00-{session_id}.jsonl"));
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T06:00:00Z",
                "type": "session_meta",
                "payload": { "id": session_id, "cwd": root }
            })
            .to_string(),
            codex_user_line("inspect the current files"),
        ];
        fs::write(&path, lines.join("\n")).unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-07-10T06:02:00Z")
            .unwrap()
            .timestamp_millis();

        let recovered = recover_session(
            root.to_string_lossy().as_ref(),
            "inspect the current files",
            created_at,
            true,
        )
        .unwrap();

        assert_eq!(recovered.session_id, session_id);
        assert_eq!(recovered.session_path, path.to_string_lossy());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recover_session_rejects_different_project_and_stale_session() {
        let root = std::env::temp_dir().join(format!("aeroric-recover-{}", uuid::Uuid::new_v4()));
        let other = std::env::temp_dir().join(format!("aeroric-other-{}", uuid::Uuid::new_v4()));
        let sessions = root.join(".codex/sessions/2026/07/10");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(&other).unwrap();
        let path =
            sessions.join("rollout-2026-07-10T06-00-00-019f39d7-1111-2222-3333-444444444444.jsonl");
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T06:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "019f39d7-1111-2222-3333-444444444444",
                    "cwd": other
                }
            })
            .to_string(),
            codex_user_line("inspect the current files"),
        ];
        fs::write(path, lines.join("\n")).unwrap();
        let stale_created_at = chrono::DateTime::parse_from_rfc3339("2026-07-10T07:00:00Z")
            .unwrap()
            .timestamp_millis();

        assert!(recover_session(
            root.to_string_lossy().as_ref(),
            "inspect the current files",
            stale_created_at,
            true,
        )
        .is_none());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&other);
    }

    #[test]
    fn includes_custom_agent_session_roots_for_both_agent_types() {
        let home =
            std::env::temp_dir().join(format!("aeroric-agent-roots-{}", uuid::Uuid::new_v4()));
        let agent_home = home.join(".aeroric/agent-homes/custom");
        fs::create_dir_all(&agent_home).unwrap();
        let project_path = "/tmp/example project";

        let mut codex_roots = Vec::new();
        append_custom_agent_session_roots(&mut codex_roots, &home, project_path, true);
        assert_eq!(codex_roots, vec![agent_home.join("sessions")]);

        let mut claude_roots = Vec::new();
        append_custom_agent_session_roots(&mut claude_roots, &home, project_path, false);
        assert_eq!(
            claude_roots,
            vec![agent_home
                .join("projects")
                .join(encode_claude_project_path(project_path))]
        );

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn claude_transcript_dir_follows_the_agents_own_config_home() {
        let home = std::env::temp_dir().join(format!("aeroric-agent-dir-{}", uuid::Uuid::new_v4()));
        let project_path = "/tmp/example project";
        let encoded = encode_claude_project_path(project_path);

        // 内建 Agent 写 ~/.claude/projects/<encoded>
        for builtin in ["claude", "claude_gpt55"] {
            assert_eq!(
                claude_sessions_dir_for_agent_in(&home, builtin, project_path),
                Some(home.join(".claude").join("projects").join(&encoded)),
                "builtin agent {builtin} must use ~/.claude",
            );
        }

        // 自定义 Agent 的 CLAUDE_CONFIG_DIR 是隔离 home，transcript 也在那里；
        // 用 ~/.claude 猜路径会得到一个永不存在的文件。
        let custom = claude_sessions_dir_for_agent_in(&home, "sota_claude", project_path).unwrap();
        assert_eq!(
            custom,
            home.join(".aeroric")
                .join("agent-homes")
                .join("sota_claude")
                .join("projects")
                .join(&encoded),
        );
        assert_ne!(custom, home.join(".claude").join("projects").join(&encoded));

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn resolve_session_id_reads_claude_session_id_and_filename_fallback() {
        let dir = std::env::temp_dir();
        let meta_path = dir.join("claude-session-with-title.jsonl");
        fs::write(
            &meta_path,
            serde_json::json!({
                "type": "user",
                "sessionId": "1aee0948-e0f2-4ad1-b710-ba236fab378a",
                "message": { "content": "hello" }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            resolve_session_id_from_file(&meta_path, false),
            Some("1aee0948-e0f2-4ad1-b710-ba236fab378a".to_string())
        );
        let _ = fs::remove_file(&meta_path);

        let fallback_path = dir.join("1aee0948-e0f2-4ad1-b710-ba236fab378a.jsonl");
        fs::write(&fallback_path, "{}\n").unwrap();
        assert_eq!(
            resolve_session_id_from_file(&fallback_path, false),
            Some("1aee0948-e0f2-4ad1-b710-ba236fab378a".to_string())
        );
        let _ = fs::remove_file(&fallback_path);
    }

    #[test]
    fn extract_claude_status_session_id_from_status_output() {
        // Simple \r\n separated output
        let output = "\x1b[0m\r\n  Version: 2.1.81\r\n  Session ID: 1aee0948-e0f2-4ad1-b710-ba236fab378a\r\n  cwd: /workspace\r\n\x1b[0m";
        assert_eq!(
            extract_claude_status_session_id(output),
            Some("1aee0948-e0f2-4ad1-b710-ba236fab378a".to_string())
        );
    }

    #[test]
    fn extract_claude_status_session_id_cursor_positioned() {
        // Claude Code renders /status using cursor-positioning sequences; after ANSI
        // stripping the text collapses onto one line with no \r\n separators.
        let output = "\x1b[1;1H  Version: 2.1.83\x1b[2;1H  Session ID: 9d5533cd-af1e-48d5-99d3-a9e61b2a5250\x1b[3;1H  cwd: /workspace";
        assert_eq!(
            extract_claude_status_session_id(output),
            Some("9d5533cd-af1e-48d5-99d3-a9e61b2a5250".to_string())
        );
    }

    #[test]
    fn extract_claude_status_session_id_returns_none_when_absent() {
        assert_eq!(
            extract_claude_status_session_id("no session info here"),
            None
        );
    }

    #[test]
    fn extract_codex_status_session_id_from_status_output() {
        let output = "\r\n│  Session:                     019d247a-2a83-76f3-b5c6-e4a59955af3f                                │\r\n";
        assert_eq!(
            extract_codex_status_session_id(output),
            Some("019d247a-2a83-76f3-b5c6-e4a59955af3f".to_string())
        );
    }

    #[test]
    fn extract_codex_status_session_id_with_ansi() {
        let output = "\x1b[0m\r\n\u{2502}  Session:                     019d0a3e-3cf7-7513-b7de-e3e9bc6c7f4d  \u{2502}\r\n\x1b[0m";
        assert_eq!(
            extract_codex_status_session_id(output),
            Some("019d0a3e-3cf7-7513-b7de-e3e9bc6c7f4d".to_string())
        );
    }

    #[test]
    fn extract_codex_status_session_id_cursor_positioned() {
        // Codex renders /status using cursor-positioning sequences; after ANSI stripping
        // all content collapses onto one line with no \r\n separators — same as Claude Code.
        let output = "\x1b[1;1H  OpenAI Codex (v0.116.0)\x1b[3;1H  Session:                     019d28df-14c0-7d03-8209-07dd4ae22cd1\x1b[4;1H  Context window:  100% left";
        assert_eq!(
            extract_codex_status_session_id(output),
            Some("019d28df-14c0-7d03-8209-07dd4ae22cd1".to_string())
        );
    }

    #[test]
    fn extract_codex_status_session_id_returns_none_when_absent() {
        assert_eq!(
            extract_codex_status_session_id("no session info here"),
            None
        );
    }

    #[test]
    fn codex_status_waits_for_first_output_then_one_second() {
        assert!(!should_send_status_command(
            false,
            true,
            Duration::from_secs(2),
            None,
        ));
        assert!(!should_send_status_command(
            false,
            true,
            Duration::from_millis(2200),
            Some(Duration::from_millis(900)),
        ));
        assert!(should_send_status_command(
            false,
            true,
            Duration::from_millis(2200),
            Some(Duration::from_secs(1)),
        ));
    }

    #[test]
    fn codex_status_has_global_timeout_fallback() {
        assert!(should_send_status_command(
            false,
            true,
            Duration::from_secs(8),
            None,
        ));
    }

    #[test]
    fn claude_status_keeps_original_delay() {
        assert!(!should_send_status_command(
            false,
            false,
            Duration::from_millis(1499),
            None,
        ));
        assert!(should_send_status_command(
            false,
            false,
            Duration::from_millis(1500),
            None,
        ));
    }

    #[test]
    fn empty_agent_repl_does_not_need_status_watcher_without_hooks() {
        assert!(!should_start_status_session_watcher(false, true, true));
        assert!(!should_start_status_session_watcher(false, false, true));
        assert!(should_start_status_session_watcher(false, true, false));
        assert!(should_start_status_session_watcher(false, false, false));
        assert!(!should_start_status_session_watcher(true, false, false));
        assert!(!should_start_status_session_watcher(true, true, false));
    }

    #[test]
    fn read_only_command_detection_is_conservative() {
        assert!(looks_like_read_only_command("pwd && rg -n session src"));
        assert!(looks_like_read_only_command(
            "sed -n '1,120p' src-tauri/src/lib.rs"
        ));
        assert!(!looks_like_read_only_command(
            "cargo test --manifest-path src-tauri/Cargo.toml"
        ));
        assert!(!looks_like_read_only_command("echo hello > out.txt"));
    }

    #[test]
    fn powershell_read_only_commands_are_treated_as_safe() {
        assert!(looks_like_read_only_command(
            "Get-ChildItem -Force | Select-String -Pattern session"
        ));
        assert!(looks_like_read_only_command(
            "Get-Content README.md | Select-Object -First 20"
        ));
        assert!(looks_like_read_only_command("git.exe status --short"));
    }

    #[test]
    fn exec_command_confirmation_detection_matches_escalation_and_write_commands() {
        assert!(exec_command_requires_confirmation(
            r#"{"cmd":"rg -n session src","sandbox_permissions":"require_escalated"}"#
        ));
        assert!(exec_command_requires_confirmation(
            r#"{"cmd":"cargo test --manifest-path src-tauri/Cargo.toml --lib"}"#
        ));
        assert!(!exec_command_requires_confirmation(
            r#"{"cmd":"git status --short"}"#
        ));
    }

    #[test]
    fn apply_patch_confirmation_detection_only_flags_external_absolute_paths() {
        let project_root = Path::new("/repo");

        assert!(!apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: src/main.rs\n*** End Patch",
            project_root,
        ));
        assert!(!apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: /repo/src/main.rs\n*** End Patch",
            project_root,
        ));
        assert!(apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: /var/aeroric-outside.rs\n*** End Patch",
            project_root,
        ));
    }

    #[test]
    fn final_assistant_question_is_treated_as_input_required() {
        let payload = serde_json::json!({
            "role": "assistant",
            "phase": "final_answer",
            "content": [
                { "type": "output_text", "text": "继续按这个方案改吗？" }
            ]
        });

        assert!(assistant_message_requests_user_input(Some(&payload)));
    }

    fn sample_meta() -> ExportTaskMeta {
        ExportTaskMeta {
            name: Some("Demo task".into()),
            prompt: "do the thing".into(),
            agent: "claude".into(),
            created_at: 1_715_990_400_000, // 2024-05-18T00:00:00Z
            session_id: Some("abc-123".into()),
            worktree_branch: None,
            base_branch: None,
            additions: None,
            deletions: None,
            failure_reason: None,
        }
    }

    /// 测试 helper：把 streaming 输出收到 Vec<u8> 再转 String，方便对内容做断言。
    fn render_to_string(meta: &ExportTaskMeta, msgs: &[SessionMessage]) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_export_markdown(&mut buf, meta, msgs).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn export_markdown_includes_metadata_and_prompt() {
        let md = render_to_string(&sample_meta(), &[]);
        assert!(md.starts_with("# Demo task\n\n"), "title missing: {}", md);
        assert!(md.contains("- **Agent**: claude"));
        assert!(md.contains("- **Session ID**: `abc-123`"));
        assert!(md.contains("> do the thing"));
    }

    #[test]
    fn export_markdown_drops_tool_use_and_thinking_blocks() {
        let messages = vec![
            SessionMessage {
                role: "assistant".into(),
                content: vec![
                    SessionContent::Thinking {
                        thinking: "let me reason".into(),
                    },
                    SessionContent::Text {
                        text: "first turn".into(),
                    },
                ],
                message_id: None,
            },
            SessionMessage {
                role: "assistant".into(),
                content: vec![SessionContent::ToolUse {
                    id: "t1".into(),
                    name: "Bash".into(),
                    input: "{\"cmd\":\"ls\"}".into(),
                }],
                message_id: None,
            },
            SessionMessage {
                role: "assistant".into(),
                content: vec![SessionContent::Text {
                    text: "second turn".into(),
                }],
                message_id: None,
            },
        ];
        let md = render_to_string(&sample_meta(), &messages);
        // 连续 assistant 文本应合并到同一个标题下；tool-only 消息被整体丢弃
        assert_eq!(md.matches("### Assistant").count(), 1, "{}", md);
        assert!(!md.contains("👤"));
        assert!(!md.contains("🤖"));
        assert!(md.contains("first turn"));
        assert!(md.contains("second turn"));
        assert!(!md.contains("🔧"));
        assert!(!md.contains("Bash"));
        assert!(!md.contains("Thinking"));
        assert!(!md.contains("let me reason"));
    }

    #[test]
    fn export_markdown_falls_back_to_prompt_when_name_missing() {
        let mut meta = sample_meta();
        meta.name = None;
        meta.prompt = "fix the login bug".into();
        let md = render_to_string(&meta, &[]);
        assert!(
            md.starts_with("# fix the login bug\n\n"),
            "title fallback wrong: {}",
            md
        );
    }

    #[test]
    fn export_markdown_sanitizes_metadata_with_newlines_and_backticks() {
        let mut meta = sample_meta();
        meta.name = Some("multi\nline\ttitle".into());
        meta.session_id = Some("abc`evil`123".into());
        meta.worktree_branch = Some("feat/`branch".into());
        meta.base_branch = Some("main".into());
        meta.failure_reason = Some("first line\nsecond line".into());
        let md = render_to_string(&meta, &[]);

        // 标题压缩为单行，不能让换行/Tab 把 # 标题之外的结构撑歪
        assert!(
            md.starts_with("# multi line title\n\n"),
            "title not collapsed: {}",
            md
        );
        // session_id / branch 在行内代码 span 里，反引号必须被替换掉
        assert!(md.contains("- **Session ID**: `abc'evil'123`"), "{}", md);
        assert!(
            md.contains("- **Branch**: `feat/'branch` → `main`"),
            "{}",
            md
        );
        // failure reason 的换行被折叠成单空格，不破坏列表项
        assert!(
            md.contains("- **Failure reason**: first line second line"),
            "{}",
            md
        );
    }

    #[test]
    fn validate_export_output_path_rejects_relative_and_non_md() {
        assert!(validate_export_output_path("relative/path.md").is_err());
        assert!(validate_export_output_path("/tmp/notamd.txt").is_err());
    }

    #[test]
    fn validate_export_output_path_rejects_missing_parent() {
        // 极不可能存在的父目录
        assert!(
            validate_export_output_path("/nonexistent-9c3a/__aeroric_export_test__/out.md")
                .is_err()
        );
    }

    #[test]
    fn validate_export_output_path_accepts_md_under_existing_dir() {
        let dir = std::env::temp_dir();
        let candidate = dir.join("aeroric-validate-output.md");
        // 文件本身不必存在；只要父目录存在即可。
        let canonical = validate_export_output_path(candidate.to_str().unwrap())
            .expect("temp dir export path should validate");
        assert!(canonical.is_absolute());
        assert_eq!(canonical.extension().and_then(|e| e.to_str()), Some("md"));
    }

    #[test]
    fn adopted_codex_target_keeps_the_date_directory_layout() {
        let source = PathBuf::from(
            "/tmp/src-home/sessions/2026/08/13/rollout-2026-08-13T00-00-00-abc.jsonl",
        );
        let target = adopted_session_target_path(&source, "codex", true, "/tmp/project")
            .expect("codex target path should resolve");

        let tail: Vec<_> = target
            .components()
            .rev()
            .take(5)
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        assert_eq!(
            tail,
            vec![
                "rollout-2026-08-13T00-00-00-abc.jsonl",
                "13",
                "08",
                "2026",
                "sessions",
            ]
        );
    }

    #[test]
    fn adopted_codex_target_drops_non_date_parents() {
        // 源文件不在 `<yyyy>/<mm>/<dd>/` 布局下时只保留 sessions/ 根，避免搬进无意义的目录。
        let source = PathBuf::from("/tmp/src-home/sessions/rollout-loose.jsonl");
        let target = adopted_session_target_path(&source, "codex", true, "/tmp/project")
            .expect("codex target path should resolve");

        let tail: Vec<_> = target
            .components()
            .rev()
            .take(2)
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        assert_eq!(tail, vec!["rollout-loose.jsonl", "sessions"]);
    }

    #[test]
    fn adopted_claude_target_uses_the_encoded_project_directory() {
        let source = PathBuf::from("/tmp/src-home/projects/-tmp-project/session-uuid.jsonl");
        let target = adopted_session_target_path(&source, "claude", false, "/tmp/project")
            .expect("claude target path should resolve");

        let tail: Vec<_> = target
            .components()
            .rev()
            .take(3)
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        assert_eq!(
            tail,
            vec![
                "session-uuid.jsonl",
                encode_claude_project_path("/tmp/project").as_str(),
                "projects",
            ]
        );
    }

    #[tokio::test]
    async fn adopt_session_for_agent_rejects_unknown_agents() {
        let error = adopt_session_for_agent(
            "/tmp/whatever.jsonl".into(),
            "/tmp/project".into(),
            true,
            "not-a-real-agent".into(),
        )
        .await
        .expect_err("unknown agents must be rejected before any file access");
        assert!(error.contains("Unknown agent"), "unexpected error: {error}");
    }
}
