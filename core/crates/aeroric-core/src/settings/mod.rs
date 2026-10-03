//! 应用设置（P8-02 自宿主 `app_settings/`＋`dsh_home/omp_home` 上收）。
//!
//! 数据根注入：core 不解析全局 home——宿主启动时 [`configure_data_dir`] 注入
//! `~/.aeroric`，测试注入临时目录（与宿主 `install_scratch_home` 同语义）。
//! `proxy_test.rs` 与 `commands.rs`（Tauri 命令层）留宿主。
//!
//! **dead_code 注记**：宿主（src-tauri）仍持自己的 `app_settings` 副本（P8-02
//! 刀五接线前两侧共存）。core 侧上收的主体在本 crate 内暂无调用者，clippy
//! 的 never-used 在模块级放行；刀五宿主接线后逐条消费、注记删除。

// 宿主 mod.rs 的 glob 面：兄弟文件经 `super::` 取这些类型（AppSettings/
// ProxySettings 等定义在 schema.rs/models.rs，经这里的中转解析）。
use agent_env::*;
use agent_scripts::*;
use builtin::*;
use config_bundles::*;
#[allow(
    unused_imports,
    reason = "glob 转手：兄弟文件经 super:: 取 custom_agents 项"
)]
use custom_agents::*;
#[allow(
    unused_imports,
    reason = "glob 转手：兄弟文件经 super:: 取 launch_spec 项"
)]
use launch_spec::*;
pub(crate) use launch_spec::{
    get_agent_launch_spec_from_settings, normalize_agent_configured_path,
};
use load_save::*;
#[allow(
    unused_imports,
    reason = "glob 转手：兄弟文件经 super:: 取 model_detect 项"
)]
use model_detect::*;
use models::*;
use normalize::*;
use schema::*;
use versions::*;

pub mod agent_env;
pub mod agent_scripts;
pub mod builtin;
pub mod config_bundles;
pub mod custom_agents;
pub mod dsh_home;
pub mod launch_spec;
pub mod load_save;
pub mod model_detect;
pub mod models;
pub mod normalize;
pub mod omp_home;
pub mod schema;
pub mod versions;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

static DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 宿主启动时注入数据根（`~/.aeroric`）；未注入时报错——core 不猜。
pub fn configure_data_dir(base: impl Into<PathBuf>) {
    *DATA_DIR.write().expect("data dir lock") = Some(base.into());
}

/// 测试注入临时数据根；返回 guard，drop 时恢复。
pub fn set_data_dir_for_tests(base: impl Into<PathBuf>) -> Option<PathBuf> {
    let mut guard = DATA_DIR.write().expect("data dir lock");
    guard.replace(base.into())
}

pub fn restore_data_dir(prev: Option<PathBuf>) {
    *DATA_DIR.write().expect("data dir lock") = prev;
}

/// 数据根。语义与宿主 `storage::aeroric_dir` 一致（G24 收口后的唯一入口）。
pub fn aeroric_dir() -> Result<PathBuf, String> {
    // 读序与宿主 `storage::aeroric_dir` 同：注入的 DATA_DIR（宿主启动时给真实家；
    // 测试经 set_data_dir_for_tests）→ `AERORIC_HOME`（门禁 runner 每轮注入的
    // scratch 根）→ 真实家。DATA_DIR 已注入时不被 env 影掉（与宿主 test SLOT
    // 同优先级）；纯空白 env 值按未设置处理。
    if let Some(dir) = DATA_DIR.read().expect("data dir lock").clone() {
        return Ok(dir);
    }
    if let Some(override_dir) = std::env::var("AERORIC_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(override_dir));
    }
    let home = home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

// ── 私有 I/O（与宿主 `storage::` 同语义：0600/0700，凭据绝不全权可读）──────

pub fn ensure_private_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn ensure_private_file_permissions(path: &std::path::Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let mut permissions = meta.permissions();
        if permissions.mode() & 0o077 != 0 {
            permissions.set_mode(0o600);
            std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn atomic_write_private(path: &std::path::Path, content: &str) -> Result<(), String> {
    atomic_write_private_bytes(path, content.as_bytes())
}

pub fn atomic_write_private_bytes(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        unique
    ));
    {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        file.write_all(content).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

use std::fs;
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ── P8-02 自宿主 `app_settings/mod.rs` 非命令部分上收（markers/helpers/
// family 判定/version/upgrade 面；Tauri 命令壳与 `#[cfg(test)]` 测试留宿主）。
// 适配仅路径：`crate::platform::*` 三处与宿主同名（core platform 登录 shell 面
// 同源上收）；其余符号逐字。

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

/// 全局带缓存入口：现读设置后取启动规格（与宿主 `commands::get_agent_launch_spec`
/// 同语义；core 内 versions/builtin 的探测消费）。
pub(crate) fn get_agent_launch_spec(agent: &str) -> AgentLaunchSpec {
    get_agent_launch_spec_from(&load_settings_internal(), agent)
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
