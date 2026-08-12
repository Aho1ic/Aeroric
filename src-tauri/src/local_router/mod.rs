mod cache_injector;
mod chat_bridge;
mod circuit_breaker;
mod server;
mod session;
mod thinking_optimizer;
mod transforms;
mod usage;

use parking_lot::RwLock as ParkingRwLock;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use url::{Host, Url};

pub use circuit_breaker::{
    CircuitBreakerConfig, CircuitBreakerRegistry, CircuitBreakerStats, CircuitState,
};
pub use usage::{RouterRequestRecord, RouterUsageSummary};

pub const DEFAULT_CLAUDE_UPSTREAM: &str = "https://api.anthropic.com";
pub const DEFAULT_CODEX_UPSTREAM: &str = "https://api.openai.com/v1";
pub const DEFAULT_CODEX_CHATGPT_UPSTREAM: &str = "https://chatgpt.com/backend-api/codex";
pub const ROUTE_AGENT_HEADER: &str = "x-aeroric-route-agent";
pub const ROUTER_TOKEN_HEADER: &str = "x-aeroric-router-token";
pub const HEALTH_PATH: &str = "/_aeroric/local-router/health";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouterAgent {
    Claude,
    Codex,
}

impl RouterAgent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

impl fmt::Display for RouterAgent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A routable provider. Secrets deliberately stay private and are never serialized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpstreamTarget {
    id: String,
    name: String,
    base_url: Url,
    api_key: String,
    models: Vec<String>,
    enable_1m_context: bool,
    enable_chat_completions_proxy: bool,
}

impl UpstreamTarget {
    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        id: impl Into<String>,
        name: impl Into<String>,
        base_url: impl AsRef<str>,
        api_key: impl Into<String>,
        models: Vec<String>,
        enable_1m_context: bool,
        enable_chat_completions_proxy: bool,
    ) -> Result<Self, RouterError> {
        let id = id.into().trim().to_string();
        if id.is_empty() {
            return Err(RouterError::invalid_config(
                "local router target id cannot be empty",
            ));
        }
        let name = name.into().trim().to_string();
        let raw = base_url.as_ref().trim();
        let mut parsed = Url::parse(raw).map_err(|_| {
            RouterError::invalid_config("upstream base URL must be an absolute HTTP(S) URL")
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(RouterError::invalid_config(
                "upstream base URL must use HTTP(S) and cannot contain credentials, query, or fragment",
            ));
        }

        let normalized_path = if parsed.path() == "/" {
            String::new()
        } else {
            parsed.path().trim_end_matches('/').to_string()
        };
        parsed.set_path(&normalized_path);

        let mut seen = HashSet::new();
        let models = models
            .into_iter()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty() && seen.insert(model.to_ascii_lowercase()))
            .collect();

        Ok(Self {
            id,
            name: if name.is_empty() {
                "Unnamed".to_string()
            } else {
                name
            },
            base_url: parsed,
            api_key: api_key.into().trim().to_string(),
            models,
            enable_1m_context,
            enable_chat_completions_proxy,
        })
    }

    /// Convenience constructor that validates only the base URL, mirroring the
    /// original `new` surface used by tests that exercise URL validation.
    #[allow(dead_code)]
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, RouterError> {
        Self::with_details("target", "Target", base_url, "", Vec::new(), false, false)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn models(&self) -> &[String] {
        &self.models
    }

    pub fn enable_1m_context(&self) -> bool {
        self.enable_1m_context
    }

    pub fn enable_chat_completions_proxy(&self) -> bool {
        self.enable_chat_completions_proxy
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouterAgentPolicy {
    pub auto_failover_enabled: bool,
    pub max_retries: u8,
    pub streaming_first_byte_timeout: u64,
    pub streaming_idle_timeout: u64,
    pub non_streaming_timeout: u64,
    pub circuit_failure_threshold: u32,
    pub circuit_success_threshold: u32,
    pub circuit_timeout_seconds: u64,
    pub circuit_error_rate_percent: u8,
    pub circuit_min_requests: u32,
    pub active_target: String,
    pub failover_queue: Vec<String>,
    pub model_mapping_enabled: bool,
    pub rectifier_enabled: bool,
    pub thinking_optimizer_enabled: bool,
    pub cache_injection_enabled: bool,
}

impl Default for RouterAgentPolicy {
    fn default() -> Self {
        Self {
            auto_failover_enabled: false,
            max_retries: 3,
            streaming_first_byte_timeout: 60,
            streaming_idle_timeout: 120,
            non_streaming_timeout: 600,
            circuit_failure_threshold: 4,
            circuit_success_threshold: 2,
            circuit_timeout_seconds: 60,
            circuit_error_rate_percent: 60,
            circuit_min_requests: 10,
            active_target: String::new(),
            failover_queue: Vec::new(),
            model_mapping_enabled: true,
            rectifier_enabled: true,
            thinking_optimizer_enabled: false,
            cache_injection_enabled: false,
        }
    }
}

impl RouterAgentPolicy {
    pub fn circuit_breaker_config(&self) -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: self.circuit_failure_threshold.max(1),
            success_threshold: self.circuit_success_threshold.max(1),
            timeout_seconds: self.circuit_timeout_seconds,
            error_rate_percent: self.circuit_error_rate_percent.min(100),
            min_requests: self.circuit_min_requests.max(1),
        }
    }

    pub fn max_attempts(&self) -> usize {
        if self.auto_failover_enabled {
            usize::from(self.max_retries).saturating_add(1)
        } else {
            1
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouterAgentRuntime {
    pub targets: Vec<UpstreamTarget>,
    pub policy: RouterAgentPolicy,
}

impl RouterAgentRuntime {
    pub fn candidates(&self) -> Vec<UpstreamTarget> {
        if self.policy.auto_failover_enabled {
            let mut seen = HashSet::new();
            self.policy
                .failover_queue
                .iter()
                .filter(|id| seen.insert(id.as_str()))
                .filter_map(|id| self.targets.iter().find(|target| target.id() == id))
                .cloned()
                .collect()
        } else {
            self.targets
                .iter()
                .find(|target| target.id() == self.policy.active_target)
                .or_else(|| self.targets.first())
                .cloned()
                .into_iter()
                .collect()
        }
    }

    pub fn target(&self, target_id: &str) -> Option<&UpstreamTarget> {
        self.targets.iter().find(|target| target.id() == target_id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouterUpstreams {
    pub claude: RouterAgentRuntime,
    pub codex: RouterAgentRuntime,
}

impl RouterUpstreams {
    pub fn agent(&self, agent: RouterAgent) -> &RouterAgentRuntime {
        match agent {
            RouterAgent::Claude => &self.claude,
            RouterAgent::Codex => &self.codex,
        }
    }

    pub fn agent_mut(&mut self, agent: RouterAgent) -> &mut RouterAgentRuntime {
        match agent {
            RouterAgent::Claude => &mut self.claude,
            RouterAgent::Codex => &mut self.codex,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RouterOutboundProxy {
    url: String,
    no_proxy: String,
    username: String,
    password: String,
}

impl RouterOutboundProxy {
    pub fn new(
        url: impl Into<String>,
        no_proxy: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, RouterError> {
        let url = url.into().trim().to_string();
        if url.is_empty() {
            return Err(RouterError::invalid_config(
                "outbound proxy URL cannot be empty",
            ));
        }
        let parsed = Url::parse(&url)
            .map_err(|_| RouterError::invalid_config("outbound proxy URL is invalid"))?;
        if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h")
            || parsed.host().is_none()
        {
            return Err(RouterError::invalid_config(
                "outbound proxy must use HTTP(S), SOCKS5, or SOCKS5H",
            ));
        }
        Ok(Self {
            url,
            no_proxy: no_proxy.into().trim().to_string(),
            username: username.into().trim().to_string(),
            password: password.into(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

fn build_http_client(proxy: Option<&RouterOutboundProxy>) -> Result<reqwest::Client, RouterError> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none());

    if let Some(proxy_config) = proxy {
        let mut outbound = reqwest::Proxy::all(&proxy_config.url).map_err(|error| {
            RouterError::invalid_config(format!("invalid outbound proxy configuration: {error}"))
        })?;
        if !proxy_config.username.is_empty() {
            outbound = outbound.basic_auth(&proxy_config.username, &proxy_config.password);
        }
        if !proxy_config.no_proxy.is_empty() {
            outbound = outbound.no_proxy(reqwest::NoProxy::from_string(&proxy_config.no_proxy));
        }
        builder = builder.proxy(outbound);
    } else {
        // A local reverse proxy must not accidentally recurse through HTTP(S)_PROXY.
        builder = builder.no_proxy();
    }

    builder.build().map_err(|error| {
        RouterError::invalid_config(format!(
            "failed to initialize local router HTTP client: {error}"
        ))
    })
}

#[derive(Clone, Debug)]
pub struct RouterRuntimeConfig {
    pub listen_address: String,
    pub port: u16,
    pub record_usage: bool,
    access_token: String,
    pub upstreams: RouterUpstreams,
    pub outbound_proxy: Option<RouterOutboundProxy>,
    client: reqwest::Client,
}

impl RouterRuntimeConfig {
    pub fn new(
        listen_address: impl Into<String>,
        port: u16,
        record_usage: bool,
        upstreams: RouterUpstreams,
    ) -> Self {
        let client = build_http_client(None).expect("direct reqwest client configuration is valid");
        Self {
            listen_address: listen_address.into(),
            port,
            record_usage,
            access_token: String::new(),
            upstreams,
            outbound_proxy: None,
            client,
        }
    }

    pub fn with_access_token(mut self, access_token: impl Into<String>) -> Self {
        self.access_token = access_token.into().trim().to_string();
        self
    }

    pub fn with_outbound_proxy(
        mut self,
        outbound_proxy: Option<RouterOutboundProxy>,
    ) -> Result<Self, RouterError> {
        self.client = build_http_client(outbound_proxy.as_ref())?;
        self.outbound_proxy = outbound_proxy;
        Ok(self)
    }

    pub(crate) fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    pub(crate) fn equivalent_without_client(&self, other: &Self) -> bool {
        self.listen_address == other.listen_address
            && self.port == other.port
            && self.record_usage == other.record_usage
            && self.access_token == other.access_token
            && self.upstreams == other.upstreams
            && self.outbound_proxy == other.outbound_proxy
    }

    pub fn validate(&self) -> Result<SocketAddr, RouterError> {
        let listen_addr = validate_listen_address(&self.listen_address, self.port)?;
        if !listen_addr.ip().is_loopback() && self.access_token.len() < 32 {
            return Err(RouterError::invalid_config(
                "non-loopback local router listeners require an access token of at least 32 characters",
            ));
        }
        for (agent, runtime) in [
            (RouterAgent::Claude, &self.upstreams.claude),
            (RouterAgent::Codex, &self.upstreams.codex),
        ] {
            let mut ids = HashSet::new();
            for target in &runtime.targets {
                if !ids.insert(target.id()) {
                    return Err(RouterError::invalid_config(format!(
                        "duplicate local router target id: {}",
                        target.id()
                    )));
                }
                reject_router_loop(target, listen_addr)?;
            }
            if runtime.policy.auto_failover_enabled {
                if runtime.policy.failover_queue.is_empty() {
                    return Err(RouterError::invalid_config(format!(
                        "automatic failover for {agent} requires a non-empty failover queue"
                    )));
                }
                if let Some(target_id) = runtime
                    .policy
                    .failover_queue
                    .iter()
                    .find(|target_id| !ids.contains(target_id.as_str()))
                {
                    return Err(RouterError::invalid_config(format!(
                        "automatic failover for {agent} references unknown target: {target_id}"
                    )));
                }
            }
        }
        if let Some(proxy) = &self.outbound_proxy {
            reject_proxy_loop(proxy, listen_addr)?;
        }
        Ok(listen_addr)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RouterErrorCode {
    InvalidConfig,
    AlreadyRunning,
    BindFailed,
    StopFailed,
    StorageFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterError {
    pub code: RouterErrorCode,
    pub message: String,
}

impl RouterError {
    fn new(code: RouterErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(RouterErrorCode::InvalidConfig, message)
    }

    fn storage(message: impl Into<String>) -> Self {
        Self::new(RouterErrorCode::StorageFailed, message)
    }
}

impl fmt::Display for RouterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RouterError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouterPhase {
    Stopped,
    Running,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterErrorSummary {
    pub occurred_at: i64,
    pub agent: Option<RouterAgent>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterServerInfo {
    pub address: String,
    pub port: u16,
    pub base_url: String,
    pub claude_base_url: String,
    pub codex_base_url: String,
    pub started_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterStatus {
    pub phase: RouterPhase,
    pub running: bool,
    pub address: Option<String>,
    pub port: Option<u16>,
    pub started_at: Option<String>,
    pub uptime_seconds: u64,
    pub active_requests: u64,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub last_error: Option<RouterErrorSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RouterTargetStatus {
    pub agent: RouterAgent,
    pub target_id: String,
    pub target_name: String,
    pub base_url: String,
    pub active: bool,
    pub queue_position: Option<usize>,
    pub models: Vec<String>,
    pub enable_1m_context: bool,
    pub enable_chat_completions_proxy: bool,
    pub healthy: bool,
    pub circuit: CircuitBreakerStats,
}

#[derive(Default)]
pub(crate) struct RuntimeMetrics {
    active_requests: AtomicU64,
    total_requests: AtomicU64,
    successful_requests: AtomicU64,
    failed_requests: AtomicU64,
    last_error: ParkingRwLock<Option<RouterErrorSummary>>,
}

impl RuntimeMetrics {
    pub(crate) fn begin_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.active_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn finish_request(&self, success: bool) {
        let _ =
            self.active_requests
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(current.saturating_sub(1))
                });
        if success {
            self.successful_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn set_error(&self, agent: Option<RouterAgent>, message: impl AsRef<str>) {
        *self.last_error.write() = Some(RouterErrorSummary {
            occurred_at: chrono::Utc::now().timestamp_millis(),
            agent,
            message: usage::sanitize_summary(message.as_ref()),
        });
    }

    fn clear_error(&self) {
        *self.last_error.write() = None;
    }
}

struct RunningServer {
    config: Arc<RwLock<RouterRuntimeConfig>>,
    listener_addr: SocketAddr,
    info: RouterServerInfo,
    started: Instant,
    alive: Arc<AtomicBool>,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

pub struct LocalRouterState {
    usage_store: usage::UsageStore,
    metrics: Arc<RuntimeMetrics>,
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    lifecycle: Mutex<()>,
    running: Mutex<Option<RunningServer>>,
}

impl LocalRouterState {
    pub fn for_app() -> Result<Self, RouterError> {
        crate::storage::ensure_aeroric_dirs().map_err(RouterError::storage)?;
        let database_path = crate::storage::aeroric_dir()
            .map_err(RouterError::storage)?
            .join("usage-statistics.sqlite3");
        Self::with_database_path(database_path)
    }

    pub fn with_database_path(database_path: PathBuf) -> Result<Self, RouterError> {
        Ok(Self {
            usage_store: usage::UsageStore::new(database_path),
            metrics: Arc::new(RuntimeMetrics::default()),
            circuit_breakers: Arc::new(CircuitBreakerRegistry::default()),
            lifecycle: Mutex::new(()),
            running: Mutex::new(None),
        })
    }

    pub async fn start(
        &self,
        config: RouterRuntimeConfig,
    ) -> Result<RouterServerInfo, RouterError> {
        let _lifecycle = self.lifecycle.lock().await;
        let listen_addr = config.validate()?;
        self.usage_store.initialize().await?;

        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut() {
            if current.alive.load(Ordering::Acquire) {
                if current.listener_addr == listen_addr {
                    *current.config.write().await = config;
                    return Ok(current.info.clone());
                }
                return Err(RouterError::new(
                    RouterErrorCode::AlreadyRunning,
                    "local router is already running on a different address",
                ));
            }
        }

        if let Some(stale) = running.take() {
            drop(running);
            let _ = stale.task.await;
            running = self.running.lock().await;
        }

        let listener = tokio::net::TcpListener::bind(listen_addr)
            .await
            .map_err(|error| {
                let message = format!("failed to bind local router: {error}");
                self.metrics.set_error(None, &message);
                RouterError::new(RouterErrorCode::BindFailed, message)
            })?;
        let server = self.spawn_server(listener, config).await?;
        let info = server.info.clone();
        self.metrics.clear_error();
        *running = Some(server);
        Ok(info)
    }

    pub async fn restart(
        &self,
        config: RouterRuntimeConfig,
    ) -> Result<RouterServerInfo, RouterError> {
        let _lifecycle = self.lifecycle.lock().await;
        let listen_addr = config.validate()?;
        self.usage_store.initialize().await?;

        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut() {
            if current.alive.load(Ordering::Acquire) && current.listener_addr == listen_addr {
                *current.config.write().await = config;
                self.metrics.clear_error();
                return Ok(current.info.clone());
            }
        }

        // Bind first so a bad address or occupied port never tears down the working server.
        let listener = tokio::net::TcpListener::bind(listen_addr)
            .await
            .map_err(|error| {
                let message = format!("failed to bind replacement local router: {error}");
                self.metrics.set_error(None, &message);
                RouterError::new(RouterErrorCode::BindFailed, message)
            })?;
        let replacement = self.spawn_server(listener, config).await?;
        let info = replacement.info.clone();
        let old = running.replace(replacement);
        drop(running);

        if let Some(old) = old {
            if let Err(error) = stop_running_server(old).await {
                self.metrics.set_error(None, &error.message);
            }
        }
        self.metrics.clear_error();
        Ok(info)
    }

    pub async fn stop(&self) -> Result<(), RouterError> {
        let _lifecycle = self.lifecycle.lock().await;
        let running = self.running.lock().await.take();
        let result = match running {
            Some(server) => stop_running_server(server).await,
            None => Ok(()),
        };
        if let Err(error) = &result {
            self.metrics.set_error(None, &error.message);
        }
        result
    }

    pub async fn status(&self) -> RouterStatus {
        let running = self.running.lock().await;
        let server = running.as_ref();
        let alive = server
            .map(|server| server.alive.load(Ordering::Acquire))
            .unwrap_or(false);
        let phase = match (server.is_some(), alive) {
            (_, true) => RouterPhase::Running,
            (true, false) => RouterPhase::Failed,
            (false, false) => RouterPhase::Stopped,
        };

        RouterStatus {
            phase,
            running: alive,
            address: server.map(|server| server.info.address.clone()),
            port: server.map(|server| server.info.port),
            started_at: server.map(|server| server.info.started_at.clone()),
            uptime_seconds: server
                .filter(|_| alive)
                .map(|server| server.started.elapsed().as_secs())
                .unwrap_or(0),
            active_requests: self.metrics.active_requests.load(Ordering::Relaxed),
            total_requests: self.metrics.total_requests.load(Ordering::Relaxed),
            successful_requests: self.metrics.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.metrics.failed_requests.load(Ordering::Relaxed),
            last_error: self.metrics.last_error.read().clone(),
        }
    }

    pub async fn config_matches(&self, candidate: &RouterRuntimeConfig) -> bool {
        let config = {
            let running = self.running.lock().await;
            let Some(server) = running
                .as_ref()
                .filter(|server| server.alive.load(Ordering::Acquire))
            else {
                return false;
            };
            server.config.clone()
        };
        let matches = config.read().await.equivalent_without_client(candidate);
        matches
    }

    pub async fn target_statuses(
        &self,
        fallback_config: &RouterRuntimeConfig,
    ) -> Vec<RouterTargetStatus> {
        let config = {
            let running = self.running.lock().await;
            if let Some(server) = running
                .as_ref()
                .filter(|server| server.alive.load(Ordering::Acquire))
            {
                server.config.read().await.clone()
            } else {
                fallback_config.clone()
            }
        };
        let mut statuses = Vec::new();
        for agent in [RouterAgent::Claude, RouterAgent::Codex] {
            let runtime = config.upstreams.agent(agent);
            let circuit_config = runtime.policy.circuit_breaker_config();
            for target in &runtime.targets {
                let circuit = self
                    .circuit_breakers
                    .stats(agent, target.id(), circuit_config.clone())
                    .await;
                statuses.push(RouterTargetStatus {
                    agent,
                    target_id: target.id().to_string(),
                    target_name: target.name().to_string(),
                    base_url: target.base_url().as_str().to_string(),
                    active: runtime.policy.active_target == target.id(),
                    queue_position: runtime
                        .policy
                        .failover_queue
                        .iter()
                        .position(|id| id == target.id())
                        .map(|position| position + 1),
                    models: target.models().to_vec(),
                    enable_1m_context: target.enable_1m_context(),
                    enable_chat_completions_proxy: target.enable_chat_completions_proxy(),
                    healthy: circuit.state != CircuitState::Open,
                    circuit,
                });
            }
        }
        statuses
    }

    pub async fn reset_circuit_breaker(
        &self,
        config: &RouterRuntimeConfig,
        agent: RouterAgent,
        target_id: &str,
    ) -> Result<(), RouterError> {
        let runtime = config.upstreams.agent(agent);
        if runtime.target(target_id).is_none() {
            return Err(RouterError::invalid_config("unknown local router target"));
        }
        self.circuit_breakers
            .reset(agent, target_id, runtime.policy.circuit_breaker_config())
            .await;
        Ok(())
    }

    pub async fn usage_summary(&self) -> Result<RouterUsageSummary, RouterError> {
        self.usage_store.summary().await
    }

    pub async fn recent_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<RouterRequestRecord>, RouterError> {
        self.usage_store.recent_requests(limit).await
    }

    async fn spawn_server(
        &self,
        listener: tokio::net::TcpListener,
        config: RouterRuntimeConfig,
    ) -> Result<RunningServer, RouterError> {
        let listener_addr = listener.local_addr().map_err(|error| {
            RouterError::new(
                RouterErrorCode::BindFailed,
                format!("failed to inspect local router listener: {error}"),
            )
        })?;
        let started_at = chrono::Utc::now();
        let info = server_info(&config.listen_address, listener_addr.port(), started_at);
        let config = Arc::new(RwLock::new(config));
        let context = server::ServerContext {
            config: config.clone(),
            usage_store: self.usage_store.clone(),
            metrics: self.metrics.clone(),
            circuit_breakers: self.circuit_breakers.clone(),
        };
        let app = server::router(context);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let alive = Arc::new(AtomicBool::new(true));
        let task_alive = alive.clone();
        let task_metrics = self.metrics.clone();
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
            task_alive.store(false, Ordering::Release);
            if let Err(error) = result {
                task_metrics.set_error(None, format!("local router server stopped: {error}"));
            }
        });

        Ok(RunningServer {
            config,
            listener_addr,
            info,
            started: Instant::now(),
            alive,
            shutdown: Some(shutdown_tx),
            task,
        })
    }
}

async fn stop_running_server(mut server: RunningServer) -> Result<(), RouterError> {
    if let Some(shutdown) = server.shutdown.take() {
        let _ = shutdown.send(());
    }
    let result = tokio::time::timeout(Duration::from_secs(5), &mut server.task).await;
    server.alive.store(false, Ordering::Release);
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(RouterError::new(
            RouterErrorCode::StopFailed,
            format!("local router task failed while stopping: {error}"),
        )),
        Err(_) => {
            server.task.abort();
            let _ = server.task.await;
            Err(RouterError::new(
                RouterErrorCode::StopFailed,
                "timed out while stopping local router",
            ))
        }
    }
}

fn validate_listen_address(address: &str, port: u16) -> Result<SocketAddr, RouterError> {
    if !(1024..=u16::MAX).contains(&port) {
        return Err(RouterError::invalid_config(
            "local router port must be between 1024 and 65535",
        ));
    }

    let address = address.trim();
    if address.eq_ignore_ascii_case("localhost") {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    let address = address
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(address);
    let ip = address.parse::<IpAddr>().map_err(|_| {
        RouterError::invalid_config(
            "local router address must be localhost or a valid IPv4/IPv6 address",
        )
    })?;
    Ok(SocketAddr::new(ip, port))
}

fn host_matches_listener(host: Option<Host<&str>>, listen_addr: SocketAddr) -> bool {
    match host {
        Some(Host::Ipv4(address)) => {
            listen_addr.ip().is_unspecified()
                || IpAddr::V4(address) == listen_addr.ip()
                || (address.is_loopback() && listen_addr.ip().is_loopback())
        }
        Some(Host::Ipv6(address)) => {
            listen_addr.ip().is_unspecified()
                || IpAddr::V6(address) == listen_addr.ip()
                || (address.is_loopback() && listen_addr.ip().is_loopback())
        }
        Some(Host::Domain(domain)) => {
            domain.eq_ignore_ascii_case("localhost") && listen_addr.ip().is_loopback()
        }
        None => false,
    }
}

fn reject_router_loop(target: &UpstreamTarget, listen_addr: SocketAddr) -> Result<(), RouterError> {
    if target.base_url.port_or_known_default() == Some(listen_addr.port())
        && host_matches_listener(target.base_url.host(), listen_addr)
    {
        return Err(RouterError::invalid_config(format!(
            "upstream target '{}' points back to the local router listener",
            target.name()
        )));
    }
    Ok(())
}

fn reject_proxy_loop(
    proxy: &RouterOutboundProxy,
    listen_addr: SocketAddr,
) -> Result<(), RouterError> {
    let parsed = Url::parse(proxy.url())
        .map_err(|_| RouterError::invalid_config("outbound proxy URL is invalid"))?;
    if parsed.port_or_known_default() == Some(listen_addr.port())
        && host_matches_listener(parsed.host(), listen_addr)
    {
        return Err(RouterError::invalid_config(
            "outbound proxy points back to the local router listener",
        ));
    }
    Ok(())
}

fn server_info(
    address: &str,
    port: u16,
    started_at: chrono::DateTime<chrono::Utc>,
) -> RouterServerInfo {
    let host = address.trim();
    let url_host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let base_url = format!("http://{url_host}:{port}");
    RouterServerInfo {
        address: host.to_string(),
        port,
        claude_base_url: format!("{base_url}/claude"),
        codex_base_url: format!("{base_url}/codex/v1"),
        base_url,
        started_at: started_at.to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_validation_accepts_valid_ipv4_ipv6_and_unprivileged_ports() {
        assert!(validate_listen_address("localhost", 43123).is_ok());
        assert!(validate_listen_address("127.0.0.1", 43123).is_ok());
        assert!(validate_listen_address("::1", 43123).is_ok());
        assert!(validate_listen_address("0.0.0.0", 43123).is_ok());
        assert!(validate_listen_address("192.168.1.2", 43123).is_ok());
        assert!(validate_listen_address("example.com", 43123).is_err());
        assert!(validate_listen_address("127.0.0.1", 80).is_err());
        assert!(validate_listen_address("127.0.0.1", 0).is_err());
    }

    #[test]
    fn upstream_rejects_credentials_and_listener_loops() {
        assert!(UpstreamTarget::new("https://user:secret@example.com/v1").is_err());
        assert!(UpstreamTarget::new("https://example.com/v1?key=secret").is_err());

        let target = UpstreamTarget::new("http://localhost:43123/v1").unwrap();
        assert!(reject_router_loop(&target, "127.0.0.1:43123".parse().unwrap()).is_err());
        let target = UpstreamTarget::new("http://192.168.1.2:43123/v1").unwrap();
        assert!(reject_router_loop(&target, "192.168.1.2:43123".parse().unwrap()).is_err());
    }

    #[test]
    fn failover_candidates_follow_queue_while_single_target_uses_active() {
        let first = UpstreamTarget::with_details(
            "first",
            "First",
            "https://first.example/v1",
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let second = UpstreamTarget::with_details(
            "second",
            "Second",
            "https://second.example/v1",
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let mut runtime = RouterAgentRuntime {
            targets: vec![first, second],
            policy: RouterAgentPolicy {
                active_target: "second".to_string(),
                ..RouterAgentPolicy::default()
            },
        };
        assert_eq!(runtime.candidates()[0].id(), "second");

        runtime.policy.auto_failover_enabled = true;
        runtime.policy.failover_queue = vec!["second".to_string(), "first".to_string()];
        assert_eq!(
            runtime
                .candidates()
                .into_iter()
                .map(|target| target.id().to_string())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
    }

    #[test]
    fn enabled_failover_requires_a_valid_queue() {
        let target = UpstreamTarget::with_details(
            "first",
            "First",
            "https://first.example/v1",
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let runtime = |failover_queue: Vec<String>| RouterAgentRuntime {
            targets: vec![target.clone()],
            policy: RouterAgentPolicy {
                auto_failover_enabled: true,
                failover_queue,
                ..RouterAgentPolicy::default()
            },
        };

        let empty = RouterRuntimeConfig::new(
            "127.0.0.1",
            43123,
            false,
            RouterUpstreams {
                claude: runtime(Vec::new()),
                codex: RouterAgentRuntime::default(),
            },
        );
        assert!(empty
            .validate()
            .is_err_and(|error| error.message.contains("non-empty failover queue")));

        let no_targets = RouterRuntimeConfig::new(
            "127.0.0.1",
            43123,
            false,
            RouterUpstreams {
                claude: RouterAgentRuntime {
                    targets: Vec::new(),
                    policy: RouterAgentPolicy {
                        auto_failover_enabled: true,
                        failover_queue: vec!["missing".to_string()],
                        ..RouterAgentPolicy::default()
                    },
                },
                codex: RouterAgentRuntime::default(),
            },
        );
        assert!(no_targets
            .validate()
            .is_err_and(|error| error.message.contains("unknown target")));

        let unknown = RouterRuntimeConfig::new(
            "127.0.0.1",
            43123,
            false,
            RouterUpstreams {
                claude: runtime(vec!["missing".to_string()]),
                codex: RouterAgentRuntime::default(),
            },
        );
        assert!(unknown
            .validate()
            .is_err_and(|error| error.message.contains("unknown target")));

        let valid = RouterRuntimeConfig::new(
            "127.0.0.1",
            43123,
            false,
            RouterUpstreams {
                claude: runtime(vec!["first".to_string()]),
                codex: RouterAgentRuntime::default(),
            },
        );
        assert!(valid.validate().is_ok());
    }

    #[test]
    fn non_loopback_listener_requires_a_strong_access_token() {
        let without_token =
            RouterRuntimeConfig::new("0.0.0.0", 43123, false, RouterUpstreams::default());
        assert!(without_token
            .validate()
            .is_err_and(|error| error.message.contains("access token")));

        let with_token =
            RouterRuntimeConfig::new("0.0.0.0", 43123, false, RouterUpstreams::default())
                .with_access_token("aeroric-0123456789abcdef0123456789abcdef");
        assert!(with_token.validate().is_ok());
    }

    #[test]
    fn server_urls_use_agent_specific_prefixes() {
        let info = server_info("::1", 43123, chrono::Utc::now());
        assert_eq!(info.base_url, "http://[::1]:43123");
        assert_eq!(info.claude_base_url, "http://[::1]:43123/claude");
        assert_eq!(info.codex_base_url, "http://[::1]:43123/codex/v1");
    }
}
