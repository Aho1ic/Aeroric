//! Routing for tasks pinned to an agent configuration (B1 step 4b, Q23–Q34).
//!
//! A request carrying a task token whose grant names an agent configuration gets
//! its targets from `agent-connections.json` instead of the legacy settings list.
//! Resolution and ordering are the pure functions in `agent_connections::routing`;
//! this module only adds the process-global state they read:
//! - cooldowns, keyed by connection id or (connection, model) and shared by
//!   every pool and config (Q31, R3), each remembering why it was set (R5);
//! - sticky sessions, keyed by task id (CPA session affinity), moved only after
//!   quota/auth-class errors (R4);
//! - usage counters for least-used / round-robin strategies.
//!
//! Translation the router cannot do yet is excluded with a reason rather than
//! sent and left to fail upstream: today only Responses → Chat Completions is
//! bridged (Messages translation lands in 4c/4d).

use std::collections::{HashMap, HashSet};
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
    /// Why each cooldown key was set, so editing a connection can clear only
    /// what an edit can fix (R5).
    reasons: HashMap<String, FailureClass>,
    /// Consecutive failures per cooldown key, for exponential cooldowns.
    streaks: HashMap<String, u32>,
    /// Slow first output per (connection, model): (count in window, last at epoch s).
    slow: HashMap<String, (u32, u64)>,
    /// task id → (connection id, expires at epoch s).
    sticky: HashMap<String, (String, u64)>,
    /// task id → sticky lifetime from its pool (None: the target is not sticky).
    sticky_ttl: HashMap<String, Option<u64>>,
    /// Tasks whose last failure may move their sticky member (R4).
    rebind: HashSet<String>,
    /// Connections whose 429s do not rest them (R9).
    no_rest_on_limit: HashSet<String>,
    usage: UsageCounters,
}

/// Slow starts inside this window count towards a model cooldown (R3).
const SLOW_WINDOW_S: u64 = 30 * 60;

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
        state.sticky_ttl.insert(grant.task_id.clone(), sticky_ttl);
        for connection in &doc.connections {
            if connection.rest_on_limit {
                state.no_rest_on_limit.remove(&connection.id);
            } else {
                state.no_rest_on_limit.insert(connection.id.clone());
            }
        }
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

/// Record a successful attempt: clears the connection's streak and cooldown and
/// counts usage. Stickiness moves to this member only when the task had none,
/// it expired, it already pointed here, or the failure before allowed a rebind
/// (R4) — a transient error elsewhere must not drag the session (and its prompt
/// cache) to whichever member happened to answer.
pub fn record_success(task_id: &str, target_id: &str) {
    let Some(connection) = connection_of(target_id) else {
        return;
    };
    let now = now_epoch_s();
    with_state(|state| {
        state.streaks.remove(connection);
        state.cooldowns.remove(connection);
        state.reasons.remove(connection);
        *state
            .usage
            .requests
            .entry(connection.to_string())
            .or_insert(0) += 1;
        let rebind = state.rebind.remove(task_id);
        let Some(Some(ttl)) = state.sticky_ttl.get(task_id).copied() else {
            return;
        };
        let current = state
            .sticky
            .get(task_id)
            .filter(|(_, until)| *until > now)
            .map(|(id, _)| id.clone());
        if current.is_none() || rebind || current.as_deref() == Some(connection) {
            state
                .sticky
                .insert(task_id.to_string(), (connection.to_string(), now + ttl));
        }
    });
}

/// Record a failed attempt of `upstream_model` on `target_id`. Returns whether
/// the router should try the next channel (request errors never fail over and
/// never cool anything down).
pub fn record_failure(
    task_id: &str,
    target_id: &str,
    upstream_model: &str,
    class: FailureClass,
    retry_after_s: Option<u64>,
) -> bool {
    if !class.should_failover() {
        return false;
    }
    let Some(connection) = connection_of(target_id) else {
        return true;
    };
    let now = now_epoch_s();
    with_state(|state| {
        if class.allows_rebind() {
            state.rebind.insert(task_id.to_string());
        }
        let key = if class.cools_model() {
            routing::model_key(connection, upstream_model)
        } else {
            connection.to_string()
        };
        let cooldown = match class {
            FailureClass::FirstByteTimeout => {
                let entry = state.slow.entry(key.clone()).or_insert((0, 0));
                entry.0 = if now.saturating_sub(entry.1) <= SLOW_WINDOW_S {
                    entry.0 + 1
                } else {
                    1
                };
                entry.1 = now;
                class.cooldown_seconds(None, entry.0 - 1)
            }
            FailureClass::RateLimited if state.no_rest_on_limit.contains(connection) => 0,
            _ => {
                let streak = state.streaks.entry(key.clone()).or_insert(0);
                let cooldown = class.cooldown_seconds(retry_after_s, *streak);
                *streak = streak.saturating_add(1);
                cooldown
            }
        };
        if cooldown > 0 {
            state.cooldowns.insert(key.clone(), now + cooldown);
            state.reasons.insert(key, class);
        }
    });
    true
}

/// A connection's key or settings changed (R5): forget what the edit can fix —
/// auth, credit and missing-model cooldowns on it and its models — but keep
/// rate-limit rests, which an edit does not cure.
pub fn connection_changed(connection_id: &str) {
    let model_prefix = routing::model_key(connection_id, "");
    with_state(|state| {
        let cleared: Vec<String> = state
            .reasons
            .iter()
            .filter(|(key, class)| {
                (key.as_str() == connection_id || key.starts_with(&model_prefix))
                    && class.cleared_by_edit()
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in cleared {
            state.cooldowns.remove(&key);
            state.reasons.remove(&key);
            state.streaks.remove(&key);
        }
    });
}

/// Parse `Retry-After`: delta seconds or an HTTP date (R1).
pub fn parse_retry_after(value: &str, now_epoch_s: u64) -> Option<u64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let at = chrono::DateTime::parse_from_rfc2822(value)
        .ok()?
        .timestamp();
    Some(u64::try_from(at).ok()?.saturating_sub(now_epoch_s))
}

/// Drop a finished task's stickiness.
pub fn forget_task(task_id: &str) {
    with_state(|state| {
        state.sticky.remove(task_id);
        state.sticky_ttl.remove(task_id);
        state.rebind.remove(task_id);
    });
}

/// One live cooldown, for the pool page (G): which connection (and model),
/// until when, and why.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CooldownEntry {
    pub connection_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub until_epoch_s: u64,
    pub reason: String,
}

/// Cooldowns still running, soonest-ending first.
pub fn cooldown_report() -> Vec<CooldownEntry> {
    let now = now_epoch_s();
    let mut entries: Vec<CooldownEntry> = with_state(|state| {
        state
            .cooldowns
            .iter()
            .filter(|(_, until)| **until > now)
            .map(|(key, until)| {
                let (connection, model) = match key.split_once('\u{1f}') {
                    Some((connection, model)) => (connection.to_string(), Some(model.to_string())),
                    None => (key.clone(), None),
                };
                let reason = state
                    .reasons
                    .get(key)
                    .map(|class| format!("{class:?}"))
                    .unwrap_or_else(|| "Unknown".into());
                CooldownEntry {
                    connection_id: connection,
                    model,
                    until_epoch_s: *until,
                    reason,
                }
            })
            .collect()
    });
    entries.sort_by_key(|e| e.until_epoch_s);
    entries
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
                protocols: vec![],
            }],
            catalog_revision: 1,
            enabled: true,
            rest_on_limit: true,
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
            !record_failure(
                "cr-t3",
                "conn:cool-a",
                "ds-a",
                FailureClass::from_status(422),
                None
            ),
            "422 never fails over"
        );
        assert!(record_failure(
            "cr-t3",
            "conn:cool-a",
            "ds-a",
            FailureClass::from_status(503),
            None
        ));
        let got = ids(resolve(&cooled, &grant("cr-t3"), "", &secrets));
        assert_eq!(
            got.last().map(String::as_str),
            Some("conn:cool-a"),
            "cooling goes last"
        );
        record_success("cr-t3", "conn:cool-a");
        let got = ids(resolve(&cooled, &grant("cr-t3"), "", &secrets));
        assert_eq!(
            got.first().map(String::as_str),
            Some("conn:cool-a"),
            "sticky after success"
        );
        forget_task("cr-t3");
    }

    #[test]
    fn a_missing_model_cools_the_model_not_the_connection() {
        let now = now_epoch_s();
        assert!(record_failure(
            "cr-t5",
            "conn:nf-a",
            "ds-gone",
            FailureClass::NotFound,
            None
        ));
        let table = cooldowns();
        assert!(!table.contains_key("nf-a"), "the connection stays usable");
        assert!(table[&routing::model_key("nf-a", "ds-gone")] > now + 11 * 3600);
        // R5: editing the connection clears it; a rate-limit rest would stay.
        record_failure(
            "cr-t5",
            "conn:nf-a",
            "ds-x",
            FailureClass::RateLimited,
            Some(120),
        );
        connection_changed("nf-a");
        let table = cooldowns();
        assert!(!table.contains_key(&routing::model_key("nf-a", "ds-gone")));
        assert!(table.get("nf-a").is_some_and(|until| *until > now + 100));
    }

    #[test]
    fn the_report_names_the_connection_model_and_reason() {
        record_failure(
            "cr-t9",
            "conn:rep-a",
            "ds-gone",
            FailureClass::NotFound,
            None,
        );
        let entry = cooldown_report()
            .into_iter()
            .find(|e| e.connection_id == "rep-a")
            .unwrap();
        assert_eq!(entry.model.as_deref(), Some("ds-gone"));
        assert_eq!(entry.reason, "NotFound");
        assert!(entry.until_epoch_s > now_epoch_s());
    }

    #[test]
    fn a_slow_start_switches_first_and_cools_the_second_time() {
        let key = routing::model_key("slow-a", "ds-a");
        assert!(record_failure(
            "cr-t6",
            "conn:slow-a",
            "ds-a",
            FailureClass::FirstByteTimeout,
            None
        ));
        assert!(
            !cooldowns().contains_key(&key),
            "first slow start only switches"
        );
        record_failure(
            "cr-t6",
            "conn:slow-a",
            "ds-a",
            FailureClass::FirstByteTimeout,
            None,
        );
        let until = cooldowns()[&key];
        assert!(until >= now_epoch_s() + 59 && until <= now_epoch_s() + 61);
    }

    #[test]
    fn stickiness_moves_only_after_quota_or_auth_errors() {
        let secrets = MemorySecrets::default();
        let doc = doc_with(Harness::Codex, "stk");
        let task = "cr-t7";
        let _ = resolve(&doc, &grant(task), "", &secrets);
        record_success(task, "conn:stk-a");
        // A transient failure answered elsewhere must not move the session.
        record_failure(task, "conn:stk-a", "ds-a", FailureClass::ServerError, None);
        record_success(task, "conn:stk-b");
        with_state(|s| assert_eq!(s.sticky[task].0, "stk-a"));
        // A rate limit does.
        record_failure(
            task,
            "conn:stk-a",
            "ds-a",
            FailureClass::RateLimited,
            Some(30),
        );
        record_success(task, "conn:stk-b");
        with_state(|s| assert_eq!(s.sticky[task].0, "stk-b"));
        forget_task(task);
    }

    #[test]
    fn a_relay_that_does_not_rest_on_limits_is_not_cooled() {
        let secrets = MemorySecrets::default();
        let mut doc = doc_with(Harness::Codex, "relay");
        doc.connections[0].rest_on_limit = false;
        let _ = resolve(&doc, &grant("cr-t8"), "", &secrets);
        assert!(record_failure(
            "cr-t8",
            "conn:relay-a",
            "ds-a",
            FailureClass::RateLimited,
            Some(60)
        ));
        assert!(!cooldowns().contains_key("relay-a"));
        forget_task("cr-t8");
    }

    #[test]
    fn retry_after_takes_seconds_or_an_http_date() {
        assert_eq!(parse_retry_after(" 30 ", 0), Some(30));
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2026 07:28:00 GMT")
            .unwrap()
            .timestamp() as u64;
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 07:30:00 GMT", now),
            Some(120)
        );
        assert_eq!(parse_retry_after("soon", now), None);
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
