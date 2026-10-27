//! 托管 home 的配置与 hook 安装纯半边(P3-02 切片四)。
//!
//! 文件 I/O(写 `~/.aeroric/hooks/`、`~/.codex/config.toml`、omp 托管 home)
//! 留在宿主——它们依赖宿主的路径解析与原子写;这里放**内容构造**(Claude
//! settings JSON、Codex TOML 注入块)与 **hook 脚本资源**(单一来源)。
//! 后续切片把 omp/dsh 托管 home 的安装逻辑也收进来。

pub mod claude_hooks;
pub mod codex_hooks;
pub mod resources;

pub use claude_hooks::{
    build_claude_settings_value, hook_command, is_aeroric_managed, uninject_claude_value,
    with_claude_fast_mode, AERORIC_MARKER_FIELD, CLAUDE_EVENTS, LEGACY_CLAUDE_MARKER_FIELD,
};
pub use codex_hooks::{
    build_codex_block, inject_codex_text, toml_quote, uninject_codex_text, CODEX_BEGIN, CODEX_END,
    CODEX_EVENTS,
};
pub use resources::AERORIC_HOOK_SCRIPT;
