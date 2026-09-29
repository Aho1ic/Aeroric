//! DSH API 调用的编解码面（P3-05 第二片起步）。
//!
//! 本片只收 `decode_result_value`——HTTP RPC 信封的**纯**解码：稳定 envelope
//! （`result.ok`／`result.error.message`／`result.value`）与两条在网错误文案
//! （`DSH API {method} response has no result`／`… rejected the request: {message}`）
//! 逐字迁移自宿主 `src-tauri/src/dsh_protocol.rs`（补记二百五十一），`value`
//! 缺省成 `Null` 的旧默认同样保真。真正的 HTTP 客户端（reqwest／鉴权／重试）
//! 仍留宿主 `dsh_webui/api_client.rs`——它是 I/O，按 sessions 的先后次序逐步上收。

use serde_json::Value;

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
