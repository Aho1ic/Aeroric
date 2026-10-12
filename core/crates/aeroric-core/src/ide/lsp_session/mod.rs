//! P8-05 切片一百一十(ide 刀五):LSP 会话面——响应解析、uri 编码、工作区
//! 编辑落盘与(本文件后半)会话引擎。
//!
//! 单一定义纪律:`LspPosition`/`LspHover`/… 这批 wire DTO 与三个纯函数簇
//! (`parse.rs` 解析 `Option<&Value>`、`protocol.rs` 的 uri/percent-encode、
//! `edit.rs` 的多文件写+回滚)全部自 `src-tauri/src/lsp.rs` verbatim 上收,
//! Tauri 侧按旧名 re-export——两侧不会各留一份定义。
//!
//! `edit.rs` **是副作用簇**:它改用户源文件、一次写多个,失败必须回滚
//! (本地 `canonicalize` 钉根,远端只能字符串判定)——两条不变量写在它的
//! 模块头里,搬运时原样保留。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod edit_local;
pub mod edit_remote;
pub mod edit_shared;
pub mod parse;
pub mod protocol;

pub use edit_local::{apply_workspace_edit_for_root, rollback_local_workspace_writes};
pub use edit_remote::{
    apply_remote_workspace_edit_for_root, rollback_remote_workspace_writes, RemoteEditIo,
};
pub use edit_shared::{
    apply_text_edits_to_content, lsp_position_to_offset, utf16_character_to_offset,
};
pub use parse::{
    parse_code_actions, parse_document_symbols, parse_inlay_hints, parse_signature_help,
    parse_workspace_edit, parse_workspace_symbols,
};
pub mod engine;

pub use engine::{CommandSpec, DiagnosticsSink, LspDocumentSnapshot, ServerLauncher};
pub use protocol::{
    file_uri, markdown_text, parse_completion_items, parse_hover, parse_locations, parse_position,
    parse_range, path_from_file_uri,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspLocation {
    pub uri: String,
    pub path: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspHover {
    pub contents: String,
    pub range: Option<LspRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspParameterInformation {
    pub label: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSignatureInformation {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<LspParameterInformation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSignatureHelp {
    pub signatures: Vec<LspSignatureInformation>,
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCommand {
    pub title: Option<String>,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCodeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edit: Option<LspWorkspaceEdit>,
    pub command: Option<LspCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspInlayHint {
    pub label: String,
    pub position: LspPosition,
    pub kind: Option<u32>,
    pub tooltip: Option<String>,
    pub padding_left: bool,
    pub padding_right: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSymbol {
    pub name: String,
    pub kind: u32,
    pub detail: Option<String>,
    pub container_name: Option<String>,
    pub uri: String,
    pub path: String,
    pub range: LspRange,
    pub selection_range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspTextEdit {
    pub range: LspRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspFileEdit {
    pub uri: String,
    pub path: String,
    pub edits: Vec<LspTextEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspWorkspaceEdit {
    pub files: Vec<LspFileEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspApplyWorkspaceEditSummary {
    pub files_changed: usize,
    pub edits_applied: usize,
    pub edits_skipped: usize,
}

/// 一次 LSP 请求的参数形态(自 `src-tauri/src/lsp.rs` 的 `LspFeatureParams`
/// verbatim 上收):位置型(可带额外字段)、整文档范围型、或调用方给的原始值。
#[derive(Debug, Clone)]
pub enum RequestParams {
    Position { extra_params: Option<Value> },
    DocumentRange,
    Raw(Value),
}

/// 整文档范围(行 0 到末行末列;末行按 UTF-16 码元计)。
pub fn full_document_range(content: &str) -> LspRange {
    let mut end_line = 0_u32;
    let mut last_line = "";
    for (index, line) in content.split('\n').enumerate() {
        end_line = index as u32;
        last_line = line;
    }
    let last_line = last_line.strip_suffix('\r').unwrap_or(last_line);
    LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: end_line,
            character: last_line.chars().map(|ch| ch.len_utf16() as u32).sum(),
        },
    }
}

/// 请求参数构造(原 `lsp_feature_request_params` 逐字)。
pub fn feature_request_params(
    document_uri: &str,
    request: &LspDocumentRequest,
    params: RequestParams,
) -> Value {
    match params {
        RequestParams::Position { extra_params } => {
            let mut request_params = serde_json::json!({
                "textDocument": { "uri": document_uri },
                "position": {
                    "line": request.line,
                    "character": request.character
                }
            });
            if let (Some(params), Some(object)) = (extra_params, request_params.as_object_mut()) {
                if let Some(extra) = params.as_object() {
                    for (key, value) in extra {
                        object.insert(key.clone(), value.clone());
                    }
                }
            }
            request_params
        }
        RequestParams::DocumentRange => {
            let range = full_document_range(&request.content);
            serde_json::json!({
                "textDocument": { "uri": document_uri },
                "range": range
            })
        }
        RequestParams::Raw(value) => value,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentRequest {
    pub project_path: String,
    pub file_path: String,
    pub content: String,
    pub line: u32,
    pub character: u32,
}

#[cfg(test)]
pub(crate) fn temp_project(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("aeroric-lsp-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}
