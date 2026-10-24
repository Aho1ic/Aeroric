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
    let native = harness.native_protocol();
    if connection.protocols.contains(&native) {
        return Ok(Reach::Direct);
    }
    // Translation preference: keep tool-call semantics closest to the source.
    let order = match native {
        WireProtocol::Messages => [WireProtocol::ChatCompletions, WireProtocol::Responses],
        WireProtocol::Responses => [WireProtocol::ChatCompletions, WireProtocol::Messages],
        WireProtocol::ChatCompletions => [WireProtocol::Responses, WireProtocol::Messages],
    };
    let Some(to) = order.into_iter().find(|p| connection.protocols.contains(p)) else {
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

/// Cooldown keyed by `connection_id`; value is the epoch second it ends.
/// Global across pools and configs (Q31).
pub type Cooldowns = HashMap<String, u64>;

/// Running counters a strategy may read, keyed by `connection_id`.
#[derive(Clone, Debug, Default)]
pub struct UsageCounters {
    pub requests: HashMap<String, u64>,
    /// Round-robin cursor per pool/route key.
    pub cursor: HashMap<String, usize>,
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
    fn consider(
        config: &AgentConfig,
        model: &str,
        connection: &Connection,
        member: Option<&PoolMember>,
        out: &mut Vec<Candidate>,
        excluded: &mut Vec<(String, Unavailable)>,
    ) {
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
        match reach(config.harness, connection) {
            Ok(reach) => out.push(Candidate {
                connection_id: connection.id.clone(),
                upstream_model: entry.upstream_id.clone(),
                reach,
                priority: member.map(|m| m.priority).unwrap_or(0),
                weight: member.map(|m| m.weight.max(1)).unwrap_or(1),
                free: member.map(|m| m.free).unwrap_or(false),
            }),
            Err(reason) => excluded.push((connection.id.clone(), reason)),
        }
    }
    let lookup = |id: &str| doc.connections.iter().find(|c| c.id == id);
    match &config.target {
        ConfigTarget::Connection { connection_id } => match lookup(connection_id) {
            Some(c) => consider(config, model, c, None, &mut out, &mut excluded),
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
                        Some(c) => {
                            consider(config, model, c, Some(member), &mut out, &mut excluded)
                        }
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
                    config,
                    model,
                    connection,
                    member_meta(pool, &connection.id),
                    &mut out,
                    &mut excluded,
                );
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
    let cooling =
        |c: &Candidate| cooldowns.get(&c.connection_id).copied().unwrap_or(0) > now_epoch_s;
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
            // Expand by weight for weighted RR, then rotate within the top priority bucket.
            let expanded: Vec<Candidate> = if strategy == PoolStrategy::WeightedRoundRobin {
                list.iter()
                    .flat_map(|c| std::iter::repeat_n(c.clone(), c.weight as usize))
                    .collect()
            } else {
                list.clone()
            };
            if !expanded.is_empty() {
                let cursor = usage.cursor.entry(route_key.to_string()).or_insert(0);
                let start = *cursor % expanded.len();
                *cursor = cursor.wrapping_add(1);
                let mut rotated: Vec<Candidate> = expanded[start..]
                    .iter()
                    .chain(expanded[..start].iter())
                    .cloned()
                    .collect();
                rotated.dedup_by(|a, b| a.connection_id == b.connection_id);
                let mut seen = std::collections::HashSet::new();
                rotated.retain(|c| seen.insert(c.connection_id.clone()));
                list = rotated;
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
    waiting.sort_by_key(|c| cooldowns.get(&c.connection_id).copied().unwrap_or(0));
    ready.into_iter().chain(waiting).collect()
}

/// Upstream outcome classes that decide retry vs. stop (Q25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// The request itself is wrong; another channel would fail the same way.
    RequestError,
    RateLimited,
    AuthFailed,
    NotFound,
    ServerError,
    Network,
}

impl FailureClass {
    pub fn from_status(status: u16) -> Self {
        match status {
            400 | 405..=406 | 413..=415 | 422 | 501 => Self::RequestError,
            429 => Self::RateLimited,
            401..=403 => Self::AuthFailed,
            404 => Self::NotFound,
            _ => Self::ServerError,
        }
    }

    pub fn should_failover(self) -> bool {
        !matches!(self, Self::RequestError)
    }

    /// CPA-style cooldown. `retry_after` wins for 429 (minimum 10 s).
    pub fn cooldown_seconds(self, retry_after: Option<u64>, consecutive: u32) -> u64 {
        match self {
            Self::RequestError => 0,
            Self::RateLimited => retry_after
                .map(|s| s.max(10))
                .unwrap_or_else(|| (10u64 << consecutive.min(6)).min(600)),
            Self::AuthFailed => 30 * 60,
            Self::NotFound => 12 * 60 * 60,
            Self::ServerError | Self::Network => (5u64 << consecutive.min(5)).min(300),
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
        if reach(config.harness, connection).is_err() {
            continue;
        }
        for model in connection.routable_models() {
            let name = model.routing_name().to_string();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}
