//! 设置值的归一化与校验。
//!
//! 从 `app_settings.rs` 整块搬出来,内容一行没改。这里全是**纯函数**:
//! 把用户填进来的字符串/表格洗成规范形态(去空白、补默认、拒非法值),
//! 不读盘、不发请求、不碰全局缓存。`load_settings_unlocked` 和各 `update_*`
//! 命令都在落盘前先过这里。
//!
//! `sanitize_custom_agent_id` 是安全相关的一处:自定义 Agent 的 id 会拼进
//! `~/.aeroric/agent-homes/{id}` 路径,必须拒绝路径穿越。

use super::*;

pub(super) fn default_claude_gpt55_path() -> String {
    crate::platform::home_dir()
        .map(|home| home.join(".claude").join("start-gpt55.sh"))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "~/.claude/start-gpt55.sh".to_string())
}

pub(super) fn sanitize_custom_agent_id(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    let trimmed = out
        .trim_matches(|c| matches!(c, '.' | '_' | '-'))
        .to_string();
    match trimmed.as_str() {
        "" => String::new(),
        "claude" | "claude_gpt55" | "codex" => format!("local_{}", trimmed),
        _ => trimmed,
    }
}

/// 自定义 Agent 的隔离 home:`~/.aeroric/agent-homes/{id}`。
///
/// 与生成的启动脚本里的 `AGENT_HOME="${AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}"`
/// 必须保持一致——codex-like 脚本会把它 export 成 CODEX_HOME,MCP profile 要写进同一目录
/// 才能被 `-p` 读到。id 先过 `sanitize_custom_agent_id`,拒绝路径穿越。
pub fn custom_agent_home(id: &str) -> Result<PathBuf, String> {
    let normalized = sanitize_custom_agent_id(id);
    if normalized.is_empty() {
        return Err(format!("Invalid custom Agent id: {id}"));
    }
    let root = aeroric_dir()?.join("agent-homes");
    let home = root.join(&normalized);
    // 双重保险:normalized 已排除分隔符,这里再确认没有逃出隔离目录。
    if home.parent() != Some(root.as_path()) {
        return Err(
            "Refusing to resolve an Agent home outside the isolation directory".to_string(),
        );
    }
    Ok(home)
}

pub(super) fn normalize_config_lang(value: String) -> String {
    match value.as_str() {
        "json" | "toml" | "yaml" | "shellscript" => value,
        _ => default_custom_agent_config_lang(),
    }
}

pub(super) fn normalize_custom_agent_profile(
    profile: CustomAgentProfile,
) -> Option<CustomAgentProfile> {
    let id = sanitize_custom_agent_id(&profile.id);
    let label = profile.label.trim().to_string();
    let path = profile.path.trim().to_string();
    if id.is_empty() || label.is_empty() || path.is_empty() {
        return None;
    }
    let family = AgentFamily::parse(profile.family.trim())
        .unwrap_or_else(|| AgentFamily::from_codex_like(profile.codex_like));
    Some(CustomAgentProfile {
        id,
        label,
        path: normalize_agent_configured_path(&profile.id, &path),
        codex_like: profile.codex_like,
        family: AgentFamily::parse(profile.family.trim())
            .map(|family| family.as_str().to_string())
            .unwrap_or_default(),
        config_lang: normalize_config_lang(profile.config_lang),
        base_url: normalize_base_url(&profile.base_url),
        api_key: profile.api_key.trim().to_string(),
        models: normalize_model_list(profile.models),
        bridge_python_path: profile.bridge_python_path.trim().to_string(),
        enable_1m_context: family == AgentFamily::Claude && profile.enable_1m_context,
        enable_chat_completions_proxy: family == AgentFamily::Codex
            && profile.enable_chat_completions_proxy,
        username: String::new(),
        password: String::new(),
    })
}

pub(super) fn normalize_custom_agents(
    profiles: Vec<CustomAgentProfile>,
) -> Vec<CustomAgentProfile> {
    let mut normalized = Vec::new();
    for profile in profiles {
        let Some(profile) = normalize_custom_agent_profile(profile) else {
            continue;
        };
        if normalized
            .iter()
            .any(|existing: &CustomAgentProfile| existing.id == profile.id)
        {
            continue;
        }
        normalized.push(profile);
    }
    normalized
}

pub(super) fn normalize_config_path(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let home_relative = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"));
    if let Some(stripped) = home_relative {
        if let Some(home) = crate::platform::home_dir() {
            return home.join(stripped).to_string_lossy().into_owned();
        }
    }
    #[cfg(windows)]
    {
        expand_windows_env_vars(trimmed)
    }
    #[cfg(not(windows))]
    trimmed.to_string()
}

#[cfg(windows)]
pub(super) fn expand_windows_env_vars(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let name = &after_start[..end];
        if name.is_empty() {
            output.push_str("%%");
        } else if let Some(expanded) = std::env::var_os(name) {
            output.push_str(&expanded.to_string_lossy());
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        remaining = &after_start[end + 1..];
    }
    output.push_str(remaining);
    output
}

pub(super) fn normalize_agent_label_key(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches(|c| matches!(c, '.' | '_' | '-'))
        .to_string()
}

pub(super) fn normalize_agent_label_overrides(
    overrides: HashMap<String, String>,
) -> HashMap<String, String> {
    overrides
        .into_iter()
        .filter_map(|(agent, label)| {
            let key = normalize_agent_label_key(&agent);
            let label = label.trim().to_string();
            if key.is_empty() || label.is_empty() {
                None
            } else {
                Some((key, label))
            }
        })
        .collect()
}

pub(super) fn normalize_builtin_agent_credentials(
    credentials: HashMap<String, BuiltInAgentCredentials>,
) -> HashMap<String, BuiltInAgentCredentials> {
    credentials
        .into_iter()
        .filter_map(|(agent, credentials)| {
            builtin_agent_details(&agent)?;
            let normalized = BuiltInAgentCredentials {
                base_url: normalize_base_url(&credentials.base_url),
                api_key: credentials.api_key.trim().to_string(),
                models: normalize_model_list(credentials.models),
                enable_1m_context: credentials.enable_1m_context,
            };
            (!normalized.base_url.is_empty()
                || !normalized.api_key.is_empty()
                || !normalized.models.is_empty())
            .then_some((agent, normalized))
        })
        .collect()
}

pub(super) fn normalize_dsh_reasoning_efforts(
    efforts: HashMap<String, String>,
) -> HashMap<String, String> {
    efforts
        .into_iter()
        .filter_map(|(agent, effort)| {
            let agent = normalize_agent_label_key(&agent);
            let effort = effort.trim().to_ascii_lowercase();
            (!agent.is_empty() && matches!(effort.as_str(), "off" | "high" | "max"))
                .then_some((agent, effort))
        })
        .collect()
}

pub(super) fn normalize_proxy_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

pub(super) fn normalize_no_proxy(value: &str) -> String {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn normalize_proxy_settings(settings: ProxySettings) -> ProxySettings {
    ProxySettings {
        url: normalize_proxy_url(&settings.url),
        no_proxy: normalize_no_proxy(&settings.no_proxy),
        username: settings.username.trim().to_string(),
        password: settings.password.trim().to_string(),
    }
}

pub(super) fn normalize_local_router_settings(
    settings: LocalRouterSettings,
) -> LocalRouterSettings {
    let listen_host = match settings.listen_host.trim() {
        value if value.eq_ignore_ascii_case("localhost") => "127.0.0.1".to_string(),
        value if value.starts_with('[') && value.ends_with(']') => value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .map(|address| address.to_string())
            .unwrap_or_else(|_| DEFAULT_LOCAL_ROUTER_HOST.to_string()),
        value => value
            .parse::<IpAddr>()
            .map(|address| address.to_string())
            .unwrap_or_else(|_| DEFAULT_LOCAL_ROUTER_HOST.to_string()),
    };
    LocalRouterSettings {
        listen_host,
        listen_port: if settings.listen_port >= 1024 {
            settings.listen_port
        } else {
            DEFAULT_LOCAL_ROUTER_PORT
        },
        access_token: if settings.access_token.trim().is_empty() {
            format!("aeroric-{}", uuid::Uuid::new_v4().simple())
        } else {
            settings.access_token.trim().to_string()
        },
        claude: normalize_local_router_agent_settings(settings.claude),
        codex: normalize_local_router_agent_settings(settings.codex),
        ..settings
    }
}

pub(crate) fn normalize_local_router_settings_for_update(
    settings: LocalRouterSettings,
) -> LocalRouterSettings {
    normalize_local_router_settings(settings)
}

/// 随手记 embedding 配置的归一化。
///
/// 空值补回默认而不是原样留空:空 base URL 会让 `embed::endpoint_for` 回一条
/// `Config` 错误,而用户在设置页看到的是一个自己刚清空的输入框 —— 报错和现象对不上。
/// 补回默认既救得回来,也顺手告诉用户「这两个字段不能空」。
///
/// 这里**不**校验 URL 形状:那是 `embed::endpoint_for` 的活(它还要处理重复 `/v1`
/// 与末尾斜杠),两处各写一遍校验只会互相跑偏。
pub(crate) fn normalize_notebook_embedding_settings(
    settings: NotebookEmbeddingSettings,
) -> NotebookEmbeddingSettings {
    let base_url = settings.base_url.trim();
    let model = settings.model.trim();
    NotebookEmbeddingSettings {
        provider: settings.provider,
        base_url: if base_url.is_empty() {
            default_notebook_embedding_base_url()
        } else {
            base_url.to_string()
        },
        model: if model.is_empty() {
            default_notebook_embedding_model()
        } else {
            model.to_string()
        },
    }
}

pub(super) fn normalize_local_router_agent_settings(
    settings: LocalRouterAgentSettings,
) -> LocalRouterAgentSettings {
    let mut seen = HashSet::new();
    let failover_queue = settings
        .failover_queue
        .into_iter()
        .map(|target| target.trim().to_string())
        .filter(|target| !target.is_empty() && seen.insert(target.clone()))
        .collect();
    LocalRouterAgentSettings {
        max_retries: settings.max_retries.min(10),
        streaming_first_byte_timeout: settings.streaming_first_byte_timeout.clamp(1, 120),
        streaming_idle_timeout: if settings.streaming_idle_timeout == 0 {
            0
        } else {
            settings.streaming_idle_timeout.clamp(60, 600)
        },
        non_streaming_timeout: settings.non_streaming_timeout.clamp(60, 1200),
        circuit_failure_threshold: settings.circuit_failure_threshold.clamp(1, 20),
        circuit_success_threshold: settings.circuit_success_threshold.clamp(1, 10),
        circuit_timeout_seconds: settings.circuit_timeout_seconds.min(300),
        circuit_error_rate_percent: settings.circuit_error_rate_percent.min(100),
        circuit_min_requests: settings.circuit_min_requests.clamp(5, 100),
        active_target: settings.active_target.trim().to_string(),
        failover_queue,
        ..settings
    }
}

pub(super) fn normalize_agent_proxy_enabled(
    overrides: HashMap<String, bool>,
) -> HashMap<String, bool> {
    overrides
        .into_iter()
        .filter_map(|(agent, enabled)| {
            let key = normalize_agent_label_key(&agent);
            (!key.is_empty() && enabled).then_some((key, true))
        })
        .collect()
}
