//! App settings: persisted user configuration for agents, proxy, and UI prefs.
//!
//! Phase-4 split: types live in [`schema`], I/O in [`load_save`], built-in
//! agent credential updates in [`builtin`], custom profile CRUD in
//! [`custom_agents`], and Tauri command shells in [`commands`]. Public paths
//! (`crate::app_settings::*`) are re-exported from this module.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::storage::{atomic_write, atomic_write_private};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

mod agent_env;
mod agent_scripts;
mod builtin;
mod commands;
mod config_bundles;
mod custom_agents;
mod launch_spec;
mod load_save;
mod model_detect;
mod models;
mod normalize;
mod proxy_test;
mod schema;
mod versions;

// launch_spec 里的路径解析:父文件自己只用这两个,其余 14 个符号那边自用。
use launch_spec::{get_agent_launch_spec_from_settings, normalize_agent_configured_path};
// `use super::*` 拿不到兄弟模块的 pub(crate) 项,所以要在这里转一手。
// 调用点只有 agent_scripts 的测试(那个文件 L2010 起的 `#[cfg(test)]` 块),
// 不加门控在非测试构建里就是个 unused import。
// 被测的函数本身是 `#[cfg(not(windows))]`(改的是 unix 权限位),re-export 得跟着门控,
// 否则 Windows 的 test 目标解析不到这个名字。
#[cfg(all(test, not(windows)))]
pub(crate) use launch_spec::ensure_user_agent_script_executable;

use agent_env::*;
use agent_scripts::*;
use config_bundles::*;
use model_detect::*;
use models::*;
use normalize::*;
use versions::*;

// New split modules: glob into parent scope so sibling `use super::*` keeps
// working, then explicit re-exports for crate-external paths.
use builtin::*;
use custom_agents::*;
use load_save::*;
use schema::*;

// `proxy_test` 里有个 tauri 命令(`test_proxy_connection`),`lib.rs` 的
// generate_handler! 里写的是 `app_settings::test_proxy_connection`。这里**必须用
// glob** `pub use`:`#[tauri::command]` 除了函数还会生成两个隐藏宏
// (`__cmd__<名字>` / `__tauri_command_name_<名字>`),按名字 re-export 带不走它们,
// generate_handler! 会报 "macro import ... is private"。
pub use proxy_test::*;

// 下面三个搬进子模块前分别是 `pub` / `pub(crate)`。glob import 只把名字拉进本作用域,
// 不替父模块对外转发可见性,所以要显式 re-export 一手,而且**必须原样保留可见性等级**。
// 调用点写的都是 `app_settings::<名字>`(`custom_agent_home` 有 6 个模块在用),
// 这样一处都不用改。
pub(crate) use agent_env::configured_agent_path;
pub use normalize::custom_agent_home;
pub(crate) use normalize::normalize_local_router_settings_for_update;

// Re-export split-module items that other crates/modules reach via
// `crate::app_settings::*`. Globs above only bring names into this scope.
#[allow(unused_imports)] // re-exported for stable `crate::app_settings::*` paths
pub use schema::{
    AgentBalance, AgentConfigBundle, AgentConfigBundleAgent, AgentConfigBundleKind, AgentFamily,
    AgentLaunchSpec, AgentModels, AgentSetupDraft, AgentSetupKind, AgentUpgradeChannel,
    AgentUpgradeResult, AgentVersions, AppSettings, AutoCleanupSettings, BuiltInAgentCredentials,
    ChatBridgePythonStatus, CustomAgentProfile, LegacyAgentProxyConfig, LocalRouterAgentSettings,
    LocalRouterSettings, NotebookEmbeddingSettings, ProxySettings, WeeklyReportSettings,
};
// Tauri command macros (`__cmd__*` / `__tauri_command_name_*`) only travel
// with a glob re-export — see the proxy_test note below.
pub use commands::*;

// pub(crate) APIs that live in the new modules. Private helpers stay reachable
// to siblings through the `use load_save::*` / `use builtin::*` globs above.
pub(crate) use builtin::{
    default_builtin_agent_config_path, list_builtin_dsh_models, omp_managed_home,
    update_builtin_agent_config_internal, update_builtin_agent_config_remote_internal,
};
pub(crate) use custom_agents::{
    update_custom_agent_config_internal, update_custom_agent_config_remote_internal,
};
pub(crate) use load_save::{load_settings_internal, save_managed_agent_path};

// Remaining helpers/types intentionally stay in this module (see report).

const CLAUDE_BUILTIN_MODEL_ALIASES: &[&str] = &["fable", "opus", "sonnet"];
// v8:Claude wrapper 支持「禁用 Artifact 工具」开关(`CLAUDE_CODE_DISABLE_ARTIFACT`)。
// bump 后启动期自动重刷存量脚本,否则老 wrapper 不会带上这个环境变量。
const CLAUDE_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=8";
const CLAUDE_AGENT_SCRIPT_MARKER_PREFIX: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=";
const CLAUDE_CLI_RESOLUTION_MARKER: &str = "# AERORIC_CLAUDE_CLI_RESOLUTION=1";
// v6:包装脚本追加 Aeroric 的 codex hook 片段(自定义 Agent 有隔离 CODEX_HOME,
// 读不到 `~/.codex/config.toml` 里的 hook 块)。bump 后启动期自动重刷存量脚本。
const CODEX_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CODEX_WRAPPER_VERSION=6";
// v7:bridge 启动前先探测出一个真正可用的 Python 3.9+(Windows 的 Microsoft Store
// 别名桩会被 Get-Command 找到但一运行就退出),等待窗口放宽到 20s 并在失败时把
// bridge 日志尾部带进报错。
// v8:支持在设置里固定解释器路径(`bridge_python_path`),且 `--version`/`--help`
// 探测直接短路不再拉起 bridge——桌面端的版本探测就是用 `--version` 跑这个脚本,
// 旧结构会让没装 Python 的机器连版本都测不出来。
const CODEX_CHAT_PROXY_MARKER: &str = "# AERORIC_CODEX_CHAT_PROXY_VERSION=8";
const LOCAL_CHAT_PROXY_BYPASS: &str = "127.0.0.1,localhost,::1";
const CODEX_CHAT_PROXY_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/codex_chat_proxy.py"
));

pub fn get_login_shell_env() -> &'static [(String, String)] {
    crate::platform::login_shell_env()
}

pub fn get_login_shell_path() -> &'static str {
    crate::platform::login_shell_path()
}

fn configured_agent_family(settings: &AppSettings, agent: &str) -> AgentFamily {
    match agent {
        "claude" => AgentFamily::Claude,
        "codex" | "claude_gpt55" => AgentFamily::Codex,
        "dsh" => AgentFamily::Dsh,
        "omp" => AgentFamily::Omp,
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| profile.agent_family())
            // 未知 agent 沿用历史行为:默认按 codex 族处理。
            .unwrap_or(AgentFamily::Codex),
    }
}

fn configured_agent_is_codex_like(settings: &AppSettings, agent: &str) -> bool {
    configured_agent_family(settings, agent).is_codex_like()
}

pub fn agent_family(agent: &str) -> AgentFamily {
    configured_agent_family(&load_settings_internal(), agent)
}

pub(crate) fn agent_family_in(settings: &AppSettings, agent: &str) -> AgentFamily {
    configured_agent_family(settings, agent)
}

pub(crate) fn dsh_reasoning_effort_in(settings: &AppSettings, agent: &str) -> String {
    settings
        .dsh_reasoning_efforts
        .get(agent)
        .cloned()
        .unwrap_or_else(|| "high".to_string())
}

pub fn is_codex_like_agent(agent: &str) -> bool {
    configured_agent_is_codex_like(&load_settings_internal(), agent)
}

pub fn is_dsh_agent(agent: &str) -> bool {
    agent_family(agent) == AgentFamily::Dsh
}

/// dsh 族 agent 任务级模型覆盖使用的 provider 名。
pub(crate) fn dsh_model_provider_for(agent: &str) -> String {
    let settings = load_settings_internal();
    let custom_base_url = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
        .map(|profile| !profile.base_url.trim().is_empty())
        .unwrap_or(false);
    if custom_base_url {
        "aeroric".to_string()
    } else {
        "deepseek-official".to_string()
    }
}

/// dsh 族 agent 配置的 API key(内建走 builtin_agent_credentials,自定义走档案)。
pub(crate) fn dsh_api_key_for(agent: &str) -> Option<String> {
    let settings = load_settings_internal();
    let key = if agent == "dsh" {
        settings
            .builtin_agent_credentials
            .get("dsh")
            .map(|credentials| credentials.api_key.clone())
    } else {
        settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
            .map(|profile| profile.api_key.clone())
    };
    key.map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// 解析 IPC 传入的可选 family 参数:优先 family 字符串,缺省由 is_codex 推导。
pub fn resolve_family_param(family: Option<&str>, is_codex: bool) -> AgentFamily {
    family
        .and_then(AgentFamily::parse)
        .unwrap_or_else(|| AgentFamily::from_codex_like(is_codex))
}

pub fn is_known_agent(agent: &str) -> bool {
    matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh" | "omp")
        || load_settings_internal()
            .custom_agents
            .iter()
            .any(|profile| profile.id == agent)
}

/// 自定义 Agent 隔离 home 的目录名(`~/.aeroric/agent-homes/{name}`);内建 Agent 返回 None,
/// 因为它们直接使用 `~/.claude` / `~/.codex`。
///
/// 会话文件定位需要它:自定义 claude-like Agent 的启动脚本把 `CLAUDE_CONFIG_DIR` 指向
/// 隔离 home,transcript 因此落在 `<agent-home>/projects/<encoded-project>/` 而不是
/// `~/.claude/projects/...`。
pub(crate) fn custom_agent_home_dir_name(agent: &str) -> Option<String> {
    // 内建 dsh 的托管 home 由 `dsh_home` 模块管理(`~/.aeroric/agent-homes/dsh`),
    // 不走本函数的自定义 agent 路径。
    if matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh") {
        return None;
    }
    let normalized = sanitize_custom_agent_id(agent);
    (!normalized.is_empty()).then_some(normalized)
}

fn detect_path(binary: &str) -> String {
    crate::platform::detect_path(binary)
}

pub(crate) fn get_agent_launch_spec_from(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
    get_agent_launch_spec_from_settings(settings, agent)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn toml_table_key(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(crate) fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

/// Validate whether a remote (paired-device) request may reuse the key already
/// stored on the desktop.
///
/// The mobile configuration surface deliberately never receives the existing
/// plaintext key.  An omitted key therefore means "keep the desktop value";
/// that is only safe when the request still targets the exact same base URL.
/// Otherwise a paired device could change the URL to an attacker-controlled
/// endpoint and make the desktop send the saved credential there.  A caller
/// that supplies a replacement key (or explicitly clears the old one) is
/// making the credential transition explicit and is allowed to change the URL.
pub(crate) fn validate_remote_api_key_reuse(
    stored_base_url: &str,
    stored_api_key: &str,
    requested_base_url: Option<&str>,
    requested_api_key: Option<&str>,
    clear_api_key: bool,
) -> Result<(), String> {
    if clear_api_key
        || requested_api_key.is_some_and(|value| !value.trim().is_empty())
        || stored_api_key.trim().is_empty()
    {
        return Ok(());
    }

    let Some(requested_base_url) = requested_base_url else {
        return Ok(());
    };
    if normalize_base_url(stored_base_url) != normalize_base_url(requested_base_url) {
        return Err("A new API key is required when changing the Base URL".to_string());
    }
    Ok(())
}

pub(super) fn agent_upgrade_detection_program(
    configured_program: &str,
    launch: &AgentLaunchSpec,
) -> String {
    // Upgrade-channel detection must follow the configured executable, not a
    // wrapper program from the launch spec (for example cmd.exe for a Windows
    // npm shim, or pnpm for a DSH source checkout).
    if configured_program.trim().is_empty() {
        launch.program.clone()
    } else {
        configured_program.to_string()
    }
}

pub(super) fn append_upgrade_verification(
    channels: &mut Vec<AgentUpgradeChannel>,
    active_program: &str,
    previous_version: &str,
    current_version: &str,
    expected_version: Option<&str>,
) {
    if channels.iter().any(|channel| !channel.success) {
        return;
    }
    let expected_version = expected_version
        .map(str::trim)
        .filter(|version| !version.is_empty());
    let failed = current_version.trim().is_empty()
        || expected_version
            .is_some_and(|expected| !version_reaches_target(current_version, expected));
    if !failed {
        return;
    }

    channels.push(AgentUpgradeChannel {
        channel: "verification".to_string(),
        success: false,
        message: format!(
            "The active executable at {:?} did not reach the expected version {} (before: {}, after: {}).",
            active_program,
            expected_version.unwrap_or("unknown"),
            if previous_version.trim().is_empty() {
                "unknown"
            } else {
                previous_version
            },
            if current_version.trim().is_empty() {
                "unknown"
            } else {
                current_version
            },
        ),
    });
}

pub(crate) fn detect_launch_version(launch: &AgentLaunchSpec) -> Option<String> {
    versions::detect_launch_version_impl(launch)
}

pub(crate) fn extract_version(text: &str) -> Option<String> {
    versions::extract_version_impl(text)
}

pub fn detect_claude_version() -> Option<String> {
    versions::detect_claude_version_impl()
}

pub fn detect_codex_version() -> Option<String> {
    versions::detect_codex_version_impl()
}

pub fn claude_version_gte(min_version: &str) -> bool {
    versions::claude_version_gte_impl(min_version)
}

pub fn agent_version_gte(agent: &str, min_version: &str) -> bool {
    versions::agent_version_gte_impl(agent, min_version)
}

pub fn codex_version_gte(min_version: &str) -> bool {
    versions::codex_version_gte_impl(min_version)
}

pub(crate) fn upgrade_manager_for_path(program: &str) -> &'static str {
    versions::upgrade_manager_for_path_impl(program)
}

/// 把任意 agent id 归并到负责升级的二进制 agent 键(自定义 Agent 按家族归并)。
pub(crate) fn upgrade_binary_agent_for(agent: &str) -> Option<&'static str> {
    let settings = load_settings_internal();
    upgrade_kind_for_agent(&settings, agent).map(upgrade_binary_agent)
}

/// 活动安装是否存在可用的包管理器升级渠道。dsh 的策略解析用它决定是否沿用既有
/// npm/Homebrew 升级路径。
pub(crate) fn agent_upgrade_channel_available(agent: &str, active_program: &str) -> bool {
    let settings = load_settings_internal();
    let Some(kind) = upgrade_kind_for_agent(&settings, agent) else {
        return false;
    };
    build_agent_upgrade_commands(kind, active_program, None)
        .is_ok_and(|commands| !commands.is_empty())
}

/// 供 dsh 托管安装完成后改写启动路径。
pub(crate) fn set_configured_dsh_path(path: &str) -> Result<(), String> {
    let path = path.to_string();
    update_settings_locked(|settings| {
        settings.dsh_path = path;
        Ok(())
    })
    .map(|_| ())
}

pub(crate) fn clear_cached_agent_versions() {
    clear_cached_versions();
}

/// 让安装管线复用升级结果的版本校验(避免包管理器退出 0 但没真升上去)。
pub(crate) fn append_agent_upgrade_verification(
    channels: &mut Vec<AgentUpgradeChannel>,
    active_program: &str,
    previous_version: &str,
    current_version: &str,
    expected_version: Option<&str>,
) {
    append_upgrade_verification(
        channels,
        active_program,
        previous_version,
        current_version,
        expected_version,
    );
}

/// dsh 沿用包管理器渠道时执行的升级命令。阻塞式,由调用方放进 spawn_blocking。
pub(crate) fn run_dsh_package_manager_upgrade(
    active_program: &str,
    target_version: Option<&str>,
) -> Vec<AgentUpgradeChannel> {
    match build_agent_upgrade_commands(AgentUpgradeKind::Dsh, active_program, target_version) {
        Ok(commands) => run_agent_upgrades(&commands),
        Err(error) => vec![AgentUpgradeChannel {
            channel: "detection".to_string(),
            success: false,
            message: error,
        }],
    }
}

/// omp 未安装时的兜底安装:npm 全局安装官方包。npm 不在 PATH 时返回官方
/// 安装指引(curl 脚本 / Homebrew tap 由用户自行选择)。阻塞式,由调用方放进
/// spawn_blocking。
pub(crate) fn run_omp_npm_install(target_version: Option<&str>) -> Result<String, String> {
    let npm = detect_path("npm");
    if npm.trim().is_empty() {
        return Err(
            "npm is unavailable on PATH. Install oh-my-pi with `curl https://omp.sh/install | sh`, \
             `brew install can1357/tap/omp`, or `npm install -g @oh-my-pi/pi-coding-agent`."
                .to_string(),
        );
    }
    let package = "@oh-my-pi/pi-coding-agent";
    let target = target_version
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .unwrap_or("latest");
    run_agent_upgrade(&AgentUpgradeCommand {
        channel: "npm".to_string(),
        program: npm,
        args: vec![
            "install".to_string(),
            "-g".to_string(),
            "--min-release-age=0".to_string(),
            format!("{package}@{target}"),
        ],
    })
}

/// Claude/Codex 的包管理器升级(dsh 走 `agent_tools` 的托管路径)。
pub(crate) fn run_builtin_agent_upgrade(
    agent: &str,
    target_version: Option<&str>,
) -> Result<AgentUpgradeResult, String> {
    let _guard = agent_upgrade_lock().blocking_lock();
    let settings = load_settings_internal();
    let kind = upgrade_kind_for_agent(&settings, agent)
        .ok_or_else(|| format!("Unknown agent: {agent}"))?;
    let binary_agent = upgrade_binary_agent(kind);
    let launch = get_agent_launch_spec_from_settings(&settings, binary_agent);
    let configured_program = get_agent_configured_path(&settings, binary_agent);
    let active_program = agent_upgrade_detection_program(&configured_program, &launch);
    let previous_version = detect_version(&launch).unwrap_or_default();
    let mut channels = match build_agent_upgrade_commands(kind, &active_program, target_version) {
        Ok(commands) => run_agent_upgrades(&commands),
        Err(error) => vec![AgentUpgradeChannel {
            channel: "detection".to_string(),
            success: false,
            message: error,
        }],
    };
    clear_cached_versions();
    let current_version = detect_version(&launch).unwrap_or_default();
    append_upgrade_verification(
        &mut channels,
        &active_program,
        &previous_version,
        &current_version,
        target_version,
    );
    let success = channels.iter().all(|channel| channel.success);
    let message = channels
        .iter()
        .map(|channel| format!("{}: {}", channel.channel, channel.message))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(AgentUpgradeResult {
        agent: agent.to_string(),
        success,
        previous_version,
        current_version,
        message,
        channels,
        channel: upgrade_manager_for_path(&active_program).to_string(),
        managed: false,
        runtime_recovery: None,
    })
}

#[cfg(test)]
mod tests {
    // 这几个测试验的是「设置 -> 启动环境」的端到端行为,所以留在父文件,
    // 只把 launch_spec 里的入口 import 进来。
    use super::launch_spec::build_agent_launch_spec;
    use super::*;

    #[test]
    fn upgrade_detection_uses_the_configured_path_instead_of_a_launch_wrapper() {
        let launch = AgentLaunchSpec {
            program: "cmd.exe".to_string(),
            args: vec!["/C".to_string(), "codex.cmd".to_string()],
            ..AgentLaunchSpec::default()
        };

        assert_eq!(
            agent_upgrade_detection_program(
                r"C:\Users\test\AppData\Roaming\npm\codex.cmd",
                &launch,
            ),
            r"C:\Users\test\AppData\Roaming\npm\codex.cmd"
        );
        assert_eq!(agent_upgrade_detection_program("", &launch), "cmd.exe");
    }

    #[test]
    fn upgrade_verification_requires_the_active_executable_to_reach_the_target() {
        let successful_update = AgentUpgradeChannel {
            channel: "npm".to_string(),
            success: true,
            message: "updated".to_string(),
        };
        let mut verified = vec![successful_update.clone()];
        append_upgrade_verification(
            &mut verified,
            "/Users/test/.local/bin/codex",
            "1.0.0",
            "1.1.0",
            Some("1.1.0"),
        );
        assert_eq!(verified.len(), 1);

        let mut unchanged = vec![successful_update.clone()];
        append_upgrade_verification(
            &mut unchanged,
            "/Users/test/.local/bin/codex",
            "1.0.0",
            "1.0.0",
            Some("1.1.0"),
        );
        let failure = unchanged.last().expect("verification failure is appended");
        assert_eq!(failure.channel, "verification");
        assert!(!failure.success);
        assert!(failure.message.contains("/Users/test/.local/bin/codex"));
        assert!(failure.message.contains("before: 1.0.0"));
        assert!(failure.message.contains("after: 1.0.0"));
        assert!(failure.message.contains("expected version 1.1.0"));

        let mut undetectable = vec![successful_update];
        append_upgrade_verification(
            &mut undetectable,
            "/Users/test/.local/bin/claude",
            "1.0.0",
            "",
            Some("1.1.0"),
        );
        assert_eq!(undetectable.last().unwrap().channel, "verification");
        assert!(undetectable
            .last()
            .unwrap()
            .message
            .contains("after: unknown"));
    }

    #[test]
    fn built_in_dsh_accepts_official_credentials_and_models() {
        let mut settings = AppSettings::default();
        apply_builtin_agent_access_update(
            &mut settings,
            "dsh",
            Some(String::new()),
            Some("sk-deepseek".to_string()),
            false,
            Some(vec![
                "deepseek-chat".to_string(),
                "deepseek-reasoner".to_string(),
            ]),
            None,
        )
        .unwrap();

        let credentials = settings.builtin_agent_credentials.get("dsh").unwrap();
        assert_eq!(credentials.api_key, "sk-deepseek");
        assert_eq!(
            credentials.models,
            vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()]
        );
        assert!(settings.custom_agents.is_empty());
    }

    #[test]
    fn remote_key_reuse_is_bound_to_the_existing_base_url() {
        // Whitespace and trailing slashes are presentation differences, not a
        // credential redirect. The comparison must remain deterministic even
        // when both sides normalize to an empty URL.
        assert_eq!(normalize_base_url("  ///  "), "");
        assert!(
            validate_remote_api_key_reuse("///", "stored-key", Some(" / "), None, false,).is_ok()
        );
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1/",
            "stored-key",
            Some(" https://api.example.test/v1 "),
            None,
            false,
        )
        .is_ok());
        let error = validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            None,
            false,
        )
        .expect_err("an omitted key must not follow a changed endpoint");
        assert!(error.contains("new API key"));
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            Some("replacement"),
            false,
        )
        .is_ok());
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            None,
            true,
        )
        .is_ok());
        // An omitted base URL means "leave the current one alone" and is
        // therefore safe even though no URL comparison is possible.
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            None,
            None,
            false,
        )
        .is_ok());
        // A blank key is the mobile UI's "keep the existing key" value, not a
        // replacement. It must still be rejected for a changed endpoint.
        let blank_key_error = validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            Some("  \t"),
            false,
        )
        .expect_err("a whitespace-only key must not authorize a URL change");
        assert!(blank_key_error.contains("new API key"));
    }

    #[test]
    fn remote_key_boundary_allows_url_changes_when_no_key_is_stored() {
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "",
            Some("https://other.example.test/v1"),
            None,
            false,
        )
        .is_ok());
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            " \t",
            Some("https://other.example.test/v1"),
            None,
            false,
        )
        .is_ok());
    }

    #[test]
    fn empty_agent_api_key_removes_the_existing_sidecar() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-credential-sidecar-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join("agent-key");

        write_agent_api_key_at_path(&path, "  old-key  ").expect("write sidecar");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old-key");

        // Clearing a profile must make the wrapper fail closed rather than
        // leave the old secret readable from disk.
        sync_agent_credentials_at_path(&path, " \t").expect("remove sidecar");
        assert!(!path.exists());

        // A subsequent replacement recreates the sidecar with normalized
        // contents, which is the path used when the user enters a new key.
        sync_agent_credentials_at_path(&path, "  new-key\n").expect("rewrite sidecar");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new-key");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_file_transaction_restores_files_when_apply_fails() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-file-transaction-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("wrapper.sh");
        let second = root.join("credentials");
        fs::write(&first, b"old wrapper\n").unwrap();
        fs::write(&second, b"old key\n").unwrap();

        let result =
            AgentFileTransaction::capture_and_apply([first.clone(), second.clone()], || {
                fs::write(&first, b"new wrapper\n").map_err(|error| error.to_string())?;
                fs::write(&second, b"new key\n").map_err(|error| error.to_string())?;
                Err("simulated settings preparation failure".to_string())
            });

        assert_eq!(
            result.expect_err("failed apply must be reported"),
            "simulated settings preparation failure"
        );
        assert_eq!(fs::read(&first).unwrap(), b"old wrapper\n");
        assert_eq!(fs::read(&second).unwrap(), b"old key\n");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn credential_sync_replaces_a_matching_sidecar_symlink() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-credential-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let outside = root.join("outside-key");
        let sidecar = root.join("sidecar-key");
        fs::write(&outside, "same-key").unwrap();
        symlink(&outside, &sidecar).unwrap();

        sync_agent_credentials_at_path(&sidecar, "same-key").expect("repair symlink");
        let metadata = fs::symlink_metadata(&sidecar).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "same-key");
        // Replacing the sidecar must not delete or rewrite the target that was
        // outside Aeroric's credential directory.
        assert_eq!(fs::read_to_string(&outside).unwrap(), "same-key");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn updating_a_generated_claude_profile_refreshes_the_wrapper_and_sidecar() {
        let id = format!("claude-refresh-{}", uuid::Uuid::new_v4().simple());
        let root = std::env::temp_dir().join(format!("aeroric-claude-upsert-{}", id));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join(format!("agent.{}", native_agent_script_extension()));
        let old_draft = AgentSetupDraft {
            id: id.clone(),
            label: "Claude old".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://old.example/v1".to_string(),
            api_key: "old-key".to_string(),
            model: "claude-old".to_string(),
            models: vec!["claude-old".to_string()],
            enable_1m_context: false,
            disable_artifact_tool: false,
            enable_chat_completions_proxy: false,
            bridge_python_path: String::new(),
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        fs::write(&script_path, build_agent_script(&old_draft)).unwrap();

        let existing = CustomAgentProfile {
            id: id.clone(),
            label: "Claude old".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: false,
            family: "claude".to_string(),
            config_lang: "shellscript".to_string(),
            base_url: old_draft.base_url.clone(),
            api_key: old_draft.api_key.clone(),
            models: old_draft.models.clone(),
            enable_1m_context: false,
            disable_artifact_tool: false,
            enable_chat_completions_proxy: false,
            bridge_python_path: String::new(),
            username: String::new(),
            password: String::new(),
        };
        let mut updated = existing.clone();
        updated.label = "Claude new".to_string();
        updated.base_url = "https://new.example/v1".to_string();
        updated.api_key = "new-key".to_string();
        updated.models = vec!["claude-new".to_string()];
        updated.enable_1m_context = true;
        assert_eq!(updated.agent_family(), AgentFamily::Claude);

        let mut settings = AppSettings {
            custom_agents: vec![existing],
            ..AppSettings::default()
        };
        upsert_custom_agent_profile_unlocked(&mut settings, updated).unwrap();

        let saved = settings.custom_agents.first().unwrap();
        let script = fs::read_to_string(&saved.path).unwrap();
        assert!(script.contains("new.example"));
        assert!(script.contains("claude-new"));
        assert!(script.contains("[1m]"));
        assert!(!script.contains("old.example"));
        let credential_path = agent_api_key_path(&id).unwrap();
        assert_eq!(fs::read_to_string(&credential_path).unwrap(), "new-key");

        let _ = remove_agent_api_key(&id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dsh_reasoning_effort_uses_only_aeroric_supported_levels() {
        let mut settings = AppSettings::default();
        assert_eq!(dsh_reasoning_effort_in(&settings, "dsh"), "high");

        apply_dsh_reasoning_effort_update(&mut settings, "dsh", " OFF ").unwrap();
        assert_eq!(dsh_reasoning_effort_in(&settings, "dsh"), "off");
        assert!(apply_dsh_reasoning_effort_update(&mut settings, "dsh", "low").is_err());
        assert!(apply_dsh_reasoning_effort_update(&mut settings, "codex", "max").is_err());
    }

    /// 内建 dsh 目录的首项是"默认模型"的唯一来源:模型选择脚本在
    /// `AERORIC_AGENT_MODEL` 缺失时取它,新档案草稿也按同一顺序落默认值。
    /// 顺序排错的表现是新任务静默跑在旧模型上,编译期看不出来。
    #[test]
    fn builtin_dsh_catalog_defaults_to_deepseek_flash() {
        let models = list_builtin_dsh_models();
        assert_eq!(models.first().map(String::as_str), Some("deepseek-flash"));
        assert!(models.iter().any(|model| model == "deepseek-v4-flash"));
        assert!(models.iter().any(|model| model == "deepseek-v4-pro"));
    }

    /// `model_picker_shell` 只在 POSIX 上编译;Windows 走 PowerShell 另一条路径。
    #[cfg(not(windows))]
    #[test]
    fn dsh_model_picker_falls_back_to_the_catalog_head() {
        let shell = super::agent_scripts::model_picker_shell(&list_builtin_dsh_models());
        assert!(
            shell.contains("selected_model='deepseek-flash'"),
            "model picker must fall back to the catalog head: {shell}"
        );
    }

    #[test]
    fn normalizing_dsh_path_does_not_reenter_settings_lock() {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-normalize-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("apps").join("cli")).unwrap();
        std::fs::write(root.join("package.json"), "{}\n").unwrap();

        let settings = AppSettings {
            dsh_path: root.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let _guard = settings_lock().lock();
        let normalized = normalize_settings(settings);

        assert_eq!(normalized.dsh_path, root.to_string_lossy());
        drop(_guard);
        let _ = std::fs::remove_dir_all(root);
    }

    fn last_env_value<'a>(launch: &'a AgentLaunchSpec, key: &str) -> Option<&'a str> {
        launch
            .extra_env
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    /// 按"路由确实在监听"构建启动参数。测试进程里没有真的起服务，
    /// 所以显式给出这个前提，与线上从 [`crate::local_router::is_listening_on`] 读到的一致。
    fn launch_spec_with_router_listening(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
        build_agent_launch_spec(settings, agent, true)
    }

    #[test]
    fn settings_cache_invalidates_after_an_external_file_change() {
        let root =
            std::env::temp_dir().join(format!("aeroric-settings-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        std::fs::write(&path, "{}").unwrap();

        let settings = AppSettings {
            send_shortcut: "enter".to_string(),
            ..AppSettings::default()
        };
        cache_settings(&path, &settings);
        assert_eq!(
            get_cached_settings(&path).map(|cached| cached.send_shortcut),
            Some("enter".to_string())
        );

        let original_fingerprint = settings_file_fingerprint(&path);
        std::fs::write(&path, "[]").unwrap();
        let changed_fingerprint = settings_file_fingerprint(&path);
        assert_eq!(original_fingerprint.len, changed_fingerprint.len);
        assert_ne!(
            original_fingerprint.content_sha256,
            changed_fingerprint.content_sha256
        );
        assert!(get_cached_settings(&path).is_none());

        *CACHED_SETTINGS.get_or_init(|| Mutex::new(None)).lock() = None;
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_settings_default_to_a_disabled_local_router() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            settings.local_router_settings,
            LocalRouterSettings::default()
        );
        assert!(!settings.local_router_settings.enabled);
        assert!(settings.local_router_settings.claude_enabled);
        assert!(settings.local_router_settings.codex_enabled);
    }

    #[test]
    fn local_router_settings_accept_ip_listeners_and_reject_privileged_ports() {
        let normalized = normalize_settings(AppSettings {
            local_router_settings: LocalRouterSettings {
                listen_host: "0.0.0.0".to_string(),
                listen_port: 80,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        });
        assert_eq!(normalized.local_router_settings.listen_host, "0.0.0.0");
        assert_eq!(
            normalized.local_router_settings.listen_port,
            DEFAULT_LOCAL_ROUTER_PORT
        );
        assert!(normalized.local_router_settings.access_token.len() >= 32);
    }

    #[test]
    fn local_router_normalization_preserves_an_existing_access_token() {
        let normalized = normalize_local_router_settings(LocalRouterSettings {
            access_token: "  aeroric-0123456789abcdef0123456789abcdef  ".to_string(),
            ..LocalRouterSettings::default()
        });
        assert_eq!(
            normalized.access_token,
            "aeroric-0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn a_cleared_notebook_embedding_field_falls_back_to_the_default() {
        // 空 base URL 会让 `embed::endpoint_for` 回一条 Config 错误,而用户看着的是自己
        // 刚清空的输入框 —— 报错和现象对不上。
        let normalized = normalize_settings(AppSettings {
            notebook_embedding_settings: NotebookEmbeddingSettings {
                provider: crate::notebook::rag::embed::EmbedProvider::OpenAi,
                base_url: "   ".to_string(),
                model: String::new(),
            },
            ..AppSettings::default()
        });
        let defaults = NotebookEmbeddingSettings::default();
        assert_eq!(
            normalized.notebook_embedding_settings.base_url,
            defaults.base_url
        );
        assert_eq!(normalized.notebook_embedding_settings.model, defaults.model);
        // provider 不因为别的字段被洗掉而回退。
        assert_eq!(
            normalized.notebook_embedding_settings.provider,
            crate::notebook::rag::embed::EmbedProvider::OpenAi
        );
    }

    #[test]
    fn notebook_embedding_settings_are_trimmed_not_validated() {
        // 粘贴进来的地址常带首尾空白,而 URL 形状的校验归 `embed::endpoint_for`(它还要
        // 处理重复 `/v1` 与末尾斜杠)—— 两处各写一遍只会互相跑偏。
        let normalized = normalize_notebook_embedding_settings(NotebookEmbeddingSettings {
            provider: crate::notebook::rag::embed::EmbedProvider::OpenAi,
            base_url: "  https://api.openai.com/v1/  ".to_string(),
            model: " text-embedding-3-small\n".to_string(),
        });
        assert_eq!(normalized.base_url, "https://api.openai.com/v1/");
        assert_eq!(normalized.model, "text-embedding-3-small");
    }

    #[test]
    fn missing_notebook_embedding_settings_default_to_local_ollama() {
        // 老配置文件里没有这一段。落到本机 Ollama —— 那也是设置页出现之前前端硬编码的
        // 那个默认值,于是升级不改变任何人的既有行为。
        let settings: AppSettings = serde_json::from_str("{}").expect("parse");
        assert_eq!(
            settings.notebook_embedding_settings,
            NotebookEmbeddingSettings::default()
        );
        assert_eq!(
            settings.notebook_embedding_settings.base_url,
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            settings.notebook_embedding_settings.provider,
            crate::notebook::rag::embed::EmbedProvider::Ollama
        );
    }

    #[test]
    fn legacy_settings_get_safe_cleanup_and_report_defaults() {
        // 升级前的 settings.json 里没有这两段。默认必须是"清理关着、区间是周一到周日" ——
        // 一个默认开启的物理删除会在用户毫不知情的情况下清掉半年记录。
        let settings: AppSettings = serde_json::from_str("{}").expect("parse");
        assert!(!settings.auto_cleanup_settings.enabled);
        assert_eq!(settings.auto_cleanup_settings.mode, "weekly");
        assert_eq!(settings.auto_cleanup_settings.weekday, 0);
        assert_eq!(settings.auto_cleanup_settings.hour, 20);
        assert_eq!(settings.auto_cleanup_settings.retain_days, 30);
        assert_eq!(settings.auto_cleanup_settings.last_run_at, None);
        assert_eq!(settings.weekly_report_settings.week_start_day, 1);
        assert_eq!(settings.weekly_report_settings.week_end_day, 0);
        assert!(settings.weekly_report_settings.output_dir.is_empty());
    }

    #[test]
    fn normalize_settings_preserves_cleanup_and_report_sections() {
        // `normalize_settings` 逐字段重建结构体(不是 `..settings`),漏掉一段的表现是
        // "改完设置、重启就回默认",而编译期看不出来。
        let normalized = normalize_settings(AppSettings {
            auto_cleanup_settings: AutoCleanupSettings {
                enabled: true,
                mode: "interval".to_string(),
                weekday: 3,
                hour: 9,
                interval_days: 14,
                retain_days: 60,
                last_run_at: Some(1_757_000_000_000),
            },
            weekly_report_settings: WeeklyReportSettings {
                week_start_day: 0,
                week_end_day: 6,
                output_dir: "/tmp/reports".to_string(),
            },
            ..AppSettings::default()
        });
        assert!(normalized.auto_cleanup_settings.enabled);
        assert_eq!(normalized.auto_cleanup_settings.mode, "interval");
        assert_eq!(normalized.auto_cleanup_settings.interval_days, 14);
        assert_eq!(normalized.auto_cleanup_settings.retain_days, 60);
        assert_eq!(
            normalized.auto_cleanup_settings.last_run_at,
            Some(1_757_000_000_000)
        );
        assert_eq!(normalized.weekly_report_settings.week_start_day, 0);
        assert_eq!(normalized.weekly_report_settings.week_end_day, 6);
        assert_eq!(normalized.weekly_report_settings.output_dir, "/tmp/reports");
    }

    #[test]
    fn hand_edited_out_of_range_cleanup_values_are_clamped_not_rejected() {
        // 手改过 settings.json 的用户不该被一个手抖的数字挡在设置面板外,更不该让
        // `interval_days: 0` 变成"每 0 天删一次"。
        let normalized = normalize_settings(AppSettings {
            auto_cleanup_settings: AutoCleanupSettings {
                enabled: true,
                mode: "monthly".to_string(),
                weekday: 99,
                hour: 250,
                interval_days: 0,
                retain_days: 0,
                last_run_at: None,
            },
            weekly_report_settings: WeeklyReportSettings {
                week_start_day: 42,
                week_end_day: 42,
                output_dir: String::new(),
            },
            ..AppSettings::default()
        });
        assert_eq!(normalized.auto_cleanup_settings.mode, "weekly");
        assert_eq!(normalized.auto_cleanup_settings.weekday, 6);
        assert_eq!(normalized.auto_cleanup_settings.hour, 23);
        assert_eq!(normalized.auto_cleanup_settings.interval_days, 1);
        assert_eq!(normalized.auto_cleanup_settings.retain_days, 1);
        assert_eq!(normalized.weekly_report_settings.week_start_day, 6);
        assert_eq!(normalized.weekly_report_settings.week_end_day, 6);
    }

    #[test]
    fn enabled_local_router_overrides_builtin_agent_base_urls_last() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_host: "::1".to_string(),
                listen_port: 19090,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "claude".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://claude.example.test".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://codex.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );
        settings.builtin_agent_credentials.insert(
            "claude_gpt55".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://gpt55.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );

        let claude = launch_spec_with_router_listening(&settings, "claude");
        assert_eq!(
            last_env_value(&claude, "ANTHROPIC_BASE_URL"),
            Some("http://[::1]:19090/claude/targets/claude")
        );
        let codex = launch_spec_with_router_listening(&settings, "codex");
        assert_eq!(
            last_env_value(&codex, "OPENAI_BASE_URL"),
            Some("http://[::1]:19090/codex/targets/codex/v1")
        );
        let claude_gpt55 = launch_spec_with_router_listening(&settings, "claude_gpt55");
        assert_eq!(
            last_env_value(&claude_gpt55, "OPENAI_BASE_URL"),
            Some("http://[::1]:19090/codex/targets/claude_gpt55/v1")
        );
        assert_eq!(
            last_env_value(&codex, "NO_PROXY"),
            Some("127.0.0.1,localhost,::1")
        );
    }

    #[test]
    fn local_router_does_not_replace_an_unconfigured_gpt55_launcher() {
        let settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };

        let launch = launch_spec_with_router_listening(&settings, "claude_gpt55");
        assert_eq!(last_env_value(&launch, "OPENAI_BASE_URL"), None);
    }

    /// 开关是开的但服务没在监听时不能改写 base URL，否则 Agent 会一直请求
    /// `http://127.0.0.1:<port>/...` 并报 `error sending request for url`。
    #[test]
    fn a_router_that_is_not_listening_leaves_agent_base_urls_alone() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_port: 19092,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://codex.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );
        settings
            .custom_agents
            .push(test_custom_profile("custom", "custom", true));

        let codex = build_agent_launch_spec(&settings, "codex", false);
        assert_eq!(
            last_env_value(&codex, "OPENAI_BASE_URL"),
            Some("https://codex.example.test/v1")
        );
        let custom = build_agent_launch_spec(&settings, "custom", false);
        assert_eq!(last_env_value(&custom, "OPENAI_BASE_URL"), None);
    }

    #[test]
    fn non_loopback_local_router_overrides_client_credentials_with_router_token() {
        let token = "aeroric-0123456789abcdef0123456789abcdef";
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_host: "0.0.0.0".to_string(),
                access_token: token.to_string(),
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                api_key: "upstream-secret".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );

        let launch = launch_spec_with_router_listening(&settings, "codex");
        assert_eq!(last_env_value(&launch, "OPENAI_API_KEY"), Some(token));
        assert_eq!(last_env_value(&launch, "CODEX_API_KEY"), Some(token));
    }

    #[test]
    fn local_router_routes_custom_agents_by_protocol_family() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                claude_enabled: false,
                listen_port: 19091,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings
            .custom_agents
            .push(test_custom_profile("custom", "custom", true));

        let claude = launch_spec_with_router_listening(&settings, "claude");
        assert_eq!(last_env_value(&claude, "ANTHROPIC_BASE_URL"), None);
        let custom = launch_spec_with_router_listening(&settings, "custom");
        assert_eq!(
            last_env_value(&custom, "OPENAI_BASE_URL"),
            Some("http://127.0.0.1:19091/codex/targets/custom/v1")
        );

        settings.custom_agents[0].base_url.clear();
        let custom_without_router_target = launch_spec_with_router_listening(&settings, "custom");
        assert_eq!(
            last_env_value(&custom_without_router_target, "OPENAI_BASE_URL"),
            None
        );
    }

    fn test_custom_profile(id: &str, label: &str, codex_like: bool) -> CustomAgentProfile {
        CustomAgentProfile {
            id: id.to_string(),
            label: label.to_string(),
            path: format!("/tmp/{id}.sh"),
            codex_like,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["model".to_string()],
            enable_1m_context: false,
            disable_artifact_tool: false,
            enable_chat_completions_proxy: false,
            bridge_python_path: String::new(),
            username: String::new(),
            password: String::new(),
        }
    }

    #[test]
    fn new_agent_ids_include_type_and_never_overwrite_existing_profiles() {
        let mut settings = AppSettings::default();
        settings
            .custom_agents
            .push(test_custom_profile("demo_codex", "demo", true));

        assert_eq!(
            allocate_setup_agent_id("demo_codex", &AgentSetupKind::Codex, &settings).unwrap(),
            "demo_codex_2"
        );
        assert_eq!(
            allocate_setup_agent_id("demo_claude", &AgentSetupKind::ClaudeCode, &settings).unwrap(),
            "demo_claude"
        );
        assert_eq!(
            allocate_setup_agent_id("demo", &AgentSetupKind::ClaudeCode, &settings).unwrap(),
            "demo_claude"
        );
    }

    #[test]
    fn generated_agent_home_deletion_is_exact_and_does_not_touch_siblings() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-home-delete-{}",
            uuid::Uuid::new_v4()
        ));
        let homes = root.join("agent-homes");
        let selected = homes.join("demo_codex");
        let sibling = homes.join("demo_claude");
        fs::create_dir_all(selected.join("session-env")).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(selected.join("settings.json"), "{}").unwrap();
        fs::write(sibling.join("settings.json"), "{}").unwrap();

        remove_exact_generated_agent_home_at(&homes, "demo_codex").unwrap();

        assert!(!selected.exists());
        assert!(sibling.join("settings.json").exists());
        assert!(remove_exact_generated_agent_home_at(&homes, "../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }

    /// 删除 dsh / omp 档案不得删掉用户自己装的二进制。
    ///
    /// 这两族不生成 wrapper:`setup_agent_profile` 把 `detect_path("omp")` 探到的
    /// 绝对路径(`/opt/homebrew/bin/omp`)直接存进 `profile.path`。
    /// `delete_custom_agent_profile` 曾无条件 `remove_agent_profile_file(&profile.path)`,
    /// 于是"删除档案"把 omp 可执行文件本体删了,内置 omp 与其余同族档案一起失效。
    #[test]
    fn deleting_a_dsh_or_omp_profile_must_not_delete_the_user_installed_binary() {
        let root =
            std::env::temp_dir().join(format!("aeroric-omp-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("omp");
        fs::write(&binary, "#!/bin/sh\nexec real-omp \"$@\"\n").unwrap();

        for family in ["omp", "dsh"] {
            let profile = CustomAgentProfile {
                id: format!("demo_{family}"),
                label: format!("Demo {family}"),
                path: binary.to_string_lossy().into_owned(),
                codex_like: false,
                family: family.to_string(),
                // 两族都是 yaml —— 正是 profile_uses_aeroric_generated_wrapper 的
                // "shellscript" 要求落空、旧代码因此走上无条件删除的原因。
                config_lang: "yaml".to_string(),
                base_url: "https://api.example.com/v1".to_string(),
                api_key: "sk-secret".to_string(),
                models: vec!["demo-model".to_string()],
                enable_1m_context: false,
                disable_artifact_tool: false,
                enable_chat_completions_proxy: false,
                bridge_python_path: String::new(),
                username: String::new(),
                password: String::new(),
            };

            // 删除路径的守卫条件:非 Aeroric 生成的 launcher 一律不碰。
            assert!(
                !profile_uses_aeroric_generated_wrapper(&profile),
                "{family} 档案不该被认成 Aeroric 生成的 wrapper"
            );
            assert!(
                matches!(profile.agent_family(), AgentFamily::Dsh | AgentFamily::Omp),
                "{family} 档案必须归到 dsh/omp 族,隔离 home 才会被清理"
            );
        }

        // 反过来确认这条守卫确实是唯一的屏障:去掉它就会删掉这个文件。
        assert!(binary.exists());
        remove_agent_profile_file(&binary.to_string_lossy()).unwrap();
        assert!(
            !binary.exists(),
            "remove_agent_profile_file 会真的删文件 —— 所以删除路径必须靠 generated 守卫拦住"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_builtin_credentials_are_applied_to_launch_environment() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-builtin-agent-import-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("settings.json");
        let mut settings = AppSettings {
            claude_config_path: config_path.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let result = import_agent_config_entry(
            &mut settings,
            AgentConfigBundleAgent {
                id: "claude".to_string(),
                label: "Imported Claude".to_string(),
                kind: AgentConfigBundleKind::BuiltIn,
                codex_like: false,
                family: String::new(),
                config_lang: "json".to_string(),
                config_content: "{}".to_string(),
                config_present: true,
                base_url: "https://api.example.com/v1/".to_string(),
                api_key: "sk-imported".to_string(),
                models: vec!["claude-opus".to_string()],
                enable_1m_context: true,
                disable_artifact_tool: false,
                enable_chat_completions_proxy: false,
                bridge_python_path: String::new(),
                reasoning_effort: None,
            },
        )
        .unwrap();

        assert_eq!(result.config_path, config_path.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), "{}");
        let launch = get_agent_launch_spec_from_settings(&settings, "claude");
        assert!(launch.extra_env.contains(&(
            "ANTHROPIC_BASE_URL".to_string(),
            "https://api.example.com/v1".to_string()
        )));
        assert!(launch.extra_env.contains(&(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            "sk-imported".to_string()
        )));
        assert!(launch
            .extra_env
            .contains(&("ANTHROPIC_MODEL".to_string(), "claude-opus[1m]".to_string())));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_previous_claude_wrapper_versions_for_safe_refresh() {
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=2\n& 'claude' @args"
        ));
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=4\n& 'claude' @args"
        ));
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=5\n& 'claude' @args"
        ));
        assert!(!is_aeroric_generated_agent_wrapper(
            "# My Claude wrapper\n& 'claude' @args"
        ));
    }

    #[test]
    fn generated_agent_scripts_use_the_native_platform_extension() {
        let native = native_agent_script_extension();
        let other = if native == "ps1" { "sh" } else { "ps1" };
        let current = format!("/tmp/aeroric-agent.{other}");
        let target = generated_agent_script_target_path("aeroric-agent", &current).unwrap();

        assert_eq!(
            target.extension().and_then(|extension| extension.to_str()),
            Some(native)
        );
        assert_eq!(
            generated_agent_script_target_path(
                "aeroric-agent",
                &format!("/tmp/aeroric-agent.{native}")
            )
            .unwrap(),
            PathBuf::from(format!("/tmp/aeroric-agent.{native}"))
        );
    }

    #[test]
    fn launch_spec_prefers_the_executable_cli_family_over_a_stale_agent_type() {
        let claude_pointing_to_codex = AppSettings {
            claude_path: "/tmp/codex".to_string(),
            ..AppSettings::default()
        };
        let codex_pointing_to_claude = AppSettings {
            codex_path: "/tmp/claude".to_string(),
            ..AppSettings::default()
        };

        assert!(
            get_agent_launch_spec_from_settings(&claude_pointing_to_codex, "claude").codex_like
        );
        assert!(
            !get_agent_launch_spec_from_settings(&codex_pointing_to_claude, "codex").codex_like
        );
    }

    #[test]
    fn maps_custom_agent_profiles_to_their_shared_cli_runtime() {
        let settings = AppSettings {
            custom_agents: vec![
                CustomAgentProfile {
                    id: "custom_codex".to_string(),
                    label: "Custom Codex".to_string(),
                    path: "/tmp/custom-codex.sh".to_string(),
                    codex_like: true,
                    family: String::new(),
                    config_lang: "shellscript".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    models: Vec::new(),
                    enable_1m_context: false,
                    disable_artifact_tool: false,
                    enable_chat_completions_proxy: false,
                    bridge_python_path: String::new(),
                    username: String::new(),
                    password: String::new(),
                },
                CustomAgentProfile {
                    id: "custom_claude".to_string(),
                    label: "Custom Claude".to_string(),
                    path: "/tmp/custom-claude.sh".to_string(),
                    codex_like: false,
                    family: String::new(),
                    config_lang: "shellscript".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    models: Vec::new(),
                    enable_1m_context: false,
                    disable_artifact_tool: false,
                    enable_chat_completions_proxy: false,
                    bridge_python_path: String::new(),
                    username: String::new(),
                    password: String::new(),
                },
            ],
            ..AppSettings::default()
        };

        assert_eq!(
            upgrade_kind_for_agent(&settings, "custom_codex"),
            Some(AgentUpgradeKind::Codex)
        );
        assert_eq!(
            upgrade_kind_for_agent(&settings, "custom_claude"),
            Some(AgentUpgradeKind::Claude)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn global_proxy_settings_are_added_to_enabled_agent_launch_env() {
        let mut proxy_enabled = HashMap::new();
        proxy_enabled.insert("joverna".to_string(), true);
        let settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "joverna".to_string(),
                label: "Joverna".to_string(),
                path: "/Users/macbook/.claude/start-joverna.sh".to_string(),
                codex_like: false,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: String::new(),
                api_key: String::new(),
                models: Vec::new(),
                enable_1m_context: false,
                disable_artifact_tool: false,
                enable_chat_completions_proxy: false,
                bridge_python_path: String::new(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            }],
            proxy_settings: ProxySettings {
                url: "127.0.0.1:7890".to_string(),
                no_proxy: " localhost, 127.0.0.1 ".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            },
            agent_proxy_enabled: proxy_enabled,
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert_eq!(launch.program, "/Users/macbook/.claude/start-joverna.sh");
        assert!(launch.extra_env.contains(&(
            "HTTPS_PROXY".to_string(),
            "http://127.0.0.1:7890".to_string()
        )));
        assert!(launch
            .extra_env
            .contains(&("NO_PROXY".to_string(), "localhost,127.0.0.1".to_string())));
        assert!(launch
            .extra_env
            .contains(&("AERORIC_AGENT_USERNAME".to_string(), "alice".to_string())));
        assert!(launch
            .extra_env
            .contains(&("AERORIC_AGENT_PASSWORD".to_string(), "secret".to_string())));
    }

    #[test]
    fn legacy_custom_agent_credentials_migrate_to_global_proxy_settings() {
        let settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "joverna".to_string(),
                label: "Joverna".to_string(),
                path: "/Users/macbook/.claude/start-joverna.sh".to_string(),
                codex_like: false,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: String::new(),
                api_key: String::new(),
                models: Vec::new(),
                enable_1m_context: false,
                disable_artifact_tool: false,
                enable_chat_completions_proxy: false,
                bridge_python_path: String::new(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            }],
            ..AppSettings::default()
        };

        let normalized = normalize_settings(settings);

        assert_eq!(normalized.proxy_settings.username, "alice");
        assert_eq!(normalized.proxy_settings.password, "secret");
        assert_eq!(normalized.custom_agents[0].username, "");
        assert_eq!(normalized.custom_agents[0].password, "");
    }

    #[test]
    fn global_proxy_credentials_are_omitted_when_agent_proxy_is_disabled() {
        let settings = AppSettings {
            proxy_settings: ProxySettings {
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_USERNAME"));
        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_PASSWORD"));
    }

    #[test]
    fn global_proxy_credentials_are_omitted_without_proxy_url() {
        let mut proxy_enabled = HashMap::new();
        proxy_enabled.insert("joverna".to_string(), true);
        let settings = AppSettings {
            proxy_settings: ProxySettings {
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            agent_proxy_enabled: proxy_enabled,
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_USERNAME"));
        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_PASSWORD"));
    }

    #[test]
    fn legacy_agent_proxy_settings_migrate_to_global_proxy_and_enabled_flags() {
        let mut proxy_overrides = HashMap::new();
        proxy_overrides.insert(
            "Joverna".to_string(),
            LegacyAgentProxyConfig {
                enabled: true,
                url: "127.0.0.1:7890".to_string(),
                no_proxy: " localhost, 127.0.0.1 ".to_string(),
            },
        );
        let normalized = normalize_settings(AppSettings {
            agent_proxy_overrides: proxy_overrides,
            ..AppSettings::default()
        });

        assert_eq!(
            normalized.proxy_settings,
            ProxySettings {
                url: "http://127.0.0.1:7890".to_string(),
                no_proxy: "localhost,127.0.0.1".to_string(),
                username: String::new(),
                password: String::new(),
            }
        );
        assert_eq!(normalized.agent_proxy_enabled.get("joverna"), Some(&true));
        assert!(normalized.agent_proxy_overrides.is_empty());
    }
}
