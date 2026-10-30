//! Pure resolution: which channels can serve a request, and in what order.
//!
//! No I/O and no clock reads; callers pass "now" and the cooldown table, so the
//! gateway and the UI ("why is this disabled?") use the same answers.

use std::collections::HashMap;

use serde::Serialize;

use super::model::{
    AgentConfig, ConfigTarget, Connection, ConnectionsDocument, Harness, ModelEntry, Pool,
    PoolMember, PoolStrategy, WireProtocol,
};

/// How a harness reaches a channel.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// Upstream speaks the harness protocol; no translation.
    Direct,
    /// The gateway translates between the two protocols (Q30/Q34).
    ViaGateway {
        from: WireProtocol,
        to: WireProtocol,
    },
}

/// Stable reason codes for "this cannot be used" (shown in UI, AA04 style).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Unavailable {
    ConnectionMissing {
        connection_id: String,
    },
    ConnectionDisabled,
    PoolMissing {
        pool_id: String,
    },
    NoProtocol,
    /// Translation needed, but this harness is not on the gateway yet (Q32-a).
    GatewayNotYetForHarness {
        harness: Harness,
    },
    ModelNotOffered {
        model: String,
    },
    CoolingDown {
        until_epoch_s: u64,
    },
    NoCandidates,
}

/// Which protocol to talk to an upstream in, and whether translation is needed.
pub fn reach(harness: Harness, connection: &Connection) -> Result<Reach, Unavailable> {
    reach_protocols(harness, &connection.protocols)
}

/// The protocols one model is served over: its own override (R10, for
/// aggregators that route models by protocol), else the connection's.
pub fn model_protocols<'a>(
    connection: &'a Connection,
    entry: &'a ModelEntry,
) -> &'a [WireProtocol] {
    if entry.protocols.is_empty() {
        &connection.protocols
    } else {
        &entry.protocols
    }
}

pub fn reach_protocols(harness: Harness, protocols: &[WireProtocol]) -> Result<Reach, Unavailable> {
    reach_in(harness.native_protocol(), harness, protocols)
}

/// The protocol a configuration's harness talks to the gateway in. Fixed per
/// harness, except omp, which speaks what its connections need least
/// translation for (A1, 2026-10-27): Responses when every connection behind the
/// configuration offers Responses and none Messages, else Messages — the
/// gateway carries Messages to all three protocols.
pub fn config_protocol(doc: &ConnectionsDocument, config: &AgentConfig) -> WireProtocol {
    // dsh picks like omp (2026-10-29): both read a provider block that takes
    // Responses or Messages.
    if !matches!(config.harness, Harness::Omp | Harness::Dsh) {
        return config.harness.native_protocol();
    }
    let behind: Vec<&Connection> = match &config.target {
        ConfigTarget::Connection { connection_id } => doc
            .connections
            .iter()
            .filter(|c| &c.id == connection_id)
            .collect(),
        ConfigTarget::Pool { pool_id } => doc
            .pools
            .iter()
            .find(|p| &p.id == pool_id)
            .map(|pool| {
                doc.connections
                    .iter()
                    .filter(|c| pool.members.iter().any(|m| m.connection_id == c.id))
                    .collect()
            })
            .unwrap_or_default(),
        ConfigTarget::Route { .. } => doc.connections.iter().filter(|c| c.enabled).collect(),
    };
    let all_responses = !behind.is_empty()
        && behind
            .iter()
            .all(|c| c.protocols.contains(&WireProtocol::Responses));
    let any_messages = behind
        .iter()
        .any(|c| c.protocols.contains(&WireProtocol::Messages));
    if all_responses && !any_messages {
        WireProtocol::Responses
    } else {
        WireProtocol::Messages
    }
}

/// [`reach_protocols`] for a harness speaking `native` to the gateway.
pub fn reach_in(
    native: WireProtocol,
    harness: Harness,
    protocols: &[WireProtocol],
) -> Result<Reach, Unavailable> {
    if protocols.contains(&native) {
        return Ok(Reach::Direct);
    }
    // Translation preference: keep tool-call semantics closest to the source.
    let order = match native {
        WireProtocol::Messages => [WireProtocol::ChatCompletions, WireProtocol::Responses],
        WireProtocol::Responses => [WireProtocol::ChatCompletions, WireProtocol::Messages],
        WireProtocol::ChatCompletions => [WireProtocol::Responses, WireProtocol::Messages],
    };
    let Some(to) = order.into_iter().find(|p| protocols.contains(p)) else {
        return Err(Unavailable::NoProtocol);
    };
    if !harness.gateway_supported() {
        return Err(Unavailable::GatewayNotYetForHarness { harness });
    }
    Ok(Reach::ViaGateway { from: native, to })
}

/// One routable (connection, model) pair.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub connection_id: String,
    pub upstream_model: String,
    pub reach: Reach,
    pub priority: i32,
    pub weight: u32,
    pub free: bool,
}

/// Cooldown keyed by `connection_id`, or by [`model_key`] for failures that are
/// about one model (R3); value is the epoch second it ends. Global across pools
/// and configs (Q31).
pub type Cooldowns = HashMap<String, u64>;

/// Cooldown key for one model on one connection.
pub fn model_key(connection_id: &str, upstream_model: &str) -> String {
    format!("{connection_id}\u{1f}{upstream_model}")
}

/// When a candidate's cooldown ends: its connection's or its model's, whichever is later.
pub fn cooling_until(cooldowns: &Cooldowns, candidate: &Candidate) -> u64 {
    let connection = cooldowns
        .get(&candidate.connection_id)
        .copied()
        .unwrap_or(0);
    let model = cooldowns
        .get(&model_key(
            &candidate.connection_id,
            &candidate.upstream_model,
        ))
        .copied()
        .unwrap_or(0);
    connection.max(model)
}

/// Largest weight a member can carry (R6); the UI caps input at the same value.
pub const MAX_WEIGHT: u32 = 100;

/// Running counters a strategy may read, keyed by `connection_id`.
#[derive(Clone, Debug, Default)]
pub struct UsageCounters {
    pub requests: HashMap<String, u64>,
    /// Round-robin cursor per pool/route key.
    pub cursor: HashMap<String, usize>,
    /// Smooth weighted round-robin running weights per route key (R6).
    pub smooth: HashMap<String, HashMap<String, i64>>,
}

fn member_meta<'a>(pool: Option<&'a Pool>, connection_id: &str) -> Option<&'a PoolMember> {
    pool.and_then(|p| p.members.iter().find(|m| m.connection_id == connection_id))
}

fn find_model<'a>(connection: &'a Connection, mapped: &str) -> Option<&'a ModelEntry> {
    connection
        .routable_models()
        .find(|m| m.routing_name() == mapped)
}

/// Everything that could serve `model` for this config, before ordering.
/// Returns the candidates and, for the UI, every excluded connection with its reason.
pub fn candidates(
    doc: &ConnectionsDocument,
    config: &AgentConfig,
    model: &str,
) -> (Vec<Candidate>, Vec<(String, Unavailable)>) {
    let mut out = Vec::new();
    let mut excluded = Vec::new();
    let native = config_protocol(doc, config);
    fn consider(
        native: WireProtocol,
        config: &AgentConfig,
        model: &str,
        connection: &Connection,
        member: Option<&PoolMember>,
        out: &mut Vec<Candidate>,
        excluded: &mut Vec<(String, Unavailable)>,
    ) {
        // D1: a login serves only its own harness's 「本机登录」 configuration;
        // everywhere else it is simply not a channel.
        if let Some(owner) = connection.login_harness() {
            if owner != config.harness || config.id != ConnectionsDocument::via_login_id(owner) {
                return;
            }
        }
        if !connection.enabled {
            excluded.push((connection.id.clone(), Unavailable::ConnectionDisabled));
            return;
        }
        let Some(entry) = find_model(connection, model) else {
            excluded.push((
                connection.id.clone(),
                Unavailable::ModelNotOffered {
                    model: model.to_string(),
                },
            ));
            return;
        };
        match reach_in(native, config.harness, model_protocols(connection, entry)) {
            Ok(reach) => out.push(Candidate {
                connection_id: connection.id.clone(),
                upstream_model: entry.upstream_id.clone(),
                reach,
                priority: member.map(|m| m.priority).unwrap_or(0),
                weight: member.map(|m| m.weight.clamp(1, MAX_WEIGHT)).unwrap_or(1),
                free: member.map(|m| m.free).unwrap_or(false),
            }),
            Err(reason) => excluded.push((connection.id.clone(), reason)),
        }
    }
    let lookup = |id: &str| doc.connections.iter().find(|c| c.id == id);
    match &config.target {
        ConfigTarget::Connection { connection_id } => match lookup(connection_id) {
            Some(c) => consider(native, config, model, c, None, &mut out, &mut excluded),
            None => excluded.push((
                connection_id.clone(),
                Unavailable::ConnectionMissing {
                    connection_id: connection_id.clone(),
                },
            )),
        },
        ConfigTarget::Pool { pool_id } => match doc.pools.iter().find(|p| &p.id == pool_id) {
            Some(pool) => {
                for member in &pool.members {
                    match lookup(&member.connection_id) {
                        Some(c) => consider(
                            native,
                            config,
                            model,
                            c,
                            Some(member),
                            &mut out,
                            &mut excluded,
                        ),
                        None => excluded.push((
                            member.connection_id.clone(),
                            Unavailable::ConnectionMissing {
                                connection_id: member.connection_id.clone(),
                            },
                        )),
                    }
                }
            }
            None => excluded.push((
                pool_id.clone(),
                Unavailable::PoolMissing {
                    pool_id: pool_id.clone(),
                },
            )),
        },
        ConfigTarget::Route { pool_id } => {
            let pool = pool_id
                .as_ref()
                .and_then(|id| doc.pools.iter().find(|p| &p.id == id));
            for connection in &doc.connections {
                consider(
                    native,
                    config,
                    model,
                    connection,
                    member_meta(pool, &connection.id),
                    &mut out,
                    &mut excluded,
                );
            }
            // 2026-10-30: under 「本机登录」 the login takes any model no
            // third-party channel serves — Claude Code resolves `opus` to a
            // full id the login's list never names.
            if out.is_empty() && config.id == ConnectionsDocument::via_login_id(config.harness) {
                let login_id = ConnectionsDocument::login_connection_id(config.harness);
                if let Some(login) = doc
                    .connections
                    .iter()
                    .find(|c| c.id == login_id && c.enabled)
                {
                    if let Ok(reach) = reach_in(native, config.harness, &login.protocols) {
                        out.push(Candidate {
                            connection_id: login.id.clone(),
                            upstream_model: model.to_string(),
                            reach,
                            priority: 0,
                            weight: 1,
                            free: false,
                        });
                    }
                }
            }
        }
    }
    (out, excluded)
}

/// Order candidates for one attempt sequence. Cooling members go last, not away:
/// if everything is cooling, the soonest-to-recover is still tried rather than
/// failing with "no channel" while a usable one exists.
pub fn order(
    mut list: Vec<Candidate>,
    strategy: PoolStrategy,
    route_key: &str,
    sticky: Option<&str>,
    cooldowns: &Cooldowns,
    usage: &mut UsageCounters,
    now_epoch_s: u64,
) -> Vec<Candidate> {
    let cooling = |c: &Candidate| cooling_until(cooldowns, c) > now_epoch_s;
    // Stable base order: priority, then id, so every strategy is deterministic.
    list.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.connection_id.cmp(&b.connection_id))
    });
    let requests = |c: &Candidate| usage.requests.get(&c.connection_id).copied().unwrap_or(0);
    match strategy {
        PoolStrategy::Priority | PoolStrategy::FillFirst => {}
        PoolStrategy::LeastUsed | PoolStrategy::Pace => {
            list.sort_by_key(|c| (c.priority, requests(c)));
        }
        PoolStrategy::FreeFirst => list.sort_by_key(|c| (!c.free, c.priority)),
        PoolStrategy::RoundRobin | PoolStrategy::WeightedRoundRobin => {
            // R6: rotate only inside the best priority that has a ready member;
            // lower buckets stay behind it in base order.
            if let Some(top) = list
                .iter()
                .filter(|c| !cooling(c))
                .map(|c| c.priority)
                .min()
            {
                let (mut bucket, rest): (Vec<_>, Vec<_>) = list
                    .into_iter()
                    .partition(|c| c.priority == top && !cooling(c));
                if strategy == PoolStrategy::RoundRobin {
                    let cursor = usage.cursor.entry(route_key.to_string()).or_insert(0);
                    let start = *cursor % bucket.len();
                    *cursor = cursor.wrapping_add(1);
                    bucket.rotate_left(start);
                } else {
                    // Smooth weighted round-robin (nginx): add each weight, take
                    // the largest running value, subtract the total from it.
                    let running = usage.smooth.entry(route_key.to_string()).or_default();
                    let total: i64 = bucket.iter().map(|c| i64::from(c.weight)).sum();
                    for c in &bucket {
                        *running.entry(c.connection_id.clone()).or_insert(0) += i64::from(c.weight);
                    }
                    let mut best = 0;
                    for (index, c) in bucket.iter().enumerate() {
                        if running[&c.connection_id] > running[&bucket[best].connection_id] {
                            best = index;
                        }
                    }
                    if let Some(value) = running.get_mut(&bucket[best].connection_id) {
                        *value -= total;
                    }
                    let pick = bucket.remove(best);
                    bucket.insert(0, pick);
                }
                list = bucket.into_iter().chain(rest).collect();
            }
        }
    }
    if let Some(sticky) = sticky {
        if let Some(pos) = list.iter().position(|c| c.connection_id == sticky) {
            let pinned = list.remove(pos);
            list.insert(0, pinned);
        }
    }
    let (ready, mut waiting): (Vec<_>, Vec<_>) = list.into_iter().partition(|c| !cooling(c));
    waiting.sort_by_key(|c| cooling_until(cooldowns, c));
    ready.into_iter().chain(waiting).collect()
}

/// Upstream outcome classes that decide retry vs. stop (Q25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// The request itself is wrong (or refused for its content); another
    /// channel would fail the same way, so neither switch nor cool down (R2).
    RequestError,
    RateLimited,
    AuthFailed,
    /// 402: out of credit (R6).
    PaymentRequired,
    NotFound,
    ServerError,
    Network,
    /// No first output in time (R3): first time only switches.
    FirstByteTimeout,
}

impl FailureClass {
    pub fn from_status(status: u16) -> Self {
        match status {
            400 | 405..=406 | 413..=415 | 422 | 501 => Self::RequestError,
            429 => Self::RateLimited,
            402 => Self::PaymentRequired,
            401 | 403 => Self::AuthFailed,
            404 => Self::NotFound,
            _ => Self::ServerError,
        }
    }

    /// An error that arrived inside a 200 stream, by its `type`/`code` and
    /// message (R2): safety refusals and oversized prompts are request errors.
    pub fn from_semantic(kind: &str, message: &str) -> Self {
        let kind = kind.to_ascii_lowercase();
        let message = message.to_ascii_lowercase();
        let any = |words: &[&str]| {
            words
                .iter()
                .any(|w| kind.contains(w) || message.contains(w))
        };
        if any(&[
            "context_length",
            "context_window",
            "prompt is too long",
            "maximum context",
        ]) {
            return Self::RequestError;
        }
        if any(&[
            "insufficient_quota",
            "quota",
            "billing",
            "credit",
            "payment",
        ]) {
            return Self::PaymentRequired;
        }
        if any(&[
            "rate_limit",
            "rate limit",
            "too many requests",
            "usage_limit",
        ]) {
            return Self::RateLimited;
        }
        if any(&[
            "authentication",
            "permission",
            "invalid_api_key",
            "unauthorized",
        ]) {
            return Self::AuthFailed;
        }
        if any(&["not_found", "model_not_found", "does not exist"]) {
            return Self::NotFound;
        }
        if any(&[
            "invalid_request",
            "content_filter",
            "content_policy",
            "safety",
            "refusal",
            "cyber_policy",
            "invalid_prompt",
        ]) {
            return Self::RequestError;
        }
        Self::ServerError
    }

    pub fn should_failover(self) -> bool {
        !matches!(self, Self::RequestError)
    }

    /// Cools only the failing model, not the whole connection (R3).
    pub fn cools_model(self) -> bool {
        matches!(self, Self::NotFound | Self::FirstByteTimeout)
    }

    /// May move a sticky session to another member (R4); transient errors
    /// switch for one request only.
    pub fn allows_rebind(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::AuthFailed | Self::PaymentRequired | Self::NotFound
        )
    }

    /// Cleared when the connection's key or settings change (R5); rate limits stay.
    pub fn cleared_by_edit(self) -> bool {
        matches!(
            self,
            Self::AuthFailed | Self::NotFound | Self::PaymentRequired
        )
    }

    /// CPA-style cooldown. `retry_after` wins for 429 (minimum 10 s).
    pub fn cooldown_seconds(self, retry_after: Option<u64>, consecutive: u32) -> u64 {
        match self {
            Self::RequestError => 0,
            Self::RateLimited => retry_after
                .map(|s| s.max(10))
                .unwrap_or_else(|| (10u64 << consecutive.min(6)).min(600)),
            Self::AuthFailed | Self::PaymentRequired => 30 * 60,
            Self::NotFound => 12 * 60 * 60,
            Self::ServerError | Self::Network => (5u64 << consecutive.min(5)).min(300),
            // `consecutive` counts earlier slow starts inside the window.
            Self::FirstByteTimeout => match consecutive {
                0 => 0,
                n => (60u64 << (n - 1).min(5)).min(30 * 60),
            },
        }
    }
}

/// All mapped names a config can offer, for the new-session model menu.
pub fn offered_models(doc: &ConnectionsDocument, config: &AgentConfig) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let pool_members: Option<Vec<&str>> = match &config.target {
        ConfigTarget::Connection { connection_id } => Some(vec![connection_id.as_str()]),
        ConfigTarget::Pool { pool_id } => doc
            .pools
            .iter()
            .find(|p| &p.id == pool_id)
            .map(|p| p.members.iter().map(|m| m.connection_id.as_str()).collect()),
        ConfigTarget::Route { .. } => None,
    };
    for connection in &doc.connections {
        if !connection.enabled {
            continue;
        }
        if let Some(members) = &pool_members {
            if !members.contains(&connection.id.as_str()) {
                continue;
            }
        }
        for model in connection.routable_models() {
            if reach_in(
                config_protocol(doc, config),
                config.harness,
                model_protocols(connection, model),
            )
            .is_err()
            {
                continue;
            }
            let name = model.routing_name().to_string();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}
