//! oh-my-pi(omp)`--mode rpc-ui` PTY-less 驱动通道。
//!
//! 一任务一进程:每个 omp 任务启动一个 `omp --mode rpc-ui` 子进程,stdin/stdout
//! 走行分隔 JSONL 帧(非 JSON-RPC)。握手:子进程先发 `ready`,客户端回
//! `negotiate_protocol{protocolVersion:2}`,再以 `get_state` 取会话路径并注册。
//! 状态机(计划 D7):
//! - 进程启动 → `pending`;握手完成 → `running`;
//! - `agent_end`(isTerminal !== false)→ `input_required`;
//! - `extension_ui_request`(select/confirm/input/editor)→ `input_required` + `omp-ui-request`;
//! - `complete_omp_task` 关 stdin → 进程 exit 0 → `done`;
//! - `cancel_omp_task` abort + 关 stdin → `cancelled`;
//! - 退出码非 0 → `failed`(附 stderr 尾部)。

use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::TaskManager;

const OMP_RPC_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const STDERR_TAIL_LIMIT: usize = 4000;
const TOOL_ARGS_SUMMARY_LIMIT: usize = 160;

/// 需要回帧的 extension_ui_request 方法;其余(notify/setStatus/setWidget/…)只落终端。
const INTERACTIVE_UI_METHODS: [&str; 4] = ["select", "confirm", "input", "editor"];

pub struct OmpRpcManager {
    sessions: Mutex<HashMap<String, Arc<OmpSession>>>,
}

impl Default for OmpRpcManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OmpRpcManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 应用退出时调用:关闭所有 omp stdin,让其按协议正常退出(EOF → exit 0)。
    pub fn shutdown_all(&self) {
        let handles: Vec<Arc<OmpSession>> = self.sessions.lock().values().cloned().collect();
        for session in handles {
            session.close_stdin();
        }
    }
}

pub(crate) struct OmpSession {
    stdin: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    shared: Arc<OmpShared>,
    watcher_stop: Arc<AtomicBool>,
}

impl OmpSession {
    fn close_stdin(&self) {
        if let Some(mut stdin) = self.stdin.lock().take() {
            let _ = stdin.flush();
            // Drop 关闭管道:omp 在 stdin EOF 时按协议退出(exit 0)。
        }
    }
}

struct OmpShared {
    task_id: String,
    on_output: Channel<String>,
    pending_requests: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>,
    ready: AtomicBool,
    exited: AtomicBool,
    finalized: AtomicBool,
    streamed_terminal_text: AtomicBool,
    stderr_tail: Mutex<String>,
}

impl OmpShared {
    fn write_terminal(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.streamed_terminal_text.store(true, Ordering::Release);
        let _ = self.on_output.send(text.to_string());
    }

    fn record_stderr(&self, line: &str) {
        let mut tail = self.stderr_tail.lock();
        tail.push_str(line);
        tail.push('\n');
        let len = tail.len();
        if len > STDERR_TAIL_LIMIT {
            let cut = len - STDERR_TAIL_LIMIT;
            let boundary = tail
                .char_indices()
                .map(|(idx, _)| idx)
                .find(|idx| *idx >= cut)
                .unwrap_or(len);
            tail.drain(..boundary);
        }
    }
}

fn omp_permission_flag(permission_mode: &str) -> Option<&'static str> {
    match permission_mode {
        "ask" => Some("always-ask"),
        "auto_edit" => Some("write"),
        "full_access" => Some("yolo"),
        _ => None,
    }
}

/// Aeroric 统一 effort 词表 → omp thinking level(与前端 OMP_THINKING_LEVEL_MAP
/// 恒等映射一致:omp 原生 7 档透传,ultra 封顶 max)。
fn omp_thinking_level(effort: &str) -> Option<&'static str> {
    match effort {
        "off" => Some("off"),
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        "ultra" => Some("max"),
        _ => None,
    }
}

fn build_omp_cmd(
    launch: &crate::app_settings::AgentLaunchSpec,
    home: &std::path::Path,
    project_path: &str,
    permission_mode: &str,
    selected_model: Option<&str>,
    thinking_level: Option<&str>,
    resume: Option<&str>,
) -> Result<Command, String> {
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args)
        .arg("--mode")
        .arg("rpc-ui")
        .arg("--cwd")
        .arg(project_path);
    if let Some(flag) = omp_permission_flag(permission_mode) {
        cmd.arg("--approval-mode").arg(flag);
    }
    let model = selected_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    if let Some(level) = thinking_level {
        cmd.arg("--thinking").arg(level);
    }
    let resume = resume.map(str::trim).filter(|resume| !resume.is_empty());
    if let Some(resume) = resume {
        cmd.arg("--resume").arg(resume);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    for (key, value) in crate::omp_home::omp_agent_env(home) {
        cmd.env(key, value);
    }
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(cmd)
}

fn omp_images_to_content(images: &[String]) -> Vec<Value> {
    images
        .iter()
        .filter_map(|image| {
            let trimmed = image.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(rest) = trimmed.strip_prefix("data:") {
                // data:<mime>;base64,<payload>
                let (meta, data) = rest.split_once(',')?;
                let mime = meta
                    .split(';')
                    .next()
                    .filter(|mime| !mime.is_empty())
                    .unwrap_or("image/png");
                Some(json!({ "type": "image", "data": data, "mimeType": mime }))
            } else {
                Some(json!({ "type": "image", "data": trimmed, "mimeType": "image/png" }))
            }
        })
        .collect()
}

impl OmpSession {
    /// 发送一条需要响应的 RPC 命令,返回接收响应的 receiver。id 先登记再写帧,
    /// 避免响应先于登记到达的竞态。写帧走 spawn_blocking:带图片的大帧写慢消费
    /// 管道时不能阻塞 tokio worker。
    async fn send_request(
        &self,
        command: &str,
        params: Value,
    ) -> Result<(String, tokio::sync::oneshot::Receiver<Value>), String> {
        let id = format!("aeroric-{}", uuid::Uuid::new_v4());
        let (sender, receiver) = tokio::sync::oneshot::channel::<Value>();
        {
            let mut pending = self.shared.pending_requests.lock();
            pending.insert(id.clone(), sender);
        }
        let mut frame = params;
        frame["id"] = json!(id.clone());
        frame["type"] = json!(command);
        if !self.write_frame_async(frame).await {
            self.shared.pending_requests.lock().remove(&id);
            return Err(format!(
                "omp process is no longer accepting commands ({command})"
            ));
        }
        Ok((id, receiver))
    }

    /// 发送无需响应的控制帧。
    async fn send_notification(&self, frame_type: &str, params: Value) -> bool {
        let mut frame = params;
        frame["type"] = json!(frame_type);
        self.write_frame_async(frame).await
    }

    async fn write_frame_async(&self, frame: Value) -> bool {
        let stdin = self.stdin.clone();
        tokio::task::spawn_blocking(move || write_frame_to(&stdin, &frame))
            .await
            .unwrap_or(false)
    }
}

/// 持锁写一帧 JSONL。仅在 spawn_blocking 中调用。
fn write_frame_to(stdin: &Arc<Mutex<Option<Box<dyn Write + Send>>>>, frame: &Value) -> bool {
    let mut guard = stdin.lock();
    let Some(stdin) = guard.as_mut() else {
        return false;
    };
    let Ok(mut line) = serde_json::to_string(frame) else {
        return false;
    };
    line.push('\n');
    stdin.write_all(line.as_bytes()).is_ok() && stdin.flush().is_ok()
}

fn fail_pending_requests(shared: &OmpShared) {
    let mut pending = shared.pending_requests.lock();
    for (_, sender) in pending.drain() {
        let _ = sender.send(json!({ "success": false, "error": "omp process exited" }));
    }
}

/// 逐行分发 stdout 帧;非 JSON 行按终端原文回写(omp 承诺 rpc 模式 stdout 纯净,
/// 这条路径只为兼容意外输出)。
fn dispatch_stdout_line(app: &AppHandle, session: &Arc<OmpSession>, line: &str) {
    if line.trim().is_empty() {
        return;
    }
    let Ok(frame) = serde_json::from_str::<Value>(line) else {
        session.shared.write_terminal(line);
        return;
    };
    let frame_type = frame.get("type").and_then(Value::as_str).unwrap_or("");
    match frame_type {
        "ready" => {
            session.shared.ready.store(true, Ordering::Release);
        }
        "response" => {
            let id = frame.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some(sender) = session.shared.pending_requests.lock().remove(id) {
                let _ = sender.send(frame);
            }
        }
        "extension_ui_request" => {
            handle_extension_ui_request(app, session, &frame);
        }
        "agent_start" => {
            emit_status(app, &session.shared.task_id, "running");
        }
        "agent_end" => {
            let is_terminal = frame
                .get("isTerminal")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if is_terminal {
                emit_status(app, &session.shared.task_id, "input_required");
            }
        }
        "message_update" => {
            if let Some(text) = stream_text_from_message_update(&frame) {
                session.shared.write_terminal(&text);
            }
        }
        "message_end" => {
            if session
                .shared
                .streamed_terminal_text
                .load(Ordering::Acquire)
            {
                session.shared.write_terminal("\r\n");
            }
        }
        "tool_execution_start" => {
            let tool = frame
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = frame.get("args").cloned().unwrap_or(Value::Null);
            let summary = tool_args_summary(&args);
            session
                .shared
                .write_terminal(&format!("\r\n▸ {tool} {summary}\r\n"));
        }
        "tool_execution_end" => {
            let tool = frame
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let is_error = frame
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mark = if is_error { "✘" } else { "✔" };
            session.shared.write_terminal(&format!("{mark} {tool}\r\n"));
        }
        "auto_compaction_start" => {
            session
                .shared
                .write_terminal("\r\n… compacting context\r\n");
        }
        "auto_retry_start" => {
            let error = frame
                .get("errorMessage")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            session
                .shared
                .write_terminal(&format!("\r\n… retrying after error: {error}\r\n"));
        }
        "notice" => {
            let message = frame.get("message").and_then(Value::as_str).unwrap_or("");
            if !message.is_empty() {
                session.shared.write_terminal(&format!("ℹ {message}\r\n"));
            }
        }
        // turn_start/turn_end、message_start、session_info_update、
        // auto_compaction_end/auto_retry_end 等对终端展示无增量,忽略。
        _ => {}
    }
}

fn emit_status(app: &AppHandle, task_id: &str, status: &str) {
    let _ = app.emit(
        "task-status",
        json!({ "task_id": task_id, "status": status }),
    );
}

fn stream_text_from_message_update(frame: &Value) -> Option<String> {
    let message = frame.get("message")?;
    if message.get("role")?.as_str()? != "assistant" {
        return None;
    }
    let event = frame.get("assistantMessageEvent")?;
    if event.get("type")?.as_str()? != "text_delta" {
        return None;
    }
    event
        .get("delta")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn tool_args_summary(args: &Value) -> String {
    if args.is_null() {
        return String::new();
    }
    let serialized = args.to_string();
    let mut summary: String = serialized.chars().take(TOOL_ARGS_SUMMARY_LIMIT).collect();
    if summary.len() < serialized.len() {
        summary.push('…');
    }
    summary
}

fn handle_extension_ui_request(app: &AppHandle, session: &Arc<OmpSession>, frame: &Value) {
    let request_id = frame
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
    if request_id.is_empty() {
        return;
    }
    if !INTERACTIVE_UI_METHODS.contains(&method) {
        if method == "notify" {
            let message = frame.get("message").and_then(Value::as_str).unwrap_or("");
            if !message.is_empty() {
                let level = frame
                    .get("notifyType")
                    .and_then(Value::as_str)
                    .unwrap_or("info");
                let mark = match level {
                    "error" => "✘",
                    "warning" => "⚠",
                    _ => "ℹ",
                };
                session
                    .shared
                    .write_terminal(&format!("{mark} {message}\r\n"));
            }
        }
        // setStatus/setWidget/setTitle/set_editor_text/open_url/cancel:无终端增量。
        return;
    }
    let _ = app.emit(
        "omp-ui-request",
        json!({
            "task_id": session.shared.task_id,
            "request_id": request_id,
            "method": method,
            "title": frame.get("title"),
            "message": frame.get("message"),
            "options": frame.get("options"),
            "optionDetails": frame.get("optionDetails"),
            "placeholder": frame.get("placeholder"),
            "prefill": frame.get("prefill"),
        }),
    );
    emit_status(app, &session.shared.task_id, "input_required");
}

fn spawn_reader_thread(
    app: AppHandle,
    session: Arc<OmpSession>,
    stdout: std::process::ChildStdout,
) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => dispatch_stdout_line(&app, &session, &line),
                Err(_) => break,
            }
        }
    });
}

fn spawn_stderr_thread(session: Arc<OmpSession>, stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    session.shared.record_stderr(&line);
                    session.shared.write_terminal(&line);
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_waiter_thread(app: AppHandle, session: Arc<OmpSession>, mut child: Child) {
    std::thread::spawn(move || {
        let exit_status = child.wait();
        let task_id = session.shared.task_id.clone();
        session.shared.exited.store(true, Ordering::Release);
        session.watcher_stop.store(true, Ordering::Release);
        fail_pending_requests(&session.shared);
        {
            let manager = app.state::<OmpRpcManager>();
            // 仅当 map 里仍是本次会话时才移除:同 task_id 快速重入(run_omp_task
            // 新起一进程)时,迟到的旧 waiter 不得误删新会话。
            let mut sessions = manager.sessions.lock();
            if sessions
                .get(&task_id)
                .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                sessions.remove(&task_id);
            }
        }
        // 与 pty::finalize_task_exit 对齐:释放 omp_sessions 条目与占用的会话路径,
        // 否则后续任务 resume 同一会话文件时会被 claimed 检查静默跳过。
        {
            let tm = app.state::<TaskManager>();
            let omp_path = tm
                .omp_sessions
                .lock()
                .remove(&task_id)
                .map(|info| info.session_path);
            if let Some(path) = omp_path {
                tm.claimed_session_paths.lock().remove(&path);
            }
        }

        if session.shared.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        let cancelled = app
            .state::<TaskManager>()
            .cancelled_tasks
            .lock()
            .remove(&task_id);
        app.state::<TaskManager>()
            .manually_completed_tasks
            .lock()
            .remove(&task_id);
        // cancel 优先于退出码:cancel_omp_task 是 abort + 关 stdin,omp 按协议
        // 以 exit 0 收场,若退出码优先会把用户取消的任务误标为 done。
        let status = match cancelled {
            true => "cancelled",
            false => match &exit_status {
                Ok(exit) if exit.success() => "done",
                _ => "failed",
            },
        };
        let failure_reason = if status == "failed" {
            Some(
                session
                    .shared
                    .stderr_tail
                    .lock()
                    .lines()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else {
            None
        };
        let mut payload = json!({ "task_id": task_id, "status": status });
        if let Some(reason) = failure_reason {
            if !reason.trim().is_empty() {
                payload["failure_reason"] = json!(reason);
            }
        }
        let _ = app.emit("task-status", payload);
    });
}

/// 会话注册:`get_state` 给出 sessionFile 直接注册;否则启动发现线程兜底
/// (文件懒创建,首个 assistant 消息产出才落盘)。
fn register_omp_session_from_state(
    app: &AppHandle,
    task_id: &str,
    agent: &str,
    project_path: &std::path::Path,
    state_data: &Value,
    watcher_stop: Arc<AtomicBool>,
) {
    let session_file = state_data
        .get("sessionFile")
        .and_then(Value::as_str)
        .unwrap_or("");
    let session_id = state_data
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some((session_id, session_path)) = resolve_session_file(session_file, session_id) {
        crate::session_omp::register_omp_session(app, task_id, &session_id, &session_path);
        return;
    }
    if let Ok(canonical_project) = project_path.canonicalize() {
        crate::session_omp::spawn_omp_session_watcher(
            app.clone(),
            task_id.to_string(),
            agent.to_string(),
            canonical_project,
            watcher_stop,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        );
    }
}

fn resolve_session_file(
    session_file: &str,
    session_id: &str,
) -> Option<(String, std::path::PathBuf)> {
    if session_file.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(session_file);
    let id = if session_id.is_empty() {
        crate::session_omp::omp_session_id_from_file_name(&path)?
    } else {
        session_id.to_string()
    };
    Some((id, path))
}

async fn wait_for_ready(shared: &Arc<OmpShared>) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + OMP_RPC_STARTUP_TIMEOUT;
    loop {
        if shared.ready.load(Ordering::Acquire) {
            return Ok(());
        }
        if shared.exited.load(Ordering::Acquire) {
            // omp 启动失败(无模型/凭据、坏 flag 等)在 ready 之前就退出;
            // stderr 尾部是真正的原因,拼进报错给前端 toast/失败原因。
            let stderr_tail = shared
                .stderr_tail
                .lock()
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .to_string();
            return Err(if stderr_tail.trim().is_empty() {
                "omp exited before becoming ready".to_string()
            } else {
                format!("omp exited before becoming ready: {stderr_tail}")
            });
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "omp did not become ready within {}s",
                OMP_RPC_STARTUP_TIMEOUT.as_secs()
            ));
        }
        tokio::time::sleep(READY_POLL_INTERVAL).await;
    }
}

async fn request_with_timeout(
    session: &Arc<OmpSession>,
    command: &str,
    params: Value,
) -> Result<Value, String> {
    let (request_id, receiver) = session.send_request(command, params).await?;
    match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
        Ok(Ok(response)) => {
            if response
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                Ok(response.get("data").cloned().unwrap_or(Value::Null))
            } else {
                Err(response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown omp error")
                    .to_string())
            }
        }
        Ok(Err(_)) => Err(format!(
            "omp request {command} was dropped (process exited)"
        )),
        Err(_) => {
            // 响应永不到达:把登记条目一并清掉,否则要等进程退出才被 fail_pending 清理。
            session.shared.pending_requests.lock().remove(&request_id);
            Err(format!("omp request {command} timed out"))
        }
    }
}

fn validate_permission_mode(permission_mode: &str) -> Result<(), String> {
    if omp_permission_flag(permission_mode).is_none() {
        return Err(format!("Invalid permission mode: {permission_mode}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn run_omp_task(
    app: AppHandle,
    state: State<'_, OmpRpcManager>,
    task_manager: State<'_, TaskManager>,
    task_id: String,
    agent: String,
    project_path: String,
    prompt: String,
    session_id: Option<String>,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    permission_mode: String,
    images: Option<Vec<String>>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    validate_permission_mode(&permission_mode)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    if session_id.is_none() {
        let _ = crate::storage::truncate_task_terminal_history(&task_id);
        crate::remote::terminal_hub::hub().reset_for_truncate(&task_id);
    }

    let thinking_level = reasoning_effort
        .as_deref()
        .map(str::trim)
        .and_then(omp_thinking_level)
        .map(str::to_string);

    // 托管 home 初始化是纯文件 I/O,移出 async runtime。
    let agent_for_home = agent.clone();
    let (home, launch) = tokio::task::spawn_blocking(move || -> Result<(_, _), String> {
        let paths = crate::omp_home::ensure_omp_home_for(&agent_for_home)?;
        // MCP 变更随任务启动重写(omp 不支持热加载,进程重启即生效)。
        crate::mcp::omp_mcp_config_for_launch(&paths.home)?;
        let launch = crate::app_settings::get_agent_launch_spec(&agent_for_home);
        Ok((paths, launch))
    })
    .await
    .map_err(|error| error.to_string())??;
    if launch.family != crate::app_settings::AgentFamily::Omp {
        return Err(format!("Agent {agent} is not an oh-my-pi (omp) agent"));
    }

    let mut cmd = build_omp_cmd(
        &launch,
        &home.home,
        &project_path,
        &permission_mode,
        selected_model.as_deref(),
        thinking_level.as_deref(),
        session_id.as_deref(),
    )?;

    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Failed to start omp: {error}"))?;
    let (stdin, stdout, stderr) =
        match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
            (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
            // 三路都是 piped,理论不可达;真发生时杀掉子进程避免僵尸。
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("omp stdio pipes are unavailable".to_string());
            }
        };

    let shared = Arc::new(OmpShared {
        task_id: task_id.clone(),
        on_output: on_output.clone(),
        pending_requests: Mutex::new(HashMap::new()),
        ready: AtomicBool::new(false),
        exited: AtomicBool::new(false),
        finalized: AtomicBool::new(false),
        streamed_terminal_text: AtomicBool::new(false),
        stderr_tail: Mutex::new(String::new()),
    });
    let session = Arc::new(OmpSession {
        stdin: Arc::new(Mutex::new(Some(Box::new(stdin)))),
        shared: shared.clone(),
        watcher_stop: Arc::new(AtomicBool::new(false)),
    });
    state
        .sessions
        .lock()
        .insert(task_id.clone(), session.clone());

    spawn_reader_thread(app.clone(), session.clone(), stdout);
    spawn_stderr_thread(session.clone(), stderr);
    spawn_waiter_thread(app.clone(), session.clone(), child);
    emit_status(&app, &task_id, "pending");

    let handshake: Result<(), String> = async {
        wait_for_ready(&shared).await?;
        let negotiated = request_with_timeout(
            &session,
            "negotiate_protocol",
            json!({ "protocolVersion": 2 }),
        )
        .await?;
        let negotiated_version = negotiated
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if negotiated_version != 2 {
            return Err(format!(
                "omp negotiated unsupported RPC protocol version {negotiated_version}"
            ));
        }
        let session_state = request_with_timeout(&session, "get_state", json!({})).await?;
        register_omp_session_from_state(
            &app,
            &task_id,
            &agent,
            std::path::Path::new(&project_path),
            &session_state,
            session.watcher_stop.clone(),
        );
        emit_status(&app, &task_id, "running");
        if prompt.trim().is_empty() {
            // "Start terminal" 无初始输入:会话已就绪,等 composer/输入,这是
            // 交互态而非完成态,完成仍是显式动作。
            emit_status(&app, &task_id, "input_required");
            return Ok(());
        }
        let mut prompt_frame = json!({ "message": prompt });
        let image_content = omp_images_to_content(images.as_deref().unwrap_or(&[]));
        if !image_content.is_empty() {
            prompt_frame["images"] = json!(image_content);
        }
        request_with_timeout(&session, "prompt", prompt_frame).await?;
        emit_status(&app, &task_id, "running");
        Ok(())
    }
    .await;

    if let Err(error) = handshake {
        // 先占用 finalized:否则 close_stdin → 进程 exit 0 会让 waiter 补发
        // done,覆盖这里的 failed。waiter 的清理(sessions/omp_sessions/
        // pending fail)都在 finalized 检查之前,不受影响。
        session.shared.finalized.store(true, Ordering::Release);
        // cancel 与握手失败同时发生时,把 cancel 标志归还(状态归 failed 而非 cancelled)。
        task_manager.cancelled_tasks.lock().remove(&task_id);
        task_manager
            .manually_completed_tasks
            .lock()
            .remove(&task_id);
        session.close_stdin();
        let _ = app.emit(
            "task-status",
            json!({ "task_id": task_id, "status": "failed", "failure_reason": error }),
        );
        return Err(error);
    }
    Ok(())
}

/// 向运行中的 omp 会话追加输入(`prompt` 命令)。完成与否看 `agent_end.isTerminal`。
#[tauri::command]
pub async fn prompt_omp_task(
    state: State<'_, OmpRpcManager>,
    task_id: String,
    prompt: String,
    images: Option<Vec<String>>,
) -> Result<Value, String> {
    let session = state
        .sessions
        .lock()
        .get(&task_id)
        .cloned()
        .ok_or_else(|| "omp task is not running".to_string())?;
    let mut frame = json!({ "message": prompt });
    let image_content = omp_images_to_content(images.as_deref().unwrap_or(&[]));
    if !image_content.is_empty() {
        frame["images"] = json!(image_content);
    }
    request_with_timeout(&session, "prompt", frame).await
}

/// 中止当前 run 并关闭进程:任务落 `cancelled`;后续接续用 `run_omp_task --resume`。
#[tauri::command]
pub async fn cancel_omp_task(
    state: State<'_, OmpRpcManager>,
    task_manager: State<'_, TaskManager>,
    task_id: String,
) -> Result<(), String> {
    task_manager.cancelled_tasks.lock().insert(task_id.clone());
    let session = state.sessions.lock().get(&task_id).cloned();
    if let Some(session) = session {
        let _ = session.send_notification("abort", json!({})).await;
        session.close_stdin();
    } else {
        task_manager.cancelled_tasks.lock().remove(&task_id);
    }
    Ok(())
}

/// 用户显式"完成"omp 任务:关 stdin → 进程 exit 0 → 状态落 `done`。
#[tauri::command]
pub async fn complete_omp_task(
    state: State<'_, OmpRpcManager>,
    task_manager: State<'_, TaskManager>,
    task_id: String,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    task_manager
        .manually_completed_tasks
        .lock()
        .insert(task_id.clone());
    let session = state.sessions.lock().get(&task_id).cloned();
    if let Some(session) = session {
        session.close_stdin();
    }
    Ok(())
}

/// 回应 `extension_ui_request`(select/confirm/input/editor)。
/// `response` 形如 `{"value":"..."}` / `{"confirmed":true}` / `{"cancelled":true}`。
#[tauri::command]
pub async fn respond_omp_server_request(
    app: AppHandle,
    state: State<'_, OmpRpcManager>,
    task_id: String,
    request_id: String,
    response: Value,
) -> Result<(), String> {
    let session = state
        .sessions
        .lock()
        .get(&task_id)
        .cloned()
        .ok_or_else(|| "omp task is not running".to_string())?;
    let mut frame = json!({ "id": request_id });
    frame["type"] = json!("extension_ui_response");
    if let Value::Object(fields) = response {
        for (key, value) in fields {
            frame[key] = value;
        }
    }
    if !session
        .send_notification("extension_ui_response", frame)
        .await
    {
        return Err("omp process is no longer accepting responses".to_string());
    }
    let _ = app.emit(
        "omp-ui-request-resolved",
        json!({ "task_id": task_id, "request_id": request_id }),
    );
    Ok(())
}

/// 代理 omp `get_state`(模型/思考档/流式状态等),给设置与调试面用。
#[tauri::command]
pub async fn get_omp_state(
    state: State<'_, OmpRpcManager>,
    task_id: String,
) -> Result<Value, String> {
    let session = state
        .sessions
        .lock()
        .get(&task_id)
        .cloned()
        .ok_or_else(|| "omp task is not running".to_string())?;
    request_with_timeout(&session, "get_state", json!({})).await
}

/// 保存 omp 族配置的默认思考档(写入托管 config.yml 的 `defaultThinkingLevel`)。
#[tauri::command]
pub async fn update_omp_thinking_level(agent: String, effort: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        crate::omp_home::update_omp_reasoning_effort(&agent, &effort)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 读取 omp 族配置的默认思考档(未设置时为 None,前端回退 omp 默认 high)。
#[tauri::command]
pub async fn get_omp_thinking_level(agent: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        Ok::<Option<String>, String>(crate::omp_home::read_omp_reasoning_effort(&agent))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mode_maps_to_approval_flags() {
        assert_eq!(omp_permission_flag("ask"), Some("always-ask"));
        assert_eq!(omp_permission_flag("auto_edit"), Some("write"));
        assert_eq!(omp_permission_flag("full_access"), Some("yolo"));
        assert_eq!(omp_permission_flag("fast"), None);
    }

    #[test]
    fn thinking_level_maps_identity_with_ultra_cap() {
        assert_eq!(omp_thinking_level("off"), Some("off"));
        assert_eq!(omp_thinking_level("low"), Some("low"));
        assert_eq!(omp_thinking_level("xhigh"), Some("xhigh"));
        assert_eq!(omp_thinking_level("ultra"), Some("max"));
        assert_eq!(omp_thinking_level("bogus"), None);
    }

    #[test]
    fn data_url_images_decode_to_omp_image_content() {
        let images = vec![
            "data:image/jpeg;base64,QUJD".to_string(),
            "UkFW".to_string(),
            "  ".to_string(),
        ];
        let content = omp_images_to_content(&images);
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["mimeType"], json!("image/jpeg"));
        assert_eq!(content[0]["data"], json!("QUJD"));
        // 无 data: 前缀按裸 base64 处理,兜底 png。
        assert_eq!(content[1]["mimeType"], json!("image/png"));
    }

    #[test]
    fn tool_args_summary_truncates_long_arguments() {
        let long = "x".repeat(TOOL_ARGS_SUMMARY_LIMIT + 40);
        let summary = tool_args_summary(&json!({ "command": long }));
        assert!(summary.chars().count() <= TOOL_ARGS_SUMMARY_LIMIT + 1);
        assert!(summary.ends_with('…'));
        assert_eq!(tool_args_summary(&Value::Null), "");
    }

    #[test]
    fn text_streaming_only_reads_assistant_text_deltas() {
        let frame = json!({
            "type": "message_update",
            "message": { "role": "assistant" },
            "assistantMessageEvent": { "type": "text_delta", "delta": "hello" }
        });
        assert_eq!(
            stream_text_from_message_update(&frame).as_deref(),
            Some("hello")
        );
        let user_frame = json!({
            "type": "message_update",
            "message": { "role": "user" },
            "assistantMessageEvent": { "type": "text_delta", "delta": "hello" }
        });
        assert_eq!(stream_text_from_message_update(&user_frame), None);
        let thinking_frame = json!({
            "type": "message_update",
            "message": { "role": "assistant" },
            "assistantMessageEvent": { "type": "thinking_delta", "delta": "hmm" }
        });
        assert_eq!(stream_text_from_message_update(&thinking_frame), None);
    }
}
