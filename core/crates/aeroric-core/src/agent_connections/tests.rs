use super::model::*;
use super::routing::*;
use super::store;
use crate::context::MemorySecrets;

fn model(upstream: &str, mapped: &str) -> ModelEntry {
    ModelEntry {
        upstream_id: upstream.into(),
        mapped_name: mapped.into(),
        display_name: String::new(),
        source: ModelSource::Manual,
        context_window: None,
        max_output: None,
        inputs: vec![InputKind::Text],
        reasoning_tiers: vec![],
        enabled: true,
    }
}

fn conn(id: &str, protocols: Vec<WireProtocol>, models: Vec<ModelEntry>) -> Connection {
    Connection {
        id: id.into(),
        label: id.into(),
        kind: ConnectionKind::ApiKey,
        vendor: Vendor::Other,
        base_url: format!("https://{id}.example"),
        protocols,
        use_global_proxy: false,
        models,
        catalog_revision: 1,
        enabled: true,
    }
}

fn route_config(harness: Harness, pool_id: Option<&str>) -> AgentConfig {
    AgentConfig {
        id: "cfg".into(),
        label: "cfg".into(),
        harness,
        target: ConfigTarget::Route {
            pool_id: pool_id.map(str::to_string),
        },
        default_model: "deepseek-v4.1-flash".into(),
        default_reasoning: String::new(),
    }
}

/// The user's example: several APIs all map a model to `deepseek-v4.1-flash`.
fn deepseek_doc() -> ConnectionsDocument {
    ConnectionsDocument {
        version: DOCUMENT_VERSION,
        connections: vec![
            conn(
                "a",
                vec![WireProtocol::ChatCompletions],
                vec![model("deepseek-chat", "deepseek-v4.1-flash")],
            ),
            conn(
                "b",
                vec![WireProtocol::Messages],
                vec![model("ds-flash-anthropic", "deepseek-v4.1-flash")],
            ),
            conn(
                "c",
                vec![WireProtocol::Responses],
                vec![model("gpt-x", "gpt-x")],
            ),
        ],
        pools: vec![],
        configs: vec![],
    }
}

#[test]
fn mapped_name_gathers_every_channel_and_translates_protocols() {
    let doc = deepseek_doc();
    let (found, excluded) = candidates(
        &doc,
        &route_config(Harness::Claude, None),
        "deepseek-v4.1-flash",
    );
    let ids: Vec<_> = found.iter().map(|c| c.connection_id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(
        found[1].reach,
        Reach::Direct,
        "Claude speaks Messages natively"
    );
    assert_eq!(
        found[0].reach,
        Reach::ViaGateway {
            from: WireProtocol::Messages,
            to: WireProtocol::ChatCompletions
        }
    );
    assert_eq!(
        found[0].upstream_model, "deepseek-chat",
        "upstream id is what is sent"
    );
    assert!(excluded
        .iter()
        .any(|(id, why)| id == "c" && matches!(why, Unavailable::ModelNotOffered { .. })));
}

#[test]
fn omp_cannot_use_a_translated_channel_yet() {
    let doc = deepseek_doc();
    let (found, excluded) = candidates(
        &doc,
        &route_config(Harness::Omp, None),
        "deepseek-v4.1-flash",
    );
    assert_eq!(found.len(), 1, "only the native Chat Completions channel");
    assert!(excluded.iter().any(|(id, why)| id == "b"
        && *why
            == Unavailable::GatewayNotYetForHarness {
                harness: Harness::Omp
            }));
}

#[test]
fn cooling_channels_move_last_but_stay_reachable() {
    let doc = deepseek_doc();
    let (found, _) = candidates(
        &doc,
        &route_config(Harness::Claude, None),
        "deepseek-v4.1-flash",
    );
    let cooldowns = Cooldowns::from([("a".to_string(), 1_000)]);
    let mut usage = UsageCounters::default();
    let ordered = order(
        found,
        PoolStrategy::Priority,
        "r",
        None,
        &cooldowns,
        &mut usage,
        500,
    );
    let ids: Vec<_> = ordered.iter().map(|c| c.connection_id.as_str()).collect();
    assert_eq!(ids, ["b", "a"]);
}

#[test]
fn sticky_session_wins_over_strategy_order() {
    let doc = deepseek_doc();
    let (found, _) = candidates(
        &doc,
        &route_config(Harness::Claude, None),
        "deepseek-v4.1-flash",
    );
    let mut usage = UsageCounters::default();
    let ordered = order(
        found,
        PoolStrategy::Priority,
        "r",
        Some("b"),
        &Cooldowns::new(),
        &mut usage,
        0,
    );
    assert_eq!(ordered[0].connection_id, "b");
}

#[test]
fn round_robin_rotates_across_calls() {
    let doc = deepseek_doc();
    let cfg = route_config(Harness::Claude, None);
    let mut usage = UsageCounters::default();
    let mut firsts = Vec::new();
    for _ in 0..4 {
        let (found, _) = candidates(&doc, &cfg, "deepseek-v4.1-flash");
        let ordered = order(
            found,
            PoolStrategy::RoundRobin,
            "r",
            None,
            &Cooldowns::new(),
            &mut usage,
            0,
        );
        firsts.push(ordered[0].connection_id.clone());
    }
    assert_eq!(firsts, ["a", "b", "a", "b"]);
}

#[test]
fn pool_priority_and_free_first() {
    let mut doc = deepseek_doc();
    doc.pools.push(Pool {
        id: "p".into(),
        label: "p".into(),
        strategy: PoolStrategy::FreeFirst,
        members: vec![
            PoolMember {
                connection_id: "a".into(),
                priority: 0,
                weight: 1,
                free: false,
            },
            PoolMember {
                connection_id: "b".into(),
                priority: 5,
                weight: 1,
                free: true,
            },
        ],
        sticky_sessions: true,
        sticky_ttl_seconds: 3600,
    });
    let cfg = AgentConfig {
        target: ConfigTarget::Pool {
            pool_id: "p".into(),
        },
        ..route_config(Harness::Claude, None)
    };
    let (found, _) = candidates(&doc, &cfg, "deepseek-v4.1-flash");
    let mut usage = UsageCounters::default();
    let ordered = order(
        found,
        PoolStrategy::FreeFirst,
        "p",
        None,
        &Cooldowns::new(),
        &mut usage,
        0,
    );
    assert_eq!(
        ordered[0].connection_id, "b",
        "free member first despite lower priority"
    );
}

#[test]
fn request_errors_never_fail_over() {
    for status in [400, 422] {
        assert!(!FailureClass::from_status(status).should_failover());
    }
    for status in [429, 401, 500, 503] {
        assert!(FailureClass::from_status(status).should_failover());
    }
    assert_eq!(FailureClass::RateLimited.cooldown_seconds(Some(3), 0), 10);
    assert_eq!(FailureClass::AuthFailed.cooldown_seconds(None, 0), 1800);
    assert_eq!(FailureClass::NotFound.cooldown_seconds(None, 0), 43_200);
}

#[test]
fn offered_models_are_deduplicated_mapped_names() {
    let doc = deepseek_doc();
    assert_eq!(
        offered_models(&doc, &route_config(Harness::Claude, None)),
        ["deepseek-v4.1-flash", "gpt-x"]
    );
}

#[test]
fn validation_rejects_ambiguous_mapping_and_dangling_refs() {
    let mut doc = deepseek_doc();
    doc.connections[0]
        .models
        .push(model("other", "deepseek-v4.1-flash"));
    assert!(store::validate(&doc).unwrap_err().contains("twice"));

    let mut doc = deepseek_doc();
    doc.configs.push(AgentConfig {
        target: ConfigTarget::Pool {
            pool_id: "nope".into(),
        },
        ..route_config(Harness::Claude, None)
    });
    assert!(store::validate(&doc).is_err());
}

#[test]
fn document_round_trips_without_credentials() {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-agent-connections-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = store::document_path(&dir);
    assert!(store::load(&path).unwrap().connections.is_empty());
    let doc = deepseek_doc();
    store::save(&path, &doc).unwrap();
    assert_eq!(store::load(&path).unwrap(), doc);

    let secrets = MemorySecrets::default();
    store::set_credential(&secrets, "a", "sk-test").unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("sk-test"));
    assert_eq!(
        store::get_credential(&secrets, "a").unwrap().as_deref(),
        Some("sk-test")
    );
    store::delete_credential(&secrets, "a").unwrap();
    assert_eq!(store::get_credential(&secrets, "a").unwrap(), None);
}
