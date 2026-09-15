//! Built-in agent credentials, proxy toggles, and built-in model catalogs.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::*;

pub(super) fn set_agent_proxy_enabled(settings: &mut AppSettings, agent: &str, enabled: bool) {
    if enabled {
        settings.agent_proxy_enabled.insert(agent.to_string(), true);
    } else {
        settings.agent_proxy_enabled.remove(agent);
    }
}

pub(super) fn apply_builtin_agent_access_update(
    settings: &mut AppSettings,
    agent: &str,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
) -> Result<(), String> {
    if !matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh") {
        return Err(format!("Unknown built-in Agent: {agent}"));
    }
    let credentials = settings
        .builtin_agent_credentials
        .entry(agent.to_string())
        .or_default();
    if let Some(base_url) = base_url {
        credentials.base_url = base_url.trim().to_string();
    }
    if clear_api_key {
        credentials.api_key.clear();
    } else if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        credentials.api_key = api_key.trim().to_string();
    }
    if let Some(models) = models {
        credentials.models = normalize_model_list(models);
    }
    if let Some(enabled) = enable_1m_context {
        credentials.enable_1m_context = enabled;
    }
    Ok(())
}

pub(super) fn update_builtin_agent_config_internal_with_policy(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
    enforce_remote_key_boundary: bool,
) -> Result<AppSettings, String> {
    let syncs_dsh_home = agent == "dsh";
    let normalized = update_settings_locked(move |settings| {
        if enforce_remote_key_boundary {
            let current = settings
                .builtin_agent_credentials
                .get(&agent)
                .cloned()
                .unwrap_or_default();
            validate_remote_api_key_reuse(
                &current.base_url,
                &current.api_key,
                base_url.as_deref(),
                api_key.as_deref(),
                clear_api_key,
            )?;
        }
        apply_builtin_agent_access_update(
            settings,
            &agent,
            base_url,
            api_key,
            clear_api_key,
            models,
            enable_1m_context,
        )?;
        if let Some(enabled) = proxy_enabled {
            set_agent_proxy_enabled(settings, &agent, enabled);
        }
        Ok(())
    })?;
    if syncs_dsh_home {
        let home = crate::dsh_home::ensure_dsh_home_for("dsh")?;
        let api_key = normalized
            .builtin_agent_credentials
            .get("dsh")
            .map(|credentials| credentials.api_key.trim())
            .filter(|api_key| !api_key.is_empty());
        crate::dsh_home::sync_dsh_credentials(&home, api_key)?;
    }
    Ok(normalized)
}

pub(crate) fn update_builtin_agent_config_internal(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_builtin_agent_config_internal_with_policy(
        agent,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        proxy_enabled,
        false,
    )
}

/// Remote/mobile variant of [`update_builtin_agent_config_internal`].
///
/// The check lives inside the settings lock so a concurrent desktop update
/// cannot invalidate the URL/key comparison between a read and the write.
pub(crate) fn update_builtin_agent_config_remote_internal(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_builtin_agent_config_internal_with_policy(
        agent,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        proxy_enabled,
        true,
    )
}

pub(super) fn apply_dsh_reasoning_effort_update(
    settings: &mut AppSettings,
    agent: &str,
    effort: &str,
) -> Result<(), String> {
    if agent_family_in(settings, agent) != AgentFamily::Dsh {
        return Err(
            "Reasoning effort is only supported here for DeepSeek Harness agents".to_string(),
        );
    }
    // 只有内置官方 dsh 配置带 reasoning 元数据的模型目录;提供方 / 自定义提供方
    // 档案不参与推理强度传参,前端也不会展示该项。
    if agent != "dsh" {
        return Err(
            "Reasoning effort is only configurable for the built-in DeepSeek Harness agent"
                .to_string(),
        );
    }
    let effort = effort.trim().to_ascii_lowercase();
    if !matches!(effort.as_str(), "off" | "high" | "max") {
        return Err("Invalid DeepSeek Harness reasoning effort".to_string());
    }
    settings
        .dsh_reasoning_efforts
        .insert(agent.to_string(), effort);
    Ok(())
}

/// 内建 DeepSeek 官方目录。首项就是各处取的默认模型(模型选择脚本、新档案草稿),
/// 与上游 c291e7961a 的 `agent-default-model` 保持一致:`deepseek-flash`
/// (DeepSeek-V41-Flash)排在 v4 系列之前。
pub(crate) fn list_builtin_dsh_models() -> Vec<String> {
    vec![
        "deepseek-flash".to_string(),
        "deepseek-v4-flash".to_string(),
        "deepseek-v4-pro".to_string(),
    ]
}

/// 内建 omp 的托管 home(`PI_CODING_AGENT_DIR` 目标),与用户自己的 `~/.omp` 隔离。
pub(crate) fn omp_managed_home() -> Option<PathBuf> {
    crate::platform::home_dir().map(|home| home.join(".aeroric").join("agent-homes").join("omp"))
}

pub(crate) fn default_builtin_agent_config_path(agent: &str) -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    match agent {
        "claude" => Ok(home.join(".claude").join("settings.json")),
        "claude_gpt55" => Ok(home.join(".claude").join("start-gpt55.sh")),
        "codex" => Ok(home.join(".codex").join("config.toml")),
        "dsh" => crate::dsh_home::dsh_settings_path(),
        "omp" => omp_managed_home()
            .map(|dir| dir.join("config.yml"))
            .ok_or_else(|| "Cannot find home directory".to_string()),
        _ => Err(format!("Unknown built-in agent: {agent}")),
    }
}

/// omp rpc-ui 模型探测的总预算:进程冷启动(Bun + native addon 解压)最慢的一档。
pub(super) const OMP_MODEL_DISCOVERY_BUDGET: Duration = Duration::from_secs(20);

/// 起一个 `omp --mode rpc-ui --no-session` 短命进程,经 `get_available_models`
/// 拉取模型目录并归并为 `provider/model-id`。失败(omp 未装/无凭据)返回 Err,
/// 由调用方呈现;探测进程无论成败都会被杀掉。
pub(super) fn list_builtin_omp_models() -> Result<Vec<String>, String> {
    use std::io::Write as IoWrite;
    use std::sync::mpsc;

    let launch = get_agent_launch_spec("omp");
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args)
        .arg("--mode")
        .arg("rpc-ui")
        .arg("--no-session")
        .env("PATH", get_login_shell_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }
    if let Some(home) = omp_managed_home() {
        // 与 pty::setup_omp_env 对齐:上游 dirs.ts 在 activeProfile 存在时忽略
        // PI_CODING_AGENT_DIR,不清掉 profile 会让探测进程读用户自己的 ~/.omp
        // 而不是托管 home。
        cmd.env_remove("PI_PROFILE");
        cmd.env_remove("OMP_PROFILE");
        cmd.env("PI_CODING_AGENT_DIR", home);
        cmd.env("OMP_APP_NAME", "aeroric");
    }
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Failed to start omp for model discovery: {error}"))?;
    let mut stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "omp stdout is unavailable".to_string())?;

    if let Some(stdin) = stdin.as_mut() {
        let _ = writeln!(
            stdin,
            r#"{{"id":"p","type":"negotiate_protocol","protocolVersion":2}}"#
        );
        let _ = writeln!(stdin, r#"{{"id":"m","type":"get_available_models"}}"#);
        let _ = stdin.flush();
    }

    let (sender, receiver) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let started = std::time::Instant::now();
    let mut models: Vec<String> = Vec::new();
    let mut settled: Option<Result<(), String>> = None;
    while settled.is_none() {
        let elapsed = started.elapsed();
        if elapsed >= OMP_MODEL_DISCOVERY_BUDGET {
            settled = Some(Err("omp model discovery timed out".to_string()));
            break;
        }
        let Ok(line) = receiver.recv_timeout(OMP_MODEL_DISCOVERY_BUDGET - elapsed) else {
            settled = Some(Err("omp exited before listing models".to_string()));
            break;
        };
        let Ok(frame) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if frame.get("type").and_then(|v| v.as_str()) != Some("response")
            || frame.get("command").and_then(|v| v.as_str()) != Some("get_available_models")
        {
            continue;
        }
        if frame.get("success").and_then(|v| v.as_bool()) != Some(true) {
            settled = Some(Err(format!(
                "omp model discovery failed: {}",
                frame
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
            )));
            break;
        }
        if let Some(items) = frame
            .get("data")
            .and_then(|data| data.get("models"))
            .and_then(|models| models.as_array())
        {
            for item in items {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let provider = item.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || provider.is_empty() {
                    continue;
                }
                let model = format!("{provider}/{id}");
                if !models.contains(&model) {
                    models.push(model);
                }
            }
        }
        settled = Some(Ok(()));
    }

    let _ = child.kill();
    let _ = child.wait();
    settled.unwrap_or_else(|| Err("omp model discovery did not settle".to_string()))?;
    Ok(models)
}
