//! 启动期降级记录（宿主接线层）。
//!
//! P8-04 刀三：账本本体（记录结构与读侧）已上收
//! `aeroric_core::maintenance::StartupDegradationLog`。本文件只剩两件事：
//! **持有一个进程级实例**（启动链上的三处 `record` 调用点没有上下文可传）与
//! **把每一条同步打到 stderr**（UI 还没起来时崩了，日志里也要留得下线索）。
//!
//! 宿主 UDS 的 `diagnostics.list_startup_degradations` 读的就是这个实例
//! （`dispatcher::HostWiring::degradations`）。

use std::sync::{Arc, LazyLock};

use aeroric_core::maintenance::StartupDegradationLog;

pub use aeroric_core::maintenance::StartupDegradation;

/// 本进程的账。宿主与 UDS 服务共享同一个实例，因此"UI 看到的降级"与"服务看到的"
/// 不可能不一致。
pub fn log() -> &'static Arc<StartupDegradationLog> {
    static LOG: LazyLock<Arc<StartupDegradationLog>> =
        LazyLock::new(|| Arc::new(StartupDegradationLog::new()));
    &LOG
}

/// 记一条降级。启动期调用，失败不影响主流程。
pub(crate) fn record(component: &str, reason: impl Into<String>, fallback: impl Into<String>) {
    let entry = log().record(component, reason, fallback);
    // 同时打到 stderr：前端还没起来时崩了，日志里也留得下线索。
    eprintln!(
        "[aeroric] startup degraded: {} — {} (fallback: {})",
        entry.component, entry.reason, entry.fallback
    );
}

/// 前端启动后查一次；空数组表示一切正常。
#[tauri::command]
pub fn list_startup_degradations() -> Vec<StartupDegradation> {
    log().list()
}
