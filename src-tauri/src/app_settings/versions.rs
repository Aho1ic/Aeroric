use super::*;

pub(super) fn detect_version(launch: &AgentLaunchSpec) -> Option<String> {
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args)
        .arg("--version")
        .env("PATH", get_login_shell_path())
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }
    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    extract_semver(&stdout).or_else(|| extract_semver(&stderr))
}

pub(super) fn detect_launch_version_impl(launch: &AgentLaunchSpec) -> Option<String> {
    detect_version(launch)
}

pub(super) fn extract_version_impl(text: &str) -> Option<String> {
    extract_semver(text)
}

pub(super) fn extract_semver(text: &str) -> Option<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < chars.len() {
        let (start, ch) = chars[index];
        if !ch.is_ascii_digit() {
            index += 1;
            continue;
        }

        let mut end = start + ch.len_utf8();
        let mut dot_count = 0;
        let mut cursor = index + 1;
        while cursor < chars.len() {
            let (char_index, next) = chars[cursor];
            if next.is_ascii_digit() {
                end = char_index + next.len_utf8();
                cursor += 1;
                continue;
            }
            if next == '.' {
                dot_count += 1;
                end = char_index + next.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        let candidate = text[start..end].trim_matches('.');
        let parts = candidate.split('.').collect::<Vec<_>>();
        if dot_count > 0
            && parts.len() >= 2
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(candidate.to_string());
        }
        index = cursor.max(index + 1);
    }
    None
}

pub(super) fn detect_versions_for_settings(settings: &AppSettings) -> AgentVersions {
    AgentVersions {
        claude_version: detect_version(&get_agent_launch_spec_from_settings(settings, "claude"))
            .unwrap_or_default(),
        claude_gpt55_version: detect_version(&get_agent_launch_spec_from_settings(
            settings,
            "claude_gpt55",
        ))
        .unwrap_or_default(),
        codex_version: detect_version(&get_agent_launch_spec_from_settings(settings, "codex"))
            .unwrap_or_default(),
    }
}

pub(super) fn parse_semver(v: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = v.split('.').collect();
    (
        parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

pub(super) fn detect_claude_version_impl() -> Option<String> {
    let cache = CACHED_CLAUDE_VERSION.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock();
    if let Some(version) = guard.clone() {
        return version;
    }

    let detected = detect_version(&get_agent_launch_spec("claude"));
    *guard = Some(detected.clone());
    detected
}

pub(super) fn detect_codex_version_impl() -> Option<String> {
    let cache = CACHED_CODEX_VERSION.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock();
    if let Some(version) = guard.clone() {
        return version;
    }

    let detected = detect_version(&get_agent_launch_spec("codex"));
    *guard = Some(detected.clone());
    detected
}

/// 版本号统一走全局带缓存的探测；探测失败视为不满足。
pub(super) fn claude_version_gte_impl(min_version: &str) -> bool {
    match detect_claude_version_impl() {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}

/// Checks the configured launch command for the requested agent.
/// Built-in Claude/Codex keep the global cached version checks; custom agents
/// need their own launch spec so Claude-compatible wrappers can use features
/// such as `--session-id`.
pub(super) fn agent_version_gte_impl(agent: &str, min_version: &str) -> bool {
    let detected = match agent {
        "claude" => detect_claude_version_impl(),
        "codex" => detect_codex_version_impl(),
        _ => detect_version(&get_agent_launch_spec(agent)),
    };
    match detected {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}

/// 版本号统一走全局带缓存的探测；探测失败视为不满足。
pub(super) fn codex_version_gte_impl(min_version: &str) -> bool {
    match detect_codex_version_impl() {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum AgentUpgradeKind {
    Claude,
    Codex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AgentUpgradeCommand {
    pub(super) channel: String,
    pub(super) program: String,
    pub(super) args: Vec<String>,
}
pub(super) fn canonical_program_path(program: &str) -> String {
    fs::canonicalize(program)
        .unwrap_or_else(|_| PathBuf::from(program))
        .to_string_lossy()
        .into_owned()
}

pub(super) fn detected_upgrade_manager(program: &str) -> &'static str {
    let normalized = canonical_program_path(program)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.contains("/node_modules/") {
        "npm"
    } else if normalized.contains("/cellar/") || normalized.contains("/caskroom/") {
        "homebrew"
    } else {
        "standalone"
    }
}

pub(super) fn upgrade_manager_for_path_impl(program: &str) -> &'static str {
    detected_upgrade_manager(program)
}

/// Homebrew 有 formula(Cellar,如 `brew install codex`)与 cask(Caskroom,
/// 如 `brew install --cask claude-code`)两种安装方式,升级命令不同,
/// 通过已配置二进制的真实路径区分。
pub(super) fn detected_homebrew_flavor(program: &str) -> Option<&'static str> {
    let normalized = canonical_program_path(program)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.contains("/node_modules/") {
        None
    } else if normalized.contains("/caskroom/") {
        Some("cask")
    } else if normalized.contains("/cellar/") {
        Some("formula")
    } else {
        None
    }
}

pub(super) fn optional_program(binary: &str) -> Option<String> {
    let detected = detect_path(binary);
    if detected.is_empty() {
        None
    } else {
        Some(detected)
    }
}

pub(super) fn package_manager_has_install(program: &str, args: &[&str]) -> bool {
    let mut command = Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(args)
        .envs(get_login_shell_env().iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.status().is_ok_and(|status| status.success())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_agent_upgrade_commands_from_detection(
    kind: AgentUpgradeKind,
    launch_program: &str,
    native_program: Option<String>,
    npm_program: Option<String>,
    npm_installed: bool,
    brew_program: Option<String>,
    brew_formula_installed: bool,
    brew_cask_installed: bool,
) -> Vec<AgentUpgradeCommand> {
    let configured_manager = detected_upgrade_manager(launch_program);
    let brew_flavor = detected_homebrew_flavor(launch_program);
    let mut commands = Vec::new();
    let mut push_unique = |command: AgentUpgradeCommand| {
        if !commands.iter().any(|existing: &AgentUpgradeCommand| {
            existing.program == command.program && existing.args == command.args
        }) {
            commands.push(command);
        }
    };

    if kind == AgentUpgradeKind::Claude && configured_manager == "standalone" {
        push_unique(AgentUpgradeCommand {
            channel: "native".to_string(),
            program: launch_program.to_string(),
            args: vec!["update".to_string()],
        });
    }
    if kind == AgentUpgradeKind::Claude {
        if let Some(program) = native_program {
            push_unique(AgentUpgradeCommand {
                channel: "native".to_string(),
                program,
                args: vec!["update".to_string()],
            });
        }
    }
    if npm_installed || configured_manager == "npm" {
        if let Some(program) = npm_program {
            let package = match kind {
                AgentUpgradeKind::Claude => "@anthropic-ai/claude-code@latest",
                AgentUpgradeKind::Codex => "@openai/codex@latest",
            };
            push_unique(AgentUpgradeCommand {
                channel: "npm".to_string(),
                program,
                args: vec!["install".to_string(), "-g".to_string(), package.to_string()],
            });
        }
    }
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
    };
    if brew_formula_installed || brew_flavor == Some("formula") {
        if let Some(program) = brew_program.clone() {
            push_unique(AgentUpgradeCommand {
                channel: "homebrew".to_string(),
                program,
                args: vec![
                    "upgrade".to_string(),
                    "--formula".to_string(),
                    brew_name.to_string(),
                ],
            });
        }
    }
    if brew_cask_installed || brew_flavor == Some("cask") {
        if let Some(program) = brew_program {
            push_unique(AgentUpgradeCommand {
                channel: "homebrew".to_string(),
                program,
                args: vec![
                    "upgrade".to_string(),
                    "--cask".to_string(),
                    brew_name.to_string(),
                ],
            });
        }
    }
    commands
}

pub(super) fn build_agent_upgrade_commands(
    kind: AgentUpgradeKind,
    launch_program: &str,
) -> Result<Vec<AgentUpgradeCommand>, String> {
    let native_program = if kind == AgentUpgradeKind::Claude {
        crate::platform::home_dir().and_then(|home| {
            let path = if cfg!(windows) {
                home.join(".local").join("bin").join("claude.exe")
            } else {
                home.join(".local").join("bin").join("claude")
            };
            path.is_file().then(|| path.to_string_lossy().into_owned())
        })
    } else {
        None
    };
    let npm_program = optional_program("npm");
    let brew_program = optional_program("brew");
    let npm_package = match kind {
        AgentUpgradeKind::Claude => "@anthropic-ai/claude-code",
        AgentUpgradeKind::Codex => "@openai/codex",
    };
    // Homebrew 名称:Claude Code 官方走 cask(claude-code),Codex 官方走
    // formula(codex);两种渠道都探测,哪种装了就升级哪种。
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
    };
    let npm_installed = npm_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "-g", "--depth=0", npm_package])
    });
    let brew_formula_installed = brew_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "--versions", "--formula", brew_name])
    });
    let brew_cask_installed = brew_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "--versions", "--cask", brew_name])
    });
    let commands = build_agent_upgrade_commands_from_detection(
        kind,
        launch_program,
        native_program,
        npm_program,
        npm_installed,
        brew_program,
        brew_formula_installed,
        brew_cask_installed,
    );
    if commands.is_empty() {
        Err(match kind {
            AgentUpgradeKind::Claude => {
                "No supported Claude Code installation was detected (native, npm, or Homebrew)"
                    .to_string()
            }
            AgentUpgradeKind::Codex => {
                "No supported Codex installation was detected (npm or Homebrew)".to_string()
            }
        })
    } else {
        Ok(commands)
    }
}

pub(super) fn run_agent_upgrade(command: &AgentUpgradeCommand) -> Result<String, String> {
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .envs(get_login_shell_env().iter().cloned())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = process.output().map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = [stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let detail = if detail.chars().count() > 4000 {
        format!("{}...", detail.chars().take(4000).collect::<String>())
    } else {
        detail
    };
    if output.status.success() {
        Ok(detail)
    } else {
        Err(if detail.is_empty() {
            format!("Upgrade command exited with {}", output.status)
        } else {
            detail
        })
    }
}

pub(super) fn run_agent_upgrades(commands: &[AgentUpgradeCommand]) -> Vec<AgentUpgradeChannel> {
    commands
        .iter()
        .map(|command| match run_agent_upgrade(command) {
            Ok(detail) => AgentUpgradeChannel {
                channel: command.channel.clone(),
                success: true,
                message: if detail.is_empty() {
                    "upgraded".to_string()
                } else {
                    detail
                },
            },
            Err(error) => AgentUpgradeChannel {
                channel: command.channel.clone(),
                success: false,
                message: error,
            },
        })
        .collect()
}

pub(super) fn upgrade_kind_for_agent(
    settings: &AppSettings,
    agent: &str,
) -> Option<AgentUpgradeKind> {
    match agent {
        "claude" => Some(AgentUpgradeKind::Claude),
        "codex" | "claude_gpt55" => Some(AgentUpgradeKind::Codex),
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| {
                if profile.codex_like {
                    AgentUpgradeKind::Codex
                } else {
                    AgentUpgradeKind::Claude
                }
            }),
    }
}

pub(super) fn upgrade_binary_agent(kind: AgentUpgradeKind) -> &'static str {
    match kind {
        AgentUpgradeKind::Claude => "claude",
        AgentUpgradeKind::Codex => "codex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_claude_code_semver_from_new_cli_output() {
        assert_eq!(
            extract_semver("2.1.195 (Claude Code)"),
            Some("2.1.195".to_string())
        );
    }

    #[test]
    fn extracts_prefixed_codex_semver() {
        assert_eq!(
            extract_semver("OpenAI Codex v0.131.0 (research preview)"),
            Some("0.131.0".to_string())
        );
    }

    #[test]
    fn detects_npm_agent_install_inside_homebrew_prefix() {
        assert_eq!(
            detected_upgrade_manager("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            "npm"
        );
    }

    #[test]
    fn detects_homebrew_cask_and_standalone_agent_installs() {
        assert_eq!(
            detected_upgrade_manager("/opt/homebrew/Caskroom/codex/1.0.0/codex"),
            "homebrew"
        );
        assert_eq!(
            detected_upgrade_manager("/Users/test/.local/bin/claude"),
            "standalone"
        );
    }

    #[test]
    fn detects_homebrew_formula_and_cask_flavors_from_install_paths() {
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/Cellar/codex/0.46.0/bin/codex"),
            Some("formula")
        );
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/Caskroom/claude-code/2.0.14/claude"),
            Some("cask")
        );
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            None
        );
        assert_eq!(
            detected_homebrew_flavor("/aeroric-test/.local/bin/claude"),
            None
        );
    }

    #[test]
    fn builds_formula_upgrade_for_cellar_installed_codex_without_explicit_detection() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/opt/homebrew/Cellar/codex/0.46.0/bin/codex",
            None,
            None,
            false,
            Some("/opt/homebrew/bin/brew".to_string()),
            false,
            false,
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "homebrew");
        assert!(commands[0].args.contains(&"--formula".to_string()));
        assert!(commands[0].args.contains(&"codex".to_string()));
    }

    #[test]
    fn builds_both_homebrew_channels_when_formula_and_cask_are_installed() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/aeroric-test/usr/local/bin/codex",
            None,
            None,
            false,
            Some("/opt/homebrew/bin/brew".to_string()),
            true,
            true,
        );

        assert_eq!(commands.len(), 2);
        assert!(commands
            .iter()
            .any(|command| command.args.contains(&"--formula".to_string())));
        assert!(commands
            .iter()
            .any(|command| command.args.contains(&"--cask".to_string())));
    }

    #[test]
    fn builds_upgrade_commands_for_npm_and_homebrew_installations_together() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/aeroric-test/opt/homebrew/bin/codex",
            None,
            Some("/usr/local/bin/npm".to_string()),
            true,
            Some("/opt/homebrew/bin/brew".to_string()),
            true,
            false,
        );

        assert_eq!(commands.len(), 2);
        assert!(commands.iter().any(|command| command.channel == "npm"));
        assert!(commands.iter().any(|command| {
            command.channel == "homebrew" && command.args.contains(&"--formula".to_string())
        }));
    }

    #[test]
    fn builds_native_npm_and_homebrew_claude_upgrade_commands_together() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Claude,
            "/opt/homebrew/bin/claude",
            Some("/Users/test/.local/bin/claude".to_string()),
            Some("/usr/local/bin/npm".to_string()),
            true,
            Some("/opt/homebrew/bin/brew".to_string()),
            false,
            true,
        );

        assert!(commands.iter().any(|command| command.channel == "native"));
        assert!(commands.iter().any(|command| command.channel == "npm"));
        assert!(commands.iter().any(|command| {
            command.channel == "homebrew" && command.args.contains(&"--cask".to_string())
        }));
    }
}
