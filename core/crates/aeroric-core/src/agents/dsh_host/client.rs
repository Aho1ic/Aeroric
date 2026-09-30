//! DSH API 调用的编解码面（P3-05 第二片起步）。
//!
//! 本片只收 `decode_result_value`——HTTP RPC 信封的**纯**解码：稳定 envelope
//! （`result.ok`／`result.error.message`／`result.value`）与两条在网错误文案
//! （`DSH API {method} response has no result`／`… rejected the request: {message}`）
//! 逐字迁移自宿主 `src-tauri/src/dsh_protocol.rs`（补记二百五十一），`value`
//! 缺省成 `Null` 的旧默认同样保真。真正的 HTTP 客户端（reqwest／鉴权／重试）
//! 仍留宿主 `dsh_webui/api_client.rs`——它是 I/O，按 sessions 的先后次序逐步上收。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml_ng::Value as YamlValue;

/// Decode the stable DSH HTTP RPC result envelope while preserving its legacy
/// defaults and error wording.
pub fn decode_result_value(body: Value, method: &str) -> Result<Value, String> {
    let result = body
        .get("result")
        .ok_or_else(|| format!("DSH API {method} response has no result"))?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown DSH API error");
        return Err(format!("DSH API {method} rejected the request: {message}"));
    }
    Ok(result.get("value").cloned().unwrap_or(Value::Null))
}

// ── DSH 协议清单（P3-05 第三片，补记二百五十二）：钉死自上游 `c291e796…`/0.1.5-rc.2/v2
// 的 9 张现用表。Action 4「每条 inventory 映射为 RPC/capability」的数据面；G17 的
// 「10 张钉死的表/246 项」由此同源。RETIRED 三表是宿主测试对账面，随该文件留在宿主。
pub const SOURCE_COMMIT: &str = "c291e7961a515f6d7af9304e7fd1d257929aef26";
pub const PACKAGE_VERSION: &str = "0.1.5-rc.2";
pub const PROTOCOL_VERSION: u32 = 2;

pub const RPC_METHODS: &[&str] = &[
    "session.list",
    "session.search",
    "session.create",
    "session.selectModel",
    "session.modelCatalog",
    "session.canOpenWorkspacePath",
    "session.openWorkspacePath",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "session.page",
    "session.follow",
    "session.control",
    "subagents.list",
    "subagents.prompt",
    "subagents.interruptByParent",
    "directoryPicker.pick",
    "directoryPicker.list",
    "directoryPicker.createDirectory",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "workspace.follow",
    "workspaceFiles.list",
    "workspaceFiles.read",
    "workspaceFiles.readAll",
    "workspaceFiles.readBytes",
    "workspaceFiles.readRelated",
    "workspaceFiles.stat",
    "workspaceFiles.changes",
    "skills.list",
    "agentPresets.list",
    "agentPresets.select",
    "agentPresets.read",
    "agentPresets.copy",
    "agentPresets.deletePreset",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "goals.get",
    "settings.describe",
    "settings.openSettingsDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "settings.canOpenAgentPresetDirectory",
    "settings.openAgentPresetDirectory",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.listProviders",
    "llm.listConfigurableProviders",
    "llm.discoverModels",
];

pub const REMOTE_METHODS: &[&str] = &[
    "session.list",
    "session.search",
    "session.create",
    "session.selectModel",
    "session.modelCatalog",
    "session.canOpenWorkspacePath",
    "session.openWorkspacePath",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "session.page",
    "session.follow",
    "session.control",
    "subagents.list",
    "subagents.prompt",
    "subagents.interruptByParent",
    "directoryPicker.pick",
    "directoryPicker.list",
    "directoryPicker.createDirectory",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "workspace.follow",
    "workspaceFiles.list",
    "workspaceFiles.read",
    "workspaceFiles.readAll",
    "workspaceFiles.readBytes",
    "workspaceFiles.readRelated",
    "workspaceFiles.stat",
    "workspaceFiles.changes",
    "skills.list",
    "agentPresets.list",
    "agentPresets.select",
    "agentPresets.read",
    "agentPresets.copy",
    "agentPresets.deletePreset",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "goals.get",
    "settings.describe",
    "settings.openSettingsDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "settings.canOpenAgentPresetDirectory",
    "settings.openAgentPresetDirectory",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.listProviders",
    "llm.listConfigurableProviders",
    "llm.discoverModels",
    "commands.list",
    "commands.execute",
    "messageFeedback.list",
    "messageFeedback.put",
    "messageFeedback.delete",
    "pluginInventory.list",
    "dynamicCordisRunner.undefineFromPanel",
    "dynamicCordisRunner.runHostHalf",
    "dynamicCordisRunner.getClientCode",
    "dynamicCordisRunner.resolveRequestRun",
    "dynamicCordisRunner.settleUserRun",
    "dynamicCordisRunner.stopFromPanel",
    "dynamicCordisRunner.syncInspectManifest",
    "dynamicCordisRunner.resolveInspectQuery",
    "dynamicCordisRunner.inventory",
    "dynamicCordisRunner.reportRenderFailure",
    "dynamicCordisRunner.reportClientGuardFailure",
    "dynamicCordisRunner.invoke",
    "fileReferences.list",
    "sessionReferenceResolver.candidates",
    "fileUploads.upload",
    "sessionFeedback.record",
    "agentTeams.createTask",
    "agentTeams.updateTask",
    "agentTeams.view",
];

/// Event names, 1:1 with `REMOTE_EVENT_MODES`. Source:
/// `packages/api/remotes/src/remote-events.ts` at SOURCE_COMMIT.
pub const REMOTE_EVENTS: &[&str] = &[
    "agent-preset/selected",
    "approval/request",
    "api-session/activity",
    "api-session/added",
    "api-session/error",
    "api-session/removed",
    "api-session/status",
    "commands/change",
    "credentials/reference-updated",
    "goal/activation-changed",
    "cordis/request-run",
    "cordis/request-run-resolved",
    "cordis/dynamic-package",
    "cordis/dynamic-retract",
    "cordis/inspect-query",
    "cordis/inspect-query-resolved",
    "llm/adapters-updated",
    "settings/document-updated",
    "user-questions/request",
];

pub const REMOTE_EVENT_MODES: &[&str] = &[
    "emit",
    "waterfall",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "waterfall",
];

pub const STREAM_FOLLOW_FRAMES: &[&str] = &["snapshot", "event", "assistant-stream"];

pub const STREAM_CONTROL_FRAMES: &[&str] = &["baseline", "queue", "jobs", "projection"];

pub const STREAM_DOWNLINK_FRAMES: &[&str] = &["ready", "emit", "waterfall", "cancel"];

// ── 第十一片（补记二百六十）：DSH 插件的线数据形状，机械迁自宿主 dsh_plugins.rs ──
//
// cordis.patch.yml 的补丁协议、`dsh config dump` 的文本形、pluginInventory 的
// 条目形状——都是 host↔dsh 的 **wire 数据面**，与 RPC 信封同归 client；进程执行、
// 文件读写、profile 列表拉取（`command_for_agent`/`list_*`/`install`/`toggle`）
// 留宿主。`serde_yaml_ng = 0.10` 以宿主同 crate 同版本入 core（纯解析，
// 不引新 I/O 面；zstd/chrono/url 先例四连）。
//
// verbatim 的唯一改名：本模块的 `Value` 指 **YAML** 值，与上方 RPC 的
// `serde_json::Value` 同名冲突 ⇒ 统一别名 `YamlValue`；函数体逻辑零改动。

/// patch 行里的插件条目形状（`pluginInventory/list` 的 entries 元素）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshRuntimePlugin {
    pub entry_id: String,
    pub module_name: String,
    pub enabled: bool,
    pub fiber_phase: Option<String>,
}

pub fn is_dsh_plugin_package(package: &str) -> bool {
    package.starts_with("@deepseek-ai/dsh-") || package.starts_with("dsh-")
}

pub fn plugin_id(package: &str) -> &str {
    package
        .strip_prefix("@deepseek-ai/dsh-")
        .or_else(|| package.strip_prefix("dsh-"))
        .unwrap_or(package)
}

pub fn yaml_key(key: &str) -> YamlValue {
    YamlValue::String(key.to_string())
}

pub fn plugin_patch_rows(content: &str) -> Result<Vec<YamlValue>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_yaml_ng::from_str::<YamlValue>(content)
        .map_err(|error| format!("Failed to parse Aeroric DSH plugin patch: {error}"))?
    {
        YamlValue::Null => Ok(Vec::new()),
        YamlValue::Sequence(rows) => Ok(rows),
        _ => Err("Aeroric DSH plugin patch must be a YAML sequence".to_string()),
    }
}

pub fn row_id(row: &YamlValue) -> Option<&str> {
    row.as_mapping()?.get(yaml_key("id"))?.as_str()
}

pub fn plugin_enabled_from_patch(content: &str, package: &str) -> Result<bool, String> {
    let id = plugin_id(package);
    let Some(row) = plugin_patch_rows(content)?
        .into_iter()
        .find(|row| row_id(row) == Some(id))
    else {
        return Ok(true);
    };
    Ok(!row
        .as_mapping()
        .and_then(|mapping| mapping.get(yaml_key("disabled")))
        .and_then(YamlValue::as_bool)
        .unwrap_or(false))
}

pub fn update_plugin_patch(content: &str, package: &str, enabled: bool) -> Result<String, String> {
    let id = plugin_id(package);
    let mut rows = plugin_patch_rows(content)?;
    let disabled_key = yaml_key("disabled");
    if let Some(row) = rows.iter_mut().find(|row| row_id(row) == Some(id)) {
        let mapping = row
            .as_mapping_mut()
            .ok_or_else(|| "DSH plugin patch row must be a mapping".to_string())?;
        mapping.insert(disabled_key, YamlValue::Bool(!enabled));
    } else {
        let mut mapping = serde_yaml_ng::Mapping::new();
        mapping.insert(yaml_key("id"), YamlValue::String(id.to_string()));
        mapping.insert(disabled_key, YamlValue::Bool(!enabled));
        rows.push(YamlValue::Mapping(mapping));
    }
    serde_yaml_ng::to_string(&YamlValue::Sequence(rows))
        .map_err(|error| format!("Failed to serialize Aeroric DSH plugin patch: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPlugin {
    pub name: String,
    pub version: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub entry_id: String,
    pub module_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fiber_phase: Option<String>,
    pub built_in: bool,
}

pub fn platform_default_enabled(entry_id: &str) -> bool {
    match entry_id {
        "bash-sandbox" | "tool-bash" => !cfg!(windows),
        "pwsh-sandbox" | "tool-pwsh" => cfg!(windows),
        _ => true,
    }
}

pub fn parse_dsh_config_dump(content: &str) -> Result<Vec<DshPlugin>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows = serde_yaml_ng::from_str::<YamlValue>(content)
        .map_err(|error| format!("Failed to parse DSH Web profile config dump: {error}"))?
        .as_sequence()
        .cloned()
        .ok_or_else(|| "DSH Web profile config dump must be a YAML sequence".to_string())?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let mapping = row.as_mapping()?;
            if mapping
                .get(yaml_key("group"))
                .and_then(YamlValue::as_bool)
                .unwrap_or(false)
            {
                return None;
            }
            let entry_id = mapping.get(yaml_key("id"))?.as_str()?.to_string();
            let module_name = mapping.get(yaml_key("name"))?.as_str()?.to_string();
            let enabled = mapping
                .get(yaml_key("disabled"))
                .and_then(YamlValue::as_bool)
                .map(|disabled| !disabled)
                .unwrap_or_else(|| platform_default_enabled(&entry_id));
            Some(DshPlugin {
                name: module_name.clone(),
                version: "bundled".to_string(),
                enabled,
                description: None,
                entry_id,
                module_name: module_name.clone(),
                fiber_phase: enabled.then(|| "active".to_string()),
                built_in: module_name.starts_with("@deepseek-ai/")
                    || module_name.starts_with("cordis:"),
            })
        })
        .collect())
}

/// 配置视图与运行时视图的合并：live 行优先呈现，配置行补元数据；
/// 只在配置里出现的插件原样保留在尾部。
pub fn merge_runtime_plugins(
    configured: Vec<DshPlugin>,
    runtime: Vec<DshRuntimePlugin>,
) -> Vec<DshPlugin> {
    let mut configured_by_id: std::collections::HashMap<String, DshPlugin> = configured
        .into_iter()
        .map(|plugin| (plugin.entry_id.clone(), plugin))
        .collect();
    let mut merged = Vec::with_capacity(runtime.len() + configured_by_id.len());
    for live in runtime {
        let fallback = configured_by_id.remove(&live.entry_id);
        merged.push(DshPlugin {
            name: fallback
                .as_ref()
                .map(|plugin| plugin.name.clone())
                .unwrap_or_else(|| live.module_name.clone()),
            version: fallback
                .as_ref()
                .map(|plugin| plugin.version.clone())
                .unwrap_or_else(|| "bundled".to_string()),
            enabled: live.enabled,
            description: fallback.and_then(|plugin| plugin.description),
            entry_id: live.entry_id,
            built_in: live.module_name.starts_with("@deepseek-ai/")
                || live.module_name.starts_with("cordis:"),
            module_name: live.module_name,
            fiber_phase: live.fiber_phase,
        });
    }
    merged.extend(configured_by_id.into_values());
    merged
}

// ── 第十二片（补记二百六十一）：settings 形状族与 YAML mapping helpers，
// 机械迁自宿主 dsh_plugins.rs。settings.yaml 的文档形状（快照/局部更新）、
// DEFAULT_* 五常量与 mapping 读写原语是 wire 数据面；文件读、legacy 修复备份、
// 凭证侧车与 update 落盘（read/write_settings_document、credential_configured、
// settings_snapshot_at、update_* 编排）是 I/O，全部留宿主。
// 形状字段从模块私有升 `pub`＝宿主构造点跨 crate 可见性需要同步（serde 输出
// 逐字节不变；DshWebUiState/RecoveredSession 同先例）。

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshShellSettings {
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshAgentLoopSettings {
    pub max_parallel_tool_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWebSearchSettings {
    pub base_url: String,
    pub max_uses: u64,
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshAgentPreset {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSettingsSnapshot {
    pub shell: DshShellSettings,
    pub agent_loop: DshAgentLoopSettings,
    pub web_search: DshWebSearchSettings,
    pub default_preset: String,
    pub custom_presets: Vec<DshAgentPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellUpdate {
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLoopUpdate {
    pub max_parallel_tool_calls: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchUpdate {
    pub base_url: String,
    pub max_uses: u64,
    pub api_key: Option<String>,
}

pub const DEFAULT_SHELL_TIMEOUT_MS: u64 = 60_000;
pub const DEFAULT_SHELL_MAX_OUTPUT_BYTES: u64 = 64_000;
pub const DEFAULT_MAX_PARALLEL_TOOL_CALLS: u64 = 10;
pub const DEFAULT_WEB_SEARCH_MAX_USES: u64 = 5;
pub const DEFAULT_AGENT_PRESET: &str = "standard";

pub fn section<'a>(document: &'a YamlValue, key: &str) -> Option<&'a serde_yaml_ng::Mapping> {
    document.as_mapping()?.get(yaml_key(key))?.as_mapping()
}

pub fn section_mut<'a>(
    document: &'a mut YamlValue,
    key: &str,
) -> Result<&'a mut serde_yaml_ng::Mapping, String> {
    let root = document
        .as_mapping_mut()
        .ok_or_else(|| "dsh settings.yaml must contain a YAML mapping".to_string())?;
    let key_value = yaml_key(key);
    if !root.contains_key(&key_value) {
        root.insert(
            key_value.clone(),
            YamlValue::Mapping(serde_yaml_ng::Mapping::new()),
        );
    }
    let value = root
        .get_mut(&key_value)
        .ok_or_else(|| format!("dsh settings section {key} is unavailable"))?;
    if matches!(value, YamlValue::Null) {
        *value = YamlValue::Mapping(serde_yaml_ng::Mapping::new());
    }
    value
        .as_mapping_mut()
        .ok_or_else(|| format!("dsh settings section {key} must contain a YAML mapping"))
}

pub fn positive_u64(mapping: Option<&serde_yaml_ng::Mapping>, key: &str, fallback: u64) -> u64 {
    mapping
        .and_then(|value| value.get(yaml_key(key)))
        .and_then(YamlValue::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub fn optional_string(mapping: Option<&serde_yaml_ng::Mapping>, key: &str) -> Option<String> {
    mapping
        .and_then(|value| value.get(yaml_key(key)))
        .and_then(YamlValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
