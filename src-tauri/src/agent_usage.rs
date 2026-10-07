//! Agent 配置使用频次账本的宿主接线层。
//!
//! P8-04 刀二：账本（按本地日期分桶／7 天窗口／60 天裁剪／`total` 永不裁剪）已上收
//! `aeroric_core::metrics::agent_usage`。命令名、参数、返回形状一字不改。
//! 落点由 [`crate::analytics::paths`] 给出（G21：测试不再写真实 `agent-usage.json`）。

use aeroric_core::metrics::{self, AgentUsageSnapshot};

#[tauri::command]
pub fn record_agent_config_usage(agent: String) -> Result<AgentUsageSnapshot, String> {
    metrics::record_agent_config_usage_at(&crate::analytics::paths()?, &agent)
}

#[tauri::command]
pub fn load_agent_usage_snapshot() -> Result<AgentUsageSnapshot, String> {
    metrics::load_agent_usage_snapshot_at(&crate::analytics::paths()?)
}
