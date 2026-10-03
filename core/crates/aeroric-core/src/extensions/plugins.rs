//! DSH 插件的版本/配置映射纯函数（宿主 `dsh_plugins.rs` 的守卫半边）。

/// 插件目录名守卫：单段、非穿越、非 NUL（与 skill 名同族，但含 `-`/`_`/`.`）。
pub fn validate_plugin_dir(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Plugin directory name cannot be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err(format!("Invalid plugin directory: {name}"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(format!(
            "Plugin directory must not contain path separators: {name}"
        ));
    }
    Ok(())
}

/// 语义版本比较（`a.b.c`；缺段按 0）。返回 Ordering 供 enable/rollback 判
/// 「回滚目标必须不新于当前」。
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|seg| seg.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let l = parse(left);
    let r = parse(right);
    let len = l.len().max(r.len());
    for i in 0..len {
        let a = l.get(i).copied().unwrap_or(0);
        let b = r.get(i).copied().unwrap_or(0);
        if a != b {
            return a.cmp(&b);
        }
    }
    std::cmp::Ordering::Equal
}
