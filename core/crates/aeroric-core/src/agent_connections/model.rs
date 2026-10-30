//! Connection / model catalog / pool / agent configuration types (Q23–Q34).
//!
//! Layers, each owning one thing:
//! - [`Connection`]: one upstream (API key + base URL, or an official account).
//!   Secrets are never in this struct; they live in the keychain under
//!   [`Connection::secret_account`].
//! - [`ModelEntry`]: one model on a connection. `upstream_id` is what is sent
//!   upstream, `mapped_name` is what users pick and what routing groups by,
//!   `display_name` is only for UI. Q133: the catalog is versioned per connection.
//! - [`Pool`]: an independent object referencing connections, with a strategy;
//!   cooldown state is global, not per pool (Q31).
//! - [`AgentConfig`]: harness + target (connection, pool, or "route by mapped
//!   name") + reasoning mapping. Tasks snapshot it at creation (Q27).

use serde::{Deserialize, Serialize};

/// The three wire protocols in use today (Q30).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WireProtocol {
    /// OpenAI Responses (`/v1/responses`).
    Responses,
    /// Anthropic Messages (`/v1/messages`).
    Messages,
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    ChatCompletions,
}

impl WireProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::Messages => "messages",
            Self::ChatCompletions => "chat_completions",
        }
    }
}

/// The agent families a configuration can launch.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    Claude,
    Codex,
    Omp,
    Dsh,
}

impl Harness {
    /// The protocol the harness itself speaks to its upstream.
    pub fn native_protocol(self) -> WireProtocol {
        match self {
            Self::Claude => WireProtocol::Messages,
            Self::Codex => WireProtocol::Responses,
            // omp/dsh are configured per model; Chat Completions is the common
            // denominator they both accept.
            Self::Omp | Self::Dsh => WireProtocol::ChatCompletions,
        }
    }

    /// Q32-a: only these go through the gateway (omp since 2026-10-27, over the
    /// Claude or Codex entry — see `routing::config_protocol`).
    pub fn gateway_supported(self) -> bool {
        // dsh since 2026-10-29: its managed home's provider points at the
        // gateway (Chat Completions), like omp's models.yml.
        matches!(self, Self::Claude | Self::Codex | Self::Omp | Self::Dsh)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// User-entered API key + base URL.
    ApiKey,
    /// Official account obtained through the vendor's OAuth flow (Q33-b).
    OfficialOAuth,
    /// An existing local CLI login that the user explicitly linked (Q132).
    LinkedLocalLogin,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Anthropic,
    OpenAi,
    /// Any third-party endpoint.
    Other,
}

/// How a catalog entry got there (Q134: fetched and hand-entered are distinct).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Fetched,
    #[default]
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Image,
    Pdf,
    Audio,
}

/// A user-defined reasoning tier (Q135): label for the UI, value sent upstream.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReasoningTier {
    pub label: String,
    /// Upstream parameter value, e.g. `high` or a budget like `16000`.
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModelEntry {
    pub upstream_id: String,
    /// Routing key. Empty means "same as upstream_id".
    #[serde(default)]
    pub mapped_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub source: ModelSource,
    /// Unknown stays `None`, never 0 (Q129).
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output: Option<u64>,
    #[serde(default)]
    pub inputs: Vec<InputKind>,
    #[serde(default)]
    pub reasoning_tiers: Vec<ReasoningTier>,
    /// Protocols this model is served over when they differ from the
    /// connection's (R10); empty means "the connection's".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<WireProtocol>,
    /// Disabled entries stay in the catalog (refresh keeps user edits, Q134)
    /// but are not routable.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl ModelEntry {
    pub fn routing_name(&self) -> &str {
        let mapped = self.mapped_name.trim();
        if mapped.is_empty() {
            self.upstream_id.trim()
        } else {
            mapped
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    pub label: String,
    pub kind: ConnectionKind,
    pub vendor: Vendor,
    #[serde(default)]
    pub base_url: String,
    /// Protocols this upstream accepts. Routing never sends a protocol not listed here
    /// without the gateway translating it (Q34).
    pub protocols: Vec<WireProtocol>,
    /// Q139: direct by default; the global proxy only when ticked.
    #[serde(default)]
    pub use_global_proxy: bool,
    #[serde(default)]
    pub models: Vec<ModelEntry>,
    /// Bumped on every catalog change; configs and tasks pin it (Q133).
    #[serde(default)]
    pub catalog_revision: u64,
    /// Operator kill switch; disabled connections are never routed to.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// R9: a 429 rests this connection for its cooldown. Off for relays and
    /// aggregators whose 429s are momentary; the request still moves on.
    #[serde(default = "yes")]
    pub rest_on_limit: bool,
}

impl Connection {
    pub fn secret_account(&self) -> String {
        format!("connection:{}:credential", self.id)
    }

    pub fn routable_models(&self) -> impl Iterator<Item = &ModelEntry> {
        self.models.iter().filter(|m| m.enabled)
    }
}

/// Q26-b: every strategy from CLIProxyAPI / magpie.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategy {
    /// Lowest priority value first; next only on failure.
    #[default]
    Priority,
    RoundRobin,
    WeightedRoundRobin,
    /// Stay on one member until it cools down, then move on.
    FillFirst,
    LeastUsed,
    /// Free / zero-cost members before paid ones.
    FreeFirst,
    /// Spread usage evenly across a quota window.
    Pace,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PoolMember {
    pub connection_id: String,
    /// Smaller is tried first; strategies other than `Priority` use it to bucket.
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "one")]
    pub weight: u32,
    #[serde(default)]
    pub free: bool,
}

fn one() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub strategy: PoolStrategy,
    pub members: Vec<PoolMember>,
    /// Keep one session on one member while it is healthy (CPA session affinity).
    #[serde(default = "yes")]
    pub sticky_sessions: bool,
    #[serde(default = "default_sticky_ttl")]
    pub sticky_ttl_seconds: u64,
}

fn default_sticky_ttl() -> u64 {
    3600
}

/// What an agent configuration sends its requests to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigTarget {
    /// One connection, no failover.
    Connection { connection_id: String },
    /// A pool's members, under the pool's strategy.
    Pool { pool_id: String },
    /// Q25: every enabled connection whose catalog maps a model to the chosen name.
    /// `pool_id` optionally supplies the ordering strategy for those candidates.
    Route {
        #[serde(default)]
        pool_id: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentConfig {
    pub id: String,
    pub label: String,
    pub harness: Harness,
    pub target: ConfigTarget,
    /// Mapped model name chosen by default for new tasks.
    #[serde(default)]
    pub default_model: String,
    /// Default reasoning tier label (matches a `ReasoningTier::label`).
    #[serde(default)]
    pub default_reasoning: String,
    /// R8: mapped names tried in order once every channel of the requested
    /// model has failed or is cooling down. The requested model always goes
    /// first, so recovery needs no state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_models: Vec<String>,
}

/// R8-Q4: more than this usually means the configuration needs rethinking.
pub const MAX_FALLBACK_MODELS: usize = 3;

/// The whole persisted document.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct ConnectionsDocument {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub pools: Vec<Pool>,
    #[serde(default)]
    pub configs: Vec<AgentConfig>,
    /// D4/D5 (2026-10-30): whether each harness's own login also serves
    /// through the gateway. Off unless the owner turned it on and accepted.
    #[serde(default, skip_serializing_if = "LoginGateway::is_off")]
    pub login_gateway: LoginGateway,
}

/// The harness's own local login as a gateway source (task
/// `10-29-local-login-gateway`). Only Codex and Claude Code have one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct LoginGateway {
    #[serde(default)]
    pub codex: LoginGatewayState,
    #[serde(default)]
    pub claude: LoginGatewayState,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct LoginGatewayState {
    #[serde(default)]
    pub enabled: bool,
    /// When the owner accepted the terms-and-risk notice (D4); required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<String>,
    /// The login's model list as last fetched from the provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelEntry>,
}

impl LoginGateway {
    fn is_off(&self) -> bool {
        *self == Self::default()
    }

    pub fn state(&self, harness: Harness) -> Option<&LoginGatewayState> {
        match harness {
            Harness::Codex => Some(&self.codex),
            Harness::Claude => Some(&self.claude),
            Harness::Omp | Harness::Dsh => None,
        }
    }

    pub fn state_mut(&mut self, harness: Harness) -> Option<&mut LoginGatewayState> {
        match harness {
            Harness::Codex => Some(&mut self.codex),
            Harness::Claude => Some(&mut self.claude),
            Harness::Omp | Harness::Dsh => None,
        }
    }

    /// On and accepted: the only state in which the login is routable.
    pub fn active(&self, harness: Harness) -> bool {
        self.state(harness)
            .is_some_and(|s| s.enabled && s.accepted_at.is_some())
    }
}

/// Virtual connection ids, never stored: `login-codex`, `login-claude`.
pub const LOGIN_PREFIX: &str = "login-";

impl Connection {
    /// The harness whose own login this virtual connection is (D1: it serves
    /// that harness only).
    pub fn login_harness(&self) -> Option<Harness> {
        if self.kind != ConnectionKind::LinkedLocalLogin {
            return None;
        }
        let harness = Harness::from_slug(self.id.strip_prefix(LOGIN_PREFIX)?)?;
        matches!(harness, Harness::Codex | Harness::Claude).then_some(harness)
    }
}

pub const DOCUMENT_VERSION: u32 = 1;

/// 2026-10-27: the new-session picker offers every saved connection directly,
/// per harness that can reach it through the gateway. Each is an Agent
/// configuration derived on the fly — `via-<harness>-<hash of the connection
/// id>` (the catalog never exposes connection ids), single connection target —
/// never stored, so deleting the connection removes it.
pub const VIA_PREFIX: &str = "via-";

impl Harness {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Omp => "omp",
            Self::Dsh => "dsh",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        [Self::Claude, Self::Codex, Self::Omp, Self::Dsh]
            .into_iter()
            .find(|h| h.slug() == slug)
    }
}

impl ConnectionsDocument {
    /// The derived configuration id for `connection_id` under `harness`.
    pub fn via_id(harness: Harness, connection_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(connection_id.as_bytes());
        format!(
            "{VIA_PREFIX}{}-{}",
            harness.slug(),
            hex::encode(&digest[..8])
        )
    }

    /// 「本机登录」through the gateway for `harness` (2026-10-30): the login
    /// first, then every enabled third-party connection, by model name.
    pub fn via_login_id(harness: Harness) -> String {
        format!("{VIA_PREFIX}{}-login", harness.slug())
    }

    pub fn login_connection_id(harness: Harness) -> String {
        format!("{LOGIN_PREFIX}{}", harness.slug())
    }

    /// This document plus the virtual login connections that are on. Routing,
    /// launching and the session pickers read this; edits and saves use the
    /// stored document, so the virtual connections are never written.
    pub fn with_logins(mut self) -> Self {
        for harness in [Harness::Codex, Harness::Claude] {
            if !self.login_gateway.active(harness) {
                continue;
            }
            let models = self
                .login_gateway
                .state(harness)
                .map(|s| s.models.clone())
                .unwrap_or_default();
            let (vendor, protocol, base_url, label) = match harness {
                Harness::Codex => (
                    Vendor::OpenAi,
                    WireProtocol::Responses,
                    "https://chatgpt.com/backend-api/codex",
                    "本机登录 · Codex",
                ),
                _ => (
                    Vendor::Anthropic,
                    WireProtocol::Messages,
                    "https://api.anthropic.com",
                    "本机登录 · Claude Code",
                ),
            };
            self.connections.insert(
                0,
                Connection {
                    id: Self::login_connection_id(harness),
                    label: label.into(),
                    kind: ConnectionKind::LinkedLocalLogin,
                    vendor,
                    base_url: base_url.into(),
                    protocols: vec![protocol],
                    use_global_proxy: false,
                    models,
                    catalog_revision: 0,
                    enabled: true,
                    rest_on_limit: true,
                },
            );
        }
        self
    }

    /// 「全部第三方模型」for `harness` (2026-10-29): every enabled connection,
    /// routed by model name.
    pub fn via_all_id(harness: Harness) -> String {
        format!("{VIA_PREFIX}{}-all", harness.slug())
    }

    /// A stored configuration, or the derived one a `via-` id names.
    pub fn config(&self, id: &str) -> Option<AgentConfig> {
        if let Some(config) = self.configs.iter().find(|c| c.id == id) {
            return Some(config.clone());
        }
        let (slug, rest) = id.strip_prefix(VIA_PREFIX)?.split_once('-')?;
        let harness = Harness::from_slug(slug)?;
        if rest == "all" {
            return self.derived_all(harness);
        }
        if rest == "login" {
            return self.derived_login(harness);
        }
        let connection = self
            .connections
            .iter()
            .find(|c| Self::via_id(harness, &c.id) == id)?;
        self.derived_config(harness, connection)
    }

    /// One derived configuration per enabled connection and gateway harness
    /// that can reach at least one of its models, in connection order.
    pub fn connection_configs(&self) -> Vec<AgentConfig> {
        // 2026-10-29: every harness (omp and dsh too), each with 「全部第三方
        // 模型」 first, then one entry per connection.
        const HARNESSES: [Harness; 4] =
            [Harness::Codex, Harness::Claude, Harness::Omp, Harness::Dsh];
        let mut out = Vec::new();
        for harness in HARNESSES {
            if let Some(config) = self.derived_all(harness) {
                out.push(config);
            }
        }
        // Logins are offered as 「本机登录」, never as a connection entry.
        for connection in self
            .connections
            .iter()
            .filter(|c| c.enabled && c.login_harness().is_none())
        {
            for harness in HARNESSES {
                if let Some(config) = self.derived_config(harness, connection) {
                    out.push(config);
                }
            }
        }
        out
    }

    fn derived_all(&self, harness: Harness) -> Option<AgentConfig> {
        if !harness.gateway_supported() || !self.connections.iter().any(|c| c.enabled) {
            return None;
        }
        let mut config = AgentConfig {
            id: Self::via_all_id(harness),
            label: "全部第三方模型".into(),
            harness,
            target: ConfigTarget::Route { pool_id: None },
            default_model: String::new(),
            default_reasoning: String::new(),
            fallback_models: Vec::new(),
        };
        let offered = super::routing::offered_models(self, &config);
        config.default_model = self
            .connections
            .iter()
            .filter(|c| c.enabled && c.login_harness().is_none())
            .flat_map(|c| c.routable_models())
            .map(|m| m.routing_name().to_string())
            .find(|name| offered.contains(name))?;
        Some(config)
    }

    /// Present only while the login is on (`with_logins` added it) and has a
    /// model; its default is the login's own first model.
    fn derived_login(&self, harness: Harness) -> Option<AgentConfig> {
        let login_id = Self::login_connection_id(harness);
        let login = self.connections.iter().find(|c| c.id == login_id)?;
        let default_model = login.routable_models().next()?.routing_name().to_string();
        Some(AgentConfig {
            id: Self::via_login_id(harness),
            label: "本机登录".into(),
            harness,
            target: ConfigTarget::Route { pool_id: None },
            default_model,
            default_reasoning: String::new(),
            fallback_models: Vec::new(),
        })
    }

    fn derived_config(&self, harness: Harness, connection: &Connection) -> Option<AgentConfig> {
        if !harness.gateway_supported() || !connection.enabled {
            return None;
        }
        let mut config = AgentConfig {
            id: Self::via_id(harness, &connection.id),
            label: connection.label.clone(),
            harness,
            target: ConfigTarget::Connection {
                connection_id: connection.id.clone(),
            },
            default_model: String::new(),
            default_reasoning: String::new(),
            fallback_models: Vec::new(),
        };
        let offered = super::routing::offered_models(self, &config);
        // The connection's own order picks the default, not the sorted list.
        config.default_model = connection
            .routable_models()
            .map(|m| m.routing_name().to_string())
            .find(|name| offered.contains(name))?;
        Some(config)
    }
}
