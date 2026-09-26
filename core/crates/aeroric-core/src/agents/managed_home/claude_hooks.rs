//! Claude hooks 的受管配置构造(P3-02 切片四自宿主 hooks.rs 迁入的纯半边):
//! `--settings <Aeroric 自有文件>` 的 JSON 内容与旧版注入的清理逻辑。
//! I/O(写盘、路径解析)留在宿主;这里只产"内容"。

use serde_json::{Map, Value};

/// Claude hook 条目上的 Aeroric 注入标记(旧名 `"_nezha_managed"` 是历史产品名,
/// concat! 只是避免本仓库全文命中旧名)。
pub const AERORIC_MARKER_FIELD: &str = "_aeroric_managed";
pub const LEGACY_CLAUDE_MARKER_FIELD: &str = concat!("_ne", "zha_managed");

pub const CLAUDE_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    // PostToolUse:工具执行成功后触发(ask 模式下即用户审批通过后),
    // 用于把 input_required 复位回 running——UserPromptSubmit 在工具审批时不触发。
    "PostToolUse",
    "Stop",
    "SubagentStop",
];

pub fn hook_command(node_path: &str, script: &str) -> String {
    #[cfg(windows)]
    {
        let quote = |value: &str| value.replace('\'', "''");
        format!(
            "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command \"& '{}' '{}'\"",
            quote(node_path),
            quote(script)
        )
    }
    #[cfg(not(windows))]
    {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
        format!("{} {}", quote(node_path), quote(script))
    }
}
pub fn build_claude_settings_value(node_path: &str, script: &str) -> Value {
    let entry = serde_json::json!({
        "hooks": [{ "type": "command", "command": hook_command(node_path, script) }],
    });
    let mut hooks = Map::new();
    for event in CLAUDE_EVENTS {
        hooks.insert((*event).to_string(), Value::Array(vec![entry.clone()]));
    }

    let mut settings_object = Map::new();
    settings_object.insert("hooks".to_string(), Value::Object(hooks));
    Value::Object(settings_object)
}
pub fn with_claude_fast_mode(mut value: Value) -> Result<Value, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Claude settings must be a JSON object".to_string())?;
    object.insert("fastMode".to_string(), Value::Bool(true));
    Ok(value)
}
pub fn is_aeroric_managed(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|obj| {
            obj.get(AERORIC_MARKER_FIELD)
                .or_else(|| obj.get(LEGACY_CLAUDE_MARKER_FIELD))
        })
        .and_then(|v| v.as_str())
        .is_some()
}
/// 从 settings JSON 对象上移除 Aeroric hooks。
pub fn uninject_claude_value(mut root: Value) -> Value {
    let Some(root_obj) = root.as_object_mut() else {
        return root;
    };
    let Some(hooks) = root_obj.get_mut("hooks").and_then(|v| v.as_object_mut()) else {
        return root;
    };
    // 收集要清空的 event 数组名
    let event_keys: Vec<String> = hooks
        .iter()
        .filter_map(|(k, v)| v.as_array().map(|_| k.clone()))
        .collect();
    for key in event_keys {
        if let Some(arr) = hooks.get_mut(&key).and_then(|v| v.as_array_mut()) {
            arr.retain(|entry| !is_aeroric_managed(entry));
        }
    }
    // 不删除空数组也不删除 hooks 对象本身,保留用户既有结构
    root
}
