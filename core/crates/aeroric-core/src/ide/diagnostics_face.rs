//! P8-05 切片一百零七（ide 刀二）：diagnostics 命令族本地面——DTO 与剖析器/
//! 命令构建/运行主体（`src-tauri/src/diagnostics.rs` verbatim 上收；remote
//! 孪生与 `shell_command` 留 Tauri SSH 适配层）。与 tests_face 同款差异注入：
//! login shell 环境由调用方给（宿主空表／Tauri 平台装配）。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRunResult {
    pub profile: String,
    pub diagnostics: Vec<DiagnosticItem>,
    pub raw_output: String,
}

#[derive(Debug, Deserialize)]
struct EslintFileResult {
    #[serde(rename = "filePath")]
    file_path: String,
    messages: Vec<EslintMessage>,
}

#[derive(Debug, Deserialize)]
struct EslintMessage {
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    severity: u8,
    message: String,
    line: Option<usize>,
    column: Option<usize>,
}

pub fn join_diagnostic_path(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else {
        root.join(path).to_string_lossy().into_owned()
    }
}

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

pub fn parse_eslint_json(_root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let Ok(files) = serde_json::from_str::<Vec<EslintFileResult>>(output) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for file in files {
        for message in file.messages {
            diagnostics.push(DiagnosticItem {
                source: "eslint".to_string(),
                severity: match message.severity {
                    2 => DiagnosticSeverity::Error,
                    1 => DiagnosticSeverity::Warning,
                    _ => DiagnosticSeverity::Info,
                },
                message: message.message,
                file: file.file_path.clone(),
                line: message.line.unwrap_or(1),
                column: message.column.unwrap_or(1),
                code: message.rule_id,
            });
        }
    }
    diagnostics
}

pub fn detect_package_manager(root: &Path) -> Option<&'static str> {
    if root.join("pnpm-lock.yaml").is_file() {
        Some("pnpm")
    } else if root.join("yarn.lock").is_file() {
        Some("yarn")
    } else if root.join("package-lock.json").is_file() {
        Some("npm")
    } else {
        None
    }
}

fn javascript_profile_command(
    root: &Path,
    tool: &str,
    tool_args: &[&str],
) -> Result<(String, Vec<String>), String> {
    let Some(package_manager) = detect_package_manager(root) else {
        return Err(
            "No JavaScript package manager lockfile found (pnpm-lock.yaml, yarn.lock, package-lock.json)"
                .to_string(),
        );
    };
    let command = package_manager.to_string();
    let args = match package_manager {
        "pnpm" => {
            let mut args = vec!["exec".to_string(), tool.to_string()];
            args.extend(tool_args.iter().map(|arg| arg.to_string()));
            args
        }
        "yarn" => {
            let mut args = vec![tool.to_string()];
            args.extend(tool_args.iter().map(|arg| arg.to_string()));
            args
        }
        "npm" => {
            let mut args = vec!["exec".to_string(), "--".to_string(), tool.to_string()];
            args.extend(tool_args.iter().map(|arg| arg.to_string()));
            args
        }
        _ => return Err("unknown package manager".to_string()),
    };
    Ok((command, args))
}

/// Diagnostics worth running unattended in `root` (A05 auto checks): fast,
/// low-noise tools whose config is present. eslint and mypy are left to the
/// manual Diagnostics panel.
pub fn discover_diagnostic_profiles(root: &Path) -> Vec<(&'static str, &'static str)> {
    let mut profiles = Vec::new();
    if root.join("Cargo.toml").is_file() {
        profiles.push(("cargo", "cargo check"));
    }
    if root.join("tsconfig.json").is_file() && detect_package_manager(root).is_some() {
        profiles.push(("tsc", "tsc"));
    }
    let pyproject_has_ruff = std::fs::read_to_string(root.join("pyproject.toml"))
        .is_ok_and(|text| text.contains("[tool.ruff"));
    if root.join("ruff.toml").is_file() || root.join(".ruff.toml").is_file() || pyproject_has_ruff {
        profiles.push(("ruff", "ruff"));
    }
    profiles
}

/// Program and arguments for a diagnostics profile (shared with auto checks).
pub fn diagnostic_command(root: &Path, profile: &str) -> Result<(String, Vec<String>), String> {
    diagnostic_profile_command(root, profile)
}

fn diagnostic_profile_command(root: &Path, profile: &str) -> Result<(String, Vec<String>), String> {
    match profile {
        "eslint" => javascript_profile_command(root, "eslint", &[".", "--format", "json"]),
        "cargo" => Ok((
            "cargo".to_string(),
            vec!["check".to_string(), "--message-format=json".to_string()],
        )),
        "ruff" => Ok((
            "ruff".to_string(),
            vec![
                "check".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
            ],
        )),
        "mypy" => Ok((
            "mypy".to_string(),
            vec![
                "--show-column-numbers".to_string(),
                "--no-color-output".to_string(),
                ".".to_string(),
            ],
        )),
        _ => javascript_profile_command(root, "tsc", &["--noEmit"]),
    }
}

fn severity_from_level(level: &str) -> DiagnosticSeverity {
    match level {
        "error" => DiagnosticSeverity::Error,
        "warning" | "warn" => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

pub fn parse_cargo_json(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let mut diagnostics = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let Some(spans) = message.get("spans").and_then(Value::as_array) else {
            continue;
        };
        let Some(span) = spans
            .iter()
            .find(|span| {
                span.get("is_primary")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| spans.first())
        else {
            continue;
        };
        let Some(file_name) = span.get("file_name").and_then(Value::as_str) else {
            continue;
        };
        diagnostics.push(DiagnosticItem {
            source: "cargo".to_string(),
            severity: severity_from_level(
                message
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("info"),
            ),
            message: message
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            file: join_diagnostic_path(root, file_name),
            line: span.get("line_start").and_then(Value::as_u64).unwrap_or(1) as usize,
            column: span
                .get("column_start")
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize,
            code: message
                .get("code")
                .and_then(|code| code.get("code"))
                .and_then(Value::as_str)
                .map(|code| code.to_string()),
        });
    }
    diagnostics
}

pub fn parse_ruff_json(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    let Ok(items) = serde_json::from_str::<Vec<Value>>(output) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let filename = item.get("filename").and_then(Value::as_str)?;
            let location = item.get("location")?;
            Some(DiagnosticItem {
                source: "ruff".to_string(),
                severity: DiagnosticSeverity::Warning,
                message: item
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                file: join_diagnostic_path(root, filename),
                line: location.get("row").and_then(Value::as_u64).unwrap_or(1) as usize,
                column: location.get("column").and_then(Value::as_u64).unwrap_or(1) as usize,
                code: item
                    .get("code")
                    .and_then(Value::as_str)
                    .map(|code| code.to_string()),
            })
        })
        .collect()
}

pub fn parse_mypy_output(root: &Path, output: &str) -> Vec<DiagnosticItem> {
    #[allow(clippy::expect_used, reason = "字面量正则，不可能失败")]
    let pattern = regex::Regex::new(
        r"^(?P<file>.*?):(?P<line>\d+)(?::(?P<column>\d+))?: (?P<level>error|warning|note): (?P<message>.*?)(?:\s+\[(?P<code>[^\]]+)\])?$",
    )
    .expect("valid mypy diagnostic regex");
    output
        .lines()
        .filter_map(|line| {
            let captures = pattern.captures(line)?;
            let file = captures.name("file")?.as_str();
            let level = captures.name("level")?.as_str();
            Some(DiagnosticItem {
                source: "mypy".to_string(),
                severity: severity_from_level(level),
                message: captures
                    .name("message")
                    .map(|message| message.as_str().to_string())
                    .unwrap_or_default(),
                file: join_diagnostic_path(root, file),
                line: captures
                    .name("line")
                    .and_then(|line| line.as_str().parse().ok())
                    .unwrap_or(1),
                column: captures
                    .name("column")
                    .and_then(|column| column.as_str().parse().ok())
                    .unwrap_or(1),
                code: captures.name("code").map(|code| code.as_str().to_string()),
            })
        })
        .collect()
}

pub fn parse_diagnostic_run_result(
    root: &Path,
    profile: &str,
    stdout: &str,
    stderr: &str,
) -> DiagnosticRunResult {
    let raw_output = if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}{stderr}")
    };
    let diagnostics = match profile {
        "eslint" => parse_eslint_json(root, stdout),
        "cargo" => parse_cargo_json(root, stdout),
        "ruff" => parse_ruff_json(root, stdout),
        "mypy" => parse_mypy_output(root, &raw_output),
        _ => parse_tsc_output(root, &raw_output),
    };
    DiagnosticRunResult {
        profile: profile.to_string(),
        diagnostics,
        raw_output,
    }
}

pub fn run_profile(
    root: &Path,
    profile: &str,
    envs: &[(String, String)],
) -> Result<DiagnosticRunResult, String> {
    let (program, args) = diagnostic_profile_command(root, profile)?;
    let mut cmd = Command::new(program);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    let output = cmd
        .args(args)
        .current_dir(root)
        .envs(envs.iter().cloned())
        .output()
        .map_err(|e| format!("Failed to run diagnostics: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(parse_diagnostic_run_result(root, profile, &stdout, &stderr))
}
