use crate::app_settings::{self, AppSettings, LocalRouterSettings};
use crate::local_router::{
    LocalRouterState, RouterRequestRecord, RouterRuntimeConfig, RouterUpstreams, UpstreamTarget,
    DEFAULT_CLAUDE_UPSTREAM, DEFAULT_CODEX_CHATGPT_UPSTREAM, DEFAULT_CODEX_UPSTREAM,
};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value;
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
}

fn upstream_url(settings: &AppSettings, agent: &str, default: &str) -> String {
    settings
        .builtin_agent_credentials
        .get(agent)
        .map(|credentials| credentials.base_url.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .to_string()
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

fn runtime_config(settings: &AppSettings) -> Result<RouterRuntimeConfig, String> {
    let router = &settings.local_router_settings;
    let claude = router
        .claude_enabled
        .then(|| UpstreamTarget::new(upstream_url(settings, "claude", DEFAULT_CLAUDE_UPSTREAM)))
        .transpose()
        .map_err(|error| error.to_string())?;
    let codex = router
        .codex_enabled
        .then(|| {
            UpstreamTarget::new(upstream_url(
                settings,
                "codex",
                default_codex_upstream(settings),
            ))
        })
        .transpose()
        .map_err(|error| error.to_string())?;
    let config = RouterRuntimeConfig::new(
        router.listen_host.clone(),
        router.listen_port,
        router.record_usage,
        RouterUpstreams { claude, codex },
    );
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
        last_error: manager.lifecycle_error.read().clone().or(runtime_error),
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
    use uuid::Uuid;

    #[test]
    fn runtime_config_uses_managed_upstreams_and_agent_toggles() {
        let mut settings = AppSettings::default();
        settings.local_router_settings.claude_enabled = false;
        settings
            .builtin_agent_credentials
            .entry("codex".to_string())
            .or_default()
            .base_url = "https://example.test/openai/v1".to_string();

        let config = runtime_config(&settings).unwrap();
        assert!(config.upstreams.claude.is_none());
        assert_eq!(
            config.upstreams.codex.as_ref().unwrap().base_url().as_str(),
            "https://example.test/openai/v1"
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
