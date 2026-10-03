//! 自动同步的后台线程（宿主接线层）。
//!
//! P7-06：调度与循环本体已上收 core（`aeroric_core::notebook::sync::daemon`）。
//! 本文件只剩宿主接线：
//! - **G03 关闭点**：原裸 `thread::spawn` 换成 core `SyncDaemonHandle`（持
//!   JoinHandle＋停止位），guard 存进 Tauri state——app 退出 drop 即停，
//!   不再留下不可关停的孤儿线程（`usage_index.rs:1048` 那一半归 P8-04）。
//! - **v1 topic 接线**：core 只认注入的 `SyncEventSink`；这里一次回调发两条——
//!   旧桌面通道 `notebook-sync-updated`（React 客户端，verbatim 空对象 payload）
//!   ＋v1 topic `notebook.sync_updated`（`command-map.json` 的
//!   `notebook.sync_updated` 行由此变为 mapped）。
//! - watcher 的监听根跟着已注册 vault 走：`NotebookState` 从 Tauri state 取，
//!   数据根（storage-connections.json）取宿主 `aeroric_dir()`。

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use aeroric_core::notebook::state::NotebookState;
use aeroric_core::notebook::sync::daemon as daemon_core;

// 宿主只重写 `start`；core daemon 的其余公开面（状态查询/forget/常量）原样透出。
pub use daemon_core::{forget_pending, status_for, RemoteStatus};

/// 宿主持有的关停句柄。App 退出时按管理顺序 drop → core 的 Drop 置停止位
/// 并 join——显式 `stop()` 入口留给将来的设置项。
#[allow(dead_code)] // 字段只为 Drop 副作用而持有（drop → core 置停止位并 join）
pub struct SyncDaemonGuard(daemon_core::SyncDaemonHandle);

pub fn start(app: AppHandle) {
    let Some(state) = app.try_state::<Arc<NotebookState>>() else {
        // state 未注册（不应发生：manage 先于 setup）→ 不起守护，等于自动同步关闭。
        return;
    };
    let state = Arc::clone(state.inner());
    let emitter = app.clone();
    let sink: daemon_core::SyncEventSink = Arc::new(move || {
        use tauri::Emitter;
        // 旧通道（verbatim 空对象 payload）＋v1 topic。两条都必须发：
        // React 客户端只认前者，v1 事件面只认后者。旧通道用**字面量**而非
        // 常量：事件扫描器按 src-tauri 内的字面值找发射点，常量定义已随
        // P7-06 移入 core（`notebook_sync_updated` 行的 mapped 判据不变）。
        let _ = emitter.emit("notebook-sync-updated", ());
        let _ = emitter.emit(daemon_core::SYNC_UPDATED_TOPIC, ());
    });
    let handle = daemon_core::start(
        state,
        crate::storage::aeroric_dir().unwrap_or_default(),
        sink,
    );
    app.manage(SyncDaemonGuard(handle));
}
