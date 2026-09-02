mod cache_injector;
mod chat_bridge;
mod circuit_breaker;
mod inline_tool_calls;
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
    pub fn candidates_for(&self, preferred_target: Option<&str>) -> Vec<UpstreamTarget> {
        if let Some(id) = preferred_target {
            // A target-qualified URL represents an Agent configuration, not a
            // routing preference. Falling through to the shared failover queue
            // would silently run the task with a different configuration.
            return self.target(id).cloned().into_iter().collect();
        }
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

    /// 客户端主动断开：只回收在途计数，不计成功也不计失败。用户打断任务是正常操作，
    /// 算进失败率会让健康面板和"最近错误"横幅长期显示成故障。
    pub(crate) fn finish_client_abort(&self) {
        let _ =
            self.active_requests
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(current.saturating_sub(1))
                });
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

    /// 四个计数器的快照。
    ///
    /// 只给 `metrics_stress_tests` 用。生产侧读这些值走 `LocalRouterState::status()`
    /// (要一个跑着的 server 才能构造),压测只想验计数器本身的守恒性,不该为此
    /// 起一整个服务。返回顺序:active / total / successful / failed。
    #[cfg(test)]
    pub(crate) fn counters(&self) -> (u64, u64, u64, u64) {
        (
            self.active_requests.load(Ordering::Relaxed),
            self.total_requests.load(Ordering::Relaxed),
            self.successful_requests.load(Ordering::Relaxed),
            self.failed_requests.load(Ordering::Relaxed),
        )
    }
}

struct RunningServer {
    config: Arc<RwLock<RouterRuntimeConfig>>,
    listener_addr: SocketAddr,
    info: RouterServerInfo,
    started: Instant,
    alive: Arc<AtomicBool>,
    /// 与 [`LISTENING`] 里记录的代号对应，停服时只撤销自己那一代的标记。
    generation: u64,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

/// 当前真正在监听的端口，`None` 表示没有在跑。
///
/// Agent 的 `OPENAI_BASE_URL` / `ANTHROPIC_BASE_URL` 在启动进程时就被写死，只看设置里的
/// `enabled` 开关会出现这种情况：开关是开的但服务实际没起来(端口被占、绑定失败、启动
/// 中途失败)，Agent 仍被指向 127.0.0.1:<port>，于是每次请求都是
/// `error sending request for url (http://127.0.0.1:18080/...)`。启动新任务时读这个值，
/// 就能在服务没真正监听时回落到上游直连。
///
/// 记录 generation 而不只是端口：restart 会先在同一端口上绑好新 listener 再停旧的，
/// 只按端口撤销会把新服务的标记一起清掉。
static LISTENING: ParkingRwLock<Option<(u64, u16)>> = ParkingRwLock::new(None);
static LISTENING_GENERATION: AtomicU64 = AtomicU64::new(0);

fn next_listening_generation() -> u64 {
    LISTENING_GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

fn set_listening(generation: u64, port: u16) {
    *LISTENING.write() = Some((generation, port));
}

fn clear_listening(generation: u64) {
    let mut current = LISTENING.write();
    if current.is_some_and(|(active, _)| active == generation) {
        *current = None;
    }
}

/// 本地路由是否正在监听给定端口。仅在确认监听时返回 true，宁可让 Agent 直连上游，
/// 也不要把它指向一个没人接的端口。
pub fn is_listening_on(port: u16) -> bool {
    LISTENING
        .read()
        .is_some_and(|(_, active_port)| active_port == port)
}

pub struct LocalRouterState {
    usage_store: usage::UsageStore,
    metrics: Arc<RuntimeMetrics>,
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    lifecycle: Mutex<()>,
    running: Mutex<Option<RunningServer>>,
}

impl LocalRouterState {
    /// 纯构造:只存下路径,不碰磁盘,所以不返回 `Result`。
    ///
    /// 原先签名是 `Result<Self, RouterError>` 但函数体里没有一处会失败——真正的
    /// 建库延迟到 `usage_store.initialize()`。保留假的错误分支会诱使调用方在启动
    /// 路径上写 `.expect()`,那正是本次要消掉的 panic 来源;数据目录的选取交给
    /// 调用方(见 `LocalRouterManager::for_app` 的降级)。
    pub fn with_database_path(database_path: PathBuf) -> Self {
        Self {
            usage_store: usage::UsageStore::new(database_path),
            metrics: Arc::new(RuntimeMetrics::default()),
            circuit_breakers: Arc::new(CircuitBreakerRegistry::default()),
            lifecycle: Mutex::new(()),
            running: Mutex::new(None),
        }
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
        // 监听已经建立成功（listener 已 bind），到这里才允许把 Agent 指向本地端口。
        let generation = next_listening_generation();
        set_listening(generation, listener_addr.port());
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
            task_alive.store(false, Ordering::Release);
            clear_listening(generation);
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
            generation,
            shutdown: Some(shutdown_tx),
            task,
        })
    }
}

async fn stop_running_server(mut server: RunningServer) -> Result<(), RouterError> {
    // 先撤掉"正在监听"标记再关服务：关闭过程中启动的新任务应该直连上游，
    // 而不是指向一个马上就不存在的端口。
    clear_listening(server.generation);
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
        assert_eq!(runtime.candidates_for(None)[0].id(), "second");
        assert_eq!(runtime.candidates_for(Some("first"))[0].id(), "first");
        assert!(runtime.candidates_for(Some("missing")).is_empty());

        runtime.policy.auto_failover_enabled = true;
        runtime.policy.failover_queue = vec!["second".to_string(), "first".to_string()];
        assert_eq!(
            runtime
                .candidates_for(None)
                .into_iter()
                .map(|target| target.id().to_string())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
        assert_eq!(
            runtime
                .candidates_for(Some("first"))
                .into_iter()
                .map(|target| target.id().to_string())
                .collect::<Vec<_>>(),
            vec!["first"]
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

/// `RuntimeMetrics` 与监听代号的并发压力测试。
///
/// 这两块原先**一条测试都没有**(全仓库搜 `RuntimeMetrics` / `begin_request` 只有实现
/// 自己那几行)。它们都是进程级共享可变状态,而且失效方式都是"数字慢慢飘、界面一直显示
/// 错的东西",不会崩,所以只能靠断言守。
#[cfg(test)]
mod metrics_stress_tests {
    use super::*;

    /// 每个用例自己一份,不共享 —— 计数器守恒断言必须独占才有意义。
    fn metrics() -> Arc<RuntimeMetrics> {
        Arc::new(RuntimeMetrics::default())
    }

    #[test]
    fn concurrent_begin_and_finish_conserve_every_counter() {
        // 高并发下最容易出的是"丢一次加法":用 Relaxed 原子做独立计数是对的,
        // 但只要哪条路径忘了配对,`active_requests` 就会永久飘高,
        // 健康面板上会一直挂着几个不存在的在途请求。
        let metrics = metrics();
        let threads = 16;
        let per_thread = 500;

        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let metrics = Arc::clone(&metrics);
                std::thread::spawn(move || {
                    for i in 0..per_thread {
                        metrics.begin_request();
                        // 三分之一成功、三分之一失败、三分之一客户端断开,
                        // 覆盖全部三条收尾路径。
                        match (t + i) % 3 {
                            0 => metrics.finish_request(true),
                            1 => metrics.finish_request(false),
                            _ => metrics.finish_client_abort(),
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker panicked");
        }

        let (active, total, successful, failed) = metrics.counters();
        let expected_total = (threads * per_thread) as u64;
        assert_eq!(active, 0, "所有请求都收尾了,在途数必须归零,实际 {active}");
        assert_eq!(total, expected_total, "begin_request 一次都不能丢");
        // 断开既不算成功也不算失败(mod.rs:596 的注释就是这个约定),
        // 所以 successful + failed 严格小于 total,差额正好是断开数。
        let aborted = expected_total - successful - failed;
        assert!(aborted > 0, "本用例应当产生断开请求,否则第三条路径没被覆盖");
        assert_eq!(
            successful + failed + aborted,
            expected_total,
            "三条收尾路径之和必须等于 total"
        );
    }

    #[test]
    fn in_flight_requests_are_visible_while_they_are_still_running() {
        // **必须在请求"还没收尾"的时候读一次 active_requests。**
        // 上面那条守恒用例只在全部收尾后断言 `active == 0`,而 0 是这个计数器
        // 从未被累加时的同一个值 —— 变异测试实测:把 `begin_request` 里
        // `active_requests.fetch_add` 整行删掉,那条用例照样全绿(在途数恒为 0,
        // 收尾时 saturating_sub 又夹在 0)。后果是健康面板的"在途请求"永远显示 0,
        // 排查卡住的请求时完全没有信号。
        let metrics = metrics();
        let threads = 8;
        // 让 8 个线程都 begin 之后停在栅栏上,主线程此刻观测;
        // 观测完再放它们去收尾。
        let entered = Arc::new(std::sync::Barrier::new(threads + 1));
        let observed = Arc::new(std::sync::Barrier::new(threads + 1));

        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let metrics = Arc::clone(&metrics);
                let entered = Arc::clone(&entered);
                let observed = Arc::clone(&observed);
                std::thread::spawn(move || {
                    metrics.begin_request();
                    entered.wait();
                    observed.wait();
                    metrics.finish_request(true);
                })
            })
            .collect();

        entered.wait();
        let (active_mid_flight, total_mid_flight, successful, failed) = metrics.counters();
        assert_eq!(
            active_mid_flight, threads as u64,
            "8 个请求都在途时,在途数必须是 8"
        );
        assert_eq!(total_mid_flight, threads as u64);
        assert_eq!(
            (successful, failed),
            (0, 0),
            "还没收尾就不该有成功或失败计数"
        );
        observed.wait();

        for handle in handles {
            handle.join().expect("worker panicked");
        }
        assert_eq!(metrics.counters().0, 0, "收尾后归零");
    }

    #[test]
    fn a_client_abort_is_counted_as_neither_success_nor_failure() {
        // 单独钉这条语义:用户打断任务是正常操作。算进失败率会让健康面板和
        // "最近错误"横幅长期显示成故障 —— 这正是 mod.rs:596 那段注释要防的。
        let metrics = metrics();
        for _ in 0..50 {
            metrics.begin_request();
            metrics.finish_client_abort();
        }

        let (active, total, successful, failed) = metrics.counters();
        assert_eq!(active, 0);
        assert_eq!(total, 50);
        assert_eq!(successful, 0, "断开不该计成功");
        assert_eq!(
            failed, 0,
            "断开不该计失败 —— 否则失败率会被用户的正常打断污染"
        );
    }

    #[test]
    fn active_requests_never_underflows_when_finish_outnumbers_begin() {
        // `finish_request` 用 saturating_sub。多余的收尾必须夹在 0,
        // 不能回绕成 u64::MAX —— 那会让面板显示 1844 亿个在途请求。
        let metrics = metrics();
        metrics.begin_request();
        for _ in 0..10 {
            metrics.finish_request(true);
        }

        let (active, _, successful, _) = metrics.counters();
        assert_eq!(active, 0, "不能下溢回绕");
        // 同时如实记录:多余的收尾**会**被计成功。saturating_sub 掩盖了配对错误,
        // 所以这里的 10 是"实现当前行为",不是"应该如此"。谁要修配对问题,
        // 得先让调用方不重复收尾,而不是改这个夹取。
        assert_eq!(successful, 10);
    }

    #[test]
    fn concurrent_error_writes_leave_a_consistent_last_error() {
        // `last_error` 是 RwLock<Option<..>>,并发写只要求"最后留下的是某一次完整的写",
        // 不能出现字段撕裂(agent 来自 A、message 来自 B)。
        let metrics = metrics();
        let threads = 12;

        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let metrics = Arc::clone(&metrics);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        // agent 和 message 一一对应,撕裂就能被下面的断言抓到。
                        let agent = if t % 2 == 0 {
                            RouterAgent::Claude
                        } else {
                            RouterAgent::Codex
                        };
                        metrics.set_error(Some(agent), format!("agent-{t}"));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker panicked");
        }

        let last = metrics.last_error.read().clone().expect("应当留下错误");
        let index: usize = last
            .message
            .strip_prefix("agent-")
            .and_then(|s| s.parse().ok())
            .expect("message 应为 agent-<n>");
        let expected = if index.is_multiple_of(2) {
            RouterAgent::Claude
        } else {
            RouterAgent::Codex
        };
        assert_eq!(
            last.agent,
            Some(expected),
            "agent 与 message 必须来自同一次写入(撕裂了就是这条挂)"
        );
    }

    // ── 监听代号 ────────────────────────────────────────────────────────────

    /// `LISTENING` 是进程级 static,下面几条必须串行跑,否则彼此覆盖。
    /// 用独立的锁而不是 `#[serial]`(仓库没引那个 crate)。
    ///
    /// 这把锁只挡得住本模块内部。另一头 `app_settings` 那批用例会经
    /// `get_agent_launch_spec_from_settings` → `is_listening_on` 读同一个 static,
    /// 期望"没有服务在跑"。所以下面统一用 43991+ 这段端口:它既不是默认端口
    /// `DEFAULT_LOCAL_ROUTER_PORT`(15721),也不在那批用例写的 80 / 19090-19092 里,
    /// `is_listening_on` 对它们仍然如实返回 false。**新增用例请继续用这段端口。**
    static LISTENING_TEST_LOCK: ParkingRwLock<()> = ParkingRwLock::new(());

    #[test]
    fn a_restart_on_the_same_port_does_not_clear_the_new_generation() {
        // **这一条钉的是 mod.rs:632 那段注释描述的真实 bug。**
        // restart 会先在同一端口绑好新 listener 再停旧的,如果 `clear_listening`
        // 只按端口撤销,就会把新服务的标记一起清掉 —— 于是服务明明在跑,
        // `is_listening_on` 却返回 false,Agent 被回落成直连上游。
        let _guard = LISTENING_TEST_LOCK.write();
        let port = 43991;

        let old = next_listening_generation();
        set_listening(old, port);
        assert!(is_listening_on(port));

        // 新一代在同端口上线(重叠窗口),随后旧一代才收尾。
        let new = next_listening_generation();
        set_listening(new, port);
        clear_listening(old);

        assert!(
            is_listening_on(port),
            "旧一代收尾不能撤掉新一代的标记 —— 否则 Agent 会被指回上游直连"
        );

        clear_listening(new);
        assert!(!is_listening_on(port), "新一代自己收尾后才该归零");
    }

    #[test]
    fn generations_are_strictly_increasing_under_concurrency() {
        // 代号靠 fetch_add 产生,并发下必须两两不同 —— 撞号会让一次 clear
        // 撤掉另一代的标记,又回到上面那个 bug。
        let threads = 16;
        let per_thread = 200;
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                std::thread::spawn(move || {
                    (0..per_thread)
                        .map(|_| next_listening_generation())
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        let mut all: Vec<u64> = handles
            .into_iter()
            .flat_map(|h| h.join().expect("worker panicked"))
            .collect();
        let issued = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), issued, "并发取号不能撞号");
    }

    #[test]
    fn is_listening_on_only_answers_for_the_recorded_port() {
        let _guard = LISTENING_TEST_LOCK.write();
        let generation = next_listening_generation();
        set_listening(generation, 43992);

        assert!(is_listening_on(43992));
        assert!(
            !is_listening_on(43993),
            "别的端口必须为 false —— 宁可让 Agent 直连上游,也不要指向没人接的端口"
        );

        clear_listening(generation);
    }

    #[test]
    fn clearing_an_unknown_generation_is_a_no_op() {
        // 停一个早已被顶替的服务不能影响当前标记。
        let _guard = LISTENING_TEST_LOCK.write();
        let current = next_listening_generation();
        set_listening(current, 43994);

        clear_listening(current.wrapping_sub(1));
        assert!(is_listening_on(43994), "撤销不认识的代号应当无事发生");

        clear_listening(current);
        assert!(!is_listening_on(43994));
    }
}
