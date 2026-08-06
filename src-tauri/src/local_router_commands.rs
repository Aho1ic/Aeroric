use crate::app_settings::{self, AppSettings, LocalRouterAgentSettings, LocalRouterSettings};
use crate::local_router::{
    LocalRouterState, RouterAgent, RouterAgentPolicy, RouterAgentRuntime, RouterOutboundProxy,
    RouterRequestRecord, RouterRuntimeConfig, RouterTargetStatus, RouterUpstreams, UpstreamTarget,
    DEFAULT_CLAUDE_UPSTREAM, DEFAULT_CODEX_CHATGPT_UPSTREAM, DEFAULT_CODEX_UPSTREAM,
};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

pub struct LocalRouterManager {
    router: Arc<LocalRouterState>,
    starting: Arc<AtomicBool>,
    operation_lock: Mutex<()>,
    lifecycle_error: Arc<RwLock<Option<String>>>,
}

impl LocalRouterManager {
    pub fn for_app() -> Result<Self, String> {
        Ok(Self {
            router: Arc::new(LocalRouterState::for_app().map_err(|error| error.to_string())?),
            starting: Arc::new(AtomicBool::new(false)),
            operation_lock: Mutex::new(()),
            lifecycle_error: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn shutdown(&self) {
        let _operation = self.operation_lock.lock().await;
        let _ = self.router.stop().await;
    }
}

struct StartingGuard(Arc<AtomicBool>);

impl StartingGuard {
    fn new(starting: Arc<AtomicBool>) -> Self {
        starting.store(true, Ordering::Release);
        Self(starting)
    }
}

impl Drop for StartingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalRouterUiStatus {
    desired_enabled: bool,
    running: bool,
    starting: bool,
    listen_url: Option<String>,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    active_requests: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    last_error: Option<String>,
    targets: Vec<RouterTargetStatus>,
}

fn configured_codex_config_path(settings: &AppSettings) -> Option<PathBuf> {
    if settings.codex_config_path.trim().is_empty() {
        app_settings::default_builtin_agent_config_path("codex").ok()
    } else {
        Some(PathBuf::from(settings.codex_config_path.trim()))
    }
}

fn codex_uses_api_key(settings: &AppSettings) -> bool {
    let has_explicit_api_key = settings
        .builtin_agent_credentials
        .get("codex")
        .is_some_and(|credentials| !credentials.api_key.trim().is_empty());
    if has_explicit_api_key {
        return true;
    }

    let auth = configured_codex_config_path(settings)
        .and_then(|path| path.parent().map(|parent| parent.join("auth.json")))
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str::<Value>(&content).ok());
    if auth
        .as_ref()
        .and_then(|value| value.get("auth_mode"))
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("chatgpt"))
    {
        return false;
    }
    let auth_has_api_key = auth.as_ref().is_some_and(|auth| {
        ["OPENAI_API_KEY", "CODEX_API_KEY"].into_iter().any(|key| {
            auth.get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
    });
    auth_has_api_key
        || ["OPENAI_API_KEY", "CODEX_API_KEY"]
            .into_iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn default_codex_upstream(settings: &AppSettings) -> &'static str {
    if codex_uses_api_key(settings) {
        DEFAULT_CODEX_UPSTREAM
    } else {
        DEFAULT_CODEX_CHATGPT_UPSTREAM
    }
}

fn target_label(settings: &AppSettings, target_id: &str, fallback: &str) -> String {
    settings
        .agent_label_overrides
        .get(target_id)
        .map(|label| label.trim())
        .filter(|label| !label.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn built_in_target(settings: &AppSettings, agent: RouterAgent) -> Result<UpstreamTarget, String> {
    let id = agent.as_str();
    let credentials = settings.builtin_agent_credentials.get(id);
    let default_url = match agent {
        RouterAgent::Claude => DEFAULT_CLAUDE_UPSTREAM,
        RouterAgent::Codex => default_codex_upstream(settings),
    };
    let base_url = credentials
        .map(|credentials| credentials.base_url.trim())
        .filter(|url| !url.is_empty())
        .unwrap_or(default_url);
    let fallback_label = match agent {
        RouterAgent::Claude => "Claude Code",
        RouterAgent::Codex => "Codex",
    };
    UpstreamTarget::with_details(
        id,
        target_label(settings, id, fallback_label),
        base_url,
        credentials
            .map(|credentials| credentials.api_key.as_str())
            .unwrap_or_default(),
        credentials
            .map(|credentials| credentials.models.clone())
            .unwrap_or_default(),
        credentials.is_some_and(|credentials| credentials.enable_1m_context),
        false,
    )
    .map_err(|error| error.to_string())
}

fn targets_for_agent(
    settings: &AppSettings,
    agent: RouterAgent,
    enabled: bool,
) -> Result<Vec<UpstreamTarget>, String> {
    if !enabled {
        return Ok(Vec::new());
    }

    let mut targets = vec![built_in_target(settings, agent)?];
    for profile in &settings.custom_agents {
        let belongs_to_agent = match agent {
            RouterAgent::Claude => !profile.codex_like,
            RouterAgent::Codex => profile.codex_like,
        };
        if !belongs_to_agent || profile.base_url.trim().is_empty() {
            continue;
        }
        targets.push(
            UpstreamTarget::with_details(
                profile.id.clone(),
                target_label(settings, &profile.id, &profile.label),
                profile.base_url.trim(),
                profile.api_key.as_str(),
                profile.models.clone(),
                profile.enable_1m_context,
                agent == RouterAgent::Codex && profile.enable_chat_completions_proxy,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    Ok(targets)
}

fn policy_from_settings(
    settings: &LocalRouterAgentSettings,
    targets: &[UpstreamTarget],
    built_in_id: &str,
) -> RouterAgentPolicy {
    let target_ids = targets
        .iter()
        .map(|target| target.id())
        .collect::<HashSet<_>>();
    let active_target = if target_ids.contains(settings.active_target.as_str()) {
        settings.active_target.clone()
    } else if target_ids.contains(built_in_id) {
        built_in_id.to_string()
    } else {
        targets
            .first()
            .map(|target| target.id().to_string())
            .unwrap_or_default()
    };
    let mut seen = HashSet::new();
    let failover_queue = settings
        .failover_queue
        .iter()
        .filter(|target_id| {
            target_ids.contains(target_id.as_str()) && seen.insert(target_id.as_str())
        })
        .cloned()
        .collect();

    RouterAgentPolicy {
        auto_failover_enabled: settings.auto_failover_enabled,
        max_retries: settings.max_retries,
        streaming_first_byte_timeout: settings.streaming_first_byte_timeout,
        streaming_idle_timeout: settings.streaming_idle_timeout,
        non_streaming_timeout: settings.non_streaming_timeout,
        circuit_failure_threshold: settings.circuit_failure_threshold,
        circuit_success_threshold: settings.circuit_success_threshold,
        circuit_timeout_seconds: settings.circuit_timeout_seconds,
        circuit_error_rate_percent: settings.circuit_error_rate_percent,
        circuit_min_requests: settings.circuit_min_requests,
        active_target,
        failover_queue,
        model_mapping_enabled: settings.model_mapping_enabled,
        rectifier_enabled: settings.rectifier_enabled,
        thinking_optimizer_enabled: settings.thinking_optimizer_enabled,
        cache_injection_enabled: settings.cache_injection_enabled,
    }
}

fn runtime_for_agent(
    settings: &AppSettings,
    agent: RouterAgent,
    enabled: bool,
    agent_settings: &LocalRouterAgentSettings,
) -> Result<RouterAgentRuntime, String> {
    let targets = targets_for_agent(settings, agent, enabled)?;
    let policy = policy_from_settings(agent_settings, &targets, agent.as_str());
    Ok(RouterAgentRuntime { targets, policy })
}

fn runtime_config(settings: &AppSettings) -> Result<RouterRuntimeConfig, String> {
    let router = &settings.local_router_settings;
    let upstreams = RouterUpstreams {
        claude: runtime_for_agent(
            settings,
            RouterAgent::Claude,
            router.claude_enabled,
            &router.claude,
        )?,
        codex: runtime_for_agent(
            settings,
            RouterAgent::Codex,
            router.codex_enabled,
            &router.codex,
        )?,
    };
    let outbound_proxy =
        if router.use_global_proxy && !settings.proxy_settings.url.trim().is_empty() {
            Some(
                RouterOutboundProxy::new(
                    settings.proxy_settings.url.clone(),
                    settings.proxy_settings.no_proxy.clone(),
                    settings.proxy_settings.username.clone(),
                    settings.proxy_settings.password.clone(),
                )
                .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
    let config = RouterRuntimeConfig::new(
        router.listen_host.clone(),
        router.listen_port,
        router.record_usage,
        upstreams,
    )
    .with_outbound_proxy(outbound_proxy)
    .map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

async fn restore_runtime(
    manager: &LocalRouterManager,
    settings: &AppSettings,
) -> Result<(), String> {
    if settings.local_router_settings.enabled {
        manager
            .router
            .restart(runtime_config(settings)?)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        manager
            .router
            .stop()
            .await
            .map_err(|error| error.to_string())
    }
}

async fn apply_settings(
    manager: &LocalRouterManager,
    current: AppSettings,
    candidate: AppSettings,
) -> Result<AppSettings, String> {
    if candidate.local_router_settings.enabled {
        let candidate_runtime = runtime_config(&candidate)?;
        let _starting = StartingGuard::new(manager.starting.clone());
        manager
            .router
            .restart(candidate_runtime)
            .await
            .map_err(|error| error.to_string())?;
    } else if current.local_router_settings.enabled {
        manager
            .router
            .stop()
            .await
            .map_err(|error| error.to_string())?;
    }

    if let Err(save_error) = app_settings::save_app_settings(candidate) {
        let rollback_error = restore_runtime(manager, &current).await.err();
        let message = match rollback_error {
            Some(error) => format!("{save_error}; failed to restore the previous router: {error}"),
            None => save_error,
        };
        *manager.lifecycle_error.write() = Some(message.clone());
        return Err(message);
    }

    *manager.lifecycle_error.write() = None;
    Ok(app_settings::load_settings_internal())
}

pub fn init(app: &AppHandle) {
    let settings = app_settings::load_settings_internal();
    if !settings.local_router_settings.enabled {
        return;
    }
    let manager = app.state::<LocalRouterManager>();
    let router = manager.router.clone();
    let starting = manager.starting.clone();
    let lifecycle_error = manager.lifecycle_error.clone();
    tauri::async_runtime::spawn(async move {
        let _starting = StartingGuard::new(starting);
        let result = match runtime_config(&settings) {
            Ok(config) => router.start(config).await.map(|_| ()),
            Err(error) => {
                *lifecycle_error.write() = Some(error);
                return;
            }
        };
        match result {
            Ok(()) => *lifecycle_error.write() = None,
            Err(error) => *lifecycle_error.write() = Some(error.to_string()),
        }
    });
}

async fn status_for(manager: &LocalRouterManager) -> LocalRouterUiStatus {
    let settings = app_settings::load_settings_internal();
    let runtime = manager.router.status().await;
    let usage = manager.router.usage_summary().await.unwrap_or_default();
    let configured_runtime = runtime_config(&settings);
    let targets = match configured_runtime.as_ref() {
        Ok(config) => manager.router.target_statuses(config).await,
        Err(_) => Vec::new(),
    };
    let listen_url = runtime
        .address
        .as_deref()
        .zip(runtime.port)
        .map(|(host, port)| {
            let host = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.to_string()
            };
            format!("http://{host}:{port}")
        });
    let runtime_error = runtime.last_error.map(|error| error.message);
    let configuration_error = configured_runtime.err();
    LocalRouterUiStatus {
        desired_enabled: settings.local_router_settings.enabled,
        running: runtime.running,
        starting: manager.starting.load(Ordering::Acquire),
        listen_url,
        total_requests: usage.total_requests,
        successful_requests: usage.successful_requests,
        failed_requests: usage.failed_requests,
        active_requests: runtime.active_requests,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        last_error: manager
            .lifecycle_error
            .read()
            .clone()
            .or(configuration_error)
            .or(runtime_error),
        targets,
    }
}

fn parse_router_agent(value: &str) -> Result<RouterAgent, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" => Ok(RouterAgent::Claude),
        "codex" => Ok(RouterAgent::Codex),
        _ => Err("invalid local router agent".to_string()),
    }
}

#[tauri::command]
pub async fn get_local_router_status(
    manager: State<'_, LocalRouterManager>,
) -> Result<LocalRouterUiStatus, String> {
    Ok(status_for(&manager).await)
}

#[tauri::command]
pub async fn set_local_router_enabled(
    enabled: bool,
    manager: State<'_, LocalRouterManager>,
) -> Result<LocalRouterUiStatus, String> {
    let _operation = manager.operation_lock.lock().await;
    let current = app_settings::load_settings_internal();
    if current.local_router_settings.enabled != enabled {
        let mut candidate = current.clone();
        candidate.local_router_settings.enabled = enabled;
        if let Err(error) = apply_settings(&manager, current, candidate).await {
            *manager.lifecycle_error.write() = Some(error.clone());
            return Err(error);
        }
    }
    Ok(status_for(&manager).await)
}

#[tauri::command]
pub async fn update_local_router_settings(
    settings: LocalRouterSettings,
    manager: State<'_, LocalRouterManager>,
) -> Result<AppSettings, String> {
    let _operation = manager.operation_lock.lock().await;
    let current = app_settings::load_settings_internal();
    let mut candidate = current.clone();
    candidate.local_router_settings = settings;
    match apply_settings(&manager, current, candidate).await {
        Ok(settings) => Ok(settings),
        Err(error) => {
            *manager.lifecycle_error.write() = Some(error.clone());
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn switch_local_router_target(
    agent: String,
    target_id: String,
    manager: State<'_, LocalRouterManager>,
) -> Result<LocalRouterUiStatus, String> {
    let _operation = manager.operation_lock.lock().await;
    let agent = parse_router_agent(&agent)?;
    let target_id = target_id.trim();
    let current = app_settings::load_settings_internal();
    let current_runtime = runtime_config(&current)?;
    if current_runtime
        .upstreams
        .agent(agent)
        .target(target_id)
        .is_none()
    {
        return Err("unknown local router target".to_string());
    }

    let mut candidate = current.clone();
    match agent {
        RouterAgent::Claude => {
            candidate.local_router_settings.claude.active_target = target_id.to_string()
        }
        RouterAgent::Codex => {
            candidate.local_router_settings.codex.active_target = target_id.to_string()
        }
    }
    apply_settings(&manager, current, candidate).await?;
    Ok(status_for(&manager).await)
}

#[tauri::command]
pub async fn reset_local_router_circuit(
    agent: String,
    target_id: String,
    manager: State<'_, LocalRouterManager>,
) -> Result<LocalRouterUiStatus, String> {
    let _operation = manager.operation_lock.lock().await;
    let agent = parse_router_agent(&agent)?;
    let settings = app_settings::load_settings_internal();
    let config = runtime_config(&settings)?;
    manager
        .router
        .reset_circuit_breaker(&config, agent, target_id.trim())
        .await
        .map_err(|error| error.to_string())?;
    Ok(status_for(&manager).await)
}

#[tauri::command]
pub async fn get_local_router_requests(
    limit: Option<usize>,
    manager: State<'_, LocalRouterManager>,
) -> Result<Vec<RouterRequestRecord>, String> {
    manager
        .router
        .recent_requests(limit.unwrap_or(100).clamp(1, 500))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::CustomAgentProfile;
    use uuid::Uuid;

    #[test]
    fn runtime_config_uses_builtin_and_custom_targets() {
        let mut settings = AppSettings::default();
        settings
            .builtin_agent_credentials
            .entry("codex".to_string())
            .or_default()
            .base_url = "https://example.test/openai/v1".to_string();
        settings.custom_agents.push(CustomAgentProfile {
            id: "chat-provider".to_string(),
            label: "Chat Provider".to_string(),
            path: "/tmp/chat-provider".to_string(),
            codex_like: true,
            config_lang: "toml".to_string(),
            base_url: "https://chat.example/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["gpt-test".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            username: String::new(),
            password: String::new(),
        });

        let config = runtime_config(&settings).unwrap();
        assert_eq!(config.upstreams.codex.targets.len(), 2);
        assert_eq!(
            config.upstreams.codex.targets[0].base_url().as_str(),
            "https://example.test/openai/v1"
        );
        assert!(config.upstreams.codex.targets[1].enable_chat_completions_proxy());
    }

    #[test]
    fn failover_policy_only_keeps_valid_queue_targets() {
        let mut settings = AppSettings::default();
        settings.local_router_settings.codex.auto_failover_enabled = true;
        settings.local_router_settings.codex.failover_queue = vec![
            "missing".to_string(),
            "codex".to_string(),
            "codex".to_string(),
        ];
        let config = runtime_config(&settings).unwrap();
        assert_eq!(
            config.upstreams.codex.policy.failover_queue,
            vec!["codex".to_string()]
        );
    }

    #[test]
    fn codex_chatgpt_auth_uses_the_subscription_upstream() {
        let root = std::env::temp_dir().join(format!("aeroric-router-auth-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"secret"}}"#,
        )
        .unwrap();
        let settings = AppSettings {
            codex_config_path: root.join("config.toml").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };

        assert_eq!(
            default_codex_upstream(&settings),
            DEFAULT_CODEX_CHATGPT_UPSTREAM
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_api_key_uses_the_openai_api_upstream() {
        let root = std::env::temp_dir().join(format!("aeroric-router-auth-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"secret"}}"#,
        )
        .unwrap();
        let mut settings = AppSettings {
            codex_config_path: root.join("config.toml").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        settings
            .builtin_agent_credentials
            .entry("codex".to_string())
            .or_default()
            .api_key = "sk-test".to_string();
        assert_eq!(default_codex_upstream(&settings), DEFAULT_CODEX_UPSTREAM);
        let _ = fs::remove_dir_all(root);
    }
}
