//! 无头远程服务面（P8-03 刀四）：`aeroric-cli serve` 的
//! [`RemoteHostHandle`](aeroric_core::remote::services::RemoteHostHandle)
//! 实现——任务执行经 CLI 已装配的 [`TaskService`]/[`CliPtyExecutor`]，
//! 存储面经 [`ProjectService`] 与 TaskService 的记录读取。
//!
//! 窄面原则与 `services.rs` 一致：移动端所需的方法给真实实现；headless
//! 不具备的能力（桌面 UI 事件、dsh 专用面）显式报错而不是猜。

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, ProjectService};
use aeroric_core::remote::services::{RemoteHostHandle, RemoteTerminalStreams};
use aeroric_core::remote::state::OutboundSender;
use aeroric_core::tasks::executor::HostWindowFacts;
use aeroric_core::tasks::{Attachments, NewTask, TaskService};

use crate::executor::CliPtyExecutor;

pub struct CliRemoteHandle {
    pub service: Arc<TaskService>,
    pub context: Arc<CoreContext>,
    pub executor: Arc<CliPtyExecutor>,
}

impl CliRemoteHandle {
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

    async fn task_respond(&self, task_id: &str, _params: Value) -> Result<Value, String> {
        // 审批放行走桌面的键位面；无头不认识协议按键（P3-07 验收行禁猜）。
        Err(format!(
            "task.respond is not served by the headless remote plane (task {task_id})"
        ))
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

    async fn session_messages(&self, _params: Value) -> Result<Value, String> {
        // 会话读取面随 P3-04 进 core，headless 接线在 P8-04 补（窄面先显式报错）。
        Err("session.messages is not served by the headless remote plane yet".to_string())
    }

    async fn project_files(&self, _params: Value) -> Result<Value, String> {
        Err("project.files is not served by the headless remote plane yet".to_string())
    }

    async fn project_read_file(&self, _params: Value) -> Result<Value, String> {
        Err("project.readFile is not served by the headless remote plane yet".to_string())
    }

    async fn project_write_file(&self, _params: Value) -> Result<Value, String> {
        Err("project.writeFile is not served by the headless remote plane yet".to_string())
    }

    async fn git_changes(&self, _params: Value) -> Result<Value, String> {
        Err("git.changes is not served by the headless remote plane yet".to_string())
    }

    async fn git_diff(&self, _params: Value) -> Result<Value, String> {
        Err("git.diff is not served by the headless remote plane yet".to_string())
    }

    async fn agent_config_list(&self) -> Result<Value, String> {
        Err("agentConfig.list is not served by the headless remote plane yet".to_string())
    }

    async fn agent_config_save(&self, _params: Value) -> Result<Value, String> {
        Err("agentConfig.save is not served by the headless remote plane yet".to_string())
    }

    async fn agent_config_detect_models(&self, _params: Value) -> Result<Value, String> {
        Err("agentConfig.detectModels is not served by the headless remote plane yet".to_string())
    }

    async fn agent_config_create(&self, _params: Value) -> Result<Value, String> {
        Err("agentConfig.create is not served by the headless remote plane yet".to_string())
    }

    async fn agents_list(&self) -> Result<Value, String> {
        // 家族判定面在 core families；无头给最小内置表（与 host families 同源）。
        Ok(json!([
            { "value": "claude", "family": "claude" },
            { "value": "codex", "family": "codex" },
            { "value": "dsh", "family": "dsh" },
            { "value": "omp", "family": "omp" }
        ]))
    }

    async fn agents_models(&self, _agent: &str) -> Result<Value, String> {
        Ok(json!({ "models": [] }))
    }

    async fn stats_summary(&self) -> Result<Value, String> {
        Ok(
            json!({ "totalAgentsSpawned": 0, "agentTimeApproximate": true, "totalPRsCreated": Value::Null }),
        )
    }

    fn terminal_streams(&self, _outbound: OutboundSender) -> Box<dyn RemoteTerminalStreams> {
        struct NoStreams;
        impl RemoteTerminalStreams for NoStreams {
            fn handle_frame(&mut self, _bytes: &[u8]) {}
            fn abort_all(&mut self) {}
        }
        Box::new(NoStreams)
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
                    if let Err(e) = aeroric_core::remote::config::save_config(&cfg) {
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
                    if let Err(e) = aeroric_core::remote::config::save_config(&cfg) {
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
}
