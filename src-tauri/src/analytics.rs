//! 会话指标与用量统计（宿主接线层）。
//!
//! P8-04 刀二：业务体已上收 `aeroric_core::metrics::analytics`。本文件只剩两件事：
//! **拼出 core 要的注入根**（[`MetricsPaths`]：外部家目录只读、数据根可写）与
//! **把结果原样透出**。命令名、参数名、返回 JSON 形状一字不改——前端契约冻结。

use aeroric_core::metrics::{self, MetricsPaths};

/// 这一族要知道的两个根。家目录取宿主平台探测（`~`），数据根取宿主 `aeroric_dir()`
/// （它已认 `AERORIC_HOME`／`--data-dir`；门禁 runner 因此能把测试指向 scratch 目录）。
pub(crate) fn paths() -> Result<MetricsPaths, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(MetricsPaths::new(home, crate::storage::aeroric_dir()?))
}

#[tauri::command]
pub async fn read_session_metrics(
    app: tauri::AppHandle,
    session_path: String,
) -> Result<metrics::SessionMetrics, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.read_session_metrics",
        serde_json::json!({ "sessionPath": session_path }),
        async {
            let result = metrics::read_session_metrics_at(&session_path).await?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_usage_statistics(
    app: tauri::AppHandle,
    range_days: u32,
    agent: String,
) -> Result<metrics::UsageStatistics, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.read_usage_statistics",
        serde_json::json!({ "rangeDays": range_days, "agent": agent }),
        async {
            let result = metrics::read_usage_statistics_async(paths()?, range_days, agent).await?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
