//! 扩展面（P8-01 自宿主 `skills/mcp/hooks/dsh_plugins` 上收的安全关键纯逻辑）。
//!
//! 宿主命令层保留（网络/钥匙串/Tauri State），core 持：
//! - 路径守卫（skill 名、安装落点白名单）
//! - 信任判定（来源/revision/写入范围/所需权限 → verdict）
//! - MCP 配置校验＋原子替换＋失败回滚、env secret 掩码
//! - hook 脚本资源守卫（扩展名白名单——非 `.js/.ts` 静默丢弃的教训已固化）
//!
//! ⚠️ 扫描/列目录**不等于执行**：本模块任何函数都不运行第三方脚本。

pub mod hooks;
pub mod mcp;
pub mod plugins;
pub mod skills;
pub mod trust;

/// 扩展安装的原子写（tmp → 写入 → rename；失败不留半配置——调用方在
/// `validate → write → commit` 三段之间任何一步失败即回收 tmp）。
pub fn atomic_install(path: &std::path::Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Install path has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("install"),
        unique
    ));
    let write = std::fs::write(&tmp, content);
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot write install staging: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot finalize install: {e}"));
    }
    Ok(())
}
