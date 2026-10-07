//! 订阅用量快照（宿主接线层）。
//!
//! P8-04 刀二：Claude Keychain/HTTP 读取、Codex app-server JSON-RPC 客户端、
//! 窗口解析与 429 冷却全部上收 `aeroric_core::metrics::usage`。宿主只剩两件事：
//! **注入进程句柄槽**（`codex app-server` 长驻子进程由 `TaskManager::codex_rpc`
//! 持有，core 不自己去找进程归属）与**原样透出** `UsageSnapshot`。

pub use aeroric_core::metrics::usage::CodexRpcClient;

#[tauri::command]
pub async fn read_usage_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::TaskManager>,
) -> Result<aeroric_core::metrics::UsageSnapshot, String> {
    // 克隆 Arc 给 core：懒启动/坏掉重来都在 core 里，宿主只保证句柄槽是同一个。
    let codex_rpc = std::sync::Arc::clone(&state.codex_rpc);
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.read_usage_snapshot",
        serde_json::Value::Null,
        async {
            let snapshot = aeroric_core::metrics::read_usage_snapshot(codex_rpc).await?;
            serde_json::to_value(snapshot).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
