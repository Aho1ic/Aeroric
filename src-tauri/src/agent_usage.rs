//! Agent 配置使用频次账本的宿主接线层。
//!
//! P8-04 刀二：账本（按本地日期分桶／7 天窗口／60 天裁剪／`total` 永不裁剪）已上收
//! `aeroric_core::metrics::agent_usage`。命令名、参数、返回形状一字不改。
//! 落点由 [`crate::analytics::paths`] 给出（G21：测试不再写真实 `agent-usage.json`）。

use aeroric_core::metrics::{self, AgentUsageSnapshot};

#[tauri::command]
pub async fn record_agent_config_usage(
    app: tauri::AppHandle,
    agent: String,
) -> Result<AgentUsageSnapshot, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.record_agent_config_usage",
        serde_json::json!({ "agent": agent }),
        async {
            let result =
                metrics::record_agent_config_usage_at(&crate::analytics::paths()?, &agent)?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn load_agent_usage_snapshot(
    app: tauri::AppHandle,
) -> Result<AgentUsageSnapshot, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.load_agent_usage_snapshot",
        serde_json::Value::Null,
        async {
            let result = metrics::load_agent_usage_snapshot_at(&crate::analytics::paths()?)?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
