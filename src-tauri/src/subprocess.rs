//! 进程树管理已迁入 core(`aeroric_core::runtime::subprocess`,P3-01 切片二)。
//! 这里只做 re-export:全部调用点(`crate::subprocess::…`,见 `agent_tools.rs`/
//! `conda.rs`/`agent_assist.rs`/`ssh.rs` 等)路径不变,测试随函数体搬进了 core
//! 的测试二进制——src-tauri `--lib` 用例计数地板已在 checks.json 的 `$note` 里
//! 记录这次搬家(2479→2476),不是测试悄悄消失。

pub(crate) use aeroric_core::runtime::subprocess::*;
