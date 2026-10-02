//! P6-03 Action 1：LSP 协议 parser 子模块（core 独占；offset encoding 转换
//! 与位置协议——与 `src-tauri/src/lsp.rs` 同源语义 verbatim 上收；LSP process
//! 生命周期在宿主，本模块只承载协议形状与纯转换）。
use serde::{Deserialize, Serialize};

/// LSP 位置（LSP 协议 UTF-16 code unit 编码——Swift 各组件不自行换算，这里
/// 统一承载；与 `LspPosition` 字段对齐）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// LSP range。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// LSP location。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspLocation {
    pub uri: String,
    pub path: String,
    pub range: LspRange,
}

/// UTF-16 code unit offset 转换（核心统一转换——Swift 组件不自行换算）。
/// LSP 的 character 是 UTF-16 code unit；String 索引是 UTF-8 字节/字符。
pub fn utf16_character_to_string_index(line: &str, character: u32) -> usize {
    // 按 UTF-16 code unit 数推进；character 落在字符的 UTF-16 span 内时
    // clamp：surrogate pair 内返回该字符起点（LSP 惯例：UTF-16 偏移必须
    // 落在字符边界上）。逐字符推进——先返回边界，再累计 units。
    let mut units = 0u32;
    for (index, ch) in line.char_indices() {
        if units >= character {
            return index;
        }
        let ch_units = ch.len_utf16() as u32;
        if units + ch_units > character {
            // 落在 ch 的 span 内：surrogate pair 只在 ch_len_utf16 == 2 且
            // character == units + 1 时发生；返回 ch 起点。
            return index;
        }
        units += ch_units;
    }
    line.len()
}

/// String index → UTF-16 character（回转）。
pub fn string_index_to_utf16_character(line: &str, index: usize) -> u32 {
    line.char_indices()
        .take_while(|(i, _)| *i < index)
        .map(|(_, ch)| ch.len_utf16() as u32)
        .sum()
}

/// document version（didOpen/change/close 的版本簿记；过时 diagnostics 丢弃
/// 的判据）。
pub fn is_stale_document_version(current: u32, incoming: u32) -> bool {
    incoming < current
}

/// language id（路径 → LSP languageId；与 `language_id_for_path` 同源）。
pub fn language_id_for_path(path: impl AsRef<std::path::Path>) -> Option<&'static str> {
    let extension = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Some("rust"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "py" => Some("python"),
        "go" => Some("go"),
        "json" => Some("json"),
        "css" | "scss" | "less" => Some("css"),
        "html" | "htm" => Some("html"),
        "md" | "markdown" => Some("markdown"),
        "yaml" | "yml" => Some("yaml"),
        _ => None,
    }
}

/// 默认 server 命令（与 `default_server_command` 同源；按 workspace/language
/// 生命周期管理——进程由宿主管理，这里只承载命令发现）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn default_server_command(language_id: &str) -> Option<LspServerCommand> {
    match language_id {
        "rust" => Some(LspServerCommand {
            program: "rust-analyzer".to_string(),
            args: vec![],
        }),
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => {
            Some(LspServerCommand {
                program: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
            })
        }
        "python" => Some(LspServerCommand {
            program: "pylsp".to_string(),
            args: vec![],
        }),
        "go" => Some(LspServerCommand {
            program: "gopls".to_string(),
            args: vec![],
        }),
        _ => None,
    }
}
