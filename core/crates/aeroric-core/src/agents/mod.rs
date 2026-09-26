//! 外部 Agent 家族的命令规划与托管配置(P3-02)。
//!
//! 第一片:权限/effort → 启动参数映射表(自宿主 `agent_family_maps.rs` move)
//! 与 [`LaunchPlan`] 纯数据类型。后续切片逐族迁入 `claude`/`codex`/`dsh`/`omp`
//! 的命令构造、`managed_home/` 的托管 home/config/hook 安装,以及
//! `environment.rs` 的 login-shell 环境合并。

pub mod environment;
pub mod launch;
pub mod managed_home;
pub mod omp;

pub use environment::{
    aeroric_guard_ops, combined_omp_env_ops, omp_env_ops, terminal_env_ops, EnvOp,
};
pub use omp::{
    omp_hook_extension_is_loadable, omp_hook_source_guards_will_continue, OMP_HOOK_FILE_NAME,
};

pub use launch::{
    claude_permission_args, claude_plan, codex_permission_args, codex_plan, dsh_permission_mode,
    dsh_plan, omp_permission_flag, omp_plan, omp_thinking_level, LaunchInput, LaunchPlan,
};
