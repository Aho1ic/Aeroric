//! 会话 JSONL 的解析:一行文本 → `SessionMessage`。
//!
//! **实现已于 2026-09-29 迁至 core（P3-04 首片，补记二百四十一）**：
//! `aeroric_core::sessions::{parser, claude, codex}`。本文件只剩 re-export，
//! 按原可见性转发——`parse_session_lines` 对 crate 内、Claude/Codex 两个入口对
//! 父模块 `session.rs`（其 `use parse::*;` 与 `pub(crate) use parse::parse_session_lines;`
//! 一字不动，`remote/session_push.rs` 的调用点也因此不用改）。
//!
//! 迁移是逐字搬运（行为零改动）；金样/护栏测试在
//! `core/crates/aeroric-core/tests/session_parsers.rs`，宿主 40 条既有测试
//! 经本 re-export 直接跑在 core 实现上（旧客户端零回归的机器形状）。

pub(super) use aeroric_core::sessions::claude::parse_claude_session;
pub(super) use aeroric_core::sessions::codex::parse_codex_session;
pub(crate) use aeroric_core::sessions::parser::parse_session_lines;
