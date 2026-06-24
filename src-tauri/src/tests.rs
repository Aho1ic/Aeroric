use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestRunStatus {
    Passed,
    Failed,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestProfile {
    pub id: String,
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub profile: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub status: TestRunStatus,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestFailure {
    pub profile: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestRunResult {
    pub profile: String,
    pub status: TestRunStatus,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub tests: Vec<TestCase>,
    pub failures: Vec<TestFailure>,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestDiscoveryResult {
    pub profiles: Vec<TestProfile>,
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

fn join_test_path(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else {
        root.join(path).to_string_lossy().into_owned()
    }
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

fn package_json_has_vitest(root: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return raw.contains("vitest");
    };

    for key in ["dependencies", "devDependencies", "optionalDependencies"] {
        if value.get(key).and_then(|deps| deps.get("vitest")).is_some() {
            return true;
        }
    }
    value
        .get("scripts")
        .and_then(Value::as_object)
        .map(|scripts| {
            scripts
                .values()
                .filter_map(Value::as_str)
                .any(|script| script.contains("vitest"))
        })
        .unwrap_or(false)
}

fn has_vitest_config(root: &Path) -> bool {
    [
        "vitest.config.ts",
        "vitest.config.js",
        "vitest.config.mts",
        "vite.config.ts",
        "vite.config.js",
    ]
    .iter()
    .any(|file| root.join(file).is_file())
}

fn vitest_command(root: &Path) -> Option<(String, Vec<String>, String)> {
    let package_manager = detect_package_manager(root)?;
    let args = match package_manager {
        "pnpm" => vec!["exec", "vitest", "run", "--reporter=json"],
        "yarn" => vec!["vitest", "run", "--reporter=json"],
        "npm" => vec!["exec", "--", "vitest", "run", "--reporter=json"],
        _ => return None,
    }
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let display = format!("{package_manager} {}", args.join(" "));
    Some((package_manager.to_string(), args, display))
}

fn cargo_manifest(root: &Path) -> Option<PathBuf> {
    let root_manifest = root.join("Cargo.toml");
    if root_manifest.is_file() {
        return Some(root_manifest);
    }
    let tauri_manifest = root.join("src-tauri").join("Cargo.toml");
    if tauri_manifest.is_file() {
        return Some(tauri_manifest);
    }
    None
}

fn cargo_command(root: &Path) -> Option<(String, Vec<String>, String)> {
    let manifest = cargo_manifest(root)?;
    let relative_manifest = manifest
        .strip_prefix(root)
        .unwrap_or(&manifest)
        .to_string_lossy()
        .into_owned();
    let args = if relative_manifest == "Cargo.toml" {
        vec!["test".to_string(), "--message-format=json".to_string()]
    } else {
        vec![
            "test".to_string(),
            "--manifest-path".to_string(),
            relative_manifest,
            "--message-format=json".to_string(),
        ]
    };
    let display = format!("cargo {}", args.join(" "));
    Some(("cargo".to_string(), args, display))
}

pub fn discover_test_profiles_from_root(root: &Path) -> Result<Vec<TestProfile>, String> {
    let mut profiles = Vec::new();
    if package_json_has_vitest(root) || has_vitest_config(root) {
        if let Some((_, _, command)) = vitest_command(root) {
            profiles.push(TestProfile {
                id: "vitest".to_string(),
                label: "Vitest".to_string(),
                command,
            });
        }
    }
    if let Some((_, _, command)) = cargo_command(root) {
        profiles.push(TestProfile {
            id: "cargo".to_string(),
            label: "Cargo".to_string(),
            command,
        });
    }
    Ok(profiles)
}

fn numeric_field(value: &Value, name: &str) -> usize {
    value.get(name).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn string_field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn location_from_message(
    root: &Path,
    fallback_file: &str,
    message: &str,
) -> (String, usize, usize) {
    let pattern = regex::Regex::new(
        r"(?m)(?P<file>(?:[A-Za-z]:)?[/\\]?[A-Za-z0-9_./\\-][^:\n]*?):(?P<line>\d+):(?P<column>\d+)",
    )
    .expect("valid test location regex");
    if let Some(captures) = pattern.captures(message) {
        let file = captures
            .name("file")
            .map(|m| m.as_str().trim())
            .unwrap_or(fallback_file);
        let file = file.strip_prefix("at ").map(str::trim).unwrap_or(file);
        let line = captures
            .name("line")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let column = captures
            .name("column")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        return (join_test_path(root, file.trim()), line, column);
    }
    (join_test_path(root, fallback_file), 1, 1)
}

pub fn parse_vitest_json(root: &Path, output: &str) -> TestRunResult {
    let json_start = output.find('{').unwrap_or(0);
    let parsed = serde_json::from_str::<Value>(&output[json_start..]);
    let Ok(value) = parsed else {
        return TestRunResult {
            profile: "vitest".to_string(),
            status: TestRunStatus::Error,
            total: 0,
            passed: 0,
            failed: 0,
            tests: Vec::new(),
            failures: Vec::new(),
            raw_output: output.to_string(),
        };
    };

    let mut tests = Vec::new();
    let mut failures = Vec::new();
    for file in value
        .get("testResults")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let file_name = string_field(file, "name").unwrap_or("");
        for assertion in file
            .get("assertionResults")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = string_field(assertion, "fullName")
                .or_else(|| string_field(assertion, "title"))
                .unwrap_or("Unnamed test")
                .to_string();
            let status = match string_field(assertion, "status") {
                Some("failed") => TestRunStatus::Failed,
                Some("passed") => TestRunStatus::Passed,
                _ => TestRunStatus::Error,
            };
            let failure_message = assertion
                .get("failureMessages")
                .and_then(Value::as_array)
                .map(|messages| {
                    messages
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let (file_path, line, column) =
                location_from_message(root, file_name, &failure_message);
            tests.push(TestCase {
                profile: "vitest".to_string(),
                name: name.clone(),
                file: file_path.clone(),
                line,
                column,
                status: status.clone(),
                duration_ms: assertion.get("duration").and_then(Value::as_f64),
            });
            if status == TestRunStatus::Failed {
                failures.push(TestFailure {
                    profile: "vitest".to_string(),
                    name,
                    file: file_path,
                    line,
                    column,
                    message: failure_message,
                });
            }
        }
    }

    let total = numeric_field(&value, "numTotalTests");
    let passed = numeric_field(&value, "numPassedTests");
    let failed = numeric_field(&value, "numFailedTests");
    let status = if value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(failed == 0)
    {
        TestRunStatus::Passed
    } else {
        TestRunStatus::Failed
    };

    TestRunResult {
        profile: "vitest".to_string(),
        status,
        total,
        passed,
        failed,
        tests,
        failures,
        raw_output: output.to_string(),
    }
}

pub fn parse_cargo_test_output(root: &Path, output: &str) -> TestRunResult {
    let test_line = regex::Regex::new(r"^test (?P<name>.+?) \.\.\. (?P<status>ok|FAILED|ignored)")
        .expect("valid cargo test line regex");
    let summary = regex::Regex::new(
        r"test result: (?P<status>ok|FAILED)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed;",
    )
    .expect("valid cargo summary regex");
    let section = regex::Regex::new(r"^---- (?P<name>.+?) stdout ----$")
        .expect("valid cargo failure section regex");
    let panic = regex::Regex::new(r"panicked at (?P<file>.*?):(?P<line>\d+):(?P<column>\d+):")
        .expect("valid cargo panic regex");

    let mut tests = Vec::new();
    let mut current_failure: Option<(String, Vec<String>)> = None;
    let mut failure_sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        if let Some(captures) = test_line.captures(line) {
            let name = captures.name("name").map(|m| m.as_str()).unwrap_or("");
            let status = match captures.name("status").map(|m| m.as_str()) {
                Some("ok") => TestRunStatus::Passed,
                Some("FAILED") => TestRunStatus::Failed,
                _ => TestRunStatus::Error,
            };
            tests.push(TestCase {
                profile: "cargo".to_string(),
                name: name.to_string(),
                file: String::new(),
                line: 1,
                column: 1,
                status,
                duration_ms: None,
            });
        }

        if let Some(captures) = summary.captures(line) {
            passed = captures
                .name("passed")
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            failed = captures
                .name("failed")
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
        }

        if let Some(captures) = section.captures(line) {
            if let Some(previous) = current_failure.take() {
                failure_sections.push(previous);
            }
            current_failure = Some((
                captures
                    .name("name")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                Vec::new(),
            ));
            continue;
        }

        if let Some((_, lines)) = current_failure.as_mut() {
            lines.push(line.to_string());
        }
    }
    if let Some(previous) = current_failure.take() {
        failure_sections.push(previous);
    }

    let failures = failure_sections
        .into_iter()
        .map(|(name, lines)| {
            let body = lines.join("\n");
            let (file, line, column) = panic
                .captures(&body)
                .map(|captures| {
                    let file = captures.name("file").map(|m| m.as_str()).unwrap_or("");
                    let line = captures
                        .name("line")
                        .and_then(|m| m.as_str().parse().ok())
                        .unwrap_or(1);
                    let column = captures
                        .name("column")
                        .and_then(|m| m.as_str().parse().ok())
                        .unwrap_or(1);
                    (join_test_path(root, file), line, column)
                })
                .unwrap_or_else(|| (String::new(), 1, 1));
            TestFailure {
                profile: "cargo".to_string(),
                name,
                file,
                line,
                column,
                message: body.trim().to_string(),
            }
        })
        .collect::<Vec<_>>();

    let total = if passed + failed > 0 {
        passed + failed
    } else {
        tests.len()
    };
    let status = if failed > 0 {
        TestRunStatus::Failed
    } else {
        TestRunStatus::Passed
    };

    TestRunResult {
        profile: "cargo".to_string(),
        status,
        total,
        passed,
        failed,
        tests,
        failures,
        raw_output: output.to_string(),
    }
}

fn run_profile(root: &Path, profile: &str) -> Result<TestRunResult, String> {
    let (program, args, _) = match profile {
        "cargo" => {
            cargo_command(root).ok_or_else(|| "Cargo tests were not detected".to_string())?
        }
        _ => vitest_command(root).ok_or_else(|| "Vitest tests were not detected".to_string())?,
    };
    let mut command = Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    let output = command
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("Failed to run tests: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let raw_output = if stderr.trim().is_empty() {
        stdout.clone()
    } else {
        format!("{stdout}{stderr}")
    };
    let mut result = match profile {
        "cargo" => parse_cargo_test_output(root, &raw_output),
        _ => parse_vitest_json(root, &stdout),
    };
    result.raw_output = raw_output;
    if !output.status.success() && result.failures.is_empty() {
        result.status = TestRunStatus::Error;
    }
    Ok(result)
}

#[tauri::command]
pub async fn discover_tests(project_path: String) -> Result<TestDiscoveryResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        Ok(TestDiscoveryResult {
            profiles: discover_test_profiles_from_root(&root)?,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_tests(project_path: String, profile: String) -> Result<TestRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        let profile = match profile.as_str() {
            "cargo" => "cargo",
            _ => "vitest",
        };
        run_profile(&root, profile)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-test-explorer-{name}-{suffix}"))
    }

    #[test]
    fn discovers_vitest_and_cargo_profiles() {
        let root = unique_test_dir("discover");
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"devDependencies":{"vitest":"^4.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("src-tauri").join("Cargo.toml"),
            "[package]\nname='demo'",
        )
        .unwrap();

        let profiles = discover_test_profiles_from_root(&root).unwrap();

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["vitest", "cargo"]
        );
        assert!(profiles[0].command.contains("vitest run --reporter=json"));
        assert!(profiles[1].command.contains("cargo test"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_vitest_json_failures_with_file_location() {
        let root = Path::new("/repo");
        let raw = r#"{
          "success": false,
          "numTotalTests": 2,
          "numPassedTests": 1,
          "numFailedTests": 1,
          "testResults": [
            {
              "name": "/repo/src/test/math.test.ts",
              "status": "failed",
              "assertionResults": [
                {
                  "fullName": "math adds numbers",
                  "status": "passed",
                  "title": "adds numbers",
                  "failureMessages": []
                },
                {
                  "fullName": "math subtracts numbers",
                  "status": "failed",
                  "title": "subtracts numbers",
                  "failureMessages": ["AssertionError: expected 1 to be 2\n at src/test/math.test.ts:12:7"]
                }
              ]
            }
          ]
        }"#;

        let result = parse_vitest_json(root, raw);

        assert_eq!(result.status, TestRunStatus::Failed);
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.tests.len(), 2);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].name, "math subtracts numbers");
        assert_eq!(result.failures[0].file, "/repo/src/test/math.test.ts");
        assert_eq!(result.failures[0].line, 12);
        assert_eq!(result.failures[0].column, 7);
    }

    #[test]
    fn parses_cargo_failure_locations_from_raw_output() {
        let root = Path::new("/repo");
        let raw = "\
running 2 tests
test parser::tests::accepts_input ... ok
test parser::tests::rejects_bad_input ... FAILED

failures:

---- parser::tests::rejects_bad_input stdout ----
thread 'parser::tests::rejects_bad_input' panicked at src/parser.rs:42:9:
assertion failed: expected error

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";

        let result = parse_cargo_test_output(root, raw);

        assert_eq!(result.status, TestRunStatus::Failed);
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].name, "parser::tests::rejects_bad_input");
        assert_eq!(result.failures[0].file, "/repo/src/parser.rs");
        assert_eq!(result.failures[0].line, 42);
        assert_eq!(result.failures[0].column, 9);
    }
}
