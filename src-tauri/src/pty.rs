use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::session::{
    should_start_status_session_watcher, spawn_resume_session_watcher, spawn_status_session_watcher,
};
use crate::TaskManager;

const SESSION_WAIT_POLL: Duration = Duration::from_millis(50);
const SESSION_WAIT_MAX: Duration = Duration::from_millis(500);
const PTY_READ_BUFFER_SIZE: usize = 32 * 1024;
pub(crate) const PTY_EMIT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
pub(crate) const PTY_EMIT_MAX_BATCH_BYTES: usize = 64 * 1024;
/// 有界 channel 容量：满时 reader 线程阻塞，反压传播至 OS 内核 PTY 缓冲区，
/// 最终使写入进程（Claude/Codex）的 write() 系统调用阻塞，从源头限流。
const PTY_EMIT_CHANNEL_CAPACITY: usize = 32;
const STARTUP_FIRST_OUTPUT_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_NO_OUTPUT_FALLBACK: Duration = Duration::from_secs(5);
const STARTUP_OUTPUT_QUIET: Duration = Duration::from_millis(220);
const STARTUP_OUTPUT_MAX_WAIT: Duration = Duration::from_secs(2);
const STARTUP_GATE_INPUT_SETTLE: Duration = Duration::from_millis(1200);
/// 门控等待的绝对上限。误判成门控时,没有上限会让首条 prompt 永不投递,
/// 并把 blocking 线程占到 PTY 断开为止。到点后放弃注入而不是硬写入,
/// 因为此时若确实存在真实确认框,写入会落进选择器。
const STARTUP_GATE_MAX_WAIT: Duration = Duration::from_secs(120);
/// `generic_gate` 判定所用的尾部窗口大小(字节)。
const GENERIC_GATE_WINDOW: usize = 200;

/// 启动态门控信号:首条输入可能需要等 trust folder / hook 授权完成后再投递。
#[derive(Debug)]
pub(crate) enum StartupSignal {
    Output(String),
    UserInput,
    SessionReady,
}

fn task_attachments_dir(project_path: &str, task_id: &str) -> std::path::PathBuf {
    Path::new(project_path)
        .join(".aeroric")
        .join("attachments")
        .join(task_id)
}

pub(crate) fn validate_task_id(task_id: &str) -> Result<(), String> {
    crate::storage::validate_storage_id(task_id, "task")
}

fn validate_shell_id(shell_id: &str) -> Result<(), String> {
    validate_namespaced_shell_id(shell_id, "shell:", "local shell")
}

pub(crate) fn validate_ssh_shell_id(shell_id: &str) -> Result<(), String> {
    validate_namespaced_shell_id(shell_id, "ssh:", "SSH shell")
}

pub(crate) fn validate_wsl_shell_id(shell_id: &str) -> Result<(), String> {
    validate_namespaced_shell_id(shell_id, "wsl:", "WSL shell")
}

fn validate_namespaced_shell_id(
    shell_id: &str,
    expected_prefix: &str,
    label: &str,
) -> Result<(), String> {
    let suffix = shell_id
        .strip_prefix(expected_prefix)
        .ok_or_else(|| format!("Invalid {label} id"))?;
    if suffix.is_empty()
        || !suffix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.'))
    {
        return Err(format!("Invalid {label} id"));
    }
    Ok(())
}

fn has_task_session(app: &AppHandle, task_id: &str, is_codex: bool) -> bool {
    let tm = app.state::<TaskManager>();
    if is_codex {
        tm.codex_sessions.lock().contains_key(task_id)
    } else {
        tm.claude_sessions.lock().contains_key(task_id)
    }
}

/// 任务结束后，等待会话注册完成，最长等待 500ms。
fn wait_for_session(app: &AppHandle, task_id: &str, is_codex: bool) {
    let deadline = Instant::now() + SESSION_WAIT_MAX;
    while Instant::now() < deadline {
        if has_task_session(app, task_id, is_codex) {
            return;
        }
        std::thread::sleep(SESSION_WAIT_POLL);
    }
}

fn finalize_task_exit(
    app: &AppHandle,
    task_id: &str,
    project_path: &str,
    is_codex: bool,
    exit_ok: bool,
    exit_code: Option<u32>,
) {
    let (is_cancelled, is_manually_completed) = {
        let tm = app.state::<TaskManager>();
        let mut cancelled = tm.cancelled_tasks.lock();
        let mut manually_completed = tm.manually_completed_tasks.lock();
        (
            cancelled.remove(task_id),
            manually_completed.remove(task_id),
        )
    };

    let had_agent_session;
    {
        let tm = app.state::<TaskManager>();
        tm.remove_pty_handles(task_id);
        let codex_info = tm.codex_sessions.lock().remove(task_id);
        let codex_path = codex_info.map(|info| info.session_path);
        let claude_info = tm.claude_sessions.lock().remove(task_id);
        let claude_path = claude_info.as_ref().map(|info| info.session_path.clone());
        had_agent_session = if is_codex {
            codex_path.is_some()
        } else {
            // lazy attach 注入的占位条目不算"曾真正建立过会话"，
            // 否则 Claude 异常退出会被误标为 done。
            claude_info
                .as_ref()
                .map(|info| !info.is_placeholder)
                .unwrap_or(false)
        };
        let mut claimed = tm.claimed_session_paths.lock();
        if let Some(path) = codex_path {
            claimed.remove(&path);
        }
        if let Some(path) = claude_path {
            claimed.remove(&path);
        }
    }

    if is_cancelled || is_manually_completed {
        let _ = fs::remove_dir_all(task_attachments_dir(project_path, task_id));
        return;
    }

    let status = if exit_ok || had_agent_session {
        "done"
    } else {
        "failed"
    };
    let payload = if status == "failed" {
        let reason = match exit_code {
            Some(code) => format!("Process exited with code {}", code),
            None => "Process exited with non-zero status".to_string(),
        };
        serde_json::json!({ "task_id": task_id, "status": status, "failure_reason": reason })
    } else {
        serde_json::json!({ "task_id": task_id, "status": status })
    };
    let _ = app.emit("task-status", payload);

    let _ = fs::remove_dir_all(task_attachments_dir(project_path, task_id));
    crate::event_watcher::cleanup_task_events(app, task_id);
}

fn save_task_images(
    project_path: &str,
    task_id: &str,
    images: &[String],
) -> Result<Vec<String>, String> {
    if images.is_empty() {
        return Ok(vec![]);
    }
    let attachments_dir = task_attachments_dir(project_path, task_id);
    fs::create_dir_all(&attachments_dir).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, data_url) in images.iter().enumerate() {
        // 解析 "data:image/png;base64,<data>" 格式
        let comma = data_url.find(',').ok_or("invalid image data URL")?;
        let header = &data_url[..comma];
        let b64 = &data_url[comma + 1..];
        let ext = if header.contains("jpeg") || header.contains("jpg") {
            "jpg"
        } else if header.contains("gif") {
            "gif"
        } else if header.contains("webp") {
            "webp"
        } else {
            "png"
        };
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| e.to_string())?;
        let filename = format!("{}.{}", i, ext);
        let file_path = attachments_dir.join(&filename);
        fs::write(&file_path, &data).map_err(|e| e.to_string())?;
        paths.push(file_path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

fn save_task_texts(
    project_path: &str,
    task_id: &str,
    texts: &[String],
) -> Result<Vec<String>, String> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    let attachments_dir = task_attachments_dir(project_path, task_id);
    fs::create_dir_all(&attachments_dir).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, text) in texts.iter().enumerate() {
        let filename = format!("paste_{}.txt", i);
        let file_path = attachments_dir.join(&filename);
        fs::write(&file_path, text.as_bytes()).map_err(|e| e.to_string())?;
        paths.push(file_path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

fn release_claimed_session_paths(task_manager: &TaskManager, task_id: &str) {
    let codex_path = task_manager
        .codex_sessions
        .lock()
        .get(task_id)
        .map(|info| info.session_path.clone());
    let claude_path = task_manager
        .claude_sessions
        .lock()
        .get(task_id)
        .map(|info| info.session_path.clone());
    let mut claimed = task_manager.claimed_session_paths.lock();
    if let Some(path) = codex_path {
        claimed.remove(&path);
    }
    if let Some(path) = claude_path {
        claimed.remove(&path);
    }
}

// ── 共享 PTY 辅助函数 ────────────────────────────────────────────────────────

/// 设置 CommandBuilder 的标准环境变量。
pub(crate) fn setup_env(cmd: &mut CommandBuilder) {
    // CommandBuilder::new() snapshots the parent process environment before we
    // merge the login-shell environment. Merely skipping NO_COLOR below does
    // not remove an inherited value (for example when Aeroric itself is
    // launched by a terminal/agent with NO_COLOR=1).
    for key in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "FORCE_COLOR"] {
        cmd.env_remove(key);
    }

    let login_env = crate::app_settings::get_login_shell_env();
    for (key, value) in login_env {
        if key == "NO_COLOR" || key == "CLICOLOR" || key == "CLICOLOR_FORCE" || key == "FORCE_COLOR"
        {
            continue;
        }
        cmd.env(key, value);
    }

    // 确保 locale 为 UTF-8。
    // macOS 的 Terminal.app / iTerm2 会自动注入 LANG，但从 Dock 启动的 Tauri 应用
    // 进程环境中没有 locale 变量，导致 PTY 子进程无法正确处理中文等多字节输入。
    let has = |name: &str| login_env.iter().any(|(k, _)| k == name);
    if !has("LANG") {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if !has("LC_CTYPE") {
        cmd.env("LC_CTYPE", "en_US.UTF-8");
    }

    // 设置终端类型，使 Claude Code / Codex 输出正确的转义序列
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("CLICOLOR", "1");
    cmd.env("CLICOLOR_FORCE", "1");
    cmd.env("FORCE_COLOR", "3");
}

/// 注入 Aeroric hook 守卫所需的环境变量。
/// hook 脚本依靠 AERORIC_TASK_ID + AERORIC_EVENT_DIR 同时存在才工作,
/// 用户在 Aeroric 之外手动跑 agent 时这些变量缺失,脚本立即 exit 0。
fn setup_aeroric_env(cmd: &mut CommandBuilder, task_id: &str, agent: &str, is_codex: bool) {
    if let Ok(dir) = crate::hooks::events_dir_for(task_id) {
        cmd.env("AERORIC_TASK_ID", task_id);
        cmd.env("AERORIC_EVENT_DIR", dir.to_string_lossy().as_ref());
        cmd.env("AERORIC_AGENT", agent);
        cmd.env("AERORIC_AGENT_CODEX_LIKE", if is_codex { "1" } else { "0" });
    }
}

/// 将 PTY master/writer/child 注册到 TaskManager 的三个 HashMap 中。
pub(crate) fn register_pty_handles(
    task_manager: &TaskManager,
    id: &str,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
) -> Result<(), String> {
    let mut masters = task_manager.pty_masters.lock();
    let mut pending_sizes = task_manager.pending_pty_sizes.lock();
    if let Some((cols, rows)) = pending_sizes.remove(id) {
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    masters.insert(id.to_string(), Arc::new(parking_lot::Mutex::new(master)));
    drop(pending_sizes);
    drop(masters);
    task_manager
        .pty_writers
        .lock()
        .insert(id.to_string(), Arc::new(parking_lot::Mutex::new(writer)));
    task_manager
        .child_handles
        .lock()
        .insert(id.to_string(), Arc::new(parking_lot::Mutex::new(child)));
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum PtyEmitMode {
    Immediate,
    Batched {
        flush_interval: Duration,
        max_batch_bytes: usize,
    },
}

/// 输出归宿：agent / SSH 任务用 Channel 直投单一前端订阅者，跳过事件总线的全局广播
/// 与 JSON payload；本地 shell 仍走 emit，多面板挂载时由前端按 shell_id 筛选。
#[derive(Clone)]
pub(crate) enum OutputSink {
    Event {
        event_name: &'static str,
        id_key: &'static str,
    },
    Channel(Channel<String>),
}

pub(crate) type PtyOutputFilter = Box<dyn FnMut(String) -> Option<String> + Send>;

fn send_pty_chunk(app: &AppHandle, id: &str, sink: &OutputSink, data: String) {
    match sink {
        OutputSink::Event { event_name, id_key } => {
            let mut payload = serde_json::Map::new();
            payload.insert(
                (*id_key).to_string(),
                serde_json::Value::String(id.to_string()),
            );
            payload.insert("data".to_string(), serde_json::Value::String(data));
            let _ = app.emit(event_name, serde_json::Value::Object(payload));
        }
        OutputSink::Channel(channel) => {
            let _ = channel.send(data);
        }
    }
}

fn flush_pty_batch(app: &AppHandle, id: &str, sink: &OutputSink, batch: &mut String) {
    if batch.is_empty() {
        return;
    }
    send_pty_chunk(app, id, sink, std::mem::take(batch));
}

/// 在后台线程中读取 PTY 输出，按 sink 把数据投递给前端。
///
/// - `sink`：agent / SSH 传 `OutputSink::Channel`，本地 shell 传 `OutputSink::Event`
/// - `session_tx`：可选 channel，用于将原始文本转发给 session watcher
/// - `startup_tx`：把原始输出转成启动态信号,供初始输入门控判断
/// - `output_filter`：可选输出过滤器，可在发送到前端前消费或改写数据
/// - `on_finish`：PTY 关闭后执行的可选清理回调
pub(crate) fn spawn_pty_reader(
    app: AppHandle,
    id: String,
    sink: OutputSink,
    emit_mode: PtyEmitMode,
    reader: Box<dyn Read + Send>,
    persist_terminal_history: bool,
    session_tx: Option<std::sync::mpsc::Sender<String>>,
    startup_tx: Option<std::sync::mpsc::Sender<StartupSignal>>,
    output_filter: Option<PtyOutputFilter>,
    on_finish: Option<Box<dyn FnOnce() + Send>>,
) {
    tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut output_filter = output_filter;
        let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
        // 保存上次读取中不完整的 UTF-8 字节序列
        let mut leftover: Vec<u8> = Vec::new();
        let (emit_tx, emit_worker) = match emit_mode {
            PtyEmitMode::Immediate => (None, None),
            PtyEmitMode::Batched {
                flush_interval,
                max_batch_bytes,
            } => {
                let (tx, rx) = std::sync::mpsc::sync_channel::<String>(PTY_EMIT_CHANNEL_CAPACITY);
                let emit_app = app.clone();
                let emit_id = id.clone();
                let worker_sink = sink.clone();
                let worker = std::thread::spawn(move || {
                    let mut batch = String::new();
                    loop {
                        match rx.recv_timeout(flush_interval) {
                            Ok(chunk) => {
                                batch.push_str(&chunk);
                                if batch.len() >= max_batch_bytes {
                                    flush_pty_batch(&emit_app, &emit_id, &worker_sink, &mut batch);
                                }
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                flush_pty_batch(&emit_app, &emit_id, &worker_sink, &mut batch);
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                flush_pty_batch(&emit_app, &emit_id, &worker_sink, &mut batch);
                                break;
                            }
                        }
                    }
                });
                (Some(tx), Some(worker))
            }
        };
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut combined = std::mem::take(&mut leftover);
                    combined.extend_from_slice(&buf[..n]);

                    let valid_len = match std::str::from_utf8(&combined) {
                        Ok(_) => combined.len(),
                        Err(e) => e.valid_up_to(),
                    };

                    if valid_len > 0 {
                        let Ok(data) = std::str::from_utf8(&combined[..valid_len]) else {
                            continue;
                        };
                        let data = data.to_owned();
                        if let Some(ref tx) = startup_tx {
                            let _ = tx.send(StartupSignal::Output(data.clone()));
                        }
                        let data = match output_filter.as_mut() {
                            Some(filter) => match filter(data) {
                                Some(data) if !data.is_empty() => data,
                                _ => {
                                    if valid_len < combined.len() {
                                        leftover = combined[valid_len..].to_vec();
                                    }
                                    continue;
                                }
                            },
                            None => data,
                        };
                        if persist_terminal_history {
                            let _ = crate::storage::append_task_terminal_history(&id, &data);
                            // 手机远程终端流 tee:无订阅者时近零开销,不影响桌面 Channel 路径
                            crate::remote::terminal_hub::hub().publish(&id, &data);
                        }
                        // session_tx 需要独立副本；data 本身留给 emit 路径 move，避免多余堆分配
                        if let Some(ref tx) = session_tx {
                            let _ = tx.send(data.clone());
                        }
                        if let Some(ref tx) = emit_tx {
                            match tx.send(data) {
                                Ok(()) => {}
                                Err(err) => send_pty_chunk(&app, &id, &sink, err.0),
                            }
                        } else {
                            send_pty_chunk(&app, &id, &sink, data);
                        }
                    }

                    if valid_len < combined.len() {
                        leftover = combined[valid_len..].to_vec();
                    }
                }
            }
        }
        drop(emit_tx);
        if let Some(worker) = emit_worker {
            let _ = worker.join();
        }
        // session_tx 在此处被 drop，watcher 端的 Receiver 将收到 Disconnected 信号
        if let Some(f) = on_finish {
            f();
        }
    });
}

/// 在后台线程中轮询子进程退出状态，退出后调用 finalize_task_exit。
fn spawn_exit_monitor(app: AppHandle, task_id: String, project_path: String, is_codex: bool) {
    tokio::task::spawn_blocking(move || loop {
        let exit_status = {
            let tm = app.state::<TaskManager>();
            let child_arc = tm.child_handles.lock().get(&task_id).cloned();
            if let Some(arc) = child_arc {
                arc.lock().try_wait().ok().flatten()
            } else {
                return;
            }
        };

        if let Some(status) = exit_status {
            let exit_ok = status.success();
            let exit_code = if exit_ok {
                None
            } else {
                Some(status.exit_code())
            };
            // 等待会话注册完成
            wait_for_session(&app, &task_id, is_codex);
            finalize_task_exit(&app, &task_id, &project_path, is_codex, exit_ok, exit_code);
            return;
        }

        std::thread::sleep(Duration::from_millis(100));
    });
}

/// 为 Claude 命令构建 CommandBuilder，并根据 permission_mode 添加权限标志。
fn build_claude_cmd(
    launch: &crate::app_settings::AgentLaunchSpec,
    permission_mode: &str,
) -> CommandBuilder {
    let mut c = CommandBuilder::new(&launch.program);
    c.args(&launch.args);
    // Claude Code 自 v2.1.150 起默认开启 xterm 鼠标上报（mouse mode 1002），会拦截
    // 终端原生框选——表现为运行时拖动看似选中却不进选区态、无法复制。关掉它后滚轮回退
    // 到 xterm 自身 scrollback，用户运行时即可直接拖动框选。官方开关，仅影响 Claude。
    c.env("CLAUDE_CODE_DISABLE_MOUSE", "1");
    match permission_mode {
        "ask" => {
            c.arg("--permission-mode");
            c.arg("default");
        }
        "auto_edit" => {
            c.arg("--permission-mode");
            c.arg("acceptEdits");
        }
        "full_access" => {
            c.arg("--dangerously-skip-permissions");
        }
        _ => {}
    }
    c
}

/// 为 Codex 命令构建 CommandBuilder，并根据 permission_mode 添加全局执行标志。
fn build_codex_cmd(
    launch: &crate::app_settings::AgentLaunchSpec,
    permission_mode: &str,
) -> CommandBuilder {
    let mut c = CommandBuilder::new(&launch.program);
    c.args(&launch.args);
    match permission_mode {
        "auto_edit" => {
            // 等价于已弃用的 --full-auto（codex >= 0.128 已移除该别名）：
            // 工作区内自动写、越界命令才升级审批。
            c.arg("--sandbox");
            c.arg("workspace-write");
            c.arg("-a");
            c.arg("on-request");
        }
        "full_access" => {
            c.arg("--dangerously-bypass-approvals-and-sandbox");
        }
        _ => {}
    }
    c
}

fn toml_table_key(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn codex_project_trust_override(project_path: &str) -> String {
    format!(
        "projects.{}.trust_level=\"trusted\"",
        toml_table_key(project_path)
    )
}

pub(crate) fn normalized_selected_model(selected_model: Option<&str>) -> Option<String> {
    selected_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

const CODEX_REASONING_EFFORTS: &[&str] =
    &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];
const CLAUDE_REASONING_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultracode"];
const CODEX_ULTRA_MODELS: &[&str] = &["gpt-5.6-sol"];

pub(crate) fn normalized_speed(speed: Option<&str>) -> Result<Option<String>, String> {
    let Some(speed) = speed.map(str::trim).filter(|speed| !speed.is_empty()) else {
        return Ok(None);
    };
    match speed.to_ascii_lowercase().as_str() {
        "standard" => Ok(Some("standard".to_string())),
        "fast" => Ok(Some("fast".to_string())),
        _ => Err("Invalid speed".to_string()),
    }
}

pub(crate) fn normalized_reasoning_effort(
    reasoning_effort: Option<&str>,
    is_codex: bool,
    selected_model: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(raw) = reasoning_effort
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    else {
        return Ok(None);
    };
    let mut effort = raw.to_ascii_lowercase();
    if !is_codex && effort == "ultra" {
        effort = "ultracode".to_string();
    }
    let supported = if is_codex {
        CODEX_REASONING_EFFORTS
    } else {
        CLAUDE_REASONING_EFFORTS
    };
    if !supported.contains(&effort.as_str()) {
        return Err("Invalid reasoningEffort".to_string());
    }
    if is_codex && effort == "ultra" {
        let model = selected_model.ok_or_else(|| "Codex Ultra requires gpt-5.6-sol".to_string())?;
        if !CODEX_ULTRA_MODELS
            .iter()
            .any(|candidate| model.eq_ignore_ascii_case(candidate))
        {
            return Err("Codex Ultra requires gpt-5.6-sol".to_string());
        }
    }
    Ok(Some(effort))
}

fn add_claude_launch_args(
    cmd: &mut CommandBuilder,
    agent: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
) {
    if agent == "claude" {
        if let Some(model) = normalized_selected_model(selected_model) {
            cmd.arg("--model");
            cmd.arg(model);
        }
    }
    if let Some(effort) = reasoning_effort {
        cmd.arg("--effort");
        cmd.arg(effort);
    }
}

fn uses_ultracode_terminal_command(is_codex: bool, reasoning_effort: Option<&str>) -> bool {
    !is_codex && reasoning_effort == Some("ultracode")
}

fn should_use_ultracode_terminal_command(
    is_codex: bool,
    reasoning_effort: Option<&str>,
    native_cli_args_supported: bool,
) -> bool {
    uses_ultracode_terminal_command(is_codex, reasoning_effort) && !native_cli_args_supported
}

pub(crate) fn initial_ultracode_command() -> Vec<u8> {
    b"/effort ultracode\r".to_vec()
}

fn add_codex_launch_args(
    cmd: &mut CommandBuilder,
    project_path: &str,
    selected_model: Option<&str>,
    reasoning_effort: Option<&str>,
    speed: Option<&str>,
) {
    cmd.arg("-C");
    cmd.arg(project_path);
    cmd.arg("-c");
    cmd.arg(codex_project_trust_override(project_path));
    if let Some(model) = normalized_selected_model(selected_model) {
        cmd.arg("-m");
        cmd.arg(model);
    }
    if let Some(effort) = reasoning_effort {
        cmd.arg("-c");
        cmd.arg(format!(
            "model_reasoning_effort={}",
            toml::Value::String(effort.to_string())
        ));
    }
    if speed == Some("fast") {
        cmd.arg("-c");
        cmd.arg("features.fast_mode=true");
        cmd.arg("-c");
        cmd.arg("service_tier=\"fast\"");
    }
}

fn strip_startup_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            0x1b => {
                index += 1;
                if index >= bytes.len() {
                    break;
                }
                match bytes[index] {
                    b'[' => {
                        index += 1;
                        while index < bytes.len() {
                            let byte = bytes[index];
                            index += 1;
                            if (0x40..=0x7e).contains(&byte) {
                                break;
                            }
                        }
                    }
                    b']' => {
                        // OSC: consume until BEL or the ST sequence ESC \\.
                        index += 1;
                        while index < bytes.len() {
                            if bytes[index] == 0x07 {
                                index += 1;
                                break;
                            }
                            if bytes[index] == 0x1b && bytes.get(index + 1).copied() == Some(b'\\')
                            {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    _ => index += 1,
                }
            }
            byte if byte.is_ascii_control() && byte != b'\n' && byte != b'\r' => {
                output.push(' ');
                index += 1;
            }
            _ => {
                let remaining = &input[index..];
                if let Some(ch) = remaining.chars().next() {
                    output.push(ch);
                    index += ch.len_utf8();
                } else {
                    break;
                }
            }
        }
    }
    output
}

/// 返回不超过 `limit` 字节的尾部切片,且不切断 UTF-8 字符边界。
fn trailing_window(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

fn has_choice_marker(text: &str) -> bool {
    text.contains('?')
        || text.contains("[y/n]")
        || text.contains("[yes/no]")
        || text.contains("(y/n)")
        || text.contains("(yes/no)")
        || text.contains("press enter")
        || text.contains("select an option")
        || text.contains("choose an option")
        || (text.contains("1.") && text.contains("2."))
}

fn has_confirmation_scope(text: &str) -> bool {
    text.contains("continue")
        || text.contains("proceed")
        || text.contains("confirm")
        || text.contains("select")
        || text.contains("choose")
        || text.contains("allow")
        || text.contains("approve")
        || text.contains("trust")
        || text.contains("review")
        || text.contains("permission")
        || text.contains("authorize")
        || text.contains("enable")
        || text.contains("accept")
        || text.contains("want to")
        || text.contains("are you sure")
}

fn startup_gate_text(input: &str) -> bool {
    let clean = strip_startup_ansi(input)
        .to_ascii_lowercase()
        .replace(['\r', '\n'], " ");
    let compact = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let choice_marker = has_choice_marker(&compact);
    let confirmation_scope = has_confirmation_scope(&compact);
    // The generic gate is the loosest rule, so evaluate it only over the trailing
    // window. Detection runs against an accumulated tail, and a `?` printed by a
    // startup banner must not pair up with an unrelated "enable"/"review" word
    // several hundred bytes later — that misread defers the prompt indefinitely.
    let recent = trailing_window(&compact, GENERIC_GATE_WINDOW);
    let generic_gate = has_choice_marker(recent) && has_confirmation_scope(recent);
    let trust_scope = compact.contains("folder") || compact.contains("workspace");
    let trust_gate = compact.contains("trust this")
        || compact.contains("trust folder")
        || compact.contains("trust workspace")
        || compact.contains("workspace trust")
        || (compact.contains("do you trust") && trust_scope)
        || (compact.contains("trusted") && trust_scope);
    let hook_scope = compact.contains("hook") || compact.contains("hooks");
    // Codex can stop at a review selector before SessionStart is emitted. This
    // text is not necessarily phrased as a question, so treat it as a gate on
    // its own; otherwise the deferred prompt can be written into the selector.
    let hook_review_gate = hook_scope
        && (compact.contains("need review")
            || compact.contains("needs review")
            || compact.contains("review required")
            || compact.contains("review needed"));
    let hook_gate = hook_scope
        && confirmation_scope
        && (choice_marker || compact.contains("permission to") || compact.contains("run hook"));
    // Claude's full-access launch can show a separate "Bypass Permissions"
    // acceptance screen after trust and hook prompts. It may not contain a
    // question mark, so recognize the explicit acceptance wording as a gate.
    let bypass_scope = compact.contains("bypass permission")
        || compact.contains("bypass approval")
        || compact.contains("skip permission")
        || compact.contains("bypass mode")
        || compact.contains("dangerously bypass");
    let bypass_gate = bypass_scope
        && (compact.contains("i accept")
            || compact.contains("accept the risk")
            || (has_choice_marker(&compact)
                && (compact.contains("yes")
                    || compact.contains("accept")
                    || compact.contains("no"))));
    trust_gate || hook_review_gate || hook_gate || bypass_gate || generic_gate
}

fn startup_output_indicates_gate(tail: &mut String, output: &str) -> bool {
    let clean = strip_startup_ansi(output);
    if clean.is_empty() {
        return false;
    }
    tail.push_str(&clean);
    if startup_gate_text(tail) {
        tail.clear();
        return true;
    }
    const TAIL_LIMIT: usize = 512;
    if tail.len() > TAIL_LIMIT {
        let keep_from = tail.len() - TAIL_LIMIT;
        tail.drain(..keep_from);
    }
    false
}

pub(crate) fn register_initial_input_signal(
    task_manager: &TaskManager,
    task_id: &str,
    sender: std::sync::mpsc::Sender<StartupSignal>,
) {
    task_manager
        .initial_input_signals
        .lock()
        .insert(task_id.to_string(), sender);
}

pub(crate) fn notify_initial_input_session_ready(task_manager: &TaskManager, task_id: &str) {
    let sender = task_manager
        .initial_input_signals
        .lock()
        .get(task_id)
        .cloned();
    if let Some(sender) = sender {
        let _ = sender.send(StartupSignal::SessionReady);
    }
}

fn wait_for_initial_input_ready(startup_rx: std::sync::mpsc::Receiver<StartupSignal>) -> bool {
    wait_for_initial_input_ready_with_cap(startup_rx, STARTUP_GATE_MAX_WAIT)
}

fn wait_for_initial_input_ready_with_cap(
    startup_rx: std::sync::mpsc::Receiver<StartupSignal>,
    gate_max_wait: Duration,
) -> bool {
    let started_at = Instant::now();
    let no_output_deadline = started_at + STARTUP_NO_OUTPUT_FALLBACK;
    let first_output_at = None::<Instant>;
    let mut first_output_at = first_output_at;
    let mut gate_pending = false;
    let mut user_confirmed_gate = false;
    let mut detection_tail = String::new();

    loop {
        let now = Instant::now();
        let wait = match first_output_at {
            None => no_output_deadline
                .saturating_duration_since(now)
                .min(STARTUP_FIRST_OUTPUT_TIMEOUT),
            Some(_) if gate_pending => STARTUP_GATE_INPUT_SETTLE,
            Some(first_output_at) => STARTUP_OUTPUT_MAX_WAIT
                .saturating_sub(now.duration_since(first_output_at))
                .min(STARTUP_OUTPUT_QUIET),
        };

        match startup_rx.recv_timeout(wait) {
            Ok(StartupSignal::Output(output)) => {
                let now = Instant::now();
                let first_output = *first_output_at.get_or_insert(now);
                if startup_output_indicates_gate(&mut detection_tail, &output) {
                    gate_pending = true;
                    user_confirmed_gate = false;
                }
                if !gate_pending && now.duration_since(first_output) >= STARTUP_OUTPUT_MAX_WAIT {
                    return true;
                }
            }
            Ok(StartupSignal::UserInput) => {
                if gate_pending {
                    user_confirmed_gate = true;
                    detection_tail.clear();
                }
            }
            Ok(StartupSignal::SessionReady) => {
                // SessionStart 是 hook 链路的权威就绪信号,即使上一个输出块看起来像
                // 授权提示,收到它也说明门槛已经被用户/Agent处理完毕。
                return true;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if first_output_at.is_none() {
                    if Instant::now() >= no_output_deadline {
                        // 自定义 Agent 可能完全不打印 banner,保留原有超时兜底。
                        return true;
                    }
                    continue;
                }
                if gate_pending {
                    if user_confirmed_gate {
                        return true;
                    }
                    if Instant::now().duration_since(started_at) >= gate_max_wait {
                        // 门控迟迟没被应答:要么判定误报,要么用户已经放弃。
                        // 两种情况都不该继续占用 blocking 线程。
                        return false;
                    }
                    continue;
                }
                if Instant::now().duration_since(first_output_at.unwrap())
                    >= STARTUP_OUTPUT_MAX_WAIT
                {
                    return true;
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return !gate_pending || user_confirmed_gate;
            }
        }
    }
}

pub(crate) fn spawn_initial_input_injection(
    writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
    prelude: Option<Vec<u8>>,
    prompt: Option<(Vec<u8>, Vec<u8>)>,
    startup_rx: std::sync::mpsc::Receiver<StartupSignal>,
    on_finish: Option<Box<dyn FnOnce() + Send>>,
) {
    tokio::task::spawn_blocking(move || {
        let ready = wait_for_initial_input_ready(startup_rx);
        if !ready {
            if let Some(on_finish) = on_finish {
                on_finish();
            }
            return;
        }
        if let Some(prelude) = prelude {
            {
                let mut writer = writer.lock();
                let _ = writer.write_all(&prelude);
                let _ = writer.flush();
            }
            // Claude renders the effort picker asynchronously. Give it one input turn
            // before the initial prompt so `/effort ultracode` is handled first.
            std::thread::sleep(Duration::from_millis(160));
        }
        if let Some((paste, submit)) = prompt {
            {
                let mut writer = writer.lock();
                let _ = writer.write_all(&paste);
                let _ = writer.flush();
            }
            // Agent TUIs may intentionally ignore an Enter delivered in the same
            // PTY write as a bracketed paste. Submit in a later input turn so the
            // initial prompt is executed instead of remaining in the composer.
            std::thread::sleep(Duration::from_millis(80));
            let mut writer = writer.lock();
            let _ = writer.write_all(&submit);
            let _ = writer.flush();
        }
        if let Some(on_finish) = on_finish {
            on_finish();
        }
    });
}

fn prompt_with_project_prefix(prompt: &str, prompt_prefix: &str) -> String {
    if prompt.is_empty() || prompt_prefix.is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n{}", prompt_prefix, prompt)
    }
}

fn initial_prompt_args(prompt: &str, is_codex: bool) -> Vec<String> {
    if prompt.is_empty() {
        return Vec::new();
    }
    if is_codex {
        vec!["--".to_string(), prompt.to_string()]
    } else {
        vec![prompt.to_string()]
    }
}

pub(crate) fn initial_prompt_input_chunks(prompt: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    if prompt.is_empty() {
        return None;
    }
    let mut paste = Vec::with_capacity(prompt.len() + 16);
    paste.extend_from_slice(b"\x1b[200~");
    paste.extend_from_slice(prompt.as_bytes());
    paste.extend_from_slice(b"\x1b[201~");
    Some((paste, b"\r".to_vec()))
}

fn uses_native_initial_prompt(agent: &str, is_codex: bool) -> bool {
    matches!((agent, is_codex), ("claude", false) | ("codex", true))
}

/// Aeroric-generated Claude/Codex wrappers forward their positional arguments
/// to the real CLI. Prefer that native delivery path for them as well; PTY
/// injection is only a fallback for arbitrary custom wrappers that may not
/// forward args.
fn launch_supports_native_initial_prompt(
    agent: &str,
    is_codex: bool,
    launch: &crate::app_settings::AgentLaunchSpec,
) -> bool {
    if uses_native_initial_prompt(agent, is_codex) {
        return true;
    }

    let Ok(content) = fs::read_to_string(&launch.program) else {
        return false;
    };
    // Aeroric-generated wrappers forward positional arguments to the real CLI.
    // Both the standard wrapper and the chat-completions proxy wrapper support
    // this, so recognize both markers. Without the chat proxy marker here,
    // custom codex-like agents that use the proxy wrapper fall through to the
    // less reliable PTY injection path even though their script forwards args.
    let marker = if is_codex {
        "# AERORIC_CODEX_WRAPPER_VERSION="
    } else {
        "# AERORIC_CLAUDE_WRAPPER_VERSION="
    };
    let chat_proxy_marker = "# AERORIC_CODEX_CHAT_PROXY_VERSION=";
    let has_wrapper_marker =
        content.contains(marker) || (is_codex && content.contains(chat_proxy_marker));
    has_wrapper_marker && (content.contains("\"$@\"") || content.contains("@args"))
}

fn should_use_native_initial_prompt(
    agent: &str,
    is_codex: bool,
    force_prompt_injection: bool,
) -> bool {
    !force_prompt_injection && uses_native_initial_prompt(agent, is_codex)
}

pub(crate) fn should_force_prompt_injection(
    is_codex: bool,
    force_prompt_injection: Option<bool>,
) -> bool {
    // Codex may render workspace-trust and hook-review selectors before the
    // composer exists. Keep its initial prompt out of CLI positional args and
    // deliver it through the guarded PTY path after those gates settle.
    is_codex || force_prompt_injection.unwrap_or(false)
}

fn stable_agent_spawn_cwd() -> PathBuf {
    crate::platform::home_dir()
        .filter(|path| path.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

fn agent_process_cwd(project_path: &str, is_codex: bool) -> PathBuf {
    if is_codex {
        stable_agent_spawn_cwd()
    } else {
        PathBuf::from(project_path)
    }
}

// ── Tauri 命令 ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_task(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    task_id: String,
    project_path: String,
    prompt: String,
    agent: String,
    permission_mode: String,
    images: Option<Vec<String>>,
    texts: Option<Vec<String>>,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    speed: Option<String>,
    force_prompt_injection: Option<bool>,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    let _ = crate::storage::truncate_task_terminal_history(&task_id);
    // 历史清零 → 远程终端流水位换代,已订阅的手机端自动重新快照
    crate::remote::terminal_hub::hub().reset_for_truncate(&task_id);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(50),
            cols: cols.unwrap_or(220),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    // 将图片保存至 .aeroric/attachments/ 并获取文件路径
    let image_paths = save_task_images(&project_path, &task_id, &images.unwrap_or_default())?;

    // 将文本附件保存至 .aeroric/attachments/ 并获取文件路径
    // 用 spawn_blocking 把同步文件 I/O 移出 Tokio runtime（AGENTS.md 要求）
    let text_paths = {
        let project_path = project_path.clone();
        let task_id = task_id.clone();
        let texts = texts.unwrap_or_default();
        tokio::task::spawn_blocking(move || save_task_texts(&project_path, &task_id, &texts))
            .await
            .map_err(|e| e.to_string())??
    };

    // 若配置了项目级 prompt_prefix，则拼接到提示词前。
    // 空 prompt 表示“启动交互式 Agent 终端”，不能注入前缀，否则会被误判为
    // 非交互任务并触发 /status watcher。
    let config = {
        let config_project_path = project_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::read_project_config(config_project_path).unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    };
    let base_prompt = prompt_with_project_prefix(&prompt, &config.agent.prompt_prefix);

    // 将图片路径追加到提示词，供 Claude Code 通过文件工具读取
    let prompt_with_images = if image_paths.is_empty() {
        base_prompt
    } else {
        format!(
            "{}\n\n[Attached images]\n{}",
            base_prompt,
            image_paths.join("\n")
        )
    };

    // 将文本附件路径追加到提示词
    let final_prompt = if text_paths.is_empty() {
        prompt_with_images
    } else {
        format!(
            "{}\n\n[Attached text files — read these for full context]\n{}",
            prompt_with_images,
            text_paths.join("\n")
        )
    };

    let launch = crate::app_settings::get_agent_launch_spec(&agent);
    let is_codex = launch.codex_like;
    let selected_model = normalized_selected_model(selected_model.as_deref());
    let reasoning_effort = normalized_reasoning_effort(
        reasoning_effort.as_deref(),
        is_codex,
        selected_model.as_deref(),
    )?;
    let speed = normalized_speed(speed.as_deref())?;

    // hook 链路是否可信:可信则注入 AERORIC_* 守卫变量让 hook 脚本上报事件,会话发现
    // 与状态全部由 event_watcher 驱动、跳过 /status 轮询 watcher;不可信(无 node /
    // 未安装 / 版本过低)则不注入 env、并回退轮询路径——否则旧版但仍支持 hook 的 agent
    // 会同时触发已安装 hook 与轮询 watcher,导致 session 注册/状态重复上报。
    // 先于 cmd 构建计算,因为 Codex 的 --dangerously-bypass-hook-trust 必须加在
    // `--`/positional prompt 之前。
    // 提前计算 use_hooks:custom agent 的 prompt 投递路径选择需要参考它。
    let use_hooks = {
        let agent = agent.clone();
        tokio::task::spawn_blocking(move || crate::hooks::usable_for(&agent))
            .await
            .unwrap_or(false)
    };

    let force_prompt_injection = should_force_prompt_injection(is_codex, force_prompt_injection);
    let native_cli_args_supported = uses_native_initial_prompt(&agent, is_codex)
        || launch_supports_native_initial_prompt(&agent, is_codex, &launch);
    // Built-in agents (claude/codex) always use native CLI args. Custom
    // wrappers that forward positional args also prefer this path. The
    // guarded PTY injection is only used when the wrapper cannot accept
    // positional args, or when hooks are available (meaning trust/hook
    // startup gates may need to be waited for before the prompt is safe to
    // inject). When hooks are unavailable for a custom codex-like agent,
    // there are no startup gates to wait for, so native CLI args are both
    // safe and more reliable than timing-dependent PTY injection.
    let use_native_initial_prompt =
        (should_use_native_initial_prompt(&agent, is_codex, force_prompt_injection)
            || (!force_prompt_injection
                && launch_supports_native_initial_prompt(&agent, is_codex, &launch))
            || (force_prompt_injection && !use_hooks && native_cli_args_supported))
            && native_cli_args_supported;
    let uses_ultracode = should_use_ultracode_terminal_command(
        is_codex,
        reasoning_effort.as_deref(),
        native_cli_args_supported,
    );

    // 版本统一走全局探测(带缓存),判断是否支持 --session-id。
    // 缓存未命中时 *_version_gte 会启子进程探测,故放进 spawn_blocking 避免阻塞 async runtime。
    let version_agent = agent.clone();
    let use_explicit_session = !is_codex
        && tokio::task::spawn_blocking(move || {
            crate::app_settings::agent_version_gte(&version_agent, "2.1.87")
        })
        .await
        .unwrap_or(false);

    // 预生成 session id(仅 Claude >= 2.1.87 使用)
    let pre_session_id = if use_explicit_session {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    };
    let claude_settings_path = if is_codex {
        None
    } else {
        crate::hooks::claude_settings_path_for_launch(speed.as_deref() == Some("fast"), use_hooks)?
    };

    // MCP 配置路径/profile 名:Claude 用 --mcp-config,Codex 用 -p
    let claude_mcp_config_path = if is_codex {
        None
    } else {
        crate::mcp::claude_mcp_config_path_for_launch()?
    };
    let codex_mcp_profile = if is_codex {
        // 内建 Codex 用 ~/.codex,自定义 codex-like 用 agent-homes/{id}
        let codex_home = if agent == "codex" {
            crate::hooks::codex_home()?
        } else {
            crate::app_settings::custom_agent_home(&agent)?
        };
        crate::mcp::codex_mcp_profile_for_launch(&codex_home)?
    } else {
        None
    };

    let mut cmd = if is_codex {
        let mut c = build_codex_cmd(&launch, &permission_mode);
        add_codex_launch_args(
            &mut c,
            &project_path,
            selected_model.as_deref(),
            reasoning_effort.as_deref(),
            speed.as_deref(),
        );
        // Codex 对非 managed 的 command hook 默认要求 trust,Aeroric 注入的是新 hash 会被
        // skip;由 Aeroric 注入、来源可信,这里免 trust 直接运行。
        if use_hooks {
            c.arg("--dangerously-bypass-hook-trust");
        }
        // MCP profile 必须在 `--`/positional prompt 前注入
        if let Some(ref profile) = codex_mcp_profile {
            c.arg("-p");
            c.arg(profile);
        }
        if use_native_initial_prompt {
            for arg in initial_prompt_args(&final_prompt, true) {
                c.arg(arg);
            }
        }
        c
    } else {
        let mut c = build_claude_cmd(&launch, &permission_mode);
        add_claude_launch_args(
            &mut c,
            &agent,
            selected_model.as_deref(),
            reasoning_effort.as_deref(),
        );
        // Claude >= 2.1.87：通过 --session-id 指定会话，跳过 /status 发现
        if let Some(ref sid) = pre_session_id {
            c.arg("--session-id");
            c.arg(sid);
        }
        // Claude:hook 可信时通过 `--settings <Aeroric 自有文件>` 传入 hooks,不修改用户的
        // ~/.claude/settings.json(Claude 对 hooks 跨源 merge,用户 hook 不受影响)。
        if let Some(path) = claude_settings_path.as_ref() {
            c.arg("--settings");
            c.arg(path.to_string_lossy().as_ref());
        }
        // MCP 配置通过 --mcp-config 注入
        if let Some(path) = claude_mcp_config_path.as_ref() {
            c.arg("--mcp-config");
            c.arg(path.to_string_lossy().as_ref());
        }
        if use_native_initial_prompt {
            for arg in initial_prompt_args(&final_prompt, false) {
                c.arg(arg);
            }
        }
        c
    };
    cmd.cwd(agent_process_cwd(&project_path, is_codex));
    setup_env(&mut cmd);
    if let Some(model) = selected_model.as_deref() {
        cmd.env("AERORIC_AGENT_MODEL", model);
    }
    if use_hooks {
        setup_aeroric_env(&mut cmd, &task_id, &agent, is_codex);
    }
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    register_pty_handles(&task_manager, &task_id, pair.master, writer, child)?;

    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "running" }),
    );

    // Claude 默认使用原生命令行参数执行首条消息。Codex 始终走受门控保护的终端注入，
    // 等 trust / hook review 等启动选择器结束后再粘贴并提交；自定义 Agent 包装脚本若
    // 不支持 positional prompt 也沿用同一兜底，并为没有启动输出的脚本保留超时。
    // 因此这里不能启动 /status watcher，避免抢占用户输入或污染浅色终端背景。
    let starts_with_prompt = !final_prompt.is_empty();
    let session_tx = if starts_with_prompt {
        if !use_hooks && !is_codex && pre_session_id.is_some() {
            let (_session_tx, session_rx) = std::sync::mpsc::channel::<String>();
            spawn_status_session_watcher(
                app.clone(),
                task_id.clone(),
                project_path.clone(),
                is_codex,
                session_rx,
                pre_session_id.clone(),
                true,
            );
        }
        None
    } else if should_start_status_session_watcher(use_hooks, is_codex, true) {
        let (session_tx, session_rx) = std::sync::mpsc::channel::<String>();
        spawn_status_session_watcher(
            app.clone(),
            task_id.clone(),
            project_path.clone(),
            is_codex,
            session_rx,
            pre_session_id,
            true,
        );
        Some(session_tx)
    } else {
        None
    };
    let initial_prelude = uses_ultracode.then(initial_ultracode_command);
    let initial_prompt = (!use_native_initial_prompt)
        .then(|| initial_prompt_input_chunks(&final_prompt))
        .flatten();
    let needs_initial_input = initial_prelude.is_some() || initial_prompt.is_some();
    let (startup_tx, startup_rx) = std::sync::mpsc::channel();
    if needs_initial_input {
        register_initial_input_signal(&task_manager, &task_id, startup_tx.clone());
    }
    spawn_pty_reader(
        app.clone(),
        task_id.clone(),
        OutputSink::Channel(on_output),
        PtyEmitMode::Batched {
            flush_interval: PTY_EMIT_FLUSH_INTERVAL,
            max_batch_bytes: PTY_EMIT_MAX_BATCH_BYTES,
        },
        reader,
        true,
        session_tx,
        needs_initial_input.then_some(startup_tx),
        None,
        None,
    );
    if needs_initial_input {
        let writer = task_manager.pty_writers.lock().get(&task_id).cloned();
        if let Some(writer) = writer {
            let signals = Arc::clone(&task_manager.initial_input_signals);
            let cleanup_id = task_id.clone();
            spawn_initial_input_injection(
                writer,
                initial_prelude,
                initial_prompt,
                startup_rx,
                Some(Box::new(move || {
                    signals.lock().remove(&cleanup_id);
                })),
            );
        } else {
            task_manager.initial_input_signals.lock().remove(&task_id);
        }
    }
    spawn_exit_monitor(app, task_id, project_path, is_codex);

    Ok(())
}

/// cancel_task 的内核,供 tauri command 与远程 RPC(remote 模块)共用。
pub(crate) fn cancel_task_core<R: tauri::Runtime>(
    app: &AppHandle<R>,
    task_manager: &TaskManager,
    task_id: &str,
    project_path: &str,
) -> Result<(), String> {
    validate_task_id(task_id)?;
    task_manager
        .cancelled_tasks
        .lock()
        .insert(task_id.to_string());
    task_manager.manually_completed_tasks.lock().remove(task_id);

    let child_arc = task_manager.child_handles.lock().get(task_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    } else {
        // Orphaned/interrupted tasks have no live child in this app process.
        // Avoid leaving a stale cancellation marker that would affect a later manual resume.
        task_manager.cancelled_tasks.lock().remove(task_id);
    }

    // 释放已声明的会话路径，确保相同提示词的任务可以重新运行
    release_claimed_session_paths(task_manager, task_id);

    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "cancelled" }),
    );

    // 清理任务附件
    let _ = fs::remove_dir_all(task_attachments_dir(project_path, task_id));
    crate::event_watcher::cleanup_task_events(app, task_id);

    Ok(())
}

#[tauri::command]
pub async fn cancel_task(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    task_id: String,
    project_path: String,
) -> Result<(), String> {
    cancel_task_core(&app, &task_manager, &task_id, &project_path)
}

/// complete_task 的内核,供 tauri command 与远程 RPC 共用。
pub(crate) fn complete_task_core<R: tauri::Runtime>(
    app: &AppHandle<R>,
    task_manager: &TaskManager,
    task_id: &str,
    project_path: &str,
) -> Result<(), String> {
    validate_task_id(task_id)?;
    task_manager
        .manually_completed_tasks
        .lock()
        .insert(task_id.to_string());
    task_manager.cancelled_tasks.lock().remove(task_id);

    let child_arc = task_manager.child_handles.lock().get(task_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    } else {
        // No live child means no exit monitor will consume this marker.
        task_manager.manually_completed_tasks.lock().remove(task_id);
    }

    // 释放已声明的会话路径，确保相同提示词的任务可以重新运行
    release_claimed_session_paths(task_manager, task_id);

    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "done" }),
    );

    // 清理任务附件
    let _ = fs::remove_dir_all(task_attachments_dir(project_path, task_id));
    crate::event_watcher::cleanup_task_events(app, task_id);

    Ok(())
}

#[tauri::command]
pub async fn complete_task(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    task_id: String,
    project_path: String,
) -> Result<(), String> {
    complete_task_core(&app, &task_manager, &task_id, &project_path)
}

#[tauri::command]
pub async fn get_active_task_ids(
    task_manager: State<'_, TaskManager>,
) -> Result<Vec<String>, String> {
    Ok(task_manager
        .child_handles
        .lock()
        .keys()
        .filter(|id| validate_task_id(id).is_ok())
        .cloned()
        .collect())
}

#[tauri::command]
pub async fn reset_task_process(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    task_id: String,
) -> Result<(), String> {
    validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    let child_arc = {
        let mut masters = task_manager.pty_masters.lock();
        let mut pending_sizes = task_manager.pending_pty_sizes.lock();
        let mut writers = task_manager.pty_writers.lock();
        let mut children = task_manager.child_handles.lock();
        masters.remove(&task_id);
        pending_sizes.remove(&task_id);
        writers.remove(&task_id);
        children.remove(&task_id)
    };
    task_manager.initial_input_signals.lock().remove(&task_id);

    // A reset replaces the process under the same task ID. Clear the old
    // session registrations before killing the child so its watcher cannot
    // keep reporting stale status events into the replacement run.
    let codex_path = task_manager
        .codex_sessions
        .lock()
        .remove(&task_id)
        .map(|info| info.session_path);
    let claude_path = task_manager
        .claude_sessions
        .lock()
        .remove(&task_id)
        .map(|info| info.session_path);
    let mut claimed = task_manager.claimed_session_paths.lock();
    if let Some(path) = codex_path {
        claimed.remove(&path);
    }
    if let Some(path) = claude_path {
        claimed.remove(&path);
    }
    drop(claimed);
    crate::event_watcher::cleanup_task_events(&app, &task_id);

    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}

#[tauri::command]
pub async fn resume_task(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    task_id: String,
    project_path: String,
    agent: String,
    session_id: String,
    _prompt: String,
    permission_mode: String,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    speed: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(50),
            cols: cols.unwrap_or(220),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let launch = crate::app_settings::get_agent_launch_spec(&agent);
    let is_codex = launch.codex_like;
    let selected_model = normalized_selected_model(selected_model.as_deref());
    let reasoning_effort = normalized_reasoning_effort(
        reasoning_effort.as_deref(),
        is_codex,
        selected_model.as_deref(),
    )?;
    let speed = normalized_speed(speed.as_deref())?;
    let native_cli_args_supported = uses_native_initial_prompt(&agent, is_codex)
        || launch_supports_native_initial_prompt(&agent, is_codex, &launch);
    let uses_ultracode = should_use_ultracode_terminal_command(
        is_codex,
        reasoning_effort.as_deref(),
        native_cli_args_supported,
    );
    // hook 可信时会话发现/状态由 event_watcher 驱动,跳过轮询 watcher;否则回退,
    // 且不注入 AERORIC_* 守卫变量,避免旧版但已安装 hook 的 agent 与轮询路径并行重复
    // 上报。版本统一走全局带缓存的探测。
    // 先于 cmd 构建计算,因 Codex 的 bypass flag 需加在 `resume` 子命令之前。
    let use_hooks = {
        let agent = agent.clone();
        tokio::task::spawn_blocking(move || crate::hooks::usable_for(&agent))
            .await
            .unwrap_or(false)
    };
    let claude_settings_path = if is_codex {
        None
    } else {
        crate::hooks::claude_settings_path_for_launch(speed.as_deref() == Some("fast"), use_hooks)?
    };

    // MCP 配置路径/profile 名:Claude 用 --mcp-config,Codex 用 -p
    let claude_mcp_config_path = if is_codex {
        None
    } else {
        crate::mcp::claude_mcp_config_path_for_launch()?
    };
    let codex_mcp_profile = if is_codex {
        let codex_home = if agent == "codex" {
            crate::hooks::codex_home()?
        } else {
            crate::app_settings::custom_agent_home(&agent)?
        };
        crate::mcp::codex_mcp_profile_for_launch(&codex_home)?
    } else {
        None
    };

    let mut cmd = if is_codex {
        let mut c = build_codex_cmd(&launch, &permission_mode);
        add_codex_launch_args(
            &mut c,
            &project_path,
            selected_model.as_deref(),
            reasoning_effort.as_deref(),
            speed.as_deref(),
        );
        // Aeroric 注入的 hook 默认未信任会被 Codex skip;来源可信,免 trust 直接运行。
        if use_hooks {
            c.arg("--dangerously-bypass-hook-trust");
        }
        // MCP profile 必须在 resume 子命令前注入
        if let Some(ref profile) = codex_mcp_profile {
            c.arg("-p");
            c.arg(profile);
        }
        c.arg("resume");
        c.arg(&session_id);
        c
    } else {
        // resume 时 session_id 已知，使用 --resume 标志
        let mut c = build_claude_cmd(&launch, &permission_mode);
        add_claude_launch_args(
            &mut c,
            &agent,
            selected_model.as_deref(),
            reasoning_effort.as_deref(),
        );
        c.arg("--resume");
        c.arg(&session_id);
        // Claude:命令行 `--settings` 传入 Aeroric 自有 hooks 文件,不改用户配置。
        if let Some(path) = claude_settings_path.as_ref() {
            c.arg("--settings");
            c.arg(path.to_string_lossy().as_ref());
        }
        // MCP 配置通过 --mcp-config 注入
        if let Some(path) = claude_mcp_config_path.as_ref() {
            c.arg("--mcp-config");
            c.arg(path.to_string_lossy().as_ref());
        }
        c
    };
    cmd.cwd(agent_process_cwd(&project_path, is_codex));
    setup_env(&mut cmd);
    if let Some(model) = selected_model.as_deref() {
        cmd.env("AERORIC_AGENT_MODEL", model);
    }
    if use_hooks {
        setup_aeroric_env(&mut cmd, &task_id, &agent, is_codex);
    }
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    register_pty_handles(&task_manager, &task_id, pair.master, writer, child)?;

    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "running" }),
    );

    // resume 时 session_id 已知，直接查找文件并开始监视(hook 可信时跳过)
    if !use_hooks {
        spawn_resume_session_watcher(
            app.clone(),
            task_id.clone(),
            project_path.clone(),
            session_id,
            is_codex,
        );
    }
    let (startup_tx, startup_rx) = std::sync::mpsc::channel();
    if uses_ultracode {
        register_initial_input_signal(&task_manager, &task_id, startup_tx.clone());
    }
    spawn_pty_reader(
        app.clone(),
        task_id.clone(),
        OutputSink::Channel(on_output),
        PtyEmitMode::Batched {
            flush_interval: PTY_EMIT_FLUSH_INTERVAL,
            max_batch_bytes: PTY_EMIT_MAX_BATCH_BYTES,
        },
        reader,
        true,
        None,
        uses_ultracode.then_some(startup_tx),
        None,
        None,
    );
    if uses_ultracode {
        let writer = task_manager.pty_writers.lock().get(&task_id).cloned();
        if let Some(writer) = writer {
            let signals = Arc::clone(&task_manager.initial_input_signals);
            let cleanup_id = task_id.clone();
            spawn_initial_input_injection(
                writer,
                Some(initial_ultracode_command()),
                None,
                startup_rx,
                Some(Box::new(move || {
                    signals.lock().remove(&cleanup_id);
                })),
            );
        } else {
            task_manager.initial_input_signals.lock().remove(&task_id);
        }
    }
    spawn_exit_monitor(app, task_id, project_path, is_codex);

    Ok(())
}

/// send_input 的内核,供 tauri command 与远程终端流(remote 模块)共用。
pub(crate) fn write_task_input(
    task_manager: &TaskManager,
    task_id: &str,
    data: &str,
) -> Result<(), String> {
    let writer = task_manager.pty_writers.lock().get(task_id).cloned();
    if let Some(writer) = writer {
        let mut writer = writer.lock();
        writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);
        let startup_signal = task_manager
            .initial_input_signals
            .lock()
            .get(task_id)
            .cloned();
        if let Some(startup_signal) = startup_signal {
            let _ = startup_signal.send(StartupSignal::UserInput);
        }
    }
    Ok(())
}

/// resize_pty 的内核,供 tauri command 与远程终端流共用。
pub(crate) fn resize_task_pty(
    task_manager: &TaskManager,
    task_id: &str,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    // 兜底：拒绝畸形尺寸。FitAddon 在容器 display:none 时可能算出 cols=2，前端
    // 三层防御漏掉的话，会把 Claude Code / Codex 这类全屏 TUI 通过 SIGWINCH
    // 排版打散到一字一行且不可恢复。前端任何路径有 bug，这里也得挡住。
    if cols < 2 || rows < 2 || cols > 10_000 || rows > 10_000 {
        return Ok(());
    }
    let masters = task_manager.pty_masters.lock();
    let master = masters.get(task_id).cloned();
    drop(masters);
    if let Some(master) = master {
        let master = master.lock();
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    } else {
        task_manager
            .pending_pty_sizes
            .lock()
            .insert(task_id.to_string(), (cols, rows));
    }
    Ok(())
}

/// 当前 PTY 尺寸:优先活跃 master 实测,回退到 pending(尚未 spawn 时的预设)。
pub(crate) fn current_task_pty_size(
    task_manager: &TaskManager,
    task_id: &str,
) -> Option<(u16, u16)> {
    let master = task_manager.pty_masters.lock().get(task_id).cloned();
    if let Some(master) = master {
        if let Ok(size) = master.lock().get_size() {
            return Some((size.cols, size.rows));
        }
    }
    task_manager.pending_pty_sizes.lock().get(task_id).copied()
}

#[tauri::command]
pub async fn send_input(
    task_manager: State<'_, TaskManager>,
    task_id: String,
    data: String,
) -> Result<(), String> {
    write_task_input(&task_manager, &task_id, &data)
}

#[tauri::command]
pub async fn resize_pty(
    task_manager: State<'_, TaskManager>,
    task_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    resize_task_pty(&task_manager, &task_id, cols, rows)
}

#[tauri::command]
pub async fn open_shell(
    app: AppHandle,
    task_manager: State<'_, TaskManager>,
    shell_id: String,
    project_path: String,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<(), String> {
    validate_shell_id(&shell_id)?;
    // 先终止已存在的同 ID Shell
    {
        let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
        if let Some(arc) = child_arc {
            let mut child = arc.lock();
            let _ = child.kill();
            let _ = child.wait();
        }
        task_manager.remove_pty_handles(&shell_id);
    }

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(120),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let shell = crate::platform::default_shell_command();
    let mut cmd = CommandBuilder::new(&shell.program);
    for arg in &shell.args {
        cmd.arg(arg);
    }
    cmd.cwd(&project_path);
    setup_env(&mut cmd);

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    register_pty_handles(&task_manager, &shell_id, pair.master, writer, child)?;

    // Shell 退出后清理 TaskManager 中的残留句柄
    let app_cleanup = app.clone();
    let sid_cleanup = shell_id.clone();
    let on_finish = Box::new(move || {
        let tm = app_cleanup.state::<TaskManager>();
        tm.remove_pty_handles(&sid_cleanup);
    });

    spawn_pty_reader(
        app,
        shell_id,
        OutputSink::Event {
            event_name: "shell-output",
            id_key: "shell_id",
        },
        PtyEmitMode::Immediate,
        reader,
        false,
        None,
        None,
        None,
        Some(on_finish),
    );

    Ok(())
}

#[tauri::command]
pub async fn kill_shell(
    task_manager: State<'_, TaskManager>,
    shell_id: String,
) -> Result<(), String> {
    validate_shell_id(&shell_id)?;
    let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_project_trust_override_quotes_project_path() {
        assert_eq!(
            codex_project_trust_override("/tmp/has space/quote\"path"),
            "projects.\"/tmp/has space/quote\\\"path\".trust_level=\"trusted\""
        );
    }

    #[test]
    fn selected_model_is_trimmed_and_optional() {
        assert_eq!(
            normalized_selected_model(Some("  gpt-5.6-terra  ")),
            Some("gpt-5.6-terra".to_string())
        );
        assert_eq!(normalized_selected_model(Some("  ")), None);
        assert_eq!(normalized_selected_model(None), None);
    }

    #[test]
    fn task_and_shell_ids_use_disjoint_namespaces() {
        assert!(validate_task_id("1700000000000").is_ok());
        assert!(validate_task_id("shell:project:1").is_err());
        assert!(validate_shell_id("shell:project:1:1700000000000").is_ok());
        assert!(validate_shell_id("1700000000000").is_err());
        assert!(validate_ssh_shell_id("ssh:prod:1700000000000").is_ok());
        assert!(validate_ssh_shell_id("shell:prod:1700000000000").is_err());
    }

    #[test]
    fn permission_flags_stay_with_their_cli_family() {
        let claude_launch = crate::app_settings::AgentLaunchSpec {
            program: "claude".to_string(),
            ..Default::default()
        };
        let codex_launch = crate::app_settings::AgentLaunchSpec {
            program: "codex".to_string(),
            ..Default::default()
        };
        let claude_argv: Vec<_> = build_claude_cmd(&claude_launch, "ask")
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let codex_argv: Vec<_> = build_codex_cmd(&codex_launch, "ask")
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(claude_argv
            .windows(2)
            .any(|args| args == ["--permission-mode", "default"]));
        assert!(!codex_argv.iter().any(|arg| arg == "--permission-mode"));
    }

    #[test]
    fn interpreter_args_precede_agent_permission_flags() {
        let launch = crate::app_settings::AgentLaunchSpec {
            program: "bash".to_string(),
            args: vec![r"C:\Users\test\.aeroric\agents\mimo.sh".to_string()],
            ..Default::default()
        };
        let argv: Vec<_> = build_claude_cmd(&launch, "full_access")
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            argv,
            vec![
                "bash",
                r"C:\Users\test\.aeroric\agents\mimo.sh",
                "--dangerously-skip-permissions"
            ]
        );
    }

    #[test]
    fn initial_prompt_delivery_supports_native_and_custom_agents() {
        assert!(uses_native_initial_prompt("claude", false));
        assert!(uses_native_initial_prompt("codex", true));
        assert!(!uses_native_initial_prompt("local_codex", true));
        assert!(!uses_native_initial_prompt("local_tool", false));
        assert!(should_use_native_initial_prompt("claude", false, false));
        assert!(!should_use_native_initial_prompt("claude", false, true));
        assert!(should_use_native_initial_prompt("codex", true, false));
        assert!(!should_use_native_initial_prompt("codex", true, true));
        assert!(!should_force_prompt_injection(false, None));
        assert!(should_force_prompt_injection(false, Some(true)));
        assert!(should_force_prompt_injection(true, None));
        assert!(should_force_prompt_injection(true, Some(false)));
        assert!(initial_prompt_args("", true).is_empty());
        assert_eq!(
            initial_prompt_args("hello\nworld", true),
            vec!["--", "hello\nworld"]
        );
        assert_eq!(
            initial_prompt_args("hello\nworld", false),
            vec!["hello\nworld"]
        );
        assert_eq!(initial_prompt_input_chunks(""), None);
        assert_eq!(
            initial_prompt_input_chunks("hello\nworld").unwrap(),
            (b"\x1b[200~hello\nworld\x1b[201~".to_vec(), b"\r".to_vec())
        );
    }

    #[test]
    fn generated_codex_wrapper_supports_native_initial_prompt() {
        let root = std::env::temp_dir().join(format!("aeroric-pty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("hkai.sh");
        std::fs::write(
            &path,
            "#!/bin/bash\n# AERORIC_CODEX_WRAPPER_VERSION=4\nexec codex \"$@\"\n",
        )
        .unwrap();
        let launch = crate::app_settings::AgentLaunchSpec {
            program: path.to_string_lossy().into_owned(),
            codex_like: true,
            ..Default::default()
        };

        assert!(launch_supports_native_initial_prompt("hkai", true, &launch));
        assert!(!launch_supports_native_initial_prompt(
            "hkai", false, &launch
        ));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(root);
    }

    #[test]
    fn startup_gate_detection_strips_ansi_and_ignores_session_hook_logs() {
        assert!(startup_gate_text(
            "\x1b[33mDo you trust this folder? [y/N]\x1b[0m"
        ));
        assert!(startup_gate_text(
            "Hooks are enabled. Allow hooks for this workspace?"
        ));
        assert!(startup_gate_text(
            "Hook needs review before the session can start"
        ));
        assert!(startup_gate_text(
            "Select an option to continue: 1. Review 2. Allow"
        ));
        assert!(startup_gate_text(
            "Bypass Permissions mode\n1. Yes, I accept\n2. No, exit"
        ));
        assert!(!startup_gate_text("hook: SessionStart Completed"));
        assert!(!startup_gate_text("Starting workspace session"));
    }

    #[test]
    fn startup_gate_ignores_a_stale_banner_question_mark() {
        // A banner prints "? for shortcuts" and then, several hundred bytes
        // later, an unrelated tip mentioning "enable". Neither is a gate, and
        // pairing them across that distance would defer the initial prompt
        // until the cap expires.
        let banner = format!(
            "welcome to the agent. press ? for shortcuts. {}tip: enable telemetry in settings.",
            "loading modules. ".repeat(GENERIC_GATE_WINDOW / 8)
        );
        assert!(!startup_gate_text(&banner));

        // The same words inside one prompt are still a gate.
        assert!(startup_gate_text(
            "Enable telemetry for this project? [y/N]"
        ));
    }

    #[test]
    fn startup_input_gives_up_on_a_gate_that_is_never_answered() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(StartupSignal::Output(
                "Hook needs review before continuing".to_string(),
            ))
            .unwrap();
        // Hold the sender for the whole wait so the loop cannot exit through
        // Disconnected. Only the absolute cap can release it.
        let waiter = std::thread::spawn(move || {
            wait_for_initial_input_ready_with_cap(receiver, Duration::from_millis(50))
        });
        assert!(!waiter.join().unwrap());
        drop(sender);
    }

    #[test]
    fn startup_input_waits_for_confirmation_after_a_gate() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(StartupSignal::Output(
                "Workspace trust: Do you trust this folder?".to_string(),
            ))
            .unwrap();
        let waiter = std::thread::spawn(move || wait_for_initial_input_ready(receiver));
        std::thread::sleep(Duration::from_millis(40));
        sender.send(StartupSignal::UserInput).unwrap();
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn startup_input_waits_through_multiple_confirmation_screens() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(StartupSignal::Output(
                "Hook needs review: select an option".to_string(),
            ))
            .unwrap();
        let waiter = std::thread::spawn(move || wait_for_initial_input_ready(receiver));
        std::thread::sleep(Duration::from_millis(40));
        sender.send(StartupSignal::UserInput).unwrap();
        sender
            .send(StartupSignal::Output(
                "Do you trust this workspace? [y/N]".to_string(),
            ))
            .unwrap();
        sender.send(StartupSignal::UserInput).unwrap();
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn startup_input_waits_through_delayed_trust_hook_and_bypass_screens() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (done_sender, done_receiver) = std::sync::mpsc::channel();
        sender
            .send(StartupSignal::Output(
                "Workspace trust: Do you trust this folder?".to_string(),
            ))
            .unwrap();
        let waiter = std::thread::spawn(move || {
            let ready = wait_for_initial_input_ready(receiver);
            done_sender.send(ready).unwrap();
        });

        std::thread::sleep(Duration::from_millis(40));
        sender.send(StartupSignal::UserInput).unwrap();
        // The next screen can take longer than the old 600ms settle window.
        std::thread::sleep(Duration::from_millis(800));
        assert!(done_receiver.try_recv().is_err());

        sender
            .send(StartupSignal::Output(
                "Hook needs review: select an option".to_string(),
            ))
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        sender.send(StartupSignal::UserInput).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        assert!(done_receiver.try_recv().is_err());

        sender
            .send(StartupSignal::Output(
                "Bypass Permissions mode\n1. Yes, I accept\n2. No, exit".to_string(),
            ))
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        sender.send(StartupSignal::UserInput).unwrap();

        assert!(done_receiver.recv_timeout(Duration::from_secs(3)).unwrap());
        assert!(waiter.join().is_ok());
    }

    #[test]
    fn startup_input_is_not_released_by_an_unanswered_gate() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(StartupSignal::Output(
                "Hook needs review before continuing".to_string(),
            ))
            .unwrap();
        drop(sender);
        assert!(!wait_for_initial_input_ready(receiver));
    }

    #[test]
    fn startup_input_releases_immediately_on_session_start() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || wait_for_initial_input_ready(receiver));
        sender.send(StartupSignal::SessionReady).unwrap();
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn reasoning_effort_normalization_keeps_legacy_minimal_and_validates_ultra() {
        assert_eq!(
            normalized_reasoning_effort(Some("minimal"), true, None).unwrap(),
            Some("minimal".to_string())
        );
        assert_eq!(
            normalized_reasoning_effort(Some("ultra"), true, Some("gpt-5.6-sol")).unwrap(),
            Some("ultra".to_string())
        );
        assert!(normalized_reasoning_effort(Some("ultra"), true, Some("gpt-5.6")).is_err());
    }

    #[test]
    fn claude_ultra_uses_native_cli_effort_argument() {
        let launch = crate::app_settings::AgentLaunchSpec {
            program: "claude".to_string(),
            ..Default::default()
        };
        let mut cmd = build_claude_cmd(&launch, "ask");
        add_claude_launch_args(&mut cmd, "claude", None, Some("ultracode"));
        let argv: Vec<_> = cmd
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(argv
            .windows(2)
            .any(|pair| pair == ["--effort", "ultracode"]));
        assert!(!should_use_ultracode_terminal_command(
            false,
            Some("ultracode"),
            true
        ));
        assert!(should_use_ultracode_terminal_command(
            false,
            Some("ultracode"),
            false
        ));
    }

    #[test]
    fn project_prompt_prefix_is_not_applied_to_interactive_terminal_start() {
        assert_eq!(prompt_with_project_prefix("", "prefix"), "");
    }

    #[test]
    fn project_prompt_prefix_is_applied_to_non_empty_tasks() {
        assert_eq!(
            prompt_with_project_prefix("do the work", "prefix"),
            "prefix\ndo the work"
        );
    }

    #[test]
    fn codex_launch_uses_cd_argument_for_project_root() {
        let launch = crate::app_settings::AgentLaunchSpec {
            program: "codex".to_string(),
            ..Default::default()
        };
        let mut cmd = build_codex_cmd(&launch, "ask");
        add_codex_launch_args(&mut cmd, "/tmp/example-project", None, None, None);

        let argv: Vec<_> = cmd
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(argv
            .windows(2)
            .any(|pair| pair == ["-C", "/tmp/example-project"]));
    }

    #[test]
    fn fast_mode_launch_args_use_supported_cli_configuration() {
        let claude_launch = crate::app_settings::AgentLaunchSpec {
            program: "claude".to_string(),
            ..Default::default()
        };
        let mut claude_cmd = build_claude_cmd(&claude_launch, "ask");
        add_claude_launch_args(&mut claude_cmd, "claude", None, None);
        let claude_argv: Vec<_> = claude_cmd
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let unsupported_fast_flag = ["--", "fast"].concat();

        assert!(!claude_argv.iter().any(|arg| arg.contains("fastMode")));
        assert!(!claude_argv.iter().any(|arg| arg == "--settings"));
        assert!(!claude_argv.iter().any(|arg| arg == &unsupported_fast_flag));

        let codex_launch = crate::app_settings::AgentLaunchSpec {
            program: "codex".to_string(),
            ..Default::default()
        };
        let mut codex_cmd = build_codex_cmd(&codex_launch, "ask");
        add_codex_launch_args(
            &mut codex_cmd,
            "/tmp/example-project",
            None,
            None,
            Some("fast"),
        );
        let codex_argv: Vec<_> = codex_cmd
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(codex_argv
            .windows(2)
            .any(|pair| pair == ["-c", "features.fast_mode=true"]));
        assert!(codex_argv
            .windows(2)
            .any(|pair| pair == ["-c", "service_tier=\"fast\""]));
        assert!(!codex_argv.iter().any(|arg| arg == &unsupported_fast_flag));
    }

    #[test]
    fn codex_process_cwd_avoids_project_root() {
        let cwd = agent_process_cwd("/tmp/example-project", true);
        assert_ne!(cwd, std::path::PathBuf::from("/tmp/example-project"));
    }

    #[test]
    fn setup_env_removes_inherited_no_color_and_enables_truecolor() {
        let mut cmd = CommandBuilder::new("printf");
        cmd.env("NO_COLOR", "1");
        cmd.env("TERM", "dumb");
        cmd.env("COLORTERM", "");

        setup_env(&mut cmd);

        assert!(cmd.get_env("NO_COLOR").is_none());
        assert_eq!(
            cmd.get_env("TERM").and_then(|value| value.to_str()),
            Some("xterm-256color")
        );
        assert_eq!(
            cmd.get_env("COLORTERM").and_then(|value| value.to_str()),
            Some("truecolor")
        );
        assert_eq!(
            cmd.get_env("FORCE_COLOR").and_then(|value| value.to_str()),
            Some("3")
        );
    }
}
