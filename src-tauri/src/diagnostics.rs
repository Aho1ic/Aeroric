use std::path::Path;
use std::process::Output;

use crate::path_guard::{normalize_remote_project_path, validate_project_root};

// ── 切片一百零七：DTO 与本地面主体上收 core `ide::diagnostics_face`
// （单一定义；remote 面与 shell_command 留守本文件）──
use crate::ssh::SshConnection;
pub use aeroric_core::ide::diagnostics_face::DiagnosticRunResult;
#[cfg(test)]
use aeroric_core::ide::diagnostics_face::DiagnosticSeverity;

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

#[tauri::command]
pub async fn run_diagnostics<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    profile: String,
) -> Result<DiagnosticRunResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.run_diagnostics",
        serde_json::json!({ "projectPath": project_path, "profile": profile }),
        async {
            validate_project_root(&project_path)?;
            let profile = match profile.as_str() {
                "eslint" => "eslint",
                "cargo" => "cargo",
                "ruff" => "ruff",
                "mypy" => "mypy",
                _ => "typescript",
            };
            aeroric_core::ide::diagnostics_face::run_profile(
                Path::new(&project_path),
                profile,
                crate::app_settings::get_login_shell_env(),
            )
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
        let result = aeroric_core::ide::diagnostics_face::parse_diagnostic_run_result(
            Path::new(&remote_root),
            profile,
            &stdout,
            &stderr,
        );
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
    use std::path::PathBuf;
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

        let diagnostics =
            aeroric_core::ide::diagnostics_face::parse_tsc_output(Path::new("/repo"), output);

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

        let diagnostics =
            aeroric_core::ide::diagnostics_face::parse_eslint_json(Path::new("/repo"), output);

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

        assert_eq!(
            aeroric_core::ide::diagnostics_face::detect_package_manager(&root),
            None
        );

        fs::write(root.join("package-lock.json"), "").unwrap();
        assert_eq!(
            aeroric_core::ide::diagnostics_face::detect_package_manager(&root),
            Some("npm")
        );

        fs::write(root.join("yarn.lock"), "").unwrap();
        assert_eq!(
            aeroric_core::ide::diagnostics_face::detect_package_manager(&root),
            Some("yarn")
        );

        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            aeroric_core::ide::diagnostics_face::detect_package_manager(&root),
            Some("pnpm")
        );

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
        let result = aeroric_core::ide::diagnostics_face::parse_diagnostic_run_result(
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

        let diagnostics =
            aeroric_core::ide::diagnostics_face::parse_cargo_json(Path::new("/repo"), output);

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

        let diagnostics =
            aeroric_core::ide::diagnostics_face::parse_ruff_json(Path::new("/repo"), output);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "ruff");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostics[0].file, "/repo/src/app.py");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 8);
        assert_eq!(diagnostics[0].code.as_deref(), Some("F401"));
    }
}
