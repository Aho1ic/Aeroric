use std::path::Path;
use std::process::Output;

use crate::path_guard::{normalize_remote_project_path, validate_project_root};

// ── 切片一百零六：DTO 与本地面主体上收 core `ide::tests_face`（单一定义；
// 下面的 remote 面与共享 helpers 继续引用这些类型）──
use crate::ssh::SshConnection;
pub use aeroric_core::ide::tests_face::{
    TestDiscoveryResult, TestProfile, TestRunResult, TestRunStatus, TestRunTarget,
};

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

fn file_name_is_python_test(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.starts_with("test_") && file_name.ends_with(".py") || file_name.ends_with("_test.py")
}

fn scoped_test_name(target: Option<&TestRunTarget>) -> Option<String> {
    target
        .and_then(|target| target.test_name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
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
        "cargo" => aeroric_core::ide::tests_face::parse_cargo_test_output(root, raw_output),
        "python" => aeroric_core::ide::tests_face::parse_pytest_output(root, raw_output),
        _ => aeroric_core::ide::tests_face::parse_vitest_json(root, stdout),
    };
    result.raw_output = raw_output.to_string();
    if !status_success && result.failures.is_empty() {
        result.status = TestRunStatus::Error;
    }
    result
}

// ── 切片一百零六（ide 刀一）：tests 命令族本地面改宿主转发壳。业务主体在
// core `ide::tests_face`；Embedded 本地闭包与宿主服务调同一函数，External
// 绝不本地回退。remote 孪生（SSH 适配层）不动。──

#[tauri::command]
pub async fn discover_tests<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
) -> Result<TestDiscoveryResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.discover_tests",
        serde_json::json!({ "projectPath": project_path }),
        async {
            validate_project_root(&project_path)?;
            aeroric_core::ide::tests_face::discover_test_profiles_from_root(Path::new(
                &project_path,
            ))
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_tests<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    profile: String,
    target: Option<TestRunTarget>,
    coverage: Option<bool>,
) -> Result<TestRunResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.run_tests",
        serde_json::json!({
            "projectPath": project_path,
            "profile": profile,
            "target": target,
            "coverage": coverage
        }),
        async {
            validate_project_root(&project_path)?;
            let profile = match profile.as_str() {
                "cargo" => "cargo",
                "python" | "pytest" => "python",
                _ => "vitest",
            };
            aeroric_core::ide::tests_face::run_profile(
                Path::new(&project_path),
                profile,
                target.as_ref(),
                coverage.unwrap_or(false),
                &crate::platform::local_python_program,
                crate::app_settings::get_login_shell_env(),
            )
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
                    result.coverage = aeroric_core::ide::tests_face::parse_lcov_summary_from_root(
                        Path::new(&remote_root),
                        &raw_lcov,
                    );
                }
            }
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use aeroric_core::ide::tests_face::{
        TestCoverageFile, TestCoverageLine, TestCoverageMetric, TestCoverageSummary,
    };
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

        let profiles =
            aeroric_core::ide::tests_face::discover_test_profiles_from_root(&root).unwrap();

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

        let (_, args, display): (String, Vec<String>, String) =
            aeroric_core::ide::tests_face::vitest_command(&root, Some(&target), false).unwrap();

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

        let (_, args, display): (String, Vec<String>, String) =
            aeroric_core::ide::tests_face::vitest_command(&root, None, true).unwrap();

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

        let error =
            aeroric_core::ide::tests_face::vitest_command(&root, Some(&target), false).unwrap_err();

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

        let summary =
            aeroric_core::ide::tests_face::parse_lcov_summary_from_root(Path::new(""), raw)
                .unwrap();

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

        let summary = aeroric_core::ide::tests_face::read_lcov_summary_from_root(&root).unwrap();

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

        let (_, args, display): (String, Vec<String>, String) =
            aeroric_core::ide::tests_face::cargo_command(&root, Some(&target)).unwrap();

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

        let (_, args, _): (String, Vec<String>, String) =
            aeroric_core::ide::tests_face::cargo_command(&root, Some(&target)).unwrap();

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

        let (_, args, display): (String, Vec<String>, String) =
            aeroric_core::ide::tests_face::python_command(
                &root,
                Some(&target),
                &aeroric_core::ide::tests_face::default_python_program,
            )
            .unwrap();

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

        let result = aeroric_core::ide::tests_face::parse_vitest_json(root, raw);

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

        let result = aeroric_core::ide::tests_face::parse_cargo_test_output(root, raw);

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

        let result = aeroric_core::ide::tests_face::parse_pytest_output(root, raw);

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
