//! 维护面与用量族的 UDS 服务（Task P8-04 刀三）。
//!
//! §3.1 #8：每个新 core 服务都要同时可达 CLI 与 GUI，所以这些方法注册进宿主
//! dispatcher，而不是只挂在 Tauri 命令上。CLI 经 `rpc <method>` 走同一条路。
//!
//! 三组方法的注入需求不同，按需要给端口而不是给一整个窗口句柄：
//!
//! - `usage.*` / `diagnostics.*`：只认数据根与一个进程句柄槽（`usage.read_usage_snapshot`
//!   需要一个 `codex app-server` 客户端槽；无头宿主给 `None` 槽就等于"没有 Codex
//!   订阅"，是明确的 `Unavailable` 而不是错误）。
//! - `maintenance.*`：网络抓取/下载/摘要都在宿主，core 只给编排——没有 `MaintenanceHost`
//!   时这些方法**不注册**（`is_registered` 为假，客户端拿到 method-not-found 而不是
//!   一个会假装工作的空壳）。
//! - 平台权限面（`permission.*`）**故意不在本文件**：它们是 `host_local_os_ui`，
//!   必须由持有当前 app 身份的 GUI 适配器执行（计划 Action 3/4），
//!   `command-map.json` 里这六条的 `rpc_method` 是 `null`。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::maintenance::{MaintenanceHost, MaintenanceService};
use aeroric_core::metrics::{self, MetricsPaths};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::Value;

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

/// 无头宿主的降级提示（注册成功但缺该能力时回它，而不是 panic）。
const NO_CODEX_SLOT: &str = "This host does not hold a Codex app-server session; the subscription usage snapshot is unavailable here.";

fn param_str(request: &Request, key: &str) -> Result<String, Box<Frame>> {
    request
        .params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("Missing param: {key}"),
            ))
        })
}

fn param_int(request: &Request, key: &str, default: i64) -> i64 {
    request
        .params
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(default)
}

fn param_bool(request: &Request, key: &str, default: bool) -> bool {
    request
        .params
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

/// 把 `Result` 变成帧：成功走 `ok`，失败按 invalid-argument 回（不断连、不 panic）。
fn result_frame<T: serde::Serialize>(request: &Request, outcome: Result<T, String>) -> Frame {
    match outcome {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => ok(request, value),
            Err(error) => failure(
                request.id.clone(),
                ErrorCode::IoError,
                &format!("cannot serialize reply: {error}"),
            ),
        },
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// `usage.*` 六方法。`codex_rpc` 是无头宿主里那个可以为空的进程句柄槽。
pub fn register_usage_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    codex_rpc: Arc<parking_lot::Mutex<Option<aeroric_core::metrics::usage::CodexRpcClient>>>,
) {
    {
        let context = Arc::clone(&context);
        set.register(
            "usage.read_session_metrics",
            Arc::new(move |request: Request, _call| {
                let _ = &context;
                Box::pin(async move {
                    let path = match param_str(&request, "sessionPath") {
                        Ok(path) => path,
                        Err(frame) => return *frame,
                    };
                    let id = request.id.clone();
                    match metrics::read_session_metrics_at(&path).await {
                        Ok(value) => match serde_json::to_value(value) {
                            Ok(value) => ok(&request, value),
                            Err(error) => failure(
                                id,
                                ErrorCode::IoError,
                                &format!("cannot serialize reply: {error}"),
                            ),
                        },
                        Err(error) => failure(id, ErrorCode::InvalidArgument, &error),
                    }
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "usage.read_usage_statistics",
            Arc::new(move |request: Request, _call| {
                let paths = MetricsPaths::from_context(&context);
                Box::pin(async move {
                    let range_days =
                        param_int(&request, "rangeDays", 7).clamp(0, u32::MAX as i64) as u32;
                    let agent = request
                        .params
                        .get("agent")
                        .and_then(Value::as_str)
                        .unwrap_or("all")
                        .to_string();
                    result_frame(
                        &request,
                        metrics::read_usage_statistics_at(&paths, range_days, agent),
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "usage.refresh_usage_statistics_index",
            Arc::new(move |request: Request, _call| {
                let paths = MetricsPaths::from_context(&context);
                Box::pin(async move {
                    // 无头宿主没有事件出口：刷新本身是命令的语义，事件是桌面端的加法。
                    let sink: metrics::IndexEventSink = Arc::new(|_updated_at| {});
                    match metrics::index::refresh_usage_statistics_index_at(paths, sink).await {
                        Ok(changed) => ok(&request, serde_json::json!(changed)),
                        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                    }
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "usage.record_agent_config_usage",
            Arc::new(move |request: Request, _call| {
                let paths = MetricsPaths::from_context(&context);
                Box::pin(async move {
                    let agent = match param_str(&request, "agent") {
                        Ok(agent) => agent,
                        Err(frame) => return *frame,
                    };
                    result_frame(
                        &request,
                        metrics::record_agent_config_usage_at(&paths, &agent),
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "usage.load_agent_usage_snapshot",
            Arc::new(move |request: Request, _call| {
                let paths = MetricsPaths::from_context(&context);
                Box::pin(async move {
                    result_frame(&request, metrics::load_agent_usage_snapshot_at(&paths))
                })
            }),
        );
    }
    {
        set.register(
            "usage.read_usage_snapshot",
            Arc::new(move |request: Request, _call| {
                let slot = Arc::clone(&codex_rpc);
                Box::pin(async move {
                    // 槽为空＝这个宿主没有 Codex 会话。快照里 Codex 半边给
                    // `Unavailable`（Claude 半边仍可读）——不是错误，是事实。
                    if slot.lock().is_none() && !command_exists("codex") {
                        return ok(
                            &request,
                            serde_json::json!({
                                "claude": { "status": "unavailable", "reason": NO_CODEX_SLOT },
                                "codex": { "status": "unavailable", "reason": NO_CODEX_SLOT },
                                "fetchedAt": aeroric_core::clock::now_ms_i64() / 1000,
                            }),
                        );
                    }
                    match metrics::read_usage_snapshot(slot).await {
                        Ok(snapshot) => match serde_json::to_value(snapshot) {
                            Ok(value) => ok(&request, value),
                            Err(error) => failure(
                                request.id.clone(),
                                ErrorCode::IoError,
                                &format!("cannot serialize reply: {error}"),
                            ),
                        },
                        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                    }
                })
            }),
        );
    }
}

/// `codex` 在不在 PATH 上（只用于决定"要不要试"；真起进程的失败另有明确 reason）。
fn command_exists(program: &str) -> bool {
    !aeroric_core::platform::detect_path(program).is_empty()
}

/// `diagnostics.list_startup_degradations`。空数组＝一切正常（旧语义）。
pub fn register_diagnostics_services(
    set: &mut ServiceSet,
    log: Arc<aeroric_core::maintenance::StartupDegradationLog>,
) {
    set.register(
        "diagnostics.list_startup_degradations",
        Arc::new(move |request: Request, _call| {
            let log = Arc::clone(&log);
            Box::pin(async move { ok(&request, serde_json::json!(log.list())) })
        }),
    );
}

/// `maintenance.*` 九方法（通知与更新）。
///
/// 调用方必须先给出 [`MaintenanceHost`]：没有它就**不注册**这些方法——注册了也只会
/// 返回"这个宿主做不到"，而那会让客户端以为能力存在。
pub fn register_maintenance_services(set: &mut ServiceSet, service: Arc<MaintenanceService>) {
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.get_notifications",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let force = param_bool(&request, "force", false);
                    match service.get_notifications(force).await {
                        Ok(result) => match serde_json::to_value(result) {
                            Ok(value) => ok(&request, value),
                            Err(error) => failure(
                                request.id.clone(),
                                ErrorCode::IoError,
                                &format!("cannot serialize reply: {error}"),
                            ),
                        },
                        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                    }
                })
            }),
        );
    }
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.mark_notification_read",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let id = match param_str(&request, "id") {
                        Ok(id) => id,
                        Err(frame) => return *frame,
                    };
                    result_frame(&request, service.mark_notification_read(&id))
                })
            }),
        );
    }
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.mark_all_notifications_read",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(
                    async move { result_frame(&request, service.mark_all_notifications_read()) },
                )
            }),
        );
    }
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.get_pending_release_update",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let pending = service.pending_update();
                    let tag = request.params.get("tagName").and_then(Value::as_str);
                    let filtered = match (pending, tag) {
                        (Some(pending), Some(tag)) if pending.tag_name != tag => None,
                        (pending, _) => pending,
                    };
                    ok(&request, serde_json::json!(filtered))
                })
            }),
        );
    }
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.prepare_release_update",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let tag = match param_str(&request, "tagName") {
                        Ok(tag) => tag,
                        Err(frame) => return *frame,
                    };
                    match service.prepare_release_update(&tag).await {
                        Ok(result) => match serde_json::to_value(result) {
                            Ok(value) => ok(&request, value),
                            Err(error) => failure(
                                request.id.clone(),
                                ErrorCode::IoError,
                                &format!("cannot serialize reply: {error}"),
                            ),
                        },
                        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                    }
                })
            }),
        );
    }
    // `maintenance.install_release_update` / `restart_and_install_release_update`：真替换
    // 正在运行的 app 属于 GUI 适配器（Action 5 的准备与替换分离），无头宿主明确拒绝。
    for method in [
        "maintenance.install_release_update",
        "maintenance.restart_and_install_release_update",
    ] {
        set.register(
            method,
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    failure(
                        request.id.clone(),
                        ErrorCode::UnsupportedCapability,
                        "Replacing the running application is a GUI-adapter action; this host prepares updates and stops there.",
                    )
                })
            }),
        );
    }
    {
        let service = Arc::clone(&service);
        set.register(
            "maintenance.generate_weekly_report",
            Arc::new(move |request: Request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let payload = request
                        .params
                        .get("request")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let payload = if payload.is_null() {
                        request.params.clone()
                    } else {
                        payload
                    };
                    let report = match serde_json::from_value::<
                        aeroric_core::maintenance::WeeklyReportRequest,
                    >(payload)
                    {
                        Ok(report) => report,
                        Err(error) => {
                            return failure(
                                request.id.clone(),
                                ErrorCode::InvalidArgument,
                                &format!("Invalid weekly report request: {error}"),
                            )
                        }
                    };
                    match service
                        .write_weekly_report(report, &aeroric_core::maintenance::NullProgress)
                        .await
                    {
                        Ok(path) => ok(&request, serde_json::json!(path)),
                        Err(error) => failure(
                            request.id.clone(),
                            ErrorCode::InvalidArgument,
                            &error.to_string(),
                        ),
                    }
                })
            }),
        );
    }
}

/// 装配 helper：给定宿主端口就注册 `maintenance.*`（没给就跳过，见函数文档）。
pub fn register_if_hosted(
    set: &mut ServiceSet,
    host: Option<Arc<dyn MaintenanceHost>>,
    data_dir: &std::path::Path,
) -> bool {
    match host {
        Some(host) => {
            let service = Arc::new(MaintenanceService::new(host, data_dir.to_path_buf()));
            register_maintenance_services(set, service);
            true
        }
        None => false,
    }
}
