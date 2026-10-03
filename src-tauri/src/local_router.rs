//! local router（宿主 shim）。P8-02：业务本体上收 `aeroric_core::local_router`
//! （server/bridge/transforms/circuit breaker/usage 原样移动，自带测试随行）；
//! `crate::sse` 同步上收。`local_router_commands.rs`（Tauri 命令层）留宿主。
pub use aeroric_core::local_router::*;
