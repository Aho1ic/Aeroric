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

    /// Q32-a: only these go through the gateway in the first batch.
    pub fn gateway_supported(self) -> bool {
        matches!(self, Self::Claude | Self::Codex)
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
}

pub const DOCUMENT_VERSION: u32 = 1;
