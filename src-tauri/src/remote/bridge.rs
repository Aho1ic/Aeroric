//! 宿主侧 [`RemoteHostHandle`] 实现：把 core 的服务注入面接到
//! AppHandle/TaskManager/storage。方法体自宿主 rpc.rs/tasks_rpc.rs 等
//! 原位保留（P8-03 刀三b：core 只拿 trait 对象，桌面端在这里落地）。

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use async_trait::async_trait;

use aeroric_core::remote::services::{RemoteHostHandle, RemoteTerminalStreams};
use aeroric_core::remote::state::{ListenerScope, OutboundSender, RemoteState, ServerHandle};

use super::{agent_config_rpc, audit, files_rpc, session_push, tasks_rpc, terminal_streams};

/// 桌面端的 handle：持有 AppHandle，全部方法转发到原位保留的宿主实现。
pub struct RemoteHostBridge<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> RemoteHostBridge<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

#[async_trait]
impl<R: Runtime> RemoteHostHandle for RemoteHostBridge<R> {
    fn desktop_version(&self) -> String {
        self.app.package_info().version.to_string()
    }

    fn emit(&self, event: &str, payload: Value) {
        let _ = self.app.emit(event, payload);
    }

    fn audit_log(&self, event: &str, payload: Value) {
        audit::log(event, payload);
    }

    async fn load_projects(&self) -> Result<Value, String> {
        let projects = tauri::async_runtime::spawn_blocking(crate::storage::load_projects)
            .await
            .map_err(|e| e.to_string())??;
        serde_json::to_value(projects).map_err(|e| e.to_string())
    }

    async fn set_project_pinned(&self, project_id: &str, pinned: bool) -> Result<(), String> {
        let project_id = project_id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            crate::storage::update_projects(|projects| {
                let project = projects
                    .iter_mut()
                    .find(|p| p.id == project_id)
                    .ok_or_else(|| format!("Project not found: {project_id}"))?;
                project.pinned = pinned;
                Ok(())
            })
            .map(|_| ())
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(())
    }

    async fn load_project_tasks(&self, project_id: &str) -> Result<Value, String> {
        let project_id = project_id.to_string();
        let tasks = tauri::async_runtime::spawn_blocking(move || {
            crate::storage::load_project_tasks(project_id)
        })
        .await
        .map_err(|e| e.to_string())??;
        serde_json::to_value(tasks).map_err(|e| e.to_string())
    }

    async fn task_input(&self, task_id: &str, text: &str) -> Result<Value, String> {
        tasks_rpc::task_input(&self.app, task_id, text).await
    }

    async fn task_respond(&self, _task_id: &str, params: Value) -> Result<Value, String> {
        tasks_rpc::task_respond(&self.app, params).await
    }

    async fn task_cancel(&self, project_id: &str, task_id: &str) -> Result<Value, String> {
        let params = json!({ "projectId": project_id, "taskId": task_id });
        tasks_rpc::task_cancel(&self.app, params).await
    }

    async fn task_complete(&self, project_id: &str, task_id: &str) -> Result<Value, String> {
        let params = json!({ "projectId": project_id, "taskId": task_id });
        tasks_rpc::task_complete(&self.app, params).await
    }

    async fn task_request(&self, payload: Value) -> Result<Value, String> {
        tasks_rpc::execute_task_request(&self.app, payload).await
    }

    async fn session_messages(&self, params: Value) -> Result<Value, String> {
        session_push::session_messages(params).await
    }

    async fn project_files(&self, params: Value) -> Result<Value, String> {
        files_rpc::project_files(params).await
    }

    async fn project_read_file(&self, params: Value) -> Result<Value, String> {
        files_rpc::project_read_file(params).await
    }

    async fn project_write_file(&self, params: Value) -> Result<Value, String> {
        files_rpc::project_write_file(params).await
    }

    async fn git_changes(&self, params: Value) -> Result<Value, String> {
        files_rpc::git_changes(params).await
    }

    async fn git_diff(&self, params: Value) -> Result<Value, String> {
        files_rpc::git_diff(params).await
    }

    async fn agent_config_list(&self) -> Result<Value, String> {
        agent_config_rpc::agent_config_list().await
    }

    async fn agent_config_save(&self, params: Value) -> Result<Value, String> {
        agent_config_rpc::agent_config_save(&self.app, params).await
    }

    async fn agent_config_detect_models(&self, params: Value) -> Result<Value, String> {
        agent_config_rpc::agent_config_detect_models(params).await
    }

    async fn agent_config_create(&self, params: Value) -> Result<Value, String> {
        agent_config_rpc::agent_config_create(&self.app, params).await
    }

    async fn agents_list(&self) -> Result<Value, String> {
        tasks_rpc::agents_list().await
    }

    async fn agents_models(&self, agent: &str) -> Result<Value, String> {
        tasks_rpc::agents_models(&json!({ "agent": agent })).await
    }

    async fn stats_summary(&self) -> Result<Value, String> {
        tasks_rpc::stats_summary().await
    }

    fn terminal_streams(&self, outbound: OutboundSender) -> Box<dyn RemoteTerminalStreams> {
        Box::new(terminal_streams::TerminalStreams::new(
            self.app.clone(),
            outbound,
        ))
    }

    // ── AP6 推送平面 ──
    //
    // 这是**待退休**的 Tauri 宿主(AP10 才清退),而 AP6 的推送面是 Apple-only
    // 原生的能力。这里不给「假装能推」的空实现:四个方法都明确报「本宿主形态不
    // 提供推送」,客户端拿到的是能力不存在而不是静默失败。桌面端要配置推送时走
    // macOS 原生客户端的 `remote.push*`(与这里同一份 core 实现)。
    async fn push_register(&self, _params: Value) -> Result<Value, String> {
        Err("the Tauri host does not expose the AP6 push plane; use the native macOS client"
            .to_string())
    }

    async fn push_status(&self) -> Result<Value, String> {
        Ok(json!({
            "enabled": false,
            "wired": false,
            "devices": [],
            "detail": "the Tauri host does not expose the AP6 push plane",
        }))
    }

    async fn push_test(&self) -> Result<Value, String> {
        Err("the Tauri host does not expose the AP6 push plane; use the native macOS client"
            .to_string())
    }

    async fn relay_diagnostics(&self) -> Result<Value, String> {
        Ok(json!({
            "relay": {
                "url": "",
                "schemeOk": true,
                "reachable": false,
                "authenticated": false,
                "detail": "relay diagnostics are only wired in the native macOS client",
            },
            "defaultRelay": Value::Null,
            "note": "Aeroric ships no default relay; your Mac and phone must use your own relay or a trusted direct network",
        }))
    }

    /// 撤销时停推:这台宿主没有推送面,所以只需如实回报,不假装做过。
    async fn device_revoked(&self, _device_id: &str) -> Result<Value, String> {
        Ok(json!({ "push": "the Tauri host does not expose the AP6 push plane" }))
    }
}

/// 宿主启动 wrapper：挂事件桥（listen_any）→ core start_with_scope →
/// 注入 post_shutdown 钩子（unlisten 桥）→ 按配置 spawn relay 出站连接。
pub(crate) async fn start_with_host_services<R: Runtime>(
    app: &AppHandle<R>,
    port: u16,
    scope: ListenerScope,
) -> Result<ServerHandle, String> {
    use tauri::Listener;

    let state = app.state::<Arc<RemoteState>>().inner().clone();
    let bridge: Arc<dyn RemoteHostHandle> = Arc::new(RemoteHostBridge::new(app.clone()));
    let listener_ids = super::events_bridge::attach(app);

    let server = aeroric_core::remote::server::start_with_scope(state, bridge, port, scope).await?;

    let mut server = server;
    let unlisten_app = app.clone();
    let ids = listener_ids.clone();
    server.set_post_shutdown(Box::new(move || {
        for id in &ids {
            unlisten_app.unlisten(*id);
        }
    }));

    // relay 出站注册与 WS 服务同生命周期(共享 shutdown)
    let (relay_url, relay_token) = {
        let managed = app.state::<Arc<RemoteState>>();
        let cfg = managed.config.lock();
        (cfg.relay_url.clone(), cfg.relay_token.clone())
    };
    if let Some(relay_url) = relay_url.filter(|u| !u.trim().is_empty()) {
        match aeroric_core::remote::config::normalize_relay_url(&relay_url) {
            Ok(relay_url)
                if relay_token
                    .as_deref()
                    .is_some_and(|token| !token.trim().is_empty()) =>
            {
                super::relay_client::spawn(
                    app.clone(),
                    relay_url,
                    relay_token,
                    server.shutdown_sender(),
                );
            }
            Ok(_) => {
                *app.state::<Arc<RemoteState>>().relay_state.lock() =
                    "error:relay token is required".to_string();
            }
            Err(error) => {
                *app.state::<Arc<RemoteState>>().relay_state.lock() = format!("error:{error}");
            }
        }
    }
    Ok(server)
}
