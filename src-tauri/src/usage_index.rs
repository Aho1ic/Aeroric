//! 用量统计索引的后台线程（宿主接线层）。
//!
//! P8-04 刀二：索引本体（游标续扫／SQLite 落库／重解析节流）已上收
//! `aeroric_core::metrics::index`。本文件只剩宿主接线：
//!
//! - **G03 关闭点（usage_index 半边）**：原 `thread::spawn(move || run_loop(app))`
//!   换成 core `UsageIndexWatcher`（持停止位＋JoinHandle）；guard 存进 Tauri state，
//!   app 退出 drop 即停。`notebook::sync::daemon` 是同款的另一半（P7-06）。
//! - **两条通道双发**：core 只认注入的 `IndexEventSink`；这里一次回调发两条——
//!   旧桌面通道 `usage-statistics-updated`（React 客户端，payload `{updatedAt}`）
//!   ＋v1 topic `usage.statistics_updated`（`event-topics.json` 的
//!   `usage.statistics_updated` 行由此变为 mapped）。旧通道用**字面量**而非常量：
//!   事件扫描器按 `src-tauri` 内的字面值找发射点，常量定义已随刀二移入 core。
//! - 落点由 [`crate::analytics::paths`] 给出（G21）：测试指到 scratch 目录即可，
//!   不再写真实用户的 `~/.aeroric/usage-statistics.sqlite3`。

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use aeroric_core::metrics::index as index_core;

/// 宿主持有的关停句柄。App 退出时按管理顺序 drop → core 的 `Drop` 置停止位并 join。
#[allow(dead_code)] // 字段只为 Drop 副作用而持有（drop → core 置停止位并 join）
pub struct UsageIndexGuard(index_core::UsageIndexWatcher);

pub fn start(app: AppHandle) {
    let paths = match crate::analytics::paths() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("[aeroric] usage index disabled: {error}");
            return;
        }
    };
    let emitter = app.clone();
    let sink: index_core::IndexEventSink = Arc::new(move |updated_at| {
        use tauri::Emitter;
        let payload = serde_json::json!({ "updatedAt": updated_at });
        // 旧通道（React 客户端只认它）＋v1 topic。
        let _ = emitter.emit("usage-statistics-updated", payload.clone());
        let _ = emitter.emit(index_core::INDEX_TOPIC, payload);
    });
    let watcher = index_core::start(paths, sink);
    app.manage(UsageIndexGuard(watcher));
}

#[tauri::command]
pub async fn refresh_usage_statistics_index(app: AppHandle) -> Result<bool, String> {
    let paths = crate::analytics::paths()?;
    let emitter = app.clone();
    let sink: index_core::IndexEventSink = Arc::new(move |updated_at| {
        use tauri::Emitter;
        let payload = serde_json::json!({ "updatedAt": updated_at });
        let _ = emitter.emit("usage-statistics-updated", payload.clone());
        let _ = emitter.emit(index_core::INDEX_TOPIC, payload);
    });
    // 走统一判决：external 时由宿主重建索引（不然两边各自写统计库）。
    let value = crate::host_dispatch::dispatch(
        &app,
        "usage.refresh_usage_statistics_index",
        serde_json::Value::Null,
        async {
            index_core::refresh_usage_statistics_index_at(paths, sink)
                .await
                .map(serde_json::Value::Bool)
        },
    )
    .await?;
    Ok(value.as_bool().unwrap_or(false))
}
