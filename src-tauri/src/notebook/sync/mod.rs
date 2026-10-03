//! 随手记同步（宿主层）。P7-06：业务本体上收 `aeroric_core::notebook::sync`
//! （scan/store/diff/engine/local/git/schedule/remote/manifest 原样移动，
//! daemon 重写为可关停监督），这里 re-export ＋宿主接线（daemon.rs）。

pub use aeroric_core::notebook::sync::{diff, engine, git, local, remote, scan, store};
pub mod daemon;
