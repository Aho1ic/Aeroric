//! 终端层(P3-03):原始字节流、控制租约、有界回放与退出结算。
//!
//! 分工:**core 先发布原始 bytes**(offset = 原始字节计数,字节不透明——NUL、
//! 非法 UTF-8、跨帧劈开的多字节序列原样进流),旧 Tauri sink 的字符串视图是
//! 它的增量解码投影。流身份 = `(task_id, generation)`;缓冲有界,落后太多的
//! 订阅者得到显式 Gap 而不是"屏幕完全恢复"的谎话。
//!
//! 迁移对照与切片顺序见 `docs/native/extraction/terminal-stream.md`。

pub mod exit;
pub mod lease;
pub mod pty;
pub mod replay;
pub mod stream;

pub use exit::{settle_exit, ExitDecision, ExitFacts, ExitReason, SessionEvidence};
pub use lease::{ControlLease, LeaseError, LeaseHandle};
pub use pty::{
    PublishOutcome, TerminalService, Utf8Projection, TERMINAL_MAX_FINISHED_STREAMS,
    TERMINAL_MAX_LIVE_STREAMS, TERMINAL_REPLAY_WINDOW_BYTES,
};
pub use replay::{
    plan_replay, projected_utf16_units, ReplayFrame, ReplayPlan, ResetReason, ResumePoint,
};
pub use stream::{ByteStream, StreamChunk, StreamError, TerminalIdentity};
