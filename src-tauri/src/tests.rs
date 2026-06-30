use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::ssh::SshConnection;

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
    pub coverage: Option<TestCoverageSummary>,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverageMetric {
    pub covered: usize,
    pub total: usize,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverageSummary {
    pub lines: TestCoverageMetric,
    pub functions: TestCoverageMetric,
    pub branches: TestCoverageMetric,
    pub files: Vec<TestCoverageFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverageFile {
    pub file: String,
    pub lines: Vec<TestCoverageLine>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverageLine {
    pub line: usize,
    pub hits: usize,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestRunTarget {
    pub file_path: Option<String>,
    pub test_name: Option<String>,
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

fn validate_remote_relative_path(relative_path: &str) -> Result<(), String> {
    if relative_path.is_empty() {
        return Err("Test file must not be empty".to_string());
    }
    if relative_path.starts_with('/') {
        return Err("Test file must be inside the remote project".to_string());
    }
    if relative_path.contains('\0')
        || relative_path
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return Err("Test file must stay inside the remote project".to_string());
    }
    Ok(())
}

fn remote_scoped_file_arg(
    remote_project_path: &str,
    target: Option<&TestRunTarget>,
) -> Result<Option<String>, String> {
    let Some(file_path) = target
        .and_then(|target| target.file_path.as_deref())
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return Ok(None);
    };

    let remote_root = normalize_remote_project_path(remote_project_path)?;
    let relative = if file_path.starts_with('/') {
        let prefix = if remote_root == "/" {
            "/".to_string()
        } else {
            format!("{remote_root}/")
        };
        file_path
            .strip_prefix(&prefix)
            .ok_or_else(|| "Test file is outside remote project root".to_string())?
            .to_string()
    } else {
        file_path.to_string()
    };
    validate_remote_relative_path(&relative)?;
    Ok(Some(relative))
}

fn shell_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| crate::ssh::shell_word_posix(arg)))
        .collect::<Vec<_>>()
        .join(" ")
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
        .map_err(|e| format!("Failed to run remote tests: {e}"))
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

fn file_name_is_python_test(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.starts_with("test_") && file_name.ends_with(".py") || file_name.ends_with("_test.py")
}

fn directory_has_python_tests(root: &Path, max_entries: usize) -> bool {
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > max_entries {
                return false;
            }
            let path = entry.path();
            if path.is_file() && file_name_is_python_test(&path) {
                return true;
            }
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if matches!(
                name,
                ".git" | ".venv" | "venv" | "node_modules" | "target" | "__pycache__"
            ) {
                continue;
            }
            stack.push(path);
        }
    }
    false
}

fn has_pytest_project(root: &Path) -> bool {
    if ["pytest.ini", ".pytest.ini"]
        .iter()
        .any(|file| root.join(file).is_file())
    {
        return true;
    }
    for file in ["tox.ini", "setup.cfg", "pyproject.toml"] {
        let Ok(raw) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        if raw.contains("[pytest]") || raw.contains("[tool.pytest") || raw.contains("pytest") {
            return true;
        }
    }
    directory_has_python_tests(root, 10_000)
}

fn scoped_test_name(target: Option<&TestRunTarget>) -> Option<String> {
    target
        .and_then(|target| target.test_name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn scoped_file_arg(root: &Path, target: Option<&TestRunTarget>) -> Result<Option<String>, String> {
    let Some(file_path) = target
        .and_then(|target| target.file_path.as_deref())
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return Ok(None);
    };
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let path = Path::new(file_path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let path = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve test file: {e}"))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("Test file is outside project root".to_string());
    }
    Ok(Some(
        path.strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/"),
    ))
}

fn vitest_command(
    root: &Path,
    target: Option<&TestRunTarget>,
    coverage: bool,
) -> Result<(String, Vec<String>, String), String> {
    let package_manager =
        detect_package_manager(root).ok_or_else(|| "Vitest tests were not detected".to_string())?;
    let mut args = match package_manager {
        "pnpm" => vec!["exec", "vitest", "run", "--reporter=json"],
        "yarn" => vec!["vitest", "run", "--reporter=json"],
        "npm" => vec!["exec", "--", "vitest", "run", "--reporter=json"],
        _ => return Err("Vitest tests were not detected".to_string()),
    }
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    if let Some(file) = scoped_file_arg(root, target)? {
        args.push(file);
    }
    if let Some(test_name) = scoped_test_name(target) {
        args.push("-t".to_string());
        args.push(test_name);
    }
    if coverage {
        args.push("--coverage".to_string());
    }
    let display = format!("{package_manager} {}", args.join(" "));
    Ok((package_manager.to_string(), args, display))
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

fn cargo_command(
    root: &Path,
    target: Option<&TestRunTarget>,
) -> Result<(String, Vec<String>, String), String> {
    let manifest =
        cargo_manifest(root).ok_or_else(|| "Cargo tests were not detected".to_string())?;
    let relative_manifest = manifest
        .strip_prefix(root)
        .unwrap_or(&manifest)
        .to_string_lossy()
        .into_owned();
    let mut args = if relative_manifest == "Cargo.toml" {
        vec!["test".to_string(), "--message-format=json".to_string()]
    } else {
        vec![
            "test".to_string(),
            "--manifest-path".to_string(),
            relative_manifest,
            "--message-format=json".to_string(),
        ]
    };
    if let Some(file) = scoped_file_arg(root, target)? {
        let path = Path::new(&file);
        if path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("tests")
        {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                args.push("--test".to_string());
                args.push(stem.to_string());
            }
        } else if scoped_test_name(target).is_none() {
            return Err(
                "Cargo file scoped runs require an integration test file under tests/ or a test name"
                    .to_string(),
            );
        }
    }
    if let Some(test_name) = scoped_test_name(target) {
        args.push(test_name);
    }
    let display = format!("cargo {}", args.join(" "));
    Ok(("cargo".to_string(), args, display))
}

fn python_program() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

fn python_command(
    root: &Path,
    target: Option<&TestRunTarget>,
) -> Result<(String, Vec<String>, String), String> {
    if !has_pytest_project(root) {
        return Err("Python tests were not detected".to_string());
    }
    let mut args = vec!["-m".to_string(), "pytest".to_string(), "-q".to_string()];
    let file = scoped_file_arg(root, target)?;
    let test_name = scoped_test_name(target);
    if let Some(file) = file {
        if !file_name_is_python_test(Path::new(&file)) {
            return Err("Pytest scoped file must be a Python test file".to_string());
        }
        let target_arg = test_name
            .as_ref()
            .map(|name| format!("{file}::{name}"))
            .unwrap_or(file);
        args.push(target_arg);
    } else if let Some(test_name) = test_name {
        args.push("-k".to_string());
        args.push(test_name);
    }
    let program = python_program().to_string();
    let display = format!("{program} {}", args.join(" "));
    Ok((program, args, display))
}

fn remote_vitest_command(
    remote_project_path: &str,
    target: Option<&TestRunTarget>,
    coverage: bool,
) -> Result<(String, String), String> {
    let mut base_args = vec![
        "vitest".to_string(),
        "run".to_string(),
        "--reporter=json".to_string(),
    ];
    if let Some(file) = remote_scoped_file_arg(remote_project_path, target)? {
        base_args.push(file);
    }
    if let Some(test_name) = scoped_test_name(target) {
        base_args.push("-t".to_string());
        base_args.push(test_name);
    }
    if coverage {
        base_args.push("--coverage".to_string());
    }

    let pnpm_args = std::iter::once("exec".to_string())
        .chain(base_args.clone())
        .collect::<Vec<_>>();
    let yarn_args = base_args.clone();
    let npm_args = std::iter::once("exec".to_string())
        .chain(std::iter::once("--".to_string()))
        .chain(base_args)
        .collect::<Vec<_>>();
    let command = format!(
        "if [ -f pnpm-lock.yaml ]; then exec {}; elif [ -f yarn.lock ]; then exec {}; elif [ -f package-lock.json ]; then exec {}; else echo {}; exit 127; fi",
        shell_command("pnpm", &pnpm_args),
        shell_command("yarn", &yarn_args),
        shell_command("npm", &npm_args),
        crate::ssh::shell_quote_posix("Vitest tests were not detected")
    );
    Ok((command, "remote vitest".to_string()))
}

fn remote_cargo_extra_args(
    remote_project_path: &str,
    target: Option<&TestRunTarget>,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    if let Some(file) = remote_scoped_file_arg(remote_project_path, target)? {
        let path = Path::new(&file);
        if path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("tests")
        {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                args.push("--test".to_string());
                args.push(stem.to_string());
            }
        } else if scoped_test_name(target).is_none() {
            return Err(
                "Cargo file scoped runs require an integration test file under tests/ or a test name"
                    .to_string(),
            );
        }
    }
    if let Some(test_name) = scoped_test_name(target) {
        args.push(test_name);
    }
    Ok(args)
}

fn remote_cargo_command(
    remote_project_path: &str,
    target: Option<&TestRunTarget>,
) -> Result<(String, String), String> {
    let extra_args = remote_cargo_extra_args(remote_project_path, target)?;
    let mut root_args = vec!["test".to_string(), "--message-format=json".to_string()];
    root_args.extend(extra_args.clone());
    let mut tauri_args = vec![
        "test".to_string(),
        "--manifest-path".to_string(),
        "src-tauri/Cargo.toml".to_string(),
        "--message-format=json".to_string(),
    ];
    tauri_args.extend(extra_args);
    let command = format!(
        "if [ -f Cargo.toml ]; then exec {}; elif [ -f src-tauri/Cargo.toml ]; then exec {}; else echo {}; exit 127; fi",
        shell_command("cargo", &root_args),
        shell_command("cargo", &tauri_args),
        crate::ssh::shell_quote_posix("Cargo tests were not detected")
    );
    Ok((command, "remote cargo".to_string()))
}

fn remote_python_command(
    remote_project_path: &str,
    target: Option<&TestRunTarget>,
) -> Result<(String, String), String> {
    let mut args = vec!["-m".to_string(), "pytest".to_string(), "-q".to_string()];
    let file = remote_scoped_file_arg(remote_project_path, target)?;
    let test_name = scoped_test_name(target);
    if let Some(file) = file {
        if !file_name_is_python_test(Path::new(&file)) {
            return Err("Pytest scoped file must be a Python test file".to_string());
        }
        let target_arg = test_name
            .as_ref()
            .map(|name| format!("{file}::{name}"))
            .unwrap_or(file);
        args.push(target_arg);
    } else if let Some(test_name) = test_name {
        args.push("-k".to_string());
        args.push(test_name);
    }
    Ok((shell_command("python3", &args), "remote pytest".to_string()))
}

fn remote_test_command(
    remote_project_path: &str,
    profile: &str,
    target: Option<&TestRunTarget>,
    coverage: bool,
) -> Result<(String, String), String> {
    match profile {
        "cargo" => remote_cargo_command(remote_project_path, target),
        "python" => remote_python_command(remote_project_path, target),
        _ => remote_vitest_command(remote_project_path, target, coverage),
    }
}

pub fn discover_test_profiles_from_root(root: &Path) -> Result<Vec<TestProfile>, String> {
    let mut profiles = Vec::new();
    if package_json_has_vitest(root) || has_vitest_config(root) {
        if let Ok((_, _, command)) = vitest_command(root, None, false) {
            profiles.push(TestProfile {
                id: "vitest".to_string(),
                label: "Vitest".to_string(),
                command,
            });
        }
    }
    if let Ok((_, _, command)) = cargo_command(root, None) {
        profiles.push(TestProfile {
            id: "cargo".to_string(),
            label: "Cargo".to_string(),
            command,
        });
    }
    if let Ok((_, _, command)) = python_command(root, None) {
        profiles.push(TestProfile {
            id: "python".to_string(),
            label: "Pytest".to_string(),
            command,
        });
    }
    Ok(profiles)
}

fn numeric_field(value: &Value, name: &str) -> usize {
    value.get(name).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn coverage_metric(covered: usize, total: usize) -> TestCoverageMetric {
    let percent = if total == 0 {
        0.0
    } else {
        ((covered as f64 / total as f64) * 1000.0).round() / 10.0
    };
    TestCoverageMetric {
        covered,
        total,
        percent,
    }
}

fn normalize_lcov_file_path(root: &Path, file: &str) -> String {
    let path = Path::new(file.trim());
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    path.to_string_lossy().replace('\\', "/")
}

fn finish_lcov_file(
    root: &Path,
    current_file: &mut Option<String>,
    current_lines: &mut Vec<TestCoverageLine>,
    files: &mut Vec<TestCoverageFile>,
) {
    let Some(file) = current_file.take() else {
        current_lines.clear();
        return;
    };
    if current_lines.is_empty() {
        return;
    }
    let mut merged = BTreeMap::new();
    for line in current_lines.drain(..) {
        *merged.entry(line.line).or_insert(0usize) += line.hits;
    }
    files.push(TestCoverageFile {
        file: normalize_lcov_file_path(root, &file),
        lines: merged
            .into_iter()
            .map(|(line, hits)| TestCoverageLine { line, hits })
            .collect(),
    });
}

pub(crate) fn parse_lcov_summary_from_root(root: &Path, raw: &str) -> Option<TestCoverageSummary> {
    let mut lines_total = 0;
    let mut lines_covered = 0;
    let mut functions_total = 0;
    let mut functions_covered = 0;
    let mut branches_total = 0;
    let mut branches_covered = 0;
    let mut files = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_lines = Vec::new();

    for line in raw.lines() {
        if line == "end_of_record" {
            finish_lcov_file(root, &mut current_file, &mut current_lines, &mut files);
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "SF" => {
                finish_lcov_file(root, &mut current_file, &mut current_lines, &mut files);
                current_file = Some(value.trim().to_string());
            }
            "DA" => {
                let mut parts = value.split(',');
                let Some(line) = parts.next().and_then(|line| line.trim().parse().ok()) else {
                    continue;
                };
                let Some(hits) = parts.next().and_then(|hits| hits.trim().parse().ok()) else {
                    continue;
                };
                current_lines.push(TestCoverageLine { line, hits });
            }
            "LF" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    lines_total += count;
                }
            }
            "LH" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    lines_covered += count;
                }
            }
            "FNF" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    functions_total += count;
                }
            }
            "FNH" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    functions_covered += count;
                }
            }
            "BRF" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    branches_total += count;
                }
            }
            "BRH" => {
                if let Ok(count) = value.trim().parse::<usize>() {
                    branches_covered += count;
                }
            }
            _ => {}
        }
    }
    finish_lcov_file(root, &mut current_file, &mut current_lines, &mut files);
    files.sort_by(|a, b| a.file.cmp(&b.file));

    (lines_total > 0 || functions_total > 0 || branches_total > 0).then_some(TestCoverageSummary {
        lines: coverage_metric(lines_covered, lines_total),
        functions: coverage_metric(functions_covered, functions_total),
        branches: coverage_metric(branches_covered, branches_total),
        files,
    })
}

fn read_lcov_summary_from_root(root: &Path) -> Option<TestCoverageSummary> {
    let raw = std::fs::read_to_string(root.join("coverage").join("lcov.info")).ok()?;
    parse_lcov_summary_from_root(root, &raw)
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
            coverage: None,
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
        coverage: None,
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
        coverage: None,
        raw_output: output.to_string(),
    }
}

fn pytest_summary_count(output: &str, label: &str) -> usize {
    let pattern =
        regex::Regex::new(&format!(r"(?P<count>\d+)\s+{label}")).expect("valid pytest regex");
    pattern
        .captures_iter(output)
        .filter_map(|captures| {
            captures
                .name("count")
                .and_then(|count| count.as_str().parse::<usize>().ok())
        })
        .sum()
}

pub fn parse_pytest_output(root: &Path, output: &str) -> TestRunResult {
    let failed_line = regex::Regex::new(r"^FAILED\s+(?P<node>\S+)(?:\s+-\s+(?P<message>.*))?$")
        .expect("valid pytest failed line regex");
    let mut failures = Vec::new();
    let mut tests = Vec::new();

    for line in output.lines() {
        let Some(captures) = failed_line.captures(line) else {
            continue;
        };
        let node = captures.name("node").map(|m| m.as_str()).unwrap_or("");
        let message = captures
            .name("message")
            .map(|m| m.as_str().trim().to_string())
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| line.to_string());
        let mut node_parts = node.split("::");
        let file = node_parts.next().unwrap_or("");
        let name = node_parts.collect::<Vec<_>>().join("::");
        let name = if name.is_empty() {
            node.to_string()
        } else {
            name
        };
        let file = join_test_path(root, file);
        tests.push(TestCase {
            profile: "python".to_string(),
            name: name.clone(),
            file: file.clone(),
            line: 1,
            column: 1,
            status: TestRunStatus::Failed,
            duration_ms: None,
        });
        failures.push(TestFailure {
            profile: "python".to_string(),
            name,
            file,
            line: 1,
            column: 1,
            message,
        });
    }

    let passed = pytest_summary_count(output, "passed");
    let failed = pytest_summary_count(output, "failed") + pytest_summary_count(output, "error");
    let total = passed + failed;
    let status = if failed > 0 {
        TestRunStatus::Failed
    } else if total > 0 {
        TestRunStatus::Passed
    } else {
        TestRunStatus::Error
    };

    TestRunResult {
        profile: "python".to_string(),
        status,
        total,
        passed,
        failed,
        tests,
        failures,
        coverage: None,
        raw_output: output.to_string(),
    }
}

fn run_profile(
    root: &Path,
    profile: &str,
    target: Option<&TestRunTarget>,
    coverage: bool,
) -> Result<TestRunResult, String> {
    let (program, args, _) = match profile {
        "cargo" => cargo_command(root, target)?,
        "python" => python_command(root, target)?,
        _ => vitest_command(root, target, coverage)?,
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
        "python" => parse_pytest_output(root, &raw_output),
        _ => parse_vitest_json(root, &stdout),
    };
    if profile == "vitest" && coverage {
        result.coverage = read_lcov_summary_from_root(root);
    }
    result.raw_output = raw_output;
    if !output.status.success() && result.failures.is_empty() {
        result.status = TestRunStatus::Error;
    }
    Ok(result)
}

fn parse_remote_discovered_profiles(output: &str) -> Vec<TestProfile> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let id = parts.next()?.trim();
            let label = parts.next()?.trim();
            let command = parts.next()?.trim();
            if id.is_empty() || label.is_empty() || command.is_empty() {
                return None;
            }
            Some(TestProfile {
                id: id.to_string(),
                label: label.to_string(),
                command: command.to_string(),
            })
        })
        .collect()
}

fn remote_discover_tests_command() -> String {
    [
        "if { [ -f package.json ] && grep -q vitest package.json; } || [ -f vitest.config.ts ] || [ -f vitest.config.js ] || [ -f vitest.config.mts ] || [ -f vite.config.ts ] || [ -f vite.config.js ]; then printf 'vitest\\tVitest\\t%s\\n' 'remote vitest'; fi",
        "if [ -f Cargo.toml ] || [ -f src-tauri/Cargo.toml ]; then printf 'cargo\\tCargo\\t%s\\n' 'remote cargo'; fi",
        "if [ -f pytest.ini ] || [ -f .pytest.ini ] || [ -f tox.ini ] || [ -f setup.cfg ] || [ -f pyproject.toml ] || find . -path './.git' -prune -o -path './.venv' -prune -o -path './venv' -prune -o -path './node_modules' -prune -o -path './target' -prune -o -path './__pycache__' -prune -o -type f \\( -name 'test_*.py' -o -name '*_test.py' \\) -print -quit | grep -q .; then printf 'python\\tPytest\\t%s\\n' 'remote pytest'; fi",
    ]
    .join("; ")
}

fn test_result_from_output(
    root: &Path,
    profile: &str,
    stdout: &str,
    raw_output: &str,
    status_success: bool,
) -> TestRunResult {
    let mut result = match profile {
        "cargo" => parse_cargo_test_output(root, raw_output),
        "python" => parse_pytest_output(root, raw_output),
        _ => parse_vitest_json(root, stdout),
    };
    result.raw_output = raw_output.to_string();
    if !status_success && result.failures.is_empty() {
        result.status = TestRunStatus::Error;
    }
    result
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
pub async fn run_tests(
    project_path: String,
    profile: String,
    target: Option<TestRunTarget>,
    coverage: Option<bool>,
) -> Result<TestRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        let profile = match profile.as_str() {
            "cargo" => "cargo",
            "python" | "pytest" => "python",
            _ => "vitest",
        };
        run_profile(&root, profile, target.as_ref(), coverage.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_discover_tests(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<TestDiscoveryResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_project_path(&remote_project_path)?;
        let output = run_remote_project_command(
            &connection,
            &remote_root,
            &remote_discover_tests_command(),
        )?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if message.is_empty() {
                "Remote test discovery failed".to_string()
            } else {
                message
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(TestDiscoveryResult {
            profiles: parse_remote_discovered_profiles(&stdout),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_run_tests(
    connection: SshConnection,
    remote_project_path: String,
    profile: String,
    target: Option<TestRunTarget>,
    coverage: Option<bool>,
) -> Result<TestRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_project_path(&remote_project_path)?;
        let profile = match profile.as_str() {
            "cargo" => "cargo",
            "python" | "pytest" => "python",
            _ => "vitest",
        };
        let coverage = coverage.unwrap_or(false);
        let (command, _) = remote_test_command(&remote_root, profile, target.as_ref(), coverage)?;
        let output = run_remote_project_command(&connection, &remote_root, &command)?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let raw_output = if stderr.trim().is_empty() {
            stdout.clone()
        } else {
            format!("{stdout}{stderr}")
        };
        let mut result = test_result_from_output(
            Path::new(&remote_root),
            profile,
            &stdout,
            &raw_output,
            output.status.success(),
        );
        if profile == "vitest" && coverage {
            let coverage_output =
                run_remote_project_command(&connection, &remote_root, "cat coverage/lcov.info");
            if let Ok(coverage_output) = coverage_output {
                if coverage_output.status.success() {
                    let raw_lcov = String::from_utf8_lossy(&coverage_output.stdout);
                    result.coverage =
                        parse_lcov_summary_from_root(Path::new(&remote_root), &raw_lcov);
                }
            }
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
    fn remote_scoped_file_arg_maps_absolute_remote_file_to_project_relative_path() {
        let target = TestRunTarget {
            file_path: Some("/srv/app/tests/test_math.py".to_string()),
            test_name: Some("test_adds_numbers".to_string()),
        };

        assert_eq!(
            remote_scoped_file_arg("/srv/app", Some(&target)).unwrap(),
            Some("tests/test_math.py".to_string())
        );
        assert!(remote_scoped_file_arg(
            "/srv/app",
            Some(&TestRunTarget {
                file_path: Some("/srv/other/test_math.py".to_string()),
                test_name: None,
            })
        )
        .is_err());
    }

    #[test]
    fn builds_remote_vitest_command_with_package_manager_fallback_and_target() {
        let target = TestRunTarget {
            file_path: Some("/srv/app/src/math.test.ts".to_string()),
            test_name: Some("adds numbers".to_string()),
        };

        let (command, _) = remote_vitest_command("/srv/app", Some(&target), true).unwrap();

        assert!(command.contains(
            "pnpm exec vitest run --reporter=json src/math.test.ts -t 'adds numbers' --coverage"
        ));
        assert!(command.contains(
            "yarn vitest run --reporter=json src/math.test.ts -t 'adds numbers' --coverage"
        ));
        assert!(command.contains(
            "npm exec -- vitest run --reporter=json src/math.test.ts -t 'adds numbers' --coverage"
        ));
    }

    #[test]
    fn parses_remote_discovered_profiles() {
        let profiles = parse_remote_discovered_profiles(
            "vitest\tVitest\tremote vitest\ncargo\tCargo\tremote cargo\n",
        );

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "vitest");
        assert_eq!(profiles[1].command, "remote cargo");
    }

    #[test]
    fn builds_vitest_command_for_single_file_and_test_name() {
        let root = unique_test_dir("vitest-target");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        fs::create_dir_all(root.join("src").join("test")).unwrap();
        fs::write(root.join("src").join("test").join("math.test.ts"), "").unwrap();
        let target = TestRunTarget {
            file_path: Some(
                root.join("src/test/math.test.ts")
                    .to_string_lossy()
                    .into_owned(),
            ),
            test_name: Some("adds numbers".to_string()),
        };

        let (_, args, display) = vitest_command(&root, Some(&target), false).unwrap();

        assert_eq!(
            args,
            vec![
                "exec",
                "vitest",
                "run",
                "--reporter=json",
                "src/test/math.test.ts",
                "-t",
                "adds numbers"
            ]
        );
        assert!(display.contains("src/test/math.test.ts"));
        assert!(display.contains("-t adds numbers"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_vitest_coverage_command() {
        let root = unique_test_dir("vitest-coverage");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();

        let (_, args, display) = vitest_command(&root, None, true).unwrap();

        assert_eq!(
            args,
            vec!["exec", "vitest", "run", "--reporter=json", "--coverage"]
        );
        assert!(display.contains("--coverage"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_vitest_target_files_outside_project_root() {
        let root = unique_test_dir("vitest-target-root");
        let outside_root = unique_test_dir("vitest-target-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        fs::write(outside_root.join("math.test.ts"), "").unwrap();
        let target = TestRunTarget {
            file_path: Some(
                outside_root
                    .join("math.test.ts")
                    .to_string_lossy()
                    .into_owned(),
            ),
            test_name: None,
        };

        let error = vitest_command(&root, Some(&target), false).unwrap_err();

        assert!(error.contains("outside project root"));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside_root).unwrap();
    }

    #[test]
    fn parses_lcov_summary_metrics() {
        let raw = "\
TN:
SF:/repo/src/math.ts
FN:1,add
FNDA:3,add
FNF:1
FNH:1
BRDA:2,0,0,1
BRDA:2,0,1,0
BRF:2
BRH:1
DA:1,3
DA:2,0
LF:2
LH:1
end_of_record
";

        let summary = parse_lcov_summary_from_root(Path::new(""), raw).unwrap();

        assert_eq!(
            summary,
            TestCoverageSummary {
                lines: TestCoverageMetric {
                    covered: 1,
                    total: 2,
                    percent: 50.0
                },
                functions: TestCoverageMetric {
                    covered: 1,
                    total: 1,
                    percent: 100.0
                },
                branches: TestCoverageMetric {
                    covered: 1,
                    total: 2,
                    percent: 50.0
                },
                files: vec![TestCoverageFile {
                    file: "/repo/src/math.ts".to_string(),
                    lines: vec![
                        TestCoverageLine { line: 1, hits: 3 },
                        TestCoverageLine { line: 2, hits: 0 },
                    ],
                }]
            }
        );
    }

    #[test]
    fn reads_lcov_summary_from_project_coverage_dir() {
        let root = unique_test_dir("coverage-summary");
        fs::create_dir_all(root.join("coverage")).unwrap();
        fs::write(
            root.join("coverage").join("lcov.info"),
            "SF:/repo/src/math.ts\nDA:1,1\nDA:2,0\nLF:4\nLH:3\nFNF:2\nFNH:1\nBRF:0\nBRH:0\nend_of_record\n",
        )
        .unwrap();

        let summary = read_lcov_summary_from_root(&root).unwrap();

        assert_eq!(summary.lines.percent, 75.0);
        assert_eq!(summary.functions.percent, 50.0);
        assert_eq!(summary.files[0].file, "/repo/src/math.ts");
        assert_eq!(
            summary.files[0].lines[0],
            TestCoverageLine { line: 1, hits: 1 }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_cargo_command_for_test_name() {
        let root = unique_test_dir("cargo-target-name");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='demo'").unwrap();
        let target = TestRunTarget {
            file_path: None,
            test_name: Some("parser::tests::rejects_bad_input".to_string()),
        };

        let (_, args, display) = cargo_command(&root, Some(&target)).unwrap();

        assert_eq!(
            args,
            vec![
                "test",
                "--message-format=json",
                "parser::tests::rejects_bad_input"
            ]
        );
        assert!(display.contains("parser::tests::rejects_bad_input"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_cargo_command_for_integration_test_file() {
        let root = unique_test_dir("cargo-target-file");
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='demo'").unwrap();
        fs::write(root.join("tests").join("api.rs"), "").unwrap();
        let target = TestRunTarget {
            file_path: Some(
                root.join("tests")
                    .join("api.rs")
                    .to_string_lossy()
                    .into_owned(),
            ),
            test_name: Some("creates_user".to_string()),
        };

        let (_, args, _) = cargo_command(&root, Some(&target)).unwrap();

        assert_eq!(
            args,
            vec![
                "test",
                "--message-format=json",
                "--test",
                "api",
                "creates_user"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_python_command_for_file_and_test_name() {
        let root = unique_test_dir("python-target");
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
        fs::write(root.join("tests").join("test_math.py"), "").unwrap();
        let target = TestRunTarget {
            file_path: Some(
                root.join("tests")
                    .join("test_math.py")
                    .to_string_lossy()
                    .into_owned(),
            ),
            test_name: Some("test_adds_numbers".to_string()),
        };

        let (_, args, display) = python_command(&root, Some(&target)).unwrap();

        assert_eq!(
            args,
            vec![
                "-m",
                "pytest",
                "-q",
                "tests/test_math.py::test_adds_numbers"
            ]
        );
        assert!(display.contains("pytest -q tests/test_math.py::test_adds_numbers"));
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

    #[test]
    fn parses_pytest_failure_summary_lines() {
        let root = Path::new("/repo");
        let raw = "\
FAILED tests/test_math.py::test_subtracts_numbers - AssertionError: expected 1 == 2
1 failed, 2 passed in 0.04s
";

        let result = parse_pytest_output(root, raw);

        assert_eq!(result.profile, "python");
        assert_eq!(result.status, TestRunStatus::Failed);
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].file, "/repo/tests/test_math.py");
        assert_eq!(result.failures[0].name, "test_subtracts_numbers");
    }
}
