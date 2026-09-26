//! 内置 agent 家族的权限档 / effort → 启动参数映射表已随 P3-02 首片迁入 core
//! (`aeroric_core::agents::launch`)——core 是唯一真相源,宿主 re-export,
//! pty/ssh/legacy_task_executor 的 `crate::agent_family_maps::…` 路径不变。

#[allow(unused_imports)] // 部分映射当前只在 core 侧测试里被同批断言
pub use aeroric_core::agents::launch::{
    claude_permission_args, codex_permission_args, dsh_permission_mode, omp_permission_flag,
    omp_thinking_level,
};
