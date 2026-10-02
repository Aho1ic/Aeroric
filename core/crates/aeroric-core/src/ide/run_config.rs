//! P6-03 Action 1：run_config（core 独占；RunConfig 文档读写校验与快照形状
//! ——与 `src-tauri/src/run_config.rs` 同源语义 verbatim 上收；run 进程由宿主
//! 管理，本模块承载配置形状与纯校验）。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RUN_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunDebugConfigType {
    Node,
    Python,
}

/// 断点（与 Tauri `RunDebugBreakpoint` camelCase 对齐）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunDebugBreakpoint {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// RunConfig（tag = "type"：shell/debug）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunConfig {
    Shell {
        id: String,
        name: String,
        command: String,
        cwd: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Debug {
        id: String,
        name: String,
        #[serde(rename = "debugType", default = "default_run_debug_config_type")]
        debug_type: RunDebugConfigType,
        program: String,
        cwd: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        breakpoints: Vec<RunDebugBreakpoint>,
    },
}

pub fn default_run_debug_config_type() -> RunDebugConfigType {
    RunDebugConfigType::Node
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigDocument {
    #[serde(default = "default_run_config_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
}

fn default_run_config_version() -> u32 {
    RUN_CONFIG_VERSION
}

impl Default for RunConfigDocument {
    fn default() -> Self {
        Self {
            version: RUN_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

/// 运行状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunProcessStatus {
    Running,
    Exited,
    Failed,
    Stopped,
}

/// 运行快照（与 Tauri `RunProcessSnapshot` 对齐）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunProcessSnapshot {
    pub run_id: String,
    pub config_id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub status: RunProcessStatus,
    pub output: String,
    pub exit_code: Option<i32>,
    pub started_at: u128,
    pub finished_at: Option<u128>,
}

/// 配置文档校验（写前守卫：id 去重、非空 name；无效配置保存失败不污染旧
/// 配置）。
pub fn validate_document(document: &RunConfigDocument) -> Result<(), String> {
    if document.version != RUN_CONFIG_VERSION {
        return Err(format!(
            "run config version {} is not supported (expected {})",
            document.version, RUN_CONFIG_VERSION
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for config in &document.configs {
        let id = match config {
            RunConfig::Shell { id, name, .. } | RunConfig::Debug { id, name, .. } => {
                if name.trim().is_empty() {
                    return Err("config name must not be empty".to_string());
                }
                id.clone()
            }
        };
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate config id '{id}'"));
        }
    }
    Ok(())
}

/// 断点列表是否一致（BreakpointStore 的 diff 判定；mutation 身份）。
pub fn breakpoints_equal(current: &[RunDebugBreakpoint], incoming: &[RunDebugBreakpoint]) -> bool {
    let mut sorted_current = current.to_vec();
    sorted_current.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let mut sorted_incoming = incoming.to_vec();
    sorted_incoming.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    sorted_current == sorted_incoming
}

/// 按 id 查配置。
pub fn find_config<'a>(document: &'a RunConfigDocument, config_id: &str) -> Option<&'a RunConfig> {
    document.configs.iter().find(|c| match c {
        RunConfig::Shell { id, .. } | RunConfig::Debug { id, .. } => id == config_id,
    })
}
