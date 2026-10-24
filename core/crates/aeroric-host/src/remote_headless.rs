//! 无头远程服务面（P8-03 刀四）：`aeroric-cli serve` 的
//! [`RemoteHostHandle`](aeroric_core::remote::services::RemoteHostHandle)
//! 实现——任务执行经 CLI 已装配的 [`TaskService`]/[`CliPtyExecutor`]，
//! 存储面经 [`ProjectService`] 与 TaskService 的记录读取。
//!
//! 窄面原则与 `services.rs` 一致：移动端所需的方法给真实实现；headless
//! 不具备的能力（桌面 UI 事件、dsh 专用面）显式报错而不是猜。

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::agents::dsh_host::approvals::take_waterfall_answer;
use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, ProjectService};
use aeroric_core::remote::services::{RemoteHostHandle, RemoteTerminalStreams};
use aeroric_core::remote::state::{Message, OutboundSender};
use aeroric_core::remote::terminal_frames::{decode_terminal_frame, TerminalFrame, TerminalOpcode};
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::sessions::{
    dsh::parse_dsh_session_lines, omp::parse_omp_session_lines, parser::parse_session_lines,
};
use aeroric_core::tasks::executor::HostWindowFacts;
use aeroric_core::tasks::{AgentFamilies, Attachments, NewTask, ProtocolFamily, TaskService};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_core::terminal::stream::{StreamError, TerminalIdentity};

use crate::executor::CliPtyExecutor;
use crate::services::HostApprovals;

const MAX_FILE_RESPONSE_BYTES: usize = 200 * 1024;
const MAX_STREAMS_PER_CONNECTION: usize = 8;

pub struct CliRemoteHandle {
    pub service: Arc<TaskService>,
    pub context: Arc<CoreContext>,
    pub executor: Arc<CliPtyExecutor>,
    pub terminal: Arc<TerminalService>,
    pub registry: Arc<TaskRegistry>,
    pub approvals: Arc<std::sync::Mutex<HostApprovals>>,
    pub families: Arc<dyn AgentFamilies>,
    /// AP6 推送派发器。`None` = 这个宿主形态没有推送面(此时
    /// `push.register` 明确拒绝而不是假装成功)。
    pub push: Option<Arc<aeroric_core::remote::push_plane::PushDispatcher>>,
    /// AP6 推送网络出口(HTTP 到自托管发送端)。
    pub push_transport: Option<Arc<dyn aeroric_core::remote::push_plane::PushTransport>>,
    /// AP6:relay 诊断要描述**本服务正在用的**配置。
    ///
    /// 不在这里重新读 `remote-config.json`:那既会与运行中的 listener 不一致
    /// (文件改了但服务没重启),也让诊断结果依赖一个进程级共享文件——测试里就是
    /// 并发用例互相踩。调用方(serve)把启动时决定的配置传进来。
    pub relay: Option<(String, Option<String>)>,
}

impl CliRemoteHandle {
    /// 注销远端绑定(设备撤销/手机注销时)。失败只回报,不阻断本地清理。
    async fn unbind_remote(&self, device_id: &str) -> Value {
        let (Some(dispatcher), Some(transport)) = (&self.push, &self.push_transport) else {
            return json!({ "attempted": false, "reason": "no push transport wired" });
        };
        let Some(client) = dispatcher.client() else {
            return json!({ "attempted": false, "reason": "push is not configured" });
        };
        let params = json!({ "deviceId": device_id, "platform": "ios", "token": "" });
        let nonce = dispatcher.next_nonce();
        let now = aeroric_core::remote::push_plane::now_ms();
        match tokio::task::block_in_place(|| transport.bind(&client, &params, now, &nonce)) {
            Ok(value) => json!({ "attempted": true, "ok": true, "response": value }),
            Err(error) => json!({ "attempted": true, "ok": false, "reason": error }),
        }
    }

    fn project_by_id(&self, project_id: &str) -> Option<aeroric_core::storage::Record> {
        let projects = ProjectService::new(Arc::clone(&self.context)).list().ok()?;
        projects
            .into_iter()
            .find(|record| mutations::id_of(record) == Some(project_id))
    }

    fn task_record(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Option<aeroric_core::storage::Record> {
        let stamped = self.service.load(project_id).ok()?;
        stamped
            .records
            .into_iter()
            .find(|record| mutations::text(record, "id") == Some(task_id))
    }

    fn find_project_of_task(&self, task_id: &str) -> Option<aeroric_core::storage::Record> {
        let projects = ProjectService::new(Arc::clone(&self.context)).list().ok()?;
        for project in projects {
            let Some(id) = mutations::id_of(&project) else {
                continue;
            };
            let Ok(stamped) = self.service.load(id) else {
                continue;
            };
            if stamped
                .records
                .iter()
                .any(|record| mutations::text(record, "id") == Some(task_id))
            {
                return Some(project);
            }
        }
        None
    }

    fn audit_line(&self, event: &str, payload: &Value) {
        use std::io::Write;
        let dir = self.context.paths.data_dir().join("remote");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("audit.log"))
        else {
            return;
        };
        let now = self.context.clock.now_ms();
        let _ = writeln!(file, "{}\t{}\t{}", now, event, payload);
    }
    fn non_local_reason(project: &aeroric_core::storage::Record) -> Option<&'static str> {
        match project
            .0
            .get("location")
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
        {
            Some("ssh") => Some("ssh"),
            Some("wsl") => Some("wsl"),
            _ => None,
        }
    }

    fn validate_relative_path(path: &str) -> Result<(), String> {
        let p = std::path::Path::new(path);
        if path.is_empty() || p.is_absolute() {
            return Err("Invalid file path".to_string());
        }
        for component in p.components() {
            match component {
                std::path::Component::Normal(_) | std::path::Component::CurDir => {}
                _ => return Err("Invalid file path".to_string()),
            }
        }
        Ok(())
    }

    fn parse_family_messages(lines: &[String], family: ProtocolFamily) -> Vec<Value> {
        match family {
            ProtocolFamily::Dsh => parse_dsh_session_lines(lines)
                .into_iter()
                .map(serde_json::to_value)
                .map(Result::unwrap_or_default)
                .collect(),
            ProtocolFamily::Omp => parse_omp_session_lines(lines)
                .into_iter()
                .map(serde_json::to_value)
                .map(Result::unwrap_or_default)
                .collect(),
            other => {
                let mut messages = Vec::new();
                parse_session_lines(lines, other == ProtocolFamily::Codex, &mut messages);
                messages
                    .into_iter()
                    .map(serde_json::to_value)
                    .map(Result::unwrap_or_default)
                    .collect()
            }
        }
    }
}
#[async_trait]
impl RemoteHostHandle for CliRemoteHandle {
    fn desktop_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    fn emit(&self, _event: &str, _payload: Value) {
        // 无头没有桌面 UI 总线；任务状态经 task-status 事件桥（core
        // executor 的 log 面已覆盖）到达手机，这里无需重复投递。
    }

    fn audit_log(&self, event: &str, payload: Value) {
        self.audit_line(event, &payload);
    }

    async fn load_projects(&self) -> Result<Value, String> {
        let projects = ProjectService::new(Arc::clone(&self.context))
            .list()
            .map_err(|e| e.to_string())?;
        serde_json::to_value(projects).map_err(|e| e.to_string())
    }

    async fn set_project_pinned(&self, project_id: &str, pinned: bool) -> Result<(), String> {
        ProjectService::new(Arc::clone(&self.context))
            .set_pinned(project_id, pinned)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn load_project_tasks(&self, project_id: &str) -> Result<Value, String> {
        let stamped = self.service.load(project_id).map_err(|e| e.to_string())?;
        serde_json::to_value(stamped.records).map_err(|e| e.to_string())
    }

    async fn task_input(&self, task_id: &str, text: &str) -> Result<Value, String> {
        let data = format!("\x1b[200~{text}\x1b[201~\r");
        self.executor.input(task_id, data.as_bytes())?;
        Ok(json!({ "sent": true }))
    }

    async fn task_respond(&self, task_id: &str, params: Value) -> Result<Value, String> {
        let request_id = params
            .get("requestId")
            .or_else(|| params.get("eventId"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Missing param: requestId".to_string())?
            .to_string();
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("approve");
        if !matches!(action, "approve" | "deny") {
            return Err("Remote approval action must be approve or deny".to_string());
        }
        let outcome_val = json!({
            "kind": if action == "approve" { "approved" } else { "denied" },
            "action": action,
            "taskId": task_id,
        });
        let mut guard = self
            .approvals
            .lock()
            .map_err(|_| "approvals lock poisoned".to_string())?;
        match take_waterfall_answer(
            &mut guard.registry,
            "remote-mobile",
            &request_id,
            &outcome_val,
        ) {
            Ok(answer) => {
                guard.resolved_events.insert(request_id.clone());
                Ok(json!({
                    "accepted": true,
                    "taskId": task_id,
                    "eventId": request_id,
                    "sessionHint": answer.session_hint,
                }))
            }
            Err(e) => Err(format!("cannot answer approval: {e}")),
        }
    }

    async fn task_cancel(&self, _project_id: &str, task_id: &str) -> Result<Value, String> {
        let project = self
            .find_project_of_task(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;
        let target = self
            .service
            .cancel(&project, task_id)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "taskId": task_id, "target": target.target }))
    }

    async fn task_complete(&self, _project_id: &str, task_id: &str) -> Result<Value, String> {
        let project = self
            .find_project_of_task(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;
        let (completion, _) = self
            .service
            .complete(&project, task_id)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "taskId": task_id, "completion": format!("{completion:?}") }))
    }

    async fn task_request(&self, payload: Value) -> Result<Value, String> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("create");
        if kind == "resume" {
            let task_id = payload
                .get("taskId")
                .and_then(Value::as_str)
                .ok_or("Resume request is missing taskId")?;
            let project = self
                .find_project_of_task(task_id)
                .ok_or_else(|| format!("Task not found: {task_id}"))?;
            let project_id = mutations::id_of(&project).unwrap_or_default().to_string();
            let task_id: &str = task_id;
            let session_id = self
                .task_record(&project_id, task_id)
                .and_then(|record| {
                    mutations::text(&record, "sessionId")
                        .or_else(|| mutations::text(&record, "sessionPath"))
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "default-session".to_string());
            let operation_key = format!("cli-remote-resume-{task_id}");
            let applied = self
                .service
                .resume(&project, task_id, &session_id, &operation_key, true, None)
                .map_err(|e| e.to_string())?;
            let status = applied
                .records
                .iter()
                .find(|record| mutations::text(record, "id") == Some(task_id))
                .and_then(|record| mutations::text(record, "status"))
                .unwrap_or("running")
                .to_string();
            return Ok(json!({ "accepted": true, "taskId": task_id, "status": status }));
        }
        // create：与 CLI `task.run` 同一条 open_local → create → start 链。
        let project_path = payload
            .get("projectPath")
            .and_then(Value::as_str)
            .and_then(|v| {
                if v.trim().is_empty() {
                    None
                } else {
                    Some(v.to_string())
                }
            });
        let project = match project_path {
            Some(path) => {
                let projects = ProjectService::new(Arc::clone(&self.context));
                let now = self.context.clock.now_ms() as i64;
                let (upserted, _) = projects
                    .open_local(&path, "project", now)
                    .map_err(|e| format!("cannot open the project: {e}"))?;
                upserted.project
            }
            None => {
                let project_id = payload
                    .get("projectId")
                    .and_then(Value::as_str)
                    .ok_or("Project not found on the desktop")?;
                self.project_by_id(project_id)
                    .ok_or_else(|| "Project not found on the desktop".to_string())?
            }
        };
        let project_id = mutations::id_of(&project).unwrap_or_default().to_string();
        let prompt = payload
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let agent = payload
            .get("agent")
            .and_then(Value::as_str)
            .unwrap_or("claude")
            .to_string();
        let task_id = format!("remote-{}", uuid::Uuid::new_v4().simple());
        let new_task = NewTask {
            id: task_id.clone(),
            project_id: project_id.clone(),
            prompt,
            agent,
            permission_mode: payload
                .get("permissionMode")
                .and_then(Value::as_str)
                .unwrap_or("ask")
                .to_string(),
            immediate: true,
            selected_model: payload
                .get("selectedModel")
                .and_then(Value::as_str)
                .map(str::to_string),
            reasoning_effort: payload
                .get("reasoningEffort")
                .and_then(Value::as_str)
                .map(str::to_string),
            speed: payload
                .get("speed")
                .and_then(Value::as_str)
                .map(str::to_string),
            dsh_agent_preset: payload
                .get("dshAgentPreset")
                .and_then(Value::as_str)
                .map(str::to_string),
            // Remote RPC predates agent configurations; it never selects one.
            agent_config_id: None,
            images: Vec::new(),
            texts: Vec::new(),
        };
        self.service
            .create(&project, new_task)
            .map_err(|e| format!("cannot create the task: {e}"))?;
        let operation_key = format!("cli-remote-create-{task_id}");
        let applied = self
            .service
            .start(
                &project,
                &task_id,
                &operation_key,
                true,
                None,
                false,
                Attachments::default(),
                HostWindowFacts {
                    terminal_size: None,
                    dsh: None,
                },
            )
            .map_err(|e| format!("cannot start the task: {e}"))?;
        let status = applied
            .records
            .iter()
            .find(|record| mutations::text(record, "id") == Some(task_id.as_str()))
            .and_then(|record| mutations::text(record, "status"))
            .unwrap_or("unknown")
            .to_string();
        Ok(json!({ "accepted": true, "taskId": task_id, "status": status }))
    }

    async fn session_messages(&self, params: Value) -> Result<Value, String> {
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let task_id = params
            .get("taskId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: taskId")?;
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason, "messages": [] }));
        }
        let task = self
            .task_record(project_id, task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;
        let agent = mutations::text(&task, "agent").unwrap_or("claude");
        let family = self.families.session_family(agent);
        let is_codex = family == ProtocolFamily::Codex;
        let session_path = mutations::text(&task, "sessionPath")
            .or_else(|| match family {
                ProtocolFamily::Codex => mutations::text(&task, "codexSessionPath"),
                ProtocolFamily::Claude => mutations::text(&task, "claudeSessionPath"),
                ProtocolFamily::Dsh => mutations::text(&task, "dshSessionPath"),
                ProtocolFamily::Omp => mutations::text(&task, "ompSessionPath"),
            })
            .map(str::to_string);
        let Some(session_path) = session_path else {
            return Ok(json!({ "available": false, "reason": "no_session", "messages": [] }));
        };
        let raw = std::fs::read_to_string(&session_path)
            .map_err(|e| format!("cannot read session transcript: {e}"))?;
        let lines: Vec<String> = raw.lines().map(str::to_string).collect();
        let messages = Self::parse_family_messages(&lines, family);
        Ok(json!({
            "available": true,
            "isCodex": is_codex,
            "messages": messages,
        }))
    }

    async fn project_files(&self, params: Value) -> Result<Value, String> {
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason }));
        }
        let root_str = mutations::text(&project, "path")
            .ok_or("Project has no path")?
            .to_string();
        let target_str = params
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&root_str)
            .to_string();
        let root_canon = std::path::Path::new(&root_str)
            .canonicalize()
            .map_err(|e| format!("Invalid project root: {e}"))?;
        let target_canon = std::path::Path::new(&target_str)
            .canonicalize()
            .map_err(|_| "Path outside project or not found".to_string())?;
        if !target_canon.starts_with(&root_canon) || !target_canon.is_dir() {
            return Err("Path outside project or not a directory".to_string());
        }
        let mut entries = Vec::new();
        let read_dir = std::fs::read_dir(&target_canon).map_err(|e| e.to_string())?;
        for item in read_dir.flatten() {
            let meta = match item.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = item.file_name().to_string_lossy().into_owned();
            entries.push(json!({
                "name": name,
                "path": item.path().to_string_lossy(),
                "isDirectory": meta.is_dir(),
                "size": meta.len(),
            }));
        }
        entries.sort_by(|a, b| {
            let a_dir = a["isDirectory"].as_bool().unwrap_or(false);
            let b_dir = b["isDirectory"].as_bool().unwrap_or(false);
            b_dir
                .cmp(&a_dir)
                .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
        });
        Ok(json!({
            "available": true,
            "root": root_str,
            "entries": entries,
        }))
    }

    async fn project_read_file(&self, params: Value) -> Result<Value, String> {
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: path")?;
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason }));
        }
        let root_str = mutations::text(&project, "path").ok_or("Project has no path")?;
        let root_canon = std::path::Path::new(root_str)
            .canonicalize()
            .map_err(|e| format!("Invalid project root: {e}"))?;
        let resolved = std::path::Path::new(path).canonicalize().map_err(|_| {
            "Only existing regular files inside the project can be read".to_string()
        })?;
        if !resolved.starts_with(&root_canon) || !resolved.is_file() {
            return Err("Only existing regular files inside the project can be read".to_string());
        }
        let content = std::fs::read_to_string(&resolved).map_err(|e| e.to_string())?;
        let total_bytes = content.len();
        let truncated = total_bytes > MAX_FILE_RESPONSE_BYTES;
        let content = if truncated {
            let mut end = MAX_FILE_RESPONSE_BYTES;
            while end > 0 && !content.is_char_boundary(end) {
                end -= 1;
            }
            content[..end].to_string()
        } else {
            content
        };
        Ok(json!({
            "available": true,
            "content": content,
            "truncated": truncated,
            "totalBytes": total_bytes,
        }))
    }

    async fn project_write_file(&self, params: Value) -> Result<Value, String> {
        self.context.require_write().map_err(|e| e.to_string())?;
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: path")?;
        let content = params
            .get("content")
            .and_then(Value::as_str)
            .ok_or("Missing param: content")?;
        if content.len() > MAX_FILE_RESPONSE_BYTES {
            return Err("File too large to save".to_string());
        }
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason }));
        }
        let root_str = mutations::text(&project, "path").ok_or("Project has no path")?;
        let root_canon = std::path::Path::new(root_str).canonicalize().map_err(|_| {
            "Only existing regular files inside the project can be saved".to_string()
        })?;
        let resolved = std::path::Path::new(path).canonicalize().map_err(|_| {
            "Only existing regular files inside the project can be saved".to_string()
        })?;
        if !resolved.starts_with(&root_canon) || !resolved.is_file() {
            return Err("Only existing regular files inside the project can be saved".to_string());
        }
        let relative = resolved
            .strip_prefix(&root_canon)
            .map_err(|_| "Only existing regular files inside the project can be saved".to_string())?
            .to_string_lossy()
            .into_owned();
        std::fs::write(&resolved, content.as_bytes()).map_err(|e| e.to_string())?;
        self.audit_line(
            "remote-file-written",
            &json!({ "projectId": project_id, "path": relative }),
        );
        Ok(json!({ "available": true, "ok": true }))
    }

    async fn git_changes(&self, params: Value) -> Result<Value, String> {
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason }));
        }
        let mut root = mutations::text(&project, "path")
            .ok_or("Project has no path")?
            .to_string();
        if let Some(task_id) = params.get("taskId").and_then(Value::as_str) {
            if let Some(task) = self.task_record(project_id, task_id) {
                if let Some(wt) = mutations::text(&task, "worktreePath").filter(|s| !s.is_empty()) {
                    root = wt.to_string();
                }
            }
        }
        let changes = tokio::task::spawn_blocking(move || {
            aeroric_core::git::service::status(std::path::Path::new(&root))
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(json!({
            "available": true,
            "changes": serde_json::to_value(changes).map_err(|e| e.to_string())?,
        }))
    }

    async fn git_diff(&self, params: Value) -> Result<Value, String> {
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: projectId")?;
        let file_path = params
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: path")?
            .to_string();
        Self::validate_relative_path(&file_path)?;
        let staged = params
            .get("staged")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let project = self
            .project_by_id(project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))?;
        if let Some(reason) = Self::non_local_reason(&project) {
            return Ok(json!({ "available": false, "reason": reason }));
        }
        let mut root = mutations::text(&project, "path")
            .ok_or("Project has no path")?
            .to_string();
        if let Some(task_id) = params.get("taskId").and_then(Value::as_str) {
            if let Some(task) = self.task_record(project_id, task_id) {
                if let Some(wt) = mutations::text(&task, "worktreePath").filter(|s| !s.is_empty()) {
                    root = wt.to_string();
                }
            }
        }
        let diff = tokio::task::spawn_blocking(move || {
            aeroric_core::git::service::file_diff(std::path::Path::new(&root), &file_path, staged)
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(json!({ "available": true, "diff": diff }))
    }

    async fn agent_config_list(&self) -> Result<Value, String> {
        let fields = aeroric_core::storage::load_settings(&self.context)
            .map_err(|e| e.to_string())?
            .found()
            .unwrap_or_default();
        let builtin_creds = fields
            .get("builtin_agent_credentials")
            .and_then(Value::as_object);
        let proxy_map = fields.get("agent_proxy_enabled").and_then(Value::as_object);
        let builtin_entry = |id: &str, label: &str, codex_like: bool, family: &str| {
            let cred = builtin_creds
                .and_then(|m| m.get(id))
                .and_then(Value::as_object);
            let base_url = cred
                .and_then(|c| c.get("base_url").or_else(|| c.get("baseUrl")))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let api_key_configured = cred
                .and_then(|c| c.get("api_key").or_else(|| c.get("apiKey")))
                .and_then(Value::as_str)
                .map(|k| !k.trim().is_empty())
                .unwrap_or(false);
            let models = cred
                .and_then(|c| c.get("models"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            let enable_1m = cred
                .and_then(|c| {
                    c.get("enable_1m_context")
                        .or_else(|| c.get("enable1mContext"))
                })
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let proxy_enabled = proxy_map
                .and_then(|m| m.get(id))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            json!({
                "id": id,
                "label": label,
                "codexLike": codex_like,
                "family": family,
                "editable": true,
                "baseUrl": base_url,
                "apiKeyConfigured": api_key_configured,
                "models": models,
                "enable1mContext": enable_1m,
                "proxyEnabled": proxy_enabled,
            })
        };
        let mut agents = vec![
            builtin_entry("claude", "Claude Code", false, "claude"),
            builtin_entry("codex", "Codex", true, "codex"),
            builtin_entry("dsh", "DeepSeek Harness", false, "dsh"),
            builtin_entry("omp", "oh-my-pi", false, "omp"),
        ];
        if let Some(custom) = fields.get("custom_agents").and_then(Value::as_array) {
            for profile in custom {
                let Some(id) = profile
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let label = profile
                    .get("label")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(id);
                let codex_like = profile
                    .get("codex_like")
                    .or_else(|| profile.get("codexLike"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let family = profile
                    .get("family")
                    .and_then(Value::as_str)
                    .unwrap_or(if codex_like { "codex" } else { "claude" });
                let base_url = profile
                    .get("base_url")
                    .or_else(|| profile.get("baseUrl"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let api_key_configured = profile
                    .get("api_key")
                    .or_else(|| profile.get("apiKey"))
                    .and_then(Value::as_str)
                    .map(|k| !k.trim().is_empty())
                    .unwrap_or(false);
                let models = profile.get("models").cloned().unwrap_or_else(|| json!([]));
                let proxy_enabled = proxy_map
                    .and_then(|m| m.get(id))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                agents.push(json!({
                    "id": id,
                    "label": label,
                    "codexLike": codex_like,
                    "family": family,
                    "editable": true,
                    "baseUrl": base_url,
                    "apiKeyConfigured": api_key_configured,
                    "models": models,
                    "proxyEnabled": proxy_enabled,
                }));
            }
        }
        Ok(json!({ "agents": agents }))
    }

    async fn agent_config_save(&self, params: Value) -> Result<Value, String> {
        self.context.require_write().map_err(|e| e.to_string())?;
        let agent_id = params
            .get("id")
            .or_else(|| params.get("agent"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: id")?
            .to_string();
        let mut fields = aeroric_core::storage::load_settings(&self.context)
            .map_err(|e| e.to_string())?
            .found()
            .unwrap_or_default();
        let mut creds_obj = fields
            .remove("builtin_agent_credentials")
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let mut entry = creds_obj
            .remove(&agent_id)
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        if let Some(base_url) = params.get("baseUrl").and_then(Value::as_str) {
            entry.insert("base_url".to_string(), Value::String(base_url.to_string()));
        }
        if params.get("clearApiKey").and_then(Value::as_bool) == Some(true) {
            entry.insert("api_key".to_string(), Value::String(String::new()));
        } else if let Some(api_key) = params
            .get("apiKey")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            entry.insert("api_key".to_string(), Value::String(api_key.to_string()));
        }
        if let Some(models) = params.get("models").and_then(Value::as_array) {
            entry.insert("models".to_string(), Value::Array(models.clone()));
        }
        creds_obj.insert(agent_id.clone(), Value::Object(entry));
        fields.insert(
            "builtin_agent_credentials".to_string(),
            Value::Object(creds_obj),
        );
        aeroric_core::storage::save_settings(&self.context, &fields).map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "id": agent_id }))
    }

    async fn agent_config_detect_models(&self, params: Value) -> Result<Value, String> {
        let agent = params
            .get("agent")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("claude");
        self.agents_models(agent).await
    }

    async fn agent_config_create(&self, params: Value) -> Result<Value, String> {
        self.context.require_write().map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing param: id")?
            .to_string();
        let mut fields = aeroric_core::storage::load_settings(&self.context)
            .map_err(|e| e.to_string())?
            .found()
            .unwrap_or_default();
        let mut custom = fields
            .remove("custom_agents")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        if custom
            .iter()
            .any(|c| c.get("id").and_then(Value::as_str) == Some(&id))
        {
            return Err(format!("Agent already exists: {id}"));
        }
        custom.push(params.clone());
        fields.insert("custom_agents".to_string(), Value::Array(custom));
        aeroric_core::storage::save_settings(&self.context, &fields).map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "id": id }))
    }

    async fn agents_list(&self) -> Result<Value, String> {
        let mut agents = vec![
            json!({ "id": "claude", "value": "claude", "label": "Claude Code", "codexLike": false, "family": "claude" }),
            json!({ "id": "codex", "value": "codex", "label": "Codex", "codexLike": true, "family": "codex" }),
            json!({ "id": "dsh", "value": "dsh", "label": "DeepSeek Harness", "codexLike": false, "family": "dsh" }),
            json!({ "id": "omp", "value": "omp", "label": "oh-my-pi", "codexLike": false, "family": "omp" }),
        ];
        if let Ok(Some(fields)) =
            aeroric_core::storage::load_settings(&self.context).map(|o| o.found())
        {
            if let Some(custom) = fields.get("custom_agents").and_then(Value::as_array) {
                for profile in custom {
                    let Some(id) = profile
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    else {
                        continue;
                    };
                    let label = profile
                        .get("label")
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or(id);
                    let codex_like = profile
                        .get("codex_like")
                        .or_else(|| profile.get("codexLike"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let family = profile
                        .get("family")
                        .and_then(Value::as_str)
                        .unwrap_or(if codex_like { "codex" } else { "claude" });
                    agents.push(json!({
                        "id": id,
                        "value": id,
                        "label": label,
                        "codexLike": codex_like,
                        "family": family,
                    }));
                }
            }
        }
        Ok(json!(agents))
    }

    async fn agents_models(&self, _agent: &str) -> Result<Value, String> {
        Ok(json!({ "models": [] }))
    }

    async fn stats_summary(&self) -> Result<Value, String> {
        Ok(
            json!({ "totalAgentsSpawned": 0, "agentTimeApproximate": true, "totalPRsCreated": Value::Null }),
        )
    }

    fn terminal_streams(&self, outbound: OutboundSender) -> Box<dyn RemoteTerminalStreams> {
        Box::new(HeadlessTerminalStreams {
            outbound,
            terminal: Arc::clone(&self.terminal),
            registry: Arc::clone(&self.registry),
            executor: Arc::clone(&self.executor),
            streams: HashMap::new(),
        })
    }

    // ── AP6 推送平面 ────────────────────────────────────────────────────────

    /// 手机经 E2EE 上报 APNs device token。
    ///
    /// 顺序是刻意的:先记本机绑定表(这样即使发送端暂时不可达,`push.status`
    /// 与后续撤销仍然一致),再把它转给发送端;失败时把本地登记**撤回**,
    /// 避免出现「本机说登记了、发送端说没有」的分裂状态。
    async fn push_register(&self, params: Value) -> Result<Value, String> {
        let device_id = params
            .get("deviceId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Missing param: deviceId".to_string())?
            .to_string();
        let registering = params
            .get("token")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|token| !token.is_empty());

        let Some(dispatcher) = &self.push else {
            // 宿主没装配推送面:如实拒绝,不假装登记成功(否则手机以为能收通知)。
            return Err(
                "this host has no push plane wired; notifications are unavailable".to_string(),
            );
        };

        if !registering {
            // 注销:本地删,远端解绑。远端失败也要把本地删掉(用户的意图是
            // 「不要再推给我」,宁可远端留一条孤儿绑定,也不能继续推)。
            let removed = dispatcher.bindings().unregister(&device_id);
            let remote = self.unbind_remote(&device_id).await;
            return Ok(json!({
                "registered": false,
                "changed": removed,
                "remote": remote,
            }));
        }

        let platform = params
            .get("platform")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("ios");
        let now = aeroric_core::remote::push_plane::now_ms();
        dispatcher.bindings().register(&device_id, platform, now);

        let remote = match &self.push_transport {
            Some(transport) => match dispatcher.client() {
                Some(client) => {
                    let params = params.clone();
                    let nonce = dispatcher.next_nonce();
                    let result = tokio::task::block_in_place(|| {
                        transport.bind(&client, &params, now, &nonce)
                    });
                    match result {
                        Ok(value) => Ok(value),
                        Err(error) => Err(error),
                    }
                }
                None => Err("push is not configured on this host".to_string()),
            },
            None => Err("this host has no push transport wired".to_string()),
        };

        match remote {
            Ok(value) => {
                if let Some(dispatcher) = &self.push {
                    dispatcher.bindings().register(&device_id, platform, now);
                }
                Ok(json!({ "registered": true, "changed": true, "remote": value }))
            }
            Err(error) => {
                // 远端没成:撤回本地登记,如实回报失败原因。
                dispatcher.bindings().unregister(&device_id);
                Err(format!("the push sender refused the registration: {error}"))
            }
        }
    }

    async fn push_status(&self) -> Result<Value, String> {
        let Some(dispatcher) = &self.push else {
            return Ok(json!({
                "enabled": false,
                "wired": false,
                "devices": [],
                "stats": Value::Null,
                "detail": "this host has no push plane wired; notifications are unavailable",
            }));
        };
        let client = dispatcher.client();
        Ok(json!({
            "enabled": client.is_some(),
            "wired": self.push_transport.is_some(),
            "senderUrl": client.as_ref().map(|client| client.config().url.clone()),
            "user": client.as_ref().map(|client| client.config().user.clone()),
            "devices": dispatcher.bindings().snapshot(),
            "stats": dispatcher.stats().snapshot(),
            "suppressedForMs": aeroric_core::remote::push_plane::suppression_window(dispatcher.suppressor())
                .map(|window| window.as_millis() as u64),
            "detail": if client.is_some() {
                "push is configured; notifications go through your self-hosted sender"
            } else {
                "push is not configured; set a sender URL and credential to enable notifications"
            },
        }))
    }

    async fn push_test(&self) -> Result<Value, String> {
        let Some(dispatcher) = &self.push else {
            return Err("this host has no push plane wired".to_string());
        };
        dispatcher.push_manual(remote_protocol::PushKind::TaskDone, "manual-test")
    }

    /// 设备授权已结束:本地绑定失效 + 通知自托管发送端解绑。
    ///
    /// 两步都做而不是只做一步:本地失效保证这台 Mac 不再把它算作目标;远端解绑
    /// 保证**发送端自己**也不会再推(否则在途/重放的请求仍可能送达)。两者不一致
    /// 的那段时间里,远端是唯一还能发的,所以远端必须解。
    async fn device_revoked(&self, device_id: &str) -> Result<Value, String> {
        let Some(dispatcher) = &self.push else {
            return Ok(json!({ "push": "not wired on this host" }));
        };
        let removed = dispatcher.bindings().unregister(device_id);
        let invalidated = dispatcher.bindings().mark_invalidated(device_id);
        let remote = self.unbind_remote(device_id).await;
        Ok(json!({
            "bindingRemoved": removed,
            "bindingInvalidated": invalidated,
            "remote": remote,
        }))
    }

    async fn relay_diagnostics(&self) -> Result<Value, String> {
        // 诊断描述**本服务正在用的**配置:`self.relay` 由调用方在启动时给定。
        // 不在这里重读 `remote-config.json` —— 那会与运行中的 listener 不一致
        // (文件改了但服务没重启),也会让结果依赖一个进程级共享文件。
        let (relay_url, relay_token) = match &self.relay {
            Some((url, token)) => (url.clone(), token.clone()),
            None => (String::new(), None),
        };
        let diagnostics = crate::push_host::probe_relay(&relay_url, relay_token.as_deref());
        Ok(json!({
            "relay": crate::push_host::relay_diagnostics_value(&diagnostics),
            "directHint": crate::push_host::direct_endpoints_hint(),
            "defaultRelay": Value::Null,
            "note": "Aeroric ships no default relay; your Mac and phone must use your own relay or a trusted direct network",
        }))
    }
}

struct HeadlessStreamEntry {
    task_id: String,
    forwarder: tokio::task::JoinHandle<()>,
}

struct HeadlessTerminalStreams {
    outbound: OutboundSender,
    terminal: Arc<TerminalService>,
    registry: Arc<TaskRegistry>,
    executor: Arc<CliPtyExecutor>,
    streams: HashMap<u32, HeadlessStreamEntry>,
}

fn binary_frame(opcode: TerminalOpcode, stream_id: u32, seq: u64, payload: Vec<u8>) -> Message {
    Message::Binary(TerminalFrame::new(opcode, stream_id, seq, payload).encode())
}

impl RemoteTerminalStreams for HeadlessTerminalStreams {
    fn handle_frame(&mut self, bytes: &[u8]) {
        let Some(frame) = decode_terminal_frame(bytes) else {
            return;
        };
        match frame.opcode {
            TerminalOpcode::Subscribe => {
                let stream_id = frame.stream_id;
                let task_id = serde_json::from_slice::<Value>(&frame.payload)
                    .ok()
                    .and_then(|v| v.get("taskId").and_then(Value::as_str).map(str::to_string))
                    .filter(|s| !s.trim().is_empty());
                let Some(task_id) = task_id else {
                    let _ = self.outbound.send(binary_frame(
                        TerminalOpcode::Error,
                        stream_id,
                        0,
                        b"Subscribe requires taskId".to_vec(),
                    ));
                    return;
                };
                if !self.streams.contains_key(&stream_id)
                    && self.streams.len() >= MAX_STREAMS_PER_CONNECTION
                {
                    let _ = self.outbound.send(binary_frame(
                        TerminalOpcode::Error,
                        stream_id,
                        0,
                        format!("Too many terminal streams (max {MAX_STREAMS_PER_CONNECTION})")
                            .into_bytes(),
                    ));
                    return;
                }
                if let Some(old) = self.streams.remove(&stream_id) {
                    old.forwarder.abort();
                }
                let outbound = self.outbound.clone();
                let terminal = Arc::clone(&self.terminal);
                let registry = Arc::clone(&self.registry);
                let forward_task_id = task_id.clone();
                let forwarder = tokio::spawn(async move {
                    let mut seq: u64 = 0;
                    let run = registry.resource_run(&forward_task_id);
                    let live = run.is_some();
                    let meta = json!({ "cols": 120, "rows": 30, "live": live });
                    seq += 1;
                    if outbound
                        .send(binary_frame(
                            TerminalOpcode::SnapshotStart,
                            stream_id,
                            seq,
                            serde_json::to_vec(&meta).unwrap_or_default(),
                        ))
                        .is_err()
                    {
                        return;
                    }
                    let mut offset: u64 = 0;
                    if let Some(run_key) = &run {
                        let identity =
                            TerminalIdentity::new(forward_task_id.clone(), run_key.generation());
                        if let Ok(chunk) = terminal.read_from(&identity, 0, 64 * 1024) {
                            if !chunk.bytes.is_empty() {
                                offset = chunk.bytes.len() as u64;
                                seq += 1;
                                if outbound
                                    .send(binary_frame(
                                        TerminalOpcode::SnapshotChunk,
                                        stream_id,
                                        seq,
                                        chunk.bytes,
                                    ))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    seq += 1;
                    if outbound
                        .send(binary_frame(
                            TerminalOpcode::SnapshotEnd,
                            stream_id,
                            seq,
                            Vec::new(),
                        ))
                        .is_err()
                    {
                        return;
                    }
                    let Some(run_key) = run else {
                        return;
                    };
                    let identity =
                        TerminalIdentity::new(forward_task_id.clone(), run_key.generation());
                    loop {
                        match terminal.read_from(&identity, offset, 32 * 1024) {
                            Ok(chunk) if !chunk.bytes.is_empty() => {
                                offset += chunk.bytes.len() as u64;
                                seq += 1;
                                if outbound
                                    .send(binary_frame(
                                        TerminalOpcode::Output,
                                        stream_id,
                                        seq,
                                        chunk.bytes,
                                    ))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Ok(_) | Err(StreamError::OutOfRange { .. }) => {
                                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                            }
                            Err(StreamError::UnknownStream) => return,
                        }
                    }
                });
                self.streams
                    .insert(stream_id, HeadlessStreamEntry { task_id, forwarder });
            }
            TerminalOpcode::Unsubscribe => {
                if let Some(entry) = self.streams.remove(&frame.stream_id) {
                    entry.forwarder.abort();
                }
            }
            TerminalOpcode::Input => {
                if let Some(entry) = self.streams.get(&frame.stream_id) {
                    let _ = self.executor.input(&entry.task_id, &frame.payload);
                }
            }
            TerminalOpcode::Resize => {
                if let Some(entry) = self.streams.get(&frame.stream_id) {
                    if let Ok(meta) = serde_json::from_slice::<Value>(&frame.payload) {
                        if let (Some(cols), Some(rows)) = (
                            meta.get("cols").and_then(Value::as_u64),
                            meta.get("rows").and_then(Value::as_u64),
                        ) {
                            let cols = cols.min(u16::MAX as u64) as u16;
                            let rows = rows.min(u16::MAX as u64) as u16;
                            if self.executor.resize(&entry.task_id, cols, rows).is_ok() {
                                let _ = self.outbound.send(binary_frame(
                                    TerminalOpcode::Resized,
                                    frame.stream_id,
                                    0,
                                    serde_json::to_vec(&json!({ "cols": cols, "rows": rows }))
                                        .unwrap_or_default(),
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn abort_all(&mut self) {
        for (_, entry) in self.streams.drain() {
            entry.forwarder.abort();
        }
    }
}

/// headless serve 的数据目录路径（审计落点）。
pub fn audit_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("remote")
}

/// `remote.*` 方法注册（P8-03 刀五）：native 客户端经 UDS 面操作远程 plane，
/// 与桌面 Tauri 命令共用同一 core 控制面（control.rs）。调用方持有
/// `Arc<RemoteState>` 与 handle（桌面=RemoteHostBridge，cli=CliRemoteHandle）。
pub fn register_remote_services(
    set: &mut crate::dispatcher::ServiceSet,
    state: std::sync::Arc<aeroric_core::remote::state::RemoteState>,
    handle: std::sync::Arc<dyn RemoteHostHandle>,
) {
    use crate::dispatcher::failure;
    use aeroric_core::remote::control;
    use aeroric_core::remote::identity::lan_addresses;
    use aeroric_protocol::envelope::Request;
    use aeroric_protocol::error::ErrorCode;

    fn param_str(req: &Request, key: &str) -> Result<String, String> {
        req.params
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("Missing param: {key}"))
    }

    let s = std::sync::Arc::clone(&state);
    set.register(
        "remote.status",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            Box::pin(async move { crate::services::ok(&req, control::current_status(&s).await) })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.start",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let h = std::sync::Arc::clone(&h);
            let id = req.id.clone();
            Box::pin(async move {
                let port = req
                    .params
                    .get("port")
                    .and_then(Value::as_u64)
                    .map(|p| p as u16);
                {
                    // enabled/port 落盘 + 拉起（事件桥由桌面宿主 wrapper 附加；
                    // 无头面无事件桥可附，行为一致：core start 只管 TCP）。
                    let mut cfg = s.config.lock();
                    cfg.enabled = true;
                    if let Some(p) = port {
                        cfg.port = p;
                    }
                    if let Err(e) = s.persist_config(&cfg) {
                        return failure(id, ErrorCode::IoError, &e);
                    }
                }
                match aeroric_core::remote::state::ensure_started(&s, &h).await {
                    Ok(_bound) => crate::services::ok(&req, control::current_status(&s).await),
                    Err(e) => failure(id, ErrorCode::IoError, &e),
                }
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    set.register(
        "remote.stop",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let id = req.id.clone();
            Box::pin(async move {
                {
                    let mut cfg = s.config.lock();
                    cfg.enabled = false;
                    if let Err(e) = s.persist_config(&cfg) {
                        return failure(id, ErrorCode::IoError, &e);
                    }
                }
                aeroric_core::remote::state::ensure_stopped(&s).await;
                crate::services::ok(&req, control::current_status(&s).await)
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.updateConfig",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let h = std::sync::Arc::clone(&h);
            let id = req.id.clone();
            Box::pin(async move {
                match control::update_config(
                    &s,
                    &h,
                    req.params
                        .get("relayUrl")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    req.params
                        .get("relayToken")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    req.params
                        .get("publicEndpoints")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        }),
                )
                .await
                {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(id, ErrorCode::InvalidArgument, &e),
                }
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    set.register(
        "remote.selectLanIp",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let id = req.id.clone();
            Box::pin(async move {
                let lan_ip = match param_str(&req, "lanIp") {
                    Ok(v) => v,
                    Err(message) => return failure(id, ErrorCode::InvalidArgument, &message),
                };
                match control::select_lan_ip(&s, lan_ip).await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(id, ErrorCode::InvalidArgument, &e),
                }
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.createInvite",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let h = std::sync::Arc::clone(&h);
            let id = req.id.clone();
            Box::pin(async move {
                let _lifecycle = s.lifecycle.lock().await;
                match control::create_invite_for_addresses(&s, &h, &lan_addresses()).await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(id, ErrorCode::IoError, &e),
                }
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    set.register(
        "remote.listDevices",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            Box::pin(async move {
                match control::list_devices(&s) {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(req.id.clone(), ErrorCode::IoError, &e),
                }
            })
        }),
    );
    let s = std::sync::Arc::clone(&state);
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.revokeDevice",
        Arc::new(move |req: Request, _call| {
            let s = std::sync::Arc::clone(&s);
            let h = std::sync::Arc::clone(&h);
            let id = req.id.clone();
            Box::pin(async move {
                let device_id = match param_str(&req, "deviceId") {
                    Ok(v) => v,
                    Err(e) => return failure(id, ErrorCode::InvalidArgument, &e),
                };
                let _lifecycle = s.lifecycle.lock().await;
                match control::revoke_device(&s, &h, &device_id).await {
                    Ok(()) => crate::services::ok(&req, json!({ "ok": true })),
                    Err(e) => failure(id, ErrorCode::NotFound, &e),
                }
            })
        }),
    );

    // ── AP6:推送面(桌面/CLI 原生客户端经 UDS 直接操作自己的推送登记) ──
    // 这些是**本地**设置面(与 `remote.*` 同族):手机侧对应的入口是加密通道内的
    // `push.register`,两者落在同一份 `RemoteState::push_bindings` 与同一个派发器上。
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.pushStatus",
        Arc::new(move |req: Request, _call| {
            let h = std::sync::Arc::clone(&h);
            Box::pin(async move {
                match h.push_status().await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(req.id.clone(), ErrorCode::IoError, &e),
                }
            })
        }),
    );
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.pushRegister",
        Arc::new(move |req: Request, _call| {
            let h = std::sync::Arc::clone(&h);
            Box::pin(async move {
                if let Err(message) = param_str(&req, "deviceId") {
                    return failure(req.id.clone(), ErrorCode::InvalidArgument, &message);
                }
                match h.push_register(req.params.clone()).await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(req.id.clone(), ErrorCode::InvalidArgument, &e),
                }
            })
        }),
    );
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.pushTest",
        Arc::new(move |req: Request, _call| {
            let h = std::sync::Arc::clone(&h);
            Box::pin(async move {
                match h.push_test().await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(req.id.clone(), ErrorCode::InvalidArgument, &e),
                }
            })
        }),
    );
    let h = std::sync::Arc::clone(&handle);
    set.register(
        "remote.relayDiagnostics",
        Arc::new(move |req: Request, _call| {
            let h = std::sync::Arc::clone(&h);
            Box::pin(async move {
                match h.relay_diagnostics().await {
                    Ok(v) => crate::services::ok(&req, v),
                    Err(e) => failure(req.id.clone(), ErrorCode::IoError, &e),
                }
            })
        }),
    );
}
