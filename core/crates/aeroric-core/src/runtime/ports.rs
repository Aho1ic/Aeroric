//! 运行时端口(P3-01 Action 1)。
//!
//! [`Runtime`] 是宿主交给 core 的伞形端口:事件出口与 watcher 监督。它的表面**只随
//! 消费者生长**——计划原文要求的 `ProcessSpawner`/`PlatformServices` 两个名字,在
//! 各自的第一个真实消费者到位时才定义(`ProcessSpawner` 随 `subprocess.rs` 迁入与
//! `TaskRegistry` 接管进程句柄,`PlatformServices` 随宿主 `platform_adapter.rs`);
//! 先写一个没人调用的 trait,正是 grill 门禁点名的"只检查 trait 名称"。
//! 每一步的落点与理由见 MIGRATION_PLAN.md 的接线补记与 `docs/native/extraction/runtime-ports.md`。

use std::sync::Arc;

use super::manager::WatcherSupervisor;
use crate::tasks::events::TaskEventSink;

/// 宿主运行时的最小接口。**不依赖任何具体异步运行时或窗口系统**——Tauri 宿主、
/// 原生 macOS 宿主与 headless CLI 各自实现它;core 只见到这个 trait。
pub trait Runtime: Send + Sync {
    /// 任务事件的唯一出口(ADR-023:core 是 `task-status`/`task-session` 的唯一发射者,
    /// 宿主实现负责把通知变成自己前端的动作——Tauri 是 `emit`,CLI 是 stdout/日志)。
    fn event_sink(&self) -> Arc<dyn TaskEventSink>;

    /// watcher 监督者。宿主在 setup 阶段创建一次;core 不拥有它,只通过宿主的
    /// 实现拿到引用(lifecycle 验收的双启动/双 shutdown 判据挂在它身上)。
    fn watchers(&self) -> &WatcherSupervisor;
}
