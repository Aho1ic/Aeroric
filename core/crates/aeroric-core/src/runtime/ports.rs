//! 运行时端口(P3-01 Action 1)。
//!
//! [`Runtime`] 是宿主交给 core 的伞形端口:事件出口与 watcher 监督。它的表面**只随
//! 消费者生长**——计划原文要求的 `ProcessSpawner`/`PlatformServices` 两个名字,在
//! 各自的第一个真实消费者到位时才定义(`ProcessSpawner` 随 `subprocess.rs` 迁入与
//! `TaskRegistry` 接管进程句柄,`PlatformServices` 随宿主 `platform_adapter.rs`);
//! 先写一个没人调用的 trait,正是 grill 门禁点名的"只检查 trait 名称"。
//! 每一步的落点与理由见 MIGRATION_PLAN.md 的接线补记与 `docs/native/extraction/runtime-ports.md`。

use std::path::Path;
use std::sync::Arc;

use super::manager::WatcherSupervisor;
use crate::tasks::events::TaskEventSink;

/// 平台服务的错误面。`InteractiveUiRequired` 是 §2.1 的口径:headless CLI 对
/// 必须有窗口才能回答的服务,返回这个错误而不是假装成功;`Unsupported` 留给
/// 平台本身没有的能力(如 Linux 上的剪贴板文件路径)。
#[derive(Debug)]
pub enum PlatformError {
    Io(String),
    Unsupported(&'static str),
    InteractiveUiRequired(&'static str),
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformError::Io(message) => write!(f, "{message}"),
            PlatformError::Unsupported(what) => write!(f, "unsupported on this platform: {what}"),
            PlatformError::InteractiveUiRequired(what) => {
                write!(f, "interactive UI required: {what}")
            }
        }
    }
}

/// 平台 UI/OS 服务的端口(P3-01 Action 4)。**消费者是真实存在的**：宿主 fs.rs
/// 的 `open_in_system_file_manager` 与 `read_clipboard_file_paths` 两条命令过去把
/// OS 调用内联在命令体里，现在改经此端口（宿主 `platform_adapter.rs` 实现）；
/// headless CLI（P3-06/07）实现同一端口——reveal/clipboard 原生可答，窗口类
/// 服务按 §2.1 返回 [`PlatformError::InteractiveUiRequired`]。方法表只随消费者
/// 生长：系统 dialog 等待它的第一个调用者。
pub trait PlatformServices: Send + Sync {
    /// 在系统文件管理器里显示一个文件（`open -R` / `explorer /select`）或打开
    /// 一个目录（`open` / `explorer`）。
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError>;

    /// 读取剪贴板中的文件路径列表。平台没有对应能力时返回空列表（与迁移前
    /// `read_clipboard_file_paths_sync` 的非 macOS 分支一致），而不是报错。
    fn read_clipboard_file_paths(&self) -> Result<Vec<String>, PlatformError>;
}

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
