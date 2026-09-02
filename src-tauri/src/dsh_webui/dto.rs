//! DSH Web 的传输结构(前端 ⇄ 后端 ⇄ dsh 三方共用的 JSON 形状)。
//!
//! 从 `dsh_webui.rs` 整块搬出来,内容一行没改。原来这些结构夹在
//! `impl DshApiClient` 的两半之间,读那个 impl 得跨过 300 行 DTO。
//!
//! 这里只有数据和三个纯函数(`default_true`、`supported_dsh_reasoning_effort`、
//! 以及 `DshProtocolCapabilities` 的构造 impl),没有 IO。

use super::*;

/// Protocol inventory pinned to the source tree that Aeroric was audited
/// against.  Keeping this in the host (rather than inferring capabilities from
/// one optional endpoint) lets the UI show a useful compatibility diagnostic
/// when users point Aeroric at a newer/older Harness checkout.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshProtocolCapabilities {
    pub source_commit: &'static str,
    pub package_version: &'static str,
    pub protocol_version: u32,
    pub rpc_methods: Vec<&'static str>,
    pub remote_methods: Vec<&'static str>,
    pub remote_events: Vec<&'static str>,
    pub mux_frames: Vec<&'static str>,
    pub host_frames: Vec<&'static str>,
}

impl DshProtocolCapabilities {
    pub fn snapshot() -> Self {
        Self {
            source_commit: protocol_inventory::SOURCE_COMMIT,
            package_version: protocol_inventory::PACKAGE_VERSION,
            protocol_version: protocol_inventory::PROTOCOL_VERSION,
            rpc_methods: protocol_inventory::RPC_METHODS.to_vec(),
            remote_methods: protocol_inventory::REMOTE_METHODS.to_vec(),
            remote_events: protocol_inventory::REMOTE_EVENTS.to_vec(),
            mux_frames: protocol_inventory::MUX_FRAMES.to_vec(),
            host_frames: protocol_inventory::HOST_FRAMES.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshReasoningEffort {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub reasoning: Option<DshModelReasoning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelReasoning {
    pub efforts: Vec<DshReasoningEffort>,
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshModelGroup {
    pub id: String,
    pub name: String,
    pub models: Vec<DshModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshSessionModels {
    pub current: DshModelSelection,
    #[serde(default = "default_true")]
    pub routable: bool,
    pub groups: Vec<DshModelGroup>,
    #[serde(default)]
    pub failures: Vec<DshModelCatalogFailure>,
}

pub(super) fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshModelSelection {
    pub provider: String,
    pub model: String,
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
}

/// Keep model selection compatible with providers that do not implement DSH's
/// optional reasoning-effort parameter. The session model catalog is the
/// authority here: an omitted or empty capability list means the provider
/// accepts the model but not an explicit effort override.
pub(super) fn supported_dsh_reasoning_effort(
    models: &DshSessionModels,
    model: &str,
    requested: Option<String>,
) -> Option<String> {
    let requested = requested?;
    let model_info = models
        .groups
        .iter()
        .flat_map(|group| group.models.iter())
        .find(|item| item.id == model)?;
    let reasoning = model_info.reasoning.as_ref()?;
    reasoning
        .efforts
        .iter()
        .any(|effort| effort.id == requested)
        .then_some(requested)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetInfo {
    pub id: String,
    pub trust: String,
    pub is_default: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub broken: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetList {
    pub presets: Vec<DshPresetInfo>,
    pub authorable: bool,
    pub has_document: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DshSettingsNamespace {
    pub(super) ns: String,
    pub(super) revision: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DshSettingsDescription {
    pub(super) writable: bool,
    pub(super) namespaces: Vec<DshSettingsNamespace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSessionSummary {
    pub session_id: String,
    pub updated_at: u64,
    pub running: bool,
    pub blank: bool,
    pub parent_session_id: Option<String>,
    pub origin: Option<String>,
    pub cwd: Option<String>,
    pub agent_preset: Option<String>,
    pub projections: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSessionHistory {
    pub events: Vec<Value>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projections: Option<Value>,
}

// ── Workspace types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWorkspace {
    pub workspace_id: String,
    pub path: String,
    pub title: String,
    pub session_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWorkspaceList {
    pub items: Vec<DshWorkspace>,
    pub archived_session_ids: Vec<String>,
}

// ── Credentials types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshCredentialView {
    pub configured: bool,
    pub source: Option<String>,
    pub writable: bool,
}

// ── LLM types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshProviderInfo {
    pub provider: Option<String>,
    pub settings_ns: String,
    pub display_name: Option<String>,
    pub settings_path: Option<Vec<String>>,
    pub active: bool,
    pub declared: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelCatalogFailure {
    pub id: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshGlobalModels {
    pub groups: Vec<DshModelGroup>,
    pub failures: Vec<DshModelCatalogFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDiscoveredModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
}

// ── Subagent types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSubagentSummary {
    pub session_id: String,
    pub parent_session_id: String,
    pub running: bool,
    pub cwd: Option<String>,
    pub mode: Option<String>,
    pub label: Option<String>,
}

// ── Goal types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshGoal {
    pub goal_id: String,
    pub title: String,
    pub revision: u64,
    pub status: String,
    pub created_at: Option<String>,
}

// ── Host types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshHostInfo {
    pub version: Option<String>,
    pub cwd: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub attached_sessions: Option<u64>,
    pub can_open_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDirectoryEntry {
    pub name: String,
    pub path: String,
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDirectoryListing {
    pub path: String,
    pub home: String,
    pub crumbs: Vec<DshDirectoryEntry>,
    pub entries: Vec<DshDirectoryEntry>,
    pub truncated: bool,
}

// ── AgentPreset extended types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetReadResult {
    pub content: String,
    pub preset: String,
    pub trust: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

// ── Skill types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSkillEntry {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    pub model_invocable: Option<bool>,
}
