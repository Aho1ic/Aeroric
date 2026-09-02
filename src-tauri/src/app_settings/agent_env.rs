//! 拼 Agent 启动时的环境变量,以及两处旧配置迁移。
//!
//! 从 `app_settings.rs` 整块搬出来,内容一行没改。`append_*_env` 这几个函数是
//! Agent 子进程能不能连上上游的关键 —— 代理、凭据、内置 Agent 的 API base、
//! 以及本地路由的注入顺序都在这里定。改动会直接影响真实请求的走向。
//!
//! `migrate_*` 两个负责把老版本的单一代理配置读成新结构,只在加载设置时用一次。

use super::*;

pub(super) fn migrate_legacy_proxy_settings(settings: &AppSettings) -> ProxySettings {
    let mut proxy = if !settings.proxy_settings.url.trim().is_empty()
        || !settings.proxy_settings.no_proxy.trim().is_empty()
        || !settings.proxy_settings.username.trim().is_empty()
        || !settings.proxy_settings.password.trim().is_empty()
    {
        normalize_proxy_settings(settings.proxy_settings.clone())
    } else {
        settings
            .agent_proxy_overrides
            .values()
            .find(|config| !config.url.trim().is_empty() || !config.no_proxy.trim().is_empty())
            .map(|config| {
                normalize_proxy_settings(ProxySettings {
                    url: config.url.clone(),
                    no_proxy: config.no_proxy.clone(),
                    username: String::new(),
                    password: String::new(),
                })
            })
            .unwrap_or_default()
    };

    if proxy.username.is_empty() && proxy.password.is_empty() {
        if let Some(profile) = settings.custom_agents.iter().find(|profile| {
            !profile.username.trim().is_empty() || !profile.password.trim().is_empty()
        }) {
            proxy.username = profile.username.trim().to_string();
            proxy.password = profile.password.trim().to_string();
        }
    }

    proxy
}

pub(super) fn migrate_agent_proxy_enabled(settings: &AppSettings) -> HashMap<String, bool> {
    let mut enabled = normalize_agent_proxy_enabled(settings.agent_proxy_enabled.clone());
    for (agent, config) in &settings.agent_proxy_overrides {
        let key = normalize_agent_label_key(agent);
        if !key.is_empty() && config.enabled {
            enabled.insert(key, true);
        }
    }
    enabled
}

pub(super) fn append_agent_proxy_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let key = normalize_agent_label_key(agent);
    if !settings
        .agent_proxy_enabled
        .get(&key)
        .copied()
        .unwrap_or(false)
    {
        return;
    }
    let proxy = normalize_proxy_settings(settings.proxy_settings.clone());
    if proxy.url.trim().is_empty() {
        return;
    }
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        extra_env.push((key.to_string(), proxy.url.clone()));
    }
    if !proxy.no_proxy.is_empty() {
        extra_env.push(("NO_PROXY".to_string(), proxy.no_proxy.clone()));
        extra_env.push(("no_proxy".to_string(), proxy.no_proxy));
    }
}

pub(super) fn append_agent_credential_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let key = normalize_agent_label_key(agent);
    if !settings
        .agent_proxy_enabled
        .get(&key)
        .copied()
        .unwrap_or(false)
    {
        return;
    }

    let proxy = normalize_proxy_settings(settings.proxy_settings.clone());
    if proxy.url.trim().is_empty() {
        return;
    }

    let username = proxy.username.trim();
    if !username.is_empty() {
        extra_env.push(("AERORIC_AGENT_USERNAME".to_string(), username.to_string()));
    }

    let password = proxy.password.trim();
    if !password.is_empty() {
        extra_env.push(("AERORIC_AGENT_PASSWORD".to_string(), password.to_string()));
    }
}

pub(super) fn append_builtin_agent_api_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let Some(credentials) = settings.builtin_agent_credentials.get(agent) else {
        return;
    };
    // dsh:身份即 API key(无 OAuth)。key 走环境变量注入——dsh 的凭据层
    // "inherited environment" 优先于 .credentials.yaml,每次启动读取最新值,
    // 换 key 无需重启。自定义 base_url 走 settings.yaml 的 provider 配置
    // (llm-pi-ai),不在这里注入。
    if configured_agent_family(settings, agent) == AgentFamily::Dsh {
        if !credentials.api_key.is_empty() {
            extra_env.push(("DEEPSEEK_API_KEY".to_string(), credentials.api_key.clone()));
        }
        return;
    }
    if !credentials.base_url.is_empty() {
        if matches!(agent, "codex" | "claude_gpt55") {
            extra_env.push(("OPENAI_BASE_URL".to_string(), credentials.base_url.clone()));
        } else {
            extra_env.push((
                "ANTHROPIC_BASE_URL".to_string(),
                credentials.base_url.clone(),
            ));
        }
    }
    if !credentials.api_key.is_empty() {
        if matches!(agent, "codex" | "claude_gpt55") {
            for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY"] {
                extra_env.push((key.to_string(), credentials.api_key.clone()));
            }
        } else {
            extra_env.push((
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                credentials.api_key.clone(),
            ));
            extra_env.push(("ANTHROPIC_API_KEY".to_string(), credentials.api_key.clone()));
        }
    }
    if !matches!(agent, "codex" | "claude_gpt55") {
        if let Some(model) = credentials.models.first() {
            let model = if credentials.enable_1m_context && !model.ends_with("[1m]") {
                format!("{model}[1m]")
            } else {
                model.clone()
            };
            for key in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            ] {
                extra_env.push((key.to_string(), model.clone()));
            }
        }
    }
}

pub(super) fn append_local_router_env(
    settings: &AppSettings,
    agent: &str,
    router_listening: bool,
    extra_env: &mut Vec<(String, String)>,
) {
    let router = &settings.local_router_settings;
    if agent == "claude_gpt55"
        && settings
            .builtin_agent_credentials
            .get(agent)
            .is_none_or(|credentials| credentials.base_url.trim().is_empty())
    {
        // Without a configured GPT-5.5 upstream the launcher script is the
        // source of truth. Pinning it to the built-in Codex target would make
        // the selected configuration look active while using Codex instead.
        return;
    }
    let codex_like = match agent {
        "claude" => false,
        "codex" | "claude_gpt55" => true,
        other => match settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
        {
            // Profiles without an upstream URL rely on their own launcher
            // configuration and cannot be pinned to a router target.
            Some(profile) if !profile.base_url.trim().is_empty() => profile.codex_like,
            None => return,
            Some(_) => return,
        },
    };
    let (enabled, base_url_key, route_prefix) = if codex_like {
        (router.codex_enabled, "OPENAI_BASE_URL", "codex/v1")
    } else {
        (router.claude_enabled, "ANTHROPIC_BASE_URL", "claude")
    };
    // `router_listening` 为假说明服务没真的在监听(开关刚打开还没起、端口被占、绑定失败、
    // 正在停服)。把 Agent 指向一个没人接的端口只会得到
    // `error sending request for url (http://127.0.0.1:18080/...)`，
    // 不如直接让它按自己的配置直连上游。
    if !router.enabled || !enabled || !router_listening {
        return;
    }

    let connect_host = match router.listen_host.as_str() {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        host => host,
    };
    let url_host = if connect_host.contains(':') {
        format!("[{connect_host}]")
    } else {
        connect_host.to_string()
    };
    let target_id = agent;
    let route_prefix = route_prefix
        .split_once('/')
        .map(|(family, suffix)| format!("{family}/targets/{target_id}/{suffix}"))
        .unwrap_or_else(|| format!("{route_prefix}/targets/{target_id}"));
    extra_env.push((
        base_url_key.to_string(),
        format!("http://{url_host}:{}/{route_prefix}", router.listen_port),
    ));

    let listen_is_loopback = router
        .listen_host
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or_else(|_| router.listen_host.eq_ignore_ascii_case("localhost"));
    if !listen_is_loopback && !router.access_token.is_empty() {
        let credential_keys: &[&str] = if codex_like {
            &["OPENAI_API_KEY", "CODEX_API_KEY"]
        } else {
            &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        };
        for key in credential_keys {
            extra_env.push(((*key).to_string(), router.access_token.clone()));
        }
    }

    let mut no_proxy = extra_env
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case("NO_PROXY"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for bypass in ["127.0.0.1", "localhost", "::1", connect_host] {
        if !no_proxy.iter().any(|item| item == bypass) {
            no_proxy.push(bypass.to_string());
        }
    }
    let no_proxy = no_proxy.join(",");
    extra_env.push(("NO_PROXY".to_string(), no_proxy.clone()));
    extra_env.push(("no_proxy".to_string(), no_proxy));
}

pub(super) fn get_agent_configured_path(settings: &AppSettings, agent: &str) -> String {
    if let Some(profile) = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
    {
        return profile.path.clone();
    }
    match agent {
        "claude_gpt55" => {
            if settings.claude_gpt55_path.is_empty() {
                default_claude_gpt55_path()
            } else {
                settings.claude_gpt55_path.clone()
            }
        }
        "codex" => settings.codex_path.clone(),
        "dsh" => settings.dsh_path.clone(),
        _ => settings.claude_path.clone(),
    }
}

pub(crate) fn configured_agent_path(settings: &AppSettings, agent: &str) -> String {
    get_agent_configured_path(settings, agent)
}
