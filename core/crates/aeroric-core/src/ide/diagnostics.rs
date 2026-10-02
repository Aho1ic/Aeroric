//! P6-03 Action 1：诊断 parser（core 独占；tsc/eslint/cargo/ruff/mypy 五源
//! 解析——与 `src-tauri/src/diagnostics.rs` 同源语义 verbatim 上收；过时
//! diagnostics 丢弃由 lsp::is_stale_document_version 承载）。
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 诊断严重度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// 诊断条目（与 Tauri `DiagnosticItem` camelCase 字段对齐）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItem {
    pub source: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub code: Option<String>,
}

/// 诊断运行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRunResult {
    pub profile: String,
    pub diagnostics: Vec<DiagnosticItem>,
}

fn join_diagnostic_path(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    if path.is_absolute() {
        file.to_string()
    } else {
        root.join(file).to_string_lossy().into_owned()
    }
}

/// tsc 输出解析（verbatim 上收）。
pub fn parse_tsc_output(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let mut diagnostics = Vec::new();
    for line in output.lines() {
        let Some((file_part, rest)) = line.split_once("): ") else {
            continue;
        };
        let Some((file, location)) = file_part.rsplit_once('(') else {
            continue;
        };
        let Some((line_number, column_number)) = location.split_once(',') else {
            continue;
        };
        let Some((level_and_code, message)) = rest.split_once(": ") else {
            continue;
        };
        let mut level_parts = level_and_code.split_whitespace();
        let severity = match level_parts.next() {
            Some("error") => DiagnosticSeverity::Error,
            Some("warning") => DiagnosticSeverity::Warning,
            _ => DiagnosticSeverity::Info,
        };
        let code = level_parts.next().map(|code| code.to_string());
        diagnostics.push(DiagnosticItem {
            source: "tsc".to_string(),
            severity,
            message: message.to_string(),
            file: join_diagnostic_path(root, file),
            line: line_number.parse().unwrap_or(1),
            column: column_number.parse().unwrap_or(1),
            code,
        });
    }
    diagnostics
}

#[derive(serde::Deserialize)]
struct EslintMessage {
    #[serde(default)]
    line: usize,
    #[serde(default)]
    column: usize,
    #[serde(default)]
    message: String,
    #[serde(default)]
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    #[serde(default)]
    severity: i64,
}

#[derive(serde::Deserialize)]
struct EslintFileResult {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(default)]
    messages: Vec<EslintMessage>,
}

/// eslint JSON 解析。
pub fn parse_eslint_json(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let Ok(files) = serde_json::from_str::<Vec<EslintFileResult>>(output) else {
        return Vec::new();
    };
    files
        .into_iter()
        .flat_map(|file| {
            file.messages
                .into_iter()
                .map(move |message| DiagnosticItem {
                    source: "eslint".to_string(),
                    severity: if message.severity >= 2 {
                        DiagnosticSeverity::Error
                    } else {
                        DiagnosticSeverity::Warning
                    },
                    message: message.message,
                    file: join_diagnostic_path(root, &file.file_path),
                    line: message.line,
                    column: message.column,
                    code: message.rule_id,
                })
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct CargoMessage {
    #[serde(default)]
    message: Option<CargoMessageInner>,
}

#[derive(serde::Deserialize)]
struct CargoMessageInner {
    #[serde(default)]
    rendered: Option<String>,
    #[serde(default)]
    message: String,
    #[serde(default)]
    level: String,
    #[serde(default)]
    spans: Vec<CargoSpan>,
}

#[derive(serde::Deserialize, Clone)]
struct CargoSpan {
    #[serde(rename = "file_name", default)]
    file_name: String,
    #[serde(default)]
    line_start: usize,
    #[serde(default)]
    column_start: usize,
}

/// cargo JSON 解析。
pub fn parse_cargo_json(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let mut diagnostics = Vec::new();
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<CargoMessage>(line) else {
            continue;
        };
        let Some(message) = value.message else {
            continue;
        };
        let severity = match message.level.as_str() {
            "error" => DiagnosticSeverity::Error,
            "warning" => DiagnosticSeverity::Warning,
            _ => DiagnosticSeverity::Info,
        };
        let span = message.spans.first().cloned().unwrap_or(CargoSpan {
            file_name: String::new(),
            line_start: 1,
            column_start: 1,
        });
        diagnostics.push(DiagnosticItem {
            source: "cargo".to_string(),
            severity,
            message: message.rendered.unwrap_or(message.message),
            file: join_diagnostic_path(root, &span.file_name),
            line: span.line_start,
            column: span.column_start,
            code: None,
        });
    }
    diagnostics
}

#[derive(serde::Deserialize)]
struct RuffDiagnostic {
    #[serde(default)]
    code: Option<RuffCode>,
    #[serde(default)]
    message: String,
    #[serde(default)]
    location: RuffLocation,
    #[serde(default)]
    filename: String,
}

#[derive(serde::Deserialize, Default)]
struct RuffCode {
    #[serde(default)]
    code: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct RuffLocation {
    #[serde(default)]
    row: usize,
    #[serde(default)]
    column: usize,
}

/// ruff JSON 解析。
pub fn parse_ruff_json(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let Ok(items) = serde_json::from_str::<Vec<RuffDiagnostic>>(output) else {
        return Vec::new();
    };
    items
        .into_iter()
        .map(|item| DiagnosticItem {
            source: "ruff".to_string(),
            severity: DiagnosticSeverity::Warning,
            message: item.message,
            file: join_diagnostic_path(root, &item.filename),
            line: item.location.row,
            column: item.location.column,
            code: item.code.and_then(|c| c.code),
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct MypyOutput {
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: usize,
    #[serde(default)]
    column: usize,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: Option<String>,
}

/// mypy 输出解析。
pub fn parse_mypy_output(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let Ok(items) = serde_json::from_str::<Vec<MypyOutput>>(output) else {
        return Vec::new();
    };
    items
        .into_iter()
        .map(|item| DiagnosticItem {
            source: "mypy".to_string(),
            severity: match item.severity.as_str() {
                "error" => DiagnosticSeverity::Error,
                "warning" => DiagnosticSeverity::Warning,
                _ => DiagnosticSeverity::Info,
            },
            message: item.message,
            file: join_diagnostic_path(root, &item.file),
            line: item.line,
            column: item.column,
            code: item.code,
        })
        .collect()
}

/// 过时诊断丢弃（Action 4：过时 diagnostics 丢弃——document version 守卫）。
pub fn drop_stale(
    current_version: u32,
    incoming_version: u32,
    diagnostics: Vec<DiagnosticItem>,
) -> Vec<DiagnosticItem> {
    if incoming_version < current_version {
        Vec::new()
    } else {
        diagnostics
    }
}
