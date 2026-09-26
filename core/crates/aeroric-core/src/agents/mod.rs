//! 外部 Agent 家族的命令规划与托管配置(P3-02)。
//!
//! 第一片:权限/effort → 启动参数映射表(自宿主 `agent_family_maps.rs` move)
//! 与 [`LaunchPlan`] 纯数据类型。后续切片逐族迁入 `claude`/`codex`/`dsh`/`omp`
//! 的命令构造、`managed_home/` 的托管 home/config/hook 安装,以及
//! `environment.rs` 的 login-shell 环境合并。

pub mod launch;

pub use launch::{
    claude_permission_args, codex_permission_args, dsh_permission_mode, omp_permission_flag,
    omp_thinking_level, LaunchPlan,
};
