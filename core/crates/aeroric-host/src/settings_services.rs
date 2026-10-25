//! 设置面的宿主服务（Task P8-05 切片九）：`settings.*` 的读、整份写与单字段更新。
//!
//! 这一族的形状与它处不同，值得先说清：`settings.json` 在 core 里是**一整份 map**（
//! `storage::settings::load_settings` 返回 `Map<String, Value>`，不套 `AppSettings`
//! 结构体）。这样做的理由在 core 的模块头写着——旧客户端把"读不出"当成"没有设置"，
//! 下一次保存就用默认值把用户的文件盖掉；只在核心持有原始 map 才可能保住不认识的键。
//!
//! 因此宿主这一层只做三件事：
//!
//! 1. **读**：把 map 原样返回（缺文件＝`{}`，不是错误）。
//! 2. **整份写**：调用方给什么写什么（`save_settings` 会经 `require_writer` 拒只读）。
//! 3. **定向更新**：core 事务保留无关原始键（proxy 明确消费旧字段）；改完返回整份，
//!    与 Tauri 侧 `update_*` 返回的 `AppSettings` 同形。
//!
//! 切片十五至十八：七项纯偏好先按真实 typed wire 解码，再用 core 统一 normalization；
//! 所有 map 更新都使用 core 的整段 RMW 事务，不能在 host 另做 load→save。
//! [`FieldUpdate`] 记录参数/键，注册点仍显式列出，并有真调用测试交叉核对。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::Value;

use crate::dispatcher::{failure, ServiceSet};
use crate::local_targets::local_target_error;
use crate::services::ok;

const UNSUPPORTED_PROJECT_CONFIG_TARGET: &str =
    "Project-config target is not hosted: SSH/WSL configuration IO is unavailable";

/// 一个"更新单个设置字段"的方法。
pub struct FieldUpdate {
    pub method: &'static str,
    /// 请求里的参数名（camelCase，与 UDS 面其余方法一致）。
    pub param: &'static str,
    /// `settings.json` 里的键名（snake_case，与 core 的 map 一致）。
    pub key: &'static str,
}

const FIELD_UPDATES: &[FieldUpdate] = &[
    FieldUpdate {
        method: "settings.update_proxy_settings",
        param: "proxySettings",
        key: "proxy_settings",
    },
    FieldUpdate {
        method: "settings.update_notebook_embedding_settings",
        param: "notebookEmbeddingSettings",
        key: "notebook_embedding_settings",
    },
    FieldUpdate {
        method: "settings.update_auto_cleanup_settings",
        param: "autoCleanupSettings",
        key: "auto_cleanup_settings",
    },
    FieldUpdate {
        method: "settings.update_weekly_report_settings",
        param: "weeklyReportSettings",
        key: "weekly_report_settings",
    },
    FieldUpdate {
        method: "settings.save_send_shortcut",
        param: "sendShortcut",
        key: "send_shortcut",
    },
    FieldUpdate {
        method: "settings.save_shift_enter_newline",
        param: "enabled",
        key: "terminal_shift_enter_newline",
    },
    FieldUpdate {
        method: "settings.save_keep_awake_while_running",
        param: "enabled",
        key: "keep_awake_while_running",
    },
];

/// Disk IO and the settings critical section stay off the async executor.
fn settings_handler(
    context: &Arc<CoreContext>,
    run: impl Fn(Request, Arc<CoreContext>) -> Frame + Send + Sync + 'static,
) -> crate::dispatcher::Handler {
    let context = Arc::clone(context);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, context)).await {
                Ok(frame) => frame,
                Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
            }
        })
    })
}

fn agent_config_handler(
    context: &Arc<CoreContext>,
    paths: &Arc<AppPaths>,
    run: fn(Request, Arc<CoreContext>, &AppPaths) -> Frame,
) -> crate::dispatcher::Handler {
    let paths = Arc::clone(paths);
    settings_handler(context, move |request, context| {
        run(request, context, &paths)
    })
}

pub fn register_settings_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    agent_config_external_home: Option<std::path::PathBuf>,
) {
    // This optional home comes from trusted composition, never request params.
    // Config defaults may use it without expanding unrelated discovery roots.
    let config_paths = Arc::new(AppPaths::with_data_dir(
        agent_config_external_home
            .as_deref()
            .unwrap_or(context.paths.user_home()),
        Some(context.paths.data_dir()),
    ));
    // Literal registrations remain machine-auditable; FIELD_UPDATES is checked
    // against this actual callable surface, not used to fabricate coverage.
    set.register(
        "settings.load_app_settings",
        settings_handler(&context, settings_load),
    );
    set.register(
        "settings.save_app_settings",
        settings_handler(&context, settings_save),
    );
    set.register(
        "settings.update_proxy_settings",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::Proxy)
        }),
    );
    set.register(
        "settings.update_notebook_embedding_settings",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::NotebookEmbedding)
        }),
    );
    set.register(
        "settings.update_auto_cleanup_settings",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::AutoCleanup)
        }),
    );
    set.register(
        "settings.update_weekly_report_settings",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::WeeklyReport)
        }),
    );
    set.register(
        "settings.save_send_shortcut",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::SendShortcut)
        }),
    );
    set.register(
        "settings.save_shift_enter_newline",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::ShiftEnterNewline)
        }),
    );
    set.register(
        "settings.save_keep_awake_while_running",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::KeepAwakeWhileRunning)
        }),
    );
    set.register(
        "settings.update_dsh_reasoning_effort",
        settings_handler(&context, |request, context| {
            settings_preference_update(request, context, PreferenceKind::DshReasoningEffort)
        }),
    );
    set.register(
        "settings.get_agent_config_file_path",
        agent_config_handler(&context, &config_paths, settings_agent_config_file_path),
    );
    set.register(
        "settings.read_agent_config_file",
        agent_config_handler(&context, &config_paths, settings_read_agent_config_file),
    );
    set.register(
        "settings.write_agent_config_file",
        agent_config_handler(&context, &config_paths, settings_write_agent_config_file),
    );
    set.register(
        "settings.read_project_config",
        settings_handler(&context, settings_read_project_config),
    );
    set.register(
        "settings.init_project_config",
        settings_handler(&context, settings_init_project_config),
    );
    set.register(
        "settings.write_project_config",
        settings_handler(&context, settings_write_project_config),
    );
}

fn settings_read_project_config(request: Request, _context: Arc<CoreContext>) -> Frame {
    let project_path: String = match preference_param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    if let Some(error) =
        local_target_error(&request, &project_path, UNSUPPORTED_PROJECT_CONFIG_TARGET)
    {
        return error;
    }
    match aeroric_core::settings::project_config::read_project_config(&project_path) {
        Ok(config) => ok(&request, serde_json::json!(config)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn settings_init_project_config(request: Request, context: Arc<CoreContext>) -> Frame {
    let project_path: String = match preference_param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    if let Some(error) =
        local_target_error(&request, &project_path, UNSUPPORTED_PROJECT_CONFIG_TARGET)
    {
        return error;
    }
    match aeroric_core::settings::project_config::init_project_config(&context, &project_path) {
        Ok(config) => ok(&request, serde_json::json!(config)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

fn settings_write_project_config(request: Request, context: Arc<CoreContext>) -> Frame {
    let project_path: String = match preference_param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    let config: aeroric_core::settings::project_config::ProjectConfig =
        match preference_param(&request, "config") {
            Ok(config) => config,
            Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
        };
    if let Some(error) =
        local_target_error(&request, &project_path, UNSUPPORTED_PROJECT_CONFIG_TARGET)
    {
        return error;
    }
    match aeroric_core::settings::project_config::write_project_config(
        &context,
        &project_path,
        &config,
    ) {
        Ok(()) => ok(&request, Value::Null),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

#[derive(Clone, Copy)]
enum PreferenceKind {
    SendShortcut,
    ShiftEnterNewline,
    KeepAwakeWhileRunning,
    AutoCleanup,
    WeeklyReport,
    NotebookEmbedding,
    DshReasoningEffort,
    Proxy,
}

fn preference_param<T: serde::de::DeserializeOwned>(
    request: &Request,
    name: &str,
) -> Result<T, String> {
    let value = request
        .params
        .get(name)
        .ok_or_else(|| format!("Missing param: {name}"))?;
    serde_json::from_value(value.clone()).map_err(|error| format!("Invalid param {name}: {error}"))
}

fn decode_preference(
    request: &Request,
    kind: PreferenceKind,
) -> Result<aeroric_core::settings::preferences::PreferenceUpdate, String> {
    use aeroric_core::settings::preferences::{DshReasoningEffort, PreferenceUpdate};
    Ok(match kind {
        PreferenceKind::SendShortcut => {
            PreferenceUpdate::SendShortcut(preference_param(request, "sendShortcut")?)
        }
        PreferenceKind::AutoCleanup => {
            PreferenceUpdate::AutoCleanup(preference_param(request, "autoCleanupSettings")?)
        }
        PreferenceKind::WeeklyReport => {
            PreferenceUpdate::WeeklyReport(preference_param(request, "weeklyReportSettings")?)
        }
        PreferenceKind::NotebookEmbedding => PreferenceUpdate::NotebookEmbedding(preference_param(
            request,
            "notebookEmbeddingSettings",
        )?),
        PreferenceKind::DshReasoningEffort => {
            let agent: String = preference_param(request, "agent")?;
            let effort: String = preference_param(request, "effort")?;
            PreferenceUpdate::DshReasoningEffort(DshReasoningEffort::for_builtin_agent(
                &agent, &effort,
            )?)
        }
        PreferenceKind::Proxy => {
            PreferenceUpdate::Proxy(preference_param(request, "proxySettings")?)
        }
        PreferenceKind::KeepAwakeWhileRunning => {
            PreferenceUpdate::KeepAwakeWhileRunning(preference_param(request, "enabled")?)
        }
        PreferenceKind::ShiftEnterNewline => {
            // `enabled` is the Tauri/frozen contract. Keep the previous host-only
            // spelling as an explicit alias, never silently choose between conflicts.
            let field = match (
                request.params.get("enabled"),
                request.params.get("shiftEnterNewline"),
            ) {
                (Some(a), Some(b)) if a != b => {
                    return Err("Invalid conflicting enabled/shiftEnterNewline params".into())
                }
                (Some(_), _) => "enabled",
                (None, Some(_)) => "shiftEnterNewline",
                (None, None) => return Err("Missing param: enabled".into()),
            };
            PreferenceUpdate::ShiftEnterNewline(preference_param(request, field)?)
        }
    })
}

fn settings_preference_update(
    request: Request,
    context: Arc<CoreContext>,
    kind: PreferenceKind,
) -> Frame {
    let update = match decode_preference(&request, kind) {
        Ok(update) => update,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::settings::preferences::update_preference(&context, update) {
        Ok(fields) => ok(&request, Value::Object(fields)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

fn load_typed_settings(
    context: &Arc<CoreContext>,
) -> Result<aeroric_core::settings::schema::AppSettings, String> {
    let fields = match aeroric_core::storage::settings::load_settings(context) {
        Ok(outcome) => outcome.found().unwrap_or_default(),
        Err(error) => return Err(error.to_string()),
    };
    // A bad typed field must not discard configured paths and select defaults.
    serde_json::from_value(Value::Object(fields))
        .map_err(|error| format!("invalid stored app settings: {error}"))
}

fn agent_config_path_from_settings(
    agent: &str,
    settings: &aeroric_core::settings::schema::AppSettings,
    paths: &AppPaths,
) -> Result<Option<std::path::PathBuf>, String> {
    aeroric_core::settings::agent_config_roots::resolve_path(
        agent,
        &aeroric_core::settings::agent_config::AgentConfigPaths::from(settings),
        paths,
    )
}

fn settings_agent_config_file_path(
    request: Request,
    context: Arc<CoreContext>,
    paths: &AppPaths,
) -> Frame {
    let Some(agent) = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: agent",
        );
    };
    let settings = match load_typed_settings(&context) {
        Ok(settings) => settings,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    };
    match agent_config_path_from_settings(&agent, &settings, paths) {
        Ok(Some(path)) => ok(&request, Value::String(path.to_string_lossy().into_owned())),
        Ok(None) => ok(&request, Value::String(String::new())),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

fn settings_read_agent_config_file(
    request: Request,
    context: Arc<CoreContext>,
    paths: &AppPaths,
) -> Frame {
    let Some(agent) = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: agent",
        );
    };
    let settings = match load_typed_settings(&context) {
        Ok(settings) => settings,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    };
    let path = match agent_config_path_from_settings(&agent, &settings, paths) {
        Ok(Some(path)) => path,
        Ok(None) => return ok(&request, Value::Null),
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::settings::agent_config_files::read(&path) {
        Ok(content) => ok(&request, content.map(Value::String).unwrap_or(Value::Null)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn settings_write_agent_config_file(
    request: Request,
    context: Arc<CoreContext>,
    paths: &AppPaths,
) -> Frame {
    let Some(agent) = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: agent",
        );
    };
    let Some(content) = request
        .params
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: content",
        );
    };
    // Authentication is not writer authority; check the current lease before
    // even loading settings or resolving a possibly external config path.
    if let Err(error) = context.require_write() {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    let settings = match load_typed_settings(&context) {
        Ok(settings) => settings,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    };
    let path = match agent_config_path_from_settings(&agent, &settings, paths) {
        Ok(Some(path)) => path,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "Agent config file path is not configured",
            );
        }
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::settings::agent_config_files::write(&context, &path, &content) {
        Ok(()) => ok(&request, Value::Null),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}
/// 表与注册点的对账：每条 `FIELD_UPDATES` 都必须有一个同名的显式注册点。
///
/// 这不是形式主义：注册点写成字面量之后，表里的方法名就没有机器在读，"加了一行表却忘了
/// 注册"会变成静默缺口。用例 `settings_field_updates_are_registered_for_every_row` 拿它
/// 做断言，而不是靠人记得。
pub fn field_update_methods() -> &'static [FieldUpdate] {
    FIELD_UPDATES
}

/// 读：缺文件与空对象等价（都是"还没写过"），**坏文件不是空对象**——core 的
/// `load_settings` 会把"读不出"报成 `Corrupt`，这里照原样透出，不降级成默认值。
fn settings_load(request: Request, context: Arc<CoreContext>) -> Frame {
    match aeroric_core::storage::settings::load_settings(&context) {
        Ok(outcome) => ok(&request, Value::Object(outcome.found().unwrap_or_default())),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

/// 整份写：请求体就是那份 map（`settings` 键或直接是对象都可以，方便 CLI 用现成文件）。
fn settings_save(request: Request, context: Arc<CoreContext>) -> Frame {
    let payload = request
        .params
        .get("settings")
        .cloned()
        .unwrap_or_else(|| request.params.clone());
    let Value::Object(fields) = payload else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "settings must be a JSON object",
        );
    };
    match aeroric_core::storage::settings::save_settings(&context, &fields) {
        Ok(()) => ok(&request, Value::Object(fields)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}
