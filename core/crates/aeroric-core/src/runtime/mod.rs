//! 运行时层(P3-01):把"宿主到底是什么"(Tauri / 原生 macOS / CLI)从 core 的
//! 视野里抹掉,只留下端口、可取消的 watcher 生命周期与一份迁移契约。
//!
//! **本模块存在期间,core 三个 crate 的源码里必须保持"tauri 的类型面零出现"**
//! (INV-02 的源码扫描是词法的,连文档注释里的字面量都会被它拦下——上一轮就
//! 拦住过我自己写的这条注释)——这是 P3-01 的验收判据,不是风格偏好。
//!
//! ## `TaskRegistry` 的迁移契约(为什么今天还不在这里)
//!
//! 计划 Action 2 要求把宿主 `TaskManager` 的 PTY maps/session maps/generation 容器
//! 迁入 core 的 `TaskRegistry`,并明文禁止"两份全局 map 同时成为真相源"。所以迁移
//! 必须是一次 **move**(宿主 `TaskManager` 变成委托壳),不能是 copy-then-switch:
//! 在宿主仍持有那 14 张表的今天,在 core 里先立一个平行的 `TaskRegistry` 就是把
//! 双真相源造出来。`manager.rs` 现在只放与表无关的 [`WatcherSupervisor`];
//! `TaskRegistry` 与它的 move 同刀落地。迁移清单与可移植性判定见
//! `docs/native/extraction/runtime-ports.md`。

pub mod events;
pub mod manager;
pub mod ports;
pub mod shutdown;
pub mod subprocess;

pub use events::{NullSink, TaskEventSink, TaskNotification};
pub use manager::{SuperviserError, WatcherSupervisor};
pub use ports::Runtime;
pub use shutdown::ShutdownToken;
