//! P6-03 Action 1/4：DAP 协议形状与纯决策（core 独占；与 `src-tauri/src/
//! dap.rs` 同源语义 verbatim 上收；DAP process 由宿主管理，本模块承载协议
//! 形状、断点 store 与生命周期判定——断开 debug 不误 kill 其他任务）。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEBUG_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DebugConfigType {
    Node,
    Python,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugRequestType {
    Launch,
    Attach,
}

pub fn default_debug_request_type() -> DebugRequestType {
    DebugRequestType::Launch
}

pub fn default_attach_host() -> String {
    "127.0.0.1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugBreakpoint {
    pub file: String,
    pub line: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub config_type: DebugConfigType,
    #[serde(default = "default_debug_request_type")]
    pub request: DebugRequestType,
    pub program: String,
    pub cwd: String,
    #[serde(default = "default_attach_host")]
    pub attach_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_port: Option<u16>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub breakpoints: Vec<DebugBreakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfigDocument {
    #[serde(default = "default_debug_config_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<DebugConfig>,
}

fn default_debug_config_version() -> u32 {
    DEBUG_CONFIG_VERSION
}

impl Default for DebugConfigDocument {
    fn default() -> Self {
        Self {
            version: DEBUG_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugSessionStatus {
    Starting,
    Running,
    Paused,
    Exited,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugCallFrame {
    pub function_name: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugVariable {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(default)]
    pub has_children: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugVariableScope {
    pub name: String,
    pub variables: Vec<DebugVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugSessionSnapshot {
    pub debug_id: String,
    pub config_id: String,
    pub name: String,
    pub program: String,
    pub cwd: String,
    pub status: DebugSessionStatus,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    pub call_stack: Vec<DebugCallFrame>,
    pub scopes: Vec<DebugVariableScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub started_at: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugEvaluateResult {
    pub expression: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(default)]
    pub has_children: bool,
}

/// Debug 控件（Action 4：start/attach/pause/continue/step/stop 覆盖——
/// 每个动作映射一个 wire 命令；未知 adapter 状态可诊断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugAction {
    Start,
    Attach,
    Pause,
    Continue,
    StepOver,
    StepInto,
    StepOut,
    Stop,
}

/// 动作是否合法（生命周期判定：Start/Attach 只对无会话；其余对有会话；
/// 断开 debug 不误 kill 其他任务——按 debug_id 定向）。
pub fn is_action_allowed(action: DebugAction, has_session: bool, is_paused: bool) -> bool {
    match action {
        DebugAction::Start | DebugAction::Attach => !has_session,
        DebugAction::Pause => has_session && !is_paused,
        DebugAction::Continue => has_session && is_paused,
        DebugAction::StepOver | DebugAction::StepInto | DebugAction::StepOut => {
            has_session && is_paused
        }
        DebugAction::Stop => has_session,
    }
}

/// 断点 diff（BreakpointStore 的 mutation 身份；与 run_config::breakpoints_equal
/// 同语义，含 condition/logMessage 字段）。
pub fn breakpoints_equal(current: &[DebugBreakpoint], incoming: &[DebugBreakpoint]) -> bool {
    let mut sorted_current = current.to_vec();
    sorted_current.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let mut sorted_incoming = incoming.to_vec();
    sorted_incoming.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    sorted_current == sorted_incoming
}

/// 配置文档校验（写前守卫：id 去重、非空 name；无效配置保存失败不污染旧
/// 配置）。
pub fn validate_document(document: &DebugConfigDocument) -> Result<(), String> {
    if document.version != DEBUG_CONFIG_VERSION {
        return Err(format!(
            "debug config version {} is not supported (expected {})",
            document.version, DEBUG_CONFIG_VERSION
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for config in &document.configs {
        if config.name.trim().is_empty() {
            return Err("config name must not be empty".to_string());
        }
        if !ids.insert(config.id.clone()) {
            return Err(format!("duplicate config id '{}'", config.id));
        }
    }
    Ok(())
}
