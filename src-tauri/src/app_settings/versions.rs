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
            // 保留紧随其后的预发布后缀(dsh 处于 dev preview,版本形如
            // 0.1.0-rc.6;丢掉后缀会让"当前 vs 最新"永远不相等)。
            let core_end = start + candidate.len();
            let mut result = candidate.to_string();
            if let Some(rest) = text.get(core_end..) {
                if rest.starts_with('-') {
                    let suffix: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
                        .collect();
                    let suffix = suffix.trim_end_matches(['.', '-']);
                    if suffix.len() > 1 {
                        result.push_str(suffix);
                    }
                }
            }
            return Some(result);
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
        dsh_version: detect_version(&get_agent_launch_spec_from_settings(settings, "dsh"))
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

pub(super) fn version_reaches_target(current: &str, expected: &str) -> bool {
    fn parse(value: &str) -> Option<((u64, u64, u64), Vec<&str>)> {
        let without_build = value
            .trim()
            .split_once('+')
            .map_or(value.trim(), |(head, _)| head);
        let (core, prerelease) = without_build
            .split_once('-')
            .map_or((without_build, Vec::new()), |(core, prerelease)| {
                (core, prerelease.split('.').collect())
            });
        let mut parts = core.split('.');
        let parsed = (
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        if parts.next().is_some()
            || prerelease.iter().any(|part| {
                part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
        {
            return None;
        }
        Some((parsed, prerelease))
    }

    fn compare_prerelease(left: &[&str], right: &[&str]) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        match (left.is_empty(), right.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (left, right) in left.iter().zip(right) {
            let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
                (Ok(left), Ok(right)) => left.cmp(&right),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => left.cmp(right),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        left.len().cmp(&right.len())
    }

    match (parse(current), parse(expected)) {
        (Some(current), Some(expected)) => current
            .0
            .cmp(&expected.0)
            .then_with(|| compare_prerelease(&current.1, &expected.1))
            .is_ge(),
        _ => current.trim() == expected.trim(),
    }
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

pub(super) fn detect_dsh_version_impl() -> Option<String> {
    let cache = CACHED_DSH_VERSION.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock();
    if let Some(version) = guard.clone() {
        return version;
    }

    let detected = detect_version(&get_agent_launch_spec("dsh"));
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
        "dsh" => detect_dsh_version_impl(),
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
    Dsh,
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

fn normalized_program_path(program: &str) -> String {
    canonical_program_path(program)
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn program_path_evidence(program: &str) -> Vec<PathBuf> {
    let mut evidence = Vec::new();
    let mut push = |path: PathBuf| {
        if !evidence.contains(&path) {
            evidence.push(path);
        }
    };
    let path = PathBuf::from(program);
    push(path.clone());
    if let Ok(target) = fs::read_link(&path) {
        push(if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or_else(|| Path::new("")).join(target)
        });
    }
    push(PathBuf::from(canonical_program_path(program)));
    evidence
}

fn normalized_program_path_evidence(program: &str) -> Vec<String> {
    program_path_evidence(program)
        .into_iter()
        .map(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
        })
        .collect()
}

fn npm_shim_content(program: &str) -> Option<String> {
    let extension = Path::new(program)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    matches!(extension.as_str(), "cmd" | "bat" | "ps1")
        .then(|| fs::read_to_string(program).ok())
        .flatten()
}

fn detected_upgrade_manager_from_evidence(
    normalized_program: &str,
    shim_content: Option<&str>,
) -> &'static str {
    if normalized_program.contains("/node_modules/")
        || shim_content.is_some_and(|content| {
            let normalized = content.replace('\\', "/").to_ascii_lowercase();
            normalized.contains("node_modules") && normalized.contains("node")
        })
    {
        "npm"
    } else if normalized_program.contains("/cellar/") || normalized_program.contains("/caskroom/") {
        "homebrew"
    } else {
        "standalone"
    }
}

pub(super) fn detected_upgrade_manager(program: &str) -> &'static str {
    let evidence = normalized_program_path_evidence(program);
    let shim = npm_shim_content(program);
    if evidence
        .iter()
        .any(|path| detected_upgrade_manager_from_evidence(path, shim.as_deref()) == "npm")
    {
        "npm"
    } else if evidence
        .iter()
        .any(|path| detected_upgrade_manager_from_evidence(path, None) == "homebrew")
    {
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
    let evidence = normalized_program_path_evidence(program);
    if evidence.iter().any(|path| path.contains("/node_modules/")) {
        None
    } else if evidence.iter().any(|path| path.contains("/caskroom/")) {
        Some("cask")
    } else if evidence.iter().any(|path| path.contains("/cellar/")) {
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

fn package_manager_stdout(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(args)
        .envs(get_login_shell_env().iter().cloned())
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn push_existing_candidate(candidates: &mut Vec<String>, path: PathBuf) {
    if !path.is_file() {
        return;
    }
    let value = path.to_string_lossy().into_owned();
    if !candidates.iter().any(|candidate| candidate == &value) {
        candidates.push(value);
    }
}

fn npm_candidates_for_program(program: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(parent) = Path::new(program).parent() {
        for name in ["npm", "npm.cmd", "npm.exe"] {
            push_existing_candidate(&mut candidates, parent.join(name));
        }
    }
    let canonical = PathBuf::from(canonical_program_path(program));
    if let Some(node_modules) = canonical.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name == "node_modules")
    }) {
        let root = node_modules.parent().unwrap_or(node_modules);
        let prefixes = [
            root.to_path_buf(),
            root.parent().unwrap_or(root).to_path_buf(),
        ];
        for prefix in prefixes {
            for candidate in [
                prefix.join("npm"),
                prefix.join("npm.cmd"),
                prefix.join("bin").join("npm"),
                prefix.join("bin").join("npm.cmd"),
            ] {
                push_existing_candidate(&mut candidates, candidate);
            }
        }
    }
    if let Some(program) = optional_program("npm") {
        if !candidates.iter().any(|candidate| candidate == &program) {
            candidates.push(program);
        }
    }
    candidates
}

fn npm_candidate_matches_install(program: &str, launch_program: &str, package: &str) -> bool {
    if !package_manager_has_install(program, &["list", "-g", "--depth=0", package]) {
        return false;
    }
    let Some(prefix) = package_manager_stdout(program, &["prefix", "-g"]) else {
        return false;
    };
    let Some(root) = package_manager_stdout(program, &["root", "-g"]) else {
        return false;
    };
    npm_install_paths_match(launch_program, &prefix, &root, package)
}

fn npm_install_paths_match(launch_program: &str, prefix: &str, root: &str, package: &str) -> bool {
    let launch = normalized_program_path(launch_program);
    let package_root = canonical_program_path(&PathBuf::from(root).join(package).to_string_lossy())
        .replace('\\', "/")
        .to_ascii_lowercase();
    if launch == package_root || launch.starts_with(&format!("{package_root}/")) {
        return true;
    }
    let launch_parent = Path::new(launch_program)
        .parent()
        .map(|path| canonical_program_path(&path.to_string_lossy()))
        .unwrap_or_default()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let prefix = canonical_program_path(prefix)
        .replace('\\', "/")
        .to_ascii_lowercase();
    launch_parent == prefix || launch_parent == format!("{prefix}/bin")
}

fn matching_npm_program(launch_program: &str, package: &str) -> Option<String> {
    npm_candidates_for_program(launch_program)
        .into_iter()
        .find(|program| npm_candidate_matches_install(program, launch_program, package))
}

fn matching_brew_program(launch_program: &str) -> Option<String> {
    let evidence = program_path_evidence(launch_program);
    let prefix = evidence.iter().find_map(|path| {
        let path = path.to_string_lossy().replace('\\', "/");
        let normalized = path.to_ascii_lowercase();
        ["/cellar/", "/caskroom/"]
            .into_iter()
            .find_map(|marker| normalized.find(marker))
            .map(|marker_index| path[..marker_index].to_string())
    })?;
    let prefix_path = fs::canonicalize(&prefix).unwrap_or_else(|_| PathBuf::from(prefix));
    let candidate = prefix_path.join("bin").join("brew");
    if candidate.is_file() {
        return Some(candidate.to_string_lossy().into_owned());
    }
    let fallback = optional_program("brew")?;
    let fallback_prefix = package_manager_stdout(&fallback, &["--prefix"])?;
    let fallback_prefix =
        fs::canonicalize(&fallback_prefix).unwrap_or_else(|_| PathBuf::from(fallback_prefix));
    (fallback_prefix == prefix_path).then_some(fallback)
}

fn brew_candidates_for_program(program: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(parent) = Path::new(program).parent() {
        push_existing_candidate(&mut candidates, parent.join("brew"));
    }
    if let Some(program) = optional_program("brew") {
        if !candidates.contains(&program) {
            candidates.push(program);
        }
    }
    candidates
}

fn brew_program_for_launch_prefix(launch_program: &str) -> Option<String> {
    let launch_parent = Path::new(launch_program).parent()?;
    let launch_parent =
        fs::canonicalize(launch_parent).unwrap_or_else(|_| launch_parent.to_path_buf());
    brew_candidates_for_program(launch_program)
        .into_iter()
        .find(|program| {
            package_manager_stdout(program, &["--prefix"])
                .map(|prefix| fs::canonicalize(&prefix).unwrap_or_else(|_| PathBuf::from(prefix)))
                .is_some_and(|prefix| launch_parent == prefix.join("bin"))
        })
}

fn matching_brew_install(launch_program: &str, package: &str) -> Option<(String, &'static str)> {
    if let Some(flavor) = detected_homebrew_flavor(launch_program) {
        return matching_brew_program(launch_program)
            .or_else(|| brew_program_for_launch_prefix(launch_program))
            .map(|program| (program, flavor));
    }
    let program = brew_program_for_launch_prefix(launch_program)?;
    let formula =
        package_manager_has_install(&program, &["list", "--versions", "--formula", package]);
    let cask = package_manager_has_install(&program, &["list", "--versions", "--cask", package]);
    match (formula, cask) {
        (true, false) => Some((program, "formula")),
        (false, true) => Some((program, "cask")),
        _ => None,
    }
}

pub(super) fn build_agent_upgrade_commands_from_detection(
    kind: AgentUpgradeKind,
    launch_program: &str,
    target_version: Option<&str>,
    npm_program: Option<String>,
    brew_program: Option<String>,
    brew_flavor: Option<&'static str>,
) -> Vec<AgentUpgradeCommand> {
    let configured_manager = detected_upgrade_manager(launch_program);
    if configured_manager == "npm" {
        if let Some(program) = npm_program {
            let package = match kind {
                AgentUpgradeKind::Claude => "@anthropic-ai/claude-code",
                AgentUpgradeKind::Codex => "@openai/codex",
                AgentUpgradeKind::Dsh => "@deepseek-ai/dsh",
            };
            let target_version = target_version
                .map(str::trim)
                .filter(|version| !version.is_empty());
            // The UI normally resolves the exact registry target before
            // starting the upgrade. npm's user-level min-release-age can
            // otherwise make either that target or `@latest` silently resolve
            // to an older version while exiting 0.
            let mut args = vec![
                "install".to_string(),
                "-g".to_string(),
                "--min-release-age=0".to_string(),
            ];
            args.push(format!("{package}@{}", target_version.unwrap_or("latest")));
            return vec![AgentUpgradeCommand {
                channel: "npm".to_string(),
                program,
                args,
            }];
        }
        return Vec::new();
    }
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
        // dsh 无 Homebrew 渠道;npm 是唯一官方分发。
        AgentUpgradeKind::Dsh => "",
    };
    if brew_name.is_empty() {
        return Vec::new();
    }
    if let (Some(program), Some(flavor)) = (brew_program, brew_flavor) {
        return vec![AgentUpgradeCommand {
            channel: "homebrew".to_string(),
            program,
            args: vec![
                "upgrade".to_string(),
                format!("--{flavor}"),
                brew_name.to_string(),
            ],
        }];
    }
    if kind == AgentUpgradeKind::Claude && configured_manager == "standalone" {
        return vec![AgentUpgradeCommand {
            channel: "native".to_string(),
            program: launch_program.to_string(),
            args: vec!["update".to_string()],
        }];
    }
    Vec::new()
}

pub(super) fn build_agent_upgrade_commands(
    kind: AgentUpgradeKind,
    launch_program: &str,
    target_version: Option<&str>,
) -> Result<Vec<AgentUpgradeCommand>, String> {
    let npm_package = match kind {
        AgentUpgradeKind::Claude => "@anthropic-ai/claude-code",
        AgentUpgradeKind::Codex => "@openai/codex",
        AgentUpgradeKind::Dsh => "@deepseek-ai/dsh",
    };
    let manager = detected_upgrade_manager(launch_program);
    let npm_program = (manager == "npm")
        .then(|| matching_npm_program(launch_program, npm_package))
        .flatten();
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
        AgentUpgradeKind::Dsh => "",
    };
    let brew_install = (!brew_name.is_empty() && manager != "npm")
        .then(|| matching_brew_install(launch_program, brew_name))
        .flatten();
    if kind == AgentUpgradeKind::Claude
        && manager == "standalone"
        && brew_install.is_none()
        && brew_program_for_launch_prefix(launch_program).is_some()
    {
        return Err(format!(
            "Cannot determine which Homebrew installation owns the active Claude Code executable at {launch_program:?}; refusing to run the native updater or modify another copy."
        ));
    }
    let (brew_program, brew_flavor) = brew_install
        .map(|(program, flavor)| (Some(program), Some(flavor)))
        .unwrap_or((None, None));
    let commands = build_agent_upgrade_commands_from_detection(
        kind,
        launch_program,
        target_version,
        npm_program,
        brew_program,
        brew_flavor,
    );
    if commands.is_empty() {
        let label = match kind {
            AgentUpgradeKind::Claude => "Claude Code",
            AgentUpgradeKind::Codex => "Codex",
            AgentUpgradeKind::Dsh => "DeepSeek Harness",
        };
        Err(format!(
            "Cannot upgrade the active {label} installation at {launch_program:?} (detected channel: {manager}). Aeroric will not modify another installed copy. Repair this installation or configure the executable that should be used."
        ))
    } else {
        Ok(commands)
    }
}

pub(super) fn run_agent_upgrade(command: &AgentUpgradeCommand) -> Result<String, String> {
    let mut process = Command::new(&command.program);
    // 升级走的是 npm / pnpm 这类 .cmd shim,同样要压掉控制台窗口。
    crate::subprocess::configure_background_command(&mut process);
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
        "dsh" => Some(AgentUpgradeKind::Dsh),
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| match profile.agent_family() {
                AgentFamily::Codex => AgentUpgradeKind::Codex,
                AgentFamily::Dsh => AgentUpgradeKind::Dsh,
                AgentFamily::Claude => AgentUpgradeKind::Claude,
            }),
    }
}

pub(super) fn upgrade_binary_agent(kind: AgentUpgradeKind) -> &'static str {
    match kind {
        AgentUpgradeKind::Claude => "claude",
        AgentUpgradeKind::Codex => "codex",
        AgentUpgradeKind::Dsh => "dsh",
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
    fn keeps_dsh_prerelease_suffix_in_extracted_versions() {
        assert_eq!(extract_semver("0.1.0-rc.6"), Some("0.1.0-rc.6".to_string()));
        assert_eq!(
            extract_semver("dsh 0.1.0-rc.6 (dev preview)"),
            Some("0.1.0-rc.6".to_string())
        );
        // 尾随分隔符不并入后缀。
        assert_eq!(extract_semver("1.2.3-"), Some("1.2.3".to_string()));
        // rc 版本的三元组比较把后缀按 0 处理,不影响既有 gte 判定。
        assert_eq!(parse_semver("0.1.0-rc.6"), (0, 1, 0));
    }

    #[test]
    fn verifies_release_and_prerelease_upgrade_targets_semantically() {
        assert!(version_reaches_target("2.1.234", "2.1.234"));
        assert!(version_reaches_target("2.1.235", "2.1.234"));
        assert!(version_reaches_target("0.1.0-rc.7", "0.1.0-rc.6"));
        assert!(!version_reaches_target("0.1.0-rc.6", "0.1.0-rc.7"));
        assert!(!version_reaches_target("unknown", "2.1.234"));
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
    fn detects_windows_npm_shim_from_its_node_modules_target() {
        assert_eq!(
            detected_upgrade_manager_from_evidence(
                "c:/users/test/appdata/roaming/npm/codex.cmd",
                Some(
                    "@SETLOCAL\r\n@node \"%~dp0\\node_modules\\@openai\\codex\\bin\\codex.js\" %*"
                ),
            ),
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

    #[cfg(unix)]
    #[test]
    fn detects_an_npm_install_through_its_executable_symlink() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("aeroric-npm-symlink-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let target = root
            .join("lib/node_modules/@openai/codex/bin")
            .join("codex.js");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "#!/usr/bin/env node\n").unwrap();
        let launch = bin.join("codex");
        symlink(&target, &launch).unwrap();

        assert_eq!(detected_upgrade_manager(&launch.to_string_lossy()), "npm");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn detects_a_homebrew_cask_through_its_executable_symlink() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("aeroric-brew-symlink-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let target = root.join("Caskroom/claude-code/2.0.0/claude");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "binary").unwrap();
        let launch = bin.join("claude");
        symlink(Path::new("../Caskroom/claude-code/2.0.0/claude"), &launch).unwrap();

        assert_eq!(
            detected_upgrade_manager(&launch.to_string_lossy()),
            "homebrew"
        );
        assert_eq!(
            detected_homebrew_flavor(&launch.to_string_lossy()),
            Some("cask")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn npm_prefix_matching_rejects_a_different_global_install() {
        assert!(npm_install_paths_match(
            "/prefix-a/lib/node_modules/@openai/codex/bin/codex.js",
            "/prefix-a",
            "/prefix-a/lib/node_modules",
            "@openai/codex",
        ));
        assert!(!npm_install_paths_match(
            "/prefix-a/lib/node_modules/@openai/codex/bin/codex.js",
            "/prefix-b",
            "/prefix-b/lib/node_modules",
            "@openai/codex",
        ));
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
            Some("/opt/homebrew/bin/brew".to_string()),
            Some("formula"),
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "homebrew");
        assert!(commands[0].args.contains(&"--formula".to_string()));
        assert!(commands[0].args.contains(&"codex".to_string()));
    }

    #[test]
    fn ignores_other_homebrew_channels_when_the_configured_program_is_standalone() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/aeroric-test/usr/local/bin/codex",
            None,
            None,
            Some("/opt/homebrew/bin/brew".to_string()),
            None,
        );

        assert!(commands.is_empty());
    }

    #[test]
    fn prefers_an_exact_homebrew_match_before_claude_native_update() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Claude,
            "/aeroric-test/opt/homebrew/bin/claude",
            None,
            None,
            Some("/opt/homebrew/bin/brew".to_string()),
            Some("cask"),
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "homebrew");
        assert_eq!(commands[0].args, ["upgrade", "--cask", "claude-code"]);
    }

    #[test]
    fn builds_only_the_configured_npm_upgrade() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            Some("0.147.0"),
            Some("/usr/local/bin/npm".to_string()),
            Some("/opt/homebrew/bin/brew".to_string()),
            Some("formula"),
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "npm");
        assert_eq!(commands[0].program, "/usr/local/bin/npm");
        assert_eq!(
            commands[0].args,
            [
                "install",
                "-g",
                "--min-release-age=0",
                "@openai/codex@0.147.0"
            ]
        );
    }

    #[test]
    fn npm_latest_upgrade_does_not_reinstall_an_older_release() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Claude,
            "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/bin/claude.js",
            None,
            Some("/opt/homebrew/bin/npm".to_string()),
            None,
            None,
        );

        assert_eq!(
            commands[0].args,
            [
                "install",
                "-g",
                "--min-release-age=0",
                "@anthropic-ai/claude-code@latest"
            ]
        );
    }

    #[test]
    fn never_treats_an_npm_claude_symlink_target_as_native() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Claude,
            "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/bin/claude.js",
            Some("2.1.234"),
            Some("/usr/local/bin/npm".to_string()),
            Some("/opt/homebrew/bin/brew".to_string()),
            Some("cask"),
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "npm");
        assert_eq!(commands[0].program, "/usr/local/bin/npm");
        assert_eq!(
            commands[0].args,
            [
                "install",
                "-g",
                "--min-release-age=0",
                "@anthropic-ai/claude-code@2.1.234"
            ]
        );
    }

    #[test]
    fn reports_the_active_path_for_an_unsupported_standalone_binary() {
        let error = build_agent_upgrade_commands(
            AgentUpgradeKind::Codex,
            "/opt/aeroric/custom/codex",
            Some("0.147.0"),
        )
        .expect_err("standalone Codex has no precise supported upgrade channel");

        assert!(error.contains("/opt/aeroric/custom/codex"));
        assert!(error.contains("detected channel: standalone"));
        assert!(error.contains("will not modify another installed copy"));
    }
}
