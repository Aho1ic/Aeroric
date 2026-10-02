//! P6-02 Action 1：formatter 命令发现与执行（core 独占；命令发现/超时/
//! 取消/路径校验——与 `src-tauri/src/formatter.rs` 同源语义 verbatim 上收）。
use super::path_policy;
use std::path::{Path, PathBuf};
use std::process::Command;

/// formatter 命令（program + argv；argv 用参数数组，不经 shell）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatterCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// 格式化结果（与 Tauri `FormatFileResult` camelCase 字段对齐）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatFileResult {
    pub file_path: String,
    pub command: String,
}

/// 文件路径校验（绝对＋canonicalize 落根内＋是文件）。
pub fn validate_file_path(project_root: &Path, file_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(file_path);
    if !path.is_absolute() {
        return Err("File path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve file path: {e}"))?;
    if !canonical.starts_with(project_root) {
        return Err("File path is outside the project".to_string());
    }
    if !canonical.is_file() {
        return Err("File path is not a file".to_string());
    }
    Ok(canonical)
}

/// formatter 命令发现（与 Tauri `formatter_for_extension` 同源语义）。
pub fn formatter_for_extension(extension: &str, file_path: &str) -> Option<FormatterCommand> {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "js" | "jsx" | "ts" | "tsx" | "css" | "scss" | "json" | "jsonc" | "md" | "mdx" | "html"
        | "yaml" | "yml" => Some(FormatterCommand {
            program: "pnpm".to_string(),
            args: vec![
                "exec".to_string(),
                "prettier".to_string(),
                "--write".to_string(),
                file_path.to_string(),
            ],
        }),
        "rs" => Some(FormatterCommand {
            program: "rustfmt".to_string(),
            args: vec![file_path.to_string()],
        }),
        "go" => Some(FormatterCommand {
            program: "gofmt".to_string(),
            args: vec!["-w".to_string(), file_path.to_string()],
        }),
        "py" => Some(FormatterCommand {
            program: "ruff".to_string(),
            args: vec!["format".to_string(), file_path.to_string()],
        }),
        _ => None,
    }
}

/// 格式化（业务主体）：快照先记（调用方传 snapshot 闭包）、formatter 执行、
/// 失败原文不变（formatter 非零退出即拒绝——原文在磁盘上未被动过）。无超时
/// 参数（std Command 同步；超时/取消由调用方在 async 外层处理——与 Tauri
/// `format_file` 同形状）。
pub fn format_file(
    project_root: &Path,
    file_path: &str,
    snapshot_before: Option<&dyn Fn(&str) -> Result<(), String>>,
) -> Result<FormatFileResult, String> {
    let file = validate_file_path(project_root, file_path)?;
    let file_string = file.to_string_lossy().into_owned();
    if let Some(snapshot) = snapshot_before {
        snapshot(&file_string)?;
    }
    let extension = file
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| "No formatter is configured for this file type.".to_string())?;
    let formatter = formatter_for_extension(extension, &file_string)
        .ok_or_else(|| "No formatter is configured for this file type.".to_string())?;
    let mut cmd = Command::new(&formatter.program);
    cmd.args(&formatter.args).current_dir(project_root);
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run formatter: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Formatter failed: {stderr}"));
    }
    Ok(FormatFileResult {
        file_path: file_string,
        command: format!("{} {}", formatter.program, formatter.args.join(" ")),
    })
}

/// 内容大小守卫（复用 path_policy 的 2MiB；formatter 后文件不应超限）。
pub fn validate_size(file: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if metadata.len() > path_policy::MAX_FILE_BYTES {
        return Err(format!(
            "file exceeds the {} byte limit",
            path_policy::MAX_FILE_BYTES
        ));
    }
    Ok(())
}
