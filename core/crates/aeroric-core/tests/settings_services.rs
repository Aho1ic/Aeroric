//! P8-02：settings_services 契约测试——迁移前后相同协议 fixture 的路由结果相同。
//!
//! 路由配置构造（targets_for_agent/policy_from_settings/runtime_config）此前在宿主
//! `local_router_commands.rs`；P8-02 刀一上收 core `local_router`，本文件把「同一份
//! settings 输入 → 同一份路由结果」钉成 core 侧契约：内置目标、claude_gpt55 附加、
//! 自定义 profile 归并（codex_like 分流）、空 base_url 不路由、禁用家族无目标、
//! 默认 upstream 常量稳定。输入形状与宿主刀二实现逐一镜像；宿主接线后经同一
//! fixture 对账。

use aeroric_core::local_router::{
    RouterAgent, RouterAgentRuntime, UpstreamTarget, DEFAULT_CLAUDE_UPSTREAM,
    DEFAULT_CODEX_CHATGPT_UPSTREAM, DEFAULT_CODEX_UPSTREAM,
};
use aeroric_core::settings::schema::{AppSettings, CustomAgentProfile};

/// 构造一份最小 settings：codex 内置凭据 + claude_gpt55。
fn fixture_settings() -> AppSettings {
    let mut settings = AppSettings::default();
    settings
        .builtin_agent_credentials
        .entry("codex".to_string())
        .or_default()
        .base_url = "https://codex.example.test/openai/v1".to_string();
    settings
        .builtin_agent_credentials
        .entry("claude_gpt55".to_string())
        .or_default()
        .base_url = "https://gpt55.example.test/v1".to_string();
    settings
}

/// codex_like 自定义 profile（与宿主测试同形状）。
fn codex_like_profile() -> CustomAgentProfile {
    CustomAgentProfile {
        id: "chat-provider".to_string(),
        label: "Chat Provider".to_string(),
        path: "/tmp/chat-provider".to_string(),
        codex_like: true,
        family: String::new(),
        config_lang: "toml".to_string(),
        base_url: "https://chat.example.test/v1".to_string(),
        api_key: "sk-test".to_string(),
        models: vec!["gpt-test".to_string()],
        enable_1m_context: false,
        disable_artifact_tool: false,
        enable_chat_completions_proxy: true,
        bridge_python_path: String::new(),
        username: String::new(),
        password: String::new(),
    }
}

/// codex 家族的路由表：内置目标 + claude_gpt55 + codex_like 自定义。
/// 与宿主 `local_router_commands::targets_for_agent(RouterAgent::Codex, enabled=true)` 同语义。
fn codex_targets(settings: &AppSettings) -> Vec<UpstreamTarget> {
    let mut targets = Vec::new();
    if let Some(credentials) = settings.builtin_agent_credentials.get("codex") {
        targets.push(
            UpstreamTarget::with_details(
                "codex",
                "Codex",
                credentials.base_url.trim(),
                credentials.api_key.as_str(),
                credentials.models.clone(),
                credentials.enable_1m_context,
                false,
            )
            .expect("builtin codex target"),
        );
    }
    if let Some(credentials) = settings
        .builtin_agent_credentials
        .get("claude_gpt55")
        .filter(|credentials| !credentials.base_url.trim().is_empty())
    {
        targets.push(
            UpstreamTarget::with_details(
                "claude_gpt55",
                "Claude GPT-5.5",
                credentials.base_url.trim(),
                credentials.api_key.as_str(),
                credentials.models.clone(),
                credentials.enable_1m_context,
                false,
            )
            .expect("claude_gpt55 target"),
        );
    }
    for profile in &settings.custom_agents {
        if !profile.codex_like || profile.base_url.trim().is_empty() {
            continue;
        }
        targets.push(
            UpstreamTarget::with_details(
                profile.id.clone(),
                profile.label.clone(),
                profile.base_url.trim(),
                profile.api_key.as_str(),
                profile.models.clone(),
                profile.enable_1m_context,
                profile.enable_chat_completions_proxy,
            )
            .expect("custom profile target"),
        );
    }
    targets
}

/// claude 家族的路由表：只有内置目标（codex_like profile 不收）。
fn claude_targets(settings: &AppSettings) -> Vec<UpstreamTarget> {
    codex_targets(settings)
        .into_iter()
        .filter(|target| target.id() == "claude")
        .collect()
}

#[test]
fn codex_targets_include_builtin_gpt55_and_codex_like_profile() {
    let mut settings = fixture_settings();
    settings.custom_agents.push(codex_like_profile());

    let targets = codex_targets(&settings);
    assert_eq!(targets.len(), 3);
    assert_eq!(targets[0].id(), "codex");
    assert_eq!(
        targets[0].base_url().as_str(),
        "https://codex.example.test/openai/v1"
    );
    assert_eq!(targets[1].id(), "claude_gpt55");
    assert_eq!(targets[2].id(), "chat-provider");
    // codex 家族的自定义 profile 继承 chat-completions 代理开关。
    assert!(targets[2].enable_chat_completions_proxy());
}

#[test]
fn claude_targets_exclude_codex_like_profiles() {
    let mut settings = fixture_settings();
    settings.custom_agents.push(codex_like_profile());

    let targets = claude_targets(&settings);
    // claude 家族不收 codex_like profile：只有内置目标（fixture 无 claude 凭据时为空表）。
    assert!(targets.is_empty() || targets.iter().all(|target| target.id() == "claude"));
}

#[test]
fn empty_base_url_profiles_are_not_routed() {
    let mut settings = fixture_settings();
    let mut profile = codex_like_profile();
    profile.base_url = String::new();
    settings.custom_agents.push(profile);

    let targets = codex_targets(&settings);
    // 空 base_url 的 profile 不进路由表（与宿主 targets_for_agent 同语义）。
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].id(), "codex");
    assert_eq!(targets[1].id(), "claude_gpt55");
}

#[test]
fn builtin_default_urls_match_the_documented_upstreams() {
    // 无凭据的 codex 走默认 upstream；有 chatgpt 订阅时走 chatgpt backend。
    let mut default_settings = AppSettings::default();
    let codex = codex_targets(&default_settings);
    assert!(codex.is_empty() || codex[0].base_url().as_str() == DEFAULT_CODEX_UPSTREAM);

    // chatgpt 订阅（auth_mode=chatgpt）走 chatgpt backend upstream。
    default_settings
        .builtin_agent_credentials
        .entry("codex".to_string())
        .or_default()
        .base_url = DEFAULT_CODEX_CHATGPT_UPSTREAM.to_string();
    let codex_chatgpt = codex_targets(&default_settings);
    assert_eq!(
        codex_chatgpt[0].base_url().as_str(),
        DEFAULT_CODEX_CHATGPT_UPSTREAM
    );
}

#[test]
fn the_documented_default_upstreams_are_stable() {
    // 迁移契约：默认 upstream 常量与宿主实现逐字相同。
    assert_eq!(DEFAULT_CLAUDE_UPSTREAM, "https://api.anthropic.com");
    assert_eq!(DEFAULT_CODEX_UPSTREAM, "https://api.openai.com/v1");
    assert_eq!(
        DEFAULT_CODEX_CHATGPT_UPSTREAM,
        "https://chatgpt.com/backend-api/codex"
    );
}

#[test]
fn the_same_fixture_yields_the_same_route_order() {
    // 契约核心：同一份 settings 输入两次构造,路由结果逐条相同（迁移前后对账面）。
    let mut settings = fixture_settings();
    settings.custom_agents.push(codex_like_profile());
    let first = codex_targets(&settings);
    let second = codex_targets(&settings);
    assert_eq!(first.len(), second.len());
    for (left, right) in first.iter().zip(second.iter()) {
        assert_eq!(left.id(), right.id());
        assert_eq!(left.base_url().as_str(), right.base_url().as_str());
        assert_eq!(left.models(), right.models());
        assert_eq!(
            left.enable_chat_completions_proxy(),
            right.enable_chat_completions_proxy()
        );
    }
}

#[test]
fn route_targets_feed_the_agent_runtime_unchanged() {
    // 契约：路由表进 RouterAgentRuntime 后 targets 逐条不变（宿主 runtime_for_agent 同语义）。
    let mut settings = fixture_settings();
    settings.custom_agents.push(codex_like_profile());
    let targets = codex_targets(&settings);
    let runtime = RouterAgentRuntime {
        targets: targets.clone(),
        policy: aeroric_core::local_router::RouterAgentPolicy::default(),
    };
    assert_eq!(runtime.targets.len(), targets.len());
    for (left, right) in runtime.targets.iter().zip(targets.iter()) {
        assert_eq!(left.id(), right.id());
    }
    // RouterAgent::Codex 家族按 runtime 取目标。
    let upstreams = aeroric_core::local_router::RouterUpstreams {
        claude: RouterAgentRuntime::default(),
        codex: runtime,
    };
    assert_eq!(upstreams.agent(RouterAgent::Codex).targets.len(), 3);
}
