//! hook 资源守卫（omp hook 的扩展名规则固化：loader 静默丢弃非 `.js/.ts`，
//! 失败模式＝任务卡 running——见 AGENTS.md oh-my-pi 集成节）。

/// hook 脚本资源文件必须是 `.js` 或 `.ts`（大小写不敏感）。
pub fn is_hook_script(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".js") || lower.ends_with(".ts")
}

/// hook 配置校验：脚本名过 [`is_hook_script`]，事件名非空。
pub fn validate_hook(script_name: &str, event: &str) -> Result<(), String> {
    if !is_hook_script(script_name) {
        return Err(format!(
            "Hook script \"{script_name}\" must be a .js or .ts file (the loader silently drops anything else)"
        ));
    }
    if event.trim().is_empty() {
        return Err("Hook event name is required".to_string());
    }
    Ok(())
}
