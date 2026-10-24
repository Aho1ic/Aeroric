//! Routing for tasks pinned to an agent configuration (B1 step 4b, Q23–Q34).
//!
//! A request carrying a task token whose grant names an agent configuration gets
//! its targets from `agent-connections.json` instead of the legacy settings list.
//! Resolution and ordering are the pure functions in `agent_connections::routing`;
//! this module only adds the process-global state they read:
//! - cooldowns, keyed by connection id and shared by every pool and config (Q31);
//! - sticky sessions, keyed by task id (CPA session affinity);
//! - usage counters for least-used / round-robin strategies.
//!
//! Translation the router cannot do yet is excluded with a reason rather than
//! sent and left to fail upstream: today only Responses → Chat Completions is
//! bridged (Messages translation lands in 4c/4d).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::agent_connections::model::{
    ConfigTarget, ConnectionsDocument, PoolStrategy, WireProtocol,
};
use crate::agent_connections::routing::{
    self, Candidate, Cooldowns, FailureClass, Reach, UsageCounters,
};
use crate::agent_connections::store;
use crate::context::SecretStore;

use super::task_tokens::TaskGrant;
use super::UpstreamTarget;

/// Target ids minted here carry this prefix so they never collide with legacy
/// settings targets in the shared circuit-breaker registry.
pub const TARGET_PREFIX: &str = "conn:";

#[derive(Default)]
struct State {
    cooldowns: Cooldowns,
    /// Consecutive failures per connection, for exponential cooldowns.
    streaks: HashMap<String, u32>,
    /// task id → (connection id, expires at epoch s).
    sticky: HashMap<String, (String, u64)>,
    usage: UsageCounters,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock();
    f(guard.get_or_insert_with(State::default))
}

fn now_epoch_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether the router can carry this reach today.
fn reach_supported(reach: Reach) -> bool {
    match reach {
        Reach::Direct => true,
        // 4c: Responses/Messages → Chat; 4d: Messages ↔ Responses via a Chat pivot.
        Reach::ViaGateway { from, to } => matches!(
            (from, to),
            (
                WireProtocol::Responses | WireProtocol::Messages,
                WireProtocol::ChatCompletions
            ) | (WireProtocol::Messages, WireProtocol::Responses)
                | (WireProtocol::Responses, WireProtocol::Messages)
        ),
    }
}

/// What resolution produced: ordered targets, or why there are none.
#[derive(Debug)]
pub enum Resolution {
    Targets(Vec<UpstreamTarget>),
    Unavailable(String),
}

fn strategy_and_ttl(
    doc: &ConnectionsDocument,
    target: &ConfigTarget,
) -> (PoolStrategy, Option<u64>, String) {
    let pool_id = match target {
        ConfigTarget::Connection { connection_id } => {
            return (PoolStrategy::Priority, None, format!("c:{connection_id}"))
        }
        ConfigTarget::Pool { pool_id } => Some(pool_id.as_str()),
        ConfigTarget::Route { pool_id } => pool_id.as_deref(),
    };
    match pool_id.and_then(|id| doc.pools.iter().find(|p| p.id == id)) {
        Some(pool) => (
            pool.strategy,
            pool.sticky_sessions.then_some(pool.sticky_ttl_seconds),
            format!("p:{}", pool.id),
        ),
        // Route without a pool: priority order, sticky by default (one hour).
        None => (PoolStrategy::Priority, Some(3600), "route".to_string()),
    }
}

/// Resolve `model` for the task behind `grant` into ordered upstream targets.
pub fn resolve(
    doc: &ConnectionsDocument,
    grant: &TaskGrant,
    model: &str,
    secrets: &dyn SecretStore,
) -> Resolution {
    let Some(config_id) = grant.agent_config_id.as_deref() else {
        return Resolution::Unavailable("task token carries no agent configuration".into());
    };
    let Some(config) = doc.configs.iter().find(|c| c.id == config_id) else {
        return Resolution::Unavailable(format!(
            "agent configuration `{config_id}` no longer exists"
        ));
    };
    let model = if model.trim().is_empty() {
        config.default_model.as_str()
    } else {
        model.trim()
    };
    if model.is_empty() {
        return Resolution::Unavailable(
            "request names no model and the configuration has no default".into(),
        );
    }
    let (found, excluded) = routing::candidates(doc, config, model);
    let mut reasons: Vec<String> = excluded
        .iter()
        .map(|(id, reason)| format!("{id}: {reason:?}"))
        .collect();
    let usable: Vec<Candidate> = found
        .into_iter()
        .filter(|c| {
            let ok = reach_supported(c.reach);
            if !ok {
                reasons.push(format!(
                    "{}: protocol translation not available yet",
                    c.connection_id
                ));
            }
            ok
        })
        .collect();
    if usable.is_empty() {
        return Resolution::Unavailable(format!(
            "no channel can serve `{model}`: {}",
            if reasons.is_empty() {
                "none configured".to_string()
            } else {
                reasons.join("; ")
            }
        ));
    }

    let (strategy, sticky_ttl, route_key) = strategy_and_ttl(doc, &config.target);
    let now = now_epoch_s();
    let ordered = with_state(|state| {
        let sticky = sticky_ttl.and_then(|_| {
            state
                .sticky
                .get(&grant.task_id)
                .filter(|(_, until)| *until > now)
                .map(|(id, _)| id.clone())
        });
        routing::order(
            usable,
            strategy,
            &route_key,
            sticky.as_deref(),
            &state.cooldowns,
            &mut state.usage,
            now,
        )
    });

    let mut targets = Vec::new();
    for candidate in ordered {
        let Some(connection) = doc
            .connections
            .iter()
            .find(|c| c.id == candidate.connection_id)
        else {
            continue;
        };
        let key = match store::get_credential(secrets, &connection.id) {
            Ok(Some(key)) => key,
            Ok(None) => String::new(),
            Err(e) => {
                reasons.push(format!("{}: keychain {e}", connection.id));
                continue;
            }
        };
        let bridge = matches!(candidate.reach, Reach::ViaGateway { .. });
        let speaks = match candidate.reach {
            Reach::Direct => config.harness.native_protocol(),
            Reach::ViaGateway { to, .. } => to,
        };
        match UpstreamTarget::with_details(
            format!("{TARGET_PREFIX}{}", connection.id),
            connection.label.clone(),
            &connection.base_url,
            key,
            vec![candidate.upstream_model.clone()],
            false,
            bridge,
        ) {
            Ok(target) => targets.push(target.with_upstream_protocol(speaks)),
            Err(e) => reasons.push(format!("{}: {}", connection.id, e.message)),
        }
    }
    if targets.is_empty() {
        Resolution::Unavailable(format!("no channel is usable: {}", reasons.join("; ")))
    } else {
        Resolution::Targets(targets)
    }
}

/// Load the document and resolve in one call (the router's entry point).
pub fn resolve_from_disk(
    data_dir: &std::path::Path,
    grant: &TaskGrant,
    model: &str,
    secrets: &Arc<dyn SecretStore>,
) -> Resolution {
    match store::load(&store::document_path(data_dir)) {
        Ok(doc) => resolve(&doc, grant, model, secrets.as_ref()),
        Err(e) => Resolution::Unavailable(format!("cannot read agent connections: {e}")),
    }
}

fn connection_of(target_id: &str) -> Option<&str> {
    target_id.strip_prefix(TARGET_PREFIX)
}

/// Record a successful attempt: clears the streak, refreshes stickiness, counts usage.
pub fn record_success(task_id: &str, target_id: &str, sticky_ttl_s: u64) {
    let Some(connection) = connection_of(target_id) else {
        return;
    };
    let now = now_epoch_s();
    with_state(|state| {
        state.streaks.remove(connection);
        state.cooldowns.remove(connection);
        *state
            .usage
            .requests
            .entry(connection.to_string())
            .or_insert(0) += 1;
        state.sticky.insert(
            task_id.to_string(),
            (connection.to_string(), now + sticky_ttl_s),
        );
    });
}

/// Record a failed attempt. Returns whether the router should try the next channel
/// (Q25: request errors such as 400/422 never fail over).
pub fn record_failure(target_id: &str, status: Option<u16>, retry_after_s: Option<u64>) -> bool {
    let class = status
        .map(FailureClass::from_status)
        .unwrap_or(FailureClass::Network);
    let Some(connection) = connection_of(target_id) else {
        return class.should_failover();
    };
    if class.should_failover() {
        let now = now_epoch_s();
        with_state(|state| {
            let streak = state.streaks.entry(connection.to_string()).or_insert(0);
            let cooldown = class.cooldown_seconds(retry_after_s, *streak);
            *streak = streak.saturating_add(1);
            if cooldown > 0 {
                state
                    .cooldowns
                    .insert(connection.to_string(), now + cooldown);
            }
        });
    }
    class.should_failover()
}

/// Drop a finished task's stickiness.
pub fn forget_task(task_id: &str) {
    with_state(|state| {
        state.sticky.remove(task_id);
    });
}

/// Snapshot of current cooldowns for the UI (connection id → epoch s).
pub fn cooldowns() -> Cooldowns {
    with_state(|state| state.cooldowns.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_connections::model::*;
    use crate::context::MemorySecrets;

    fn connection(id: &str, protocol: WireProtocol, upstream: &str) -> Connection {
        Connection {
            id: id.into(),
            label: id.into(),
            kind: ConnectionKind::ApiKey,
            vendor: Vendor::Other,
            base_url: format!("https://{id}.example/v1"),
            protocols: vec![protocol],
            use_global_proxy: false,
            models: vec![ModelEntry {
                upstream_id: upstream.into(),
                mapped_name: "deepseek-v4.1-flash".into(),
                display_name: String::new(),
                source: ModelSource::Manual,
                context_window: None,
                max_output: None,
                inputs: vec![],
                reasoning_tiers: vec![],
                enabled: true,
            }],
            catalog_revision: 1,
            enabled: true,
        }
    }

    fn doc(harness: Harness) -> ConnectionsDocument {
        doc_with(harness, "cr")
    }

    /// Cooldowns are process-global, so a test that records failures uses its
    /// own connection ids and cannot reorder another test's candidates.
    fn doc_with(harness: Harness, prefix: &str) -> ConnectionsDocument {
        ConnectionsDocument {
            version: DOCUMENT_VERSION,
            connections: vec![
                connection(&format!("{prefix}-a"), WireProtocol::Responses, "ds-a"),
                connection(
                    &format!("{prefix}-b"),
                    WireProtocol::ChatCompletions,
                    "ds-b",
                ),
                connection(&format!("{prefix}-m"), WireProtocol::Messages, "ds-m"),
            ],
            pools: vec![],
            configs: vec![AgentConfig {
                id: "cfg".into(),
                label: "cfg".into(),
                harness,
                target: ConfigTarget::Route { pool_id: None },
                default_model: "deepseek-v4.1-flash".into(),
                default_reasoning: String::new(),
            }],
        }
    }

    fn grant(task: &str) -> TaskGrant {
        TaskGrant {
            task_id: task.into(),
            agent_config_id: Some("cfg".into()),
        }
    }

    fn ids(resolution: Resolution) -> Vec<String> {
        match resolution {
            Resolution::Targets(t) => t.iter().map(|t| t.id().to_string()).collect(),
            Resolution::Unavailable(reason) => panic!("unavailable: {reason}"),
        }
    }

    #[test]
    fn codex_gets_direct_bridged_and_pivoted_channels() {
        // 4d: a Messages-only channel is reached through the Chat pivot.
        let secrets = MemorySecrets::default();
        let got = ids(resolve(&doc(Harness::Codex), &grant("cr-t1"), "", &secrets));
        assert_eq!(got, vec!["conn:cr-a", "conn:cr-b", "conn:cr-m"]);
    }

    #[test]
    fn claude_gets_messages_bridged_chat_and_pivoted_responses() {
        // 4c bridges Messages → Chat Completions; 4d pivots Messages → Responses.
        let secrets = MemorySecrets::default();
        let got = ids(resolve(
            &doc(Harness::Claude),
            &grant("cr-t2"),
            "deepseek-v4.1-flash",
            &secrets,
        ));
        assert_eq!(got, vec!["conn:cr-a", "conn:cr-b", "conn:cr-m"]);
    }

    #[test]
    fn a_failed_channel_cools_down_and_request_errors_do_not() {
        let secrets = MemorySecrets::default();
        let cooled = doc_with(Harness::Codex, "cool");
        assert!(
            !record_failure("conn:cool-a", Some(422), None),
            "422 never fails over"
        );
        assert!(record_failure("conn:cool-a", Some(503), None));
        let got = ids(resolve(&cooled, &grant("cr-t3"), "", &secrets));
        assert_eq!(
            got.last().map(String::as_str),
            Some("conn:cool-a"),
            "cooling goes last"
        );
        record_success("cr-t3", "conn:cool-a", 60);
        let got = ids(resolve(&cooled, &grant("cr-t3"), "", &secrets));
        assert_eq!(
            got.first().map(String::as_str),
            Some("conn:cool-a"),
            "sticky after success"
        );
        forget_task("cr-t3");
    }

    #[test]
    fn missing_config_is_reported_not_guessed() {
        let secrets = MemorySecrets::default();
        let mut g = grant("cr-t4");
        g.agent_config_id = Some("gone".into());
        assert!(matches!(
            resolve(&doc(Harness::Codex), &g, "", &secrets),
            Resolution::Unavailable(_)
        ));
    }
}
