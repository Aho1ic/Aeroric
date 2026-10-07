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
//! 3. **单字段更新**：读 → 改一个键 → 写回，**保留其余键**；改完返回整份（客户端据此
//!    刷新界面，与 Tauri 侧 `update_*` 返回 `AppSettings` 同形）。
//!
//! 单字段那一族用一张表驱动（[`FieldUpdate`]）：新增一个"update 某字段"的方法只需要在
//! 表里加一行，而"读—改—写"的三步顺序、锁、返回值形状只有一处实现。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{Map, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

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
        param: "shiftEnterNewline",
        key: "terminal_shift_enter_newline",
    },
];

pub fn register_settings_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.load_app_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { settings_load(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.save_app_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { settings_save(request, context) })
            }),
        );
    }
    // 单字段更新逐个**字面量**注册。
    //
    // 不用 `for update in FIELD_UPDATES { set.register(update.method, …) }`：那让方法名成了
    // 运行期才能知道的字符串，而本仓的扫描器（`host-coverage.mjs` 与 extraction）靠源码里的
    // 字面量找注册点——"扫描口径找不到"会被读成"没注册"。表仍然留着（它管参数名与键名的
    // 对应），注册点则显式写出来。
    for update in FIELD_UPDATES {
        let _ = update; // 表由下面六条显式注册消费；这里只做"表与注册点同步"的锚
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.update_proxy_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(request, context, "proxySettings", "proxy_settings")
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.update_notebook_embedding_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(
                        request,
                        context,
                        "notebookEmbeddingSettings",
                        "notebook_embedding_settings",
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.update_auto_cleanup_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(
                        request,
                        context,
                        "autoCleanupSettings",
                        "auto_cleanup_settings",
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.update_weekly_report_settings",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(
                        request,
                        context,
                        "weeklyReportSettings",
                        "weekly_report_settings",
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.save_send_shortcut",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(request, context, "sendShortcut", "send_shortcut")
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.save_shift_enter_newline",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    settings_update(
                        request,
                        context,
                        "shiftEnterNewline",
                        "terminal_shift_enter_newline",
                    )
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "settings.update_dsh_reasoning_effort",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { settings_update_dsh_reasoning_effort(request, context) })
            }),
        );
    }
}

/// `settings.update_dsh_reasoning_effort`：只接受内置 dsh 与 off/high/max；
/// 值原样写进 `dsh_reasoning_efforts` 对象，其余键（含第三方键）不动。
fn settings_update_dsh_reasoning_effort(request: Request, context: Arc<CoreContext>) -> Frame {
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
    let Some(effort) = request
        .params
        .get("effort")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: effort",
        );
    };
    if agent != "dsh" {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Reasoning effort is only configurable for the built-in DeepSeek Harness agent",
        );
    }
    let effort = effort.trim().to_ascii_lowercase();
    match effort.as_str() {
        "off" | "high" | "max" => {}
        _ => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "Invalid DeepSeek Harness reasoning effort",
            );
        }
    }
    let mut fields: Map<String, Value> =
        match aeroric_core::storage::settings::load_settings(&context) {
            Ok(outcome) => outcome.found().unwrap_or_default(),
            Err(error) => {
                return failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
            }
        };
    let mut reasoning = fields
        .remove("dsh_reasoning_efforts")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    reasoning.insert(agent, Value::String(effort));
    fields.insert(
        "dsh_reasoning_efforts".to_string(),
        Value::Object(reasoning),
    );
    match aeroric_core::storage::settings::save_settings(&context, &fields) {
        Ok(()) => ok(&request, Value::Object(fields)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
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

/// 单字段更新：读 → 改一个键 → 写回，其余键原样保留（**这是本族唯一允许"改动整份文件"
/// 的地方，所以"保留其余键"必须有用例**）。
fn settings_update(
    request: Request,
    context: Arc<CoreContext>,
    param: &'static str,
    key: &'static str,
) -> Frame {
    let Some(value) = request.params.get(param).cloned() else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("Missing param: {param}"),
        );
    };
    let mut fields: Map<String, Value> =
        match aeroric_core::storage::settings::load_settings(&context) {
            Ok(outcome) => outcome.found().unwrap_or_default(),
            Err(error) => {
                // 坏文件**不覆盖**：拒绝这次更新，让用户先去修它（与 core 的同一条规矩）。
                return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
            }
        };
    fields.insert(key.to_string(), value);
    match aeroric_core::storage::settings::save_settings(&context, &fields) {
        Ok(()) => ok(&request, Value::Object(fields)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}
