//! MCP 配置校验/掩码（自宿主 `mcp.rs` 上收的纯函数；文件 I/O 留宿主）。

use std::collections::BTreeMap;

/// 校验一条 MCP server 配置：command 非空且非裸 shell 元字符串、env 键名合法。
/// 命令以 argv 传递（宿主侧不经 shell），但仍拒绝空命令与 NUL。
pub fn validate_server(
    name: &str,
    command: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err(format!("MCP server \"{name}\" has an empty command"));
    }
    if command.contains('\0') {
        return Err(format!("MCP server \"{name}\" command contains NUL"));
    }
    for key in env.keys() {
        if key.is_empty() || key.contains('=') || key.contains('\0') {
            return Err(format!("MCP server \"{name}\" has an invalid env key"));
        }
    }
    Ok(())
}

/// env 值掩码：回给前端的配置里，凭据类值只保留「已设置」标记。
/// 判定按 key 名（保守集合，与宿主 launcher 的凭据集同口径方向）。
pub fn mask_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| {
            let sensitive = {
                let lower = key.to_ascii_lowercase();
                lower.contains("key")
                    || lower.contains("token")
                    || lower.contains("secret")
                    || lower.contains("password")
            };
            (
                key.clone(),
                if sensitive && !value.is_empty() {
                    "••••••••".to_string()
                } else {
                    value.clone()
                },
            )
        })
        .collect()
}
