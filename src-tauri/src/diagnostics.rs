use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::ssh::SshConnection;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

fn join_diagnostic_path(root: &Path, file: &str) -> String {
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

fn detect_package_manager(root: &Path) -> Option<&'static str> {
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
        _ => unreachable!("unknown package manager"),
    };
    Ok((command, args))
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

fn normalize_remote_project_path(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    if trimmed == "/" {
        Ok("/".to_string())
    } else {
        Ok(trimmed.trim_end_matches('/').to_string())
    }
}

fn shell_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| crate::ssh::shell_word_posix(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn remote_javascript_tool_command(tool: &str, tool_args: &[&str]) -> String {
    let pnpm_args = std::iter::once("exec".to_string())
        .chain(std::iter::once(tool.to_string()))
        .chain(tool_args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let yarn_args = std::iter::once(tool.to_string())
        .chain(tool_args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let npm_args = std::iter::once("exec".to_string())
        .chain(std::iter::once("--".to_string()))
        .chain(std::iter::once(tool.to_string()))
        .chain(tool_args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();

    format!(
        "if [ -f pnpm-lock.yaml ]; then exec {}; elif [ -f yarn.lock ]; then exec {}; elif [ -f package-lock.json ]; then exec {}; else echo {}; exit 127; fi",
        shell_command("pnpm", &pnpm_args),
        shell_command("yarn", &yarn_args),
        shell_command("npm", &npm_args),
        crate::ssh::shell_quote_posix(
            "No JavaScript package manager lockfile found (pnpm-lock.yaml, yarn.lock, package-lock.json)"
        )
    )
}

fn remote_diagnostic_profile_command(profile: &str) -> String {
    match profile {
        "eslint" => remote_javascript_tool_command("eslint", &[".", "--format", "json"]),
        "cargo" => shell_command(
            "cargo",
            &["check".to_string(), "--message-format=json".to_string()],
        ),
        "ruff" => shell_command(
            "ruff",
            &[
                "check".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
            ],
        ),
        "mypy" => shell_command(
            "mypy",
            &[
                "--show-column-numbers".to_string(),
                "--no-color-output".to_string(),
                ".".to_string(),
            ],
        ),
        _ => remote_javascript_tool_command("tsc", &["--noEmit"]),
    }
}

fn remote_project_command(remote_project_path: &str, command: &str) -> String {
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(remote_project_path),
        command
    )
}

fn run_remote_project_command(
    connection: &SshConnection,
    remote_project_path: &str,
    command: &str,
) -> Result<Output, String> {
    let remote_root = normalize_remote_project_path(remote_project_path)?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        remote_project_command(&remote_root, command),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.output()
        .map_err(|e| format!("Failed to run remote diagnostics: {e}"))
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

fn parse_diagnostic_run_result(
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

fn run_profile(root: &Path, profile: &str) -> Result<DiagnosticRunResult, String> {
    let (program, args) = diagnostic_profile_command(root, profile)?;
    let mut cmd = Command::new(program);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("Failed to run diagnostics: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(parse_diagnostic_run_result(root, profile, &stdout, &stderr))
}

#[tauri::command]
pub async fn run_diagnostics(
    project_path: String,
    profile: String,
) -> Result<DiagnosticRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        let profile = match profile.as_str() {
            "eslint" => "eslint",
            "cargo" => "cargo",
            "ruff" => "ruff",
            "mypy" => "mypy",
            _ => "typescript",
        };
        run_profile(&root, profile)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_run_diagnostics(
    connection: SshConnection,
    remote_project_path: String,
    profile: String,
) -> Result<DiagnosticRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_project_path(&remote_project_path)?;
        let profile = match profile.as_str() {
            "eslint" => "eslint",
            "cargo" => "cargo",
            "ruff" => "ruff",
            "mypy" => "mypy",
            _ => "typescript",
        };
        let output = run_remote_project_command(
            &connection,
            &remote_root,
            &remote_diagnostic_profile_command(profile),
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let result =
            parse_diagnostic_run_result(Path::new(&remote_root), profile, &stdout, &stderr);
        if !output.status.success() && result.diagnostics.is_empty() {
            let message = result.raw_output.trim();
            return Err(if message.is_empty() {
                "Remote diagnostics failed".to_string()
            } else {
                message.to_string()
            });
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-diagnostics-test-{name}-{suffix}"))
    }

    #[test]
    fn parses_tsc_diagnostics() {
        let output =
            "src/App.tsx(10,5): error TS2322: Type 'string' is not assignable to type 'number'.";

        let diagnostics = parse_tsc_output(Path::new("/repo"), output);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "tsc");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].file, "/repo/src/App.tsx");
        assert_eq!(diagnostics[0].line, 10);
        assert_eq!(diagnostics[0].column, 5);
        assert_eq!(diagnostics[0].code.as_deref(), Some("TS2322"));
    }

    #[test]
    fn parses_eslint_json_diagnostics() {
        let output = r#"[{"filePath":"/repo/src/App.tsx","messages":[{"ruleId":"no-unused-vars","severity":2,"message":"'x' is assigned a value but never used.","line":3,"column":7}]}]"#;

        let diagnostics = parse_eslint_json(Path::new("/repo"), output);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "eslint");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].file, "/repo/src/App.tsx");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 7);
        assert_eq!(diagnostics[0].code.as_deref(), Some("no-unused-vars"));
    }

    #[test]
    fn detects_javascript_package_manager_from_lockfiles() {
        let root = unique_test_dir("package-manager");
        fs::create_dir_all(&root).unwrap();

        assert_eq!(detect_package_manager(&root), None);

        fs::write(root.join("package-lock.json"), "").unwrap();
        assert_eq!(detect_package_manager(&root), Some("npm"));

        fs::write(root.join("yarn.lock"), "").unwrap();
        assert_eq!(detect_package_manager(&root), Some("yarn"));

        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect_package_manager(&root), Some("pnpm"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_remote_typescript_diagnostic_command_with_package_manager_fallback() {
        let command = remote_project_command(
            "/srv/app's repo",
            &remote_diagnostic_profile_command("typescript"),
        );

        assert!(command.starts_with("cd -- '/srv/app'\\''s repo' && "));
        assert!(command.contains("pnpm exec tsc --noEmit"));
        assert!(command.contains("yarn tsc --noEmit"));
        assert!(command.contains("npm exec -- tsc --noEmit"));
    }

    #[test]
    fn parses_remote_diagnostics_against_remote_root() {
        let result = parse_diagnostic_run_result(
            Path::new("/srv/app"),
            "typescript",
            "",
            "src/App.tsx(2,3): error TS2322: Type mismatch",
        );

        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].file, "/srv/app/src/App.tsx");
        assert_eq!(result.diagnostics[0].line, 2);
    }

    #[test]
    fn parses_cargo_json_diagnostics() {
        let output = r#"{"reason":"compiler-message","message":{"message":"expected `;`, found `}`","code":{"code":"E0425"},"level":"error","spans":[{"file_name":"src/main.rs","line_start":7,"column_start":12,"is_primary":true}]}}
{"reason":"build-finished","success":false}"#;

        let diagnostics = parse_cargo_json(Path::new("/repo"), output);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "cargo");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].file, "/repo/src/main.rs");
        assert_eq!(diagnostics[0].line, 7);
        assert_eq!(diagnostics[0].column, 12);
        assert_eq!(diagnostics[0].code.as_deref(), Some("E0425"));
    }

    #[test]
    fn parses_ruff_json_diagnostics() {
        let output = r#"[{"code":"F401","filename":"src/app.py","location":{"row":2,"column":8},"message":"`os` imported but unused"}]"#;

        let diagnostics = parse_ruff_json(Path::new("/repo"), output);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "ruff");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostics[0].file, "/repo/src/app.py");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 8);
        assert_eq!(diagnostics[0].code.as_deref(), Some("F401"));
    }
}
