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
        protocols: vec![],
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
        rest_on_limit: true,
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
        fallback_models: Vec::new(),
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
fn omp_speaks_messages_to_the_gateway_and_reaches_every_channel() {
    let doc = deepseek_doc();
    let config = route_config(Harness::Omp, None);
    assert_eq!(config_protocol(&doc, &config), WireProtocol::Messages);
    let (found, excluded) = candidates(&doc, &config, "deepseek-v4.1-flash");
    assert!(
        found.len() >= 2,
        "native and translated channels alike: {found:?}"
    );
    assert!(!excluded
        .iter()
        .any(|(_, why)| matches!(why, Unavailable::GatewayNotYetForHarness { .. })));
}

#[test]
fn omp_speaks_responses_only_when_every_channel_is_responses_only() {
    let mut doc = deepseek_doc();
    for connection in &mut doc.connections {
        connection.protocols = vec![WireProtocol::Responses];
    }
    assert_eq!(
        config_protocol(&doc, &route_config(Harness::Omp, None)),
        WireProtocol::Responses
    );
    doc.connections[0].protocols.push(WireProtocol::Messages);
    assert_eq!(
        config_protocol(&doc, &route_config(Harness::Omp, None)),
        WireProtocol::Messages
    );
    assert_eq!(
        config_protocol(&doc, &route_config(Harness::Codex, None)),
        WireProtocol::Responses
    );
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

mod routing_r6_r10 {
    use crate::agent_connections::model::*;
    use crate::agent_connections::routing::*;

    fn candidate(id: &str, priority: i32, weight: u32) -> Candidate {
        Candidate {
            connection_id: id.into(),
            upstream_model: "m".into(),
            reach: Reach::Direct,
            priority,
            weight,
            free: false,
        }
    }

    fn first(
        strategy: PoolStrategy,
        list: &[Candidate],
        usage: &mut UsageCounters,
        cooldowns: &Cooldowns,
    ) -> String {
        order(
            list.to_vec(),
            strategy,
            "route",
            None,
            cooldowns,
            usage,
            1_000,
        )[0]
        .connection_id
        .clone()
    }

    #[test]
    fn smooth_weighted_round_robin_spreads_by_weight() {
        let list = [
            candidate("a", 0, 5),
            candidate("b", 0, 1),
            candidate("c", 0, 1),
        ];
        let mut usage = UsageCounters::default();
        let picks: Vec<String> = (0..7)
            .map(|_| {
                first(
                    PoolStrategy::WeightedRoundRobin,
                    &list,
                    &mut usage,
                    &Cooldowns::new(),
                )
            })
            .collect();
        assert_eq!(picks.iter().filter(|p| *p == "a").count(), 5);
        assert_eq!(picks.iter().filter(|p| *p == "b").count(), 1);
        // Smooth: the heavy member is not taken five times in a row.
        assert_ne!(picks[..5], ["a", "a", "a", "a", "a"]);
    }

    #[test]
    fn round_robin_stays_inside_the_best_ready_priority() {
        let list = [
            candidate("a", 0, 1),
            candidate("b", 0, 1),
            candidate("c", 1, 1),
        ];
        let mut usage = UsageCounters::default();
        let picks: Vec<String> = (0..4)
            .map(|_| {
                first(
                    PoolStrategy::RoundRobin,
                    &list,
                    &mut usage,
                    &Cooldowns::new(),
                )
            })
            .collect();
        assert!(!picks.contains(&"c".to_string()), "{picks:?}");
        // With the top bucket cooling, the next priority serves.
        let mut cooling = Cooldowns::new();
        cooling.insert("a".into(), 2_000);
        cooling.insert("b".into(), 2_000);
        assert_eq!(
            first(PoolStrategy::RoundRobin, &list, &mut usage, &cooling),
            "c"
        );
    }

    #[test]
    fn a_model_cooldown_puts_only_that_model_last() {
        let mut cooldowns = Cooldowns::new();
        cooldowns.insert(model_key("a", "m"), 2_000);
        let list = [candidate("a", 0, 1), candidate("b", 1, 1)];
        let mut usage = UsageCounters::default();
        assert_eq!(
            first(PoolStrategy::Priority, &list, &mut usage, &cooldowns),
            "b"
        );
    }

    #[test]
    fn stream_errors_are_classified_by_kind() {
        use FailureClass::*;
        for (summary, class) in [
            (
                "Responses upstream context_length_exceeded: too long",
                RequestError,
            ),
            (
                "Anthropic upstream invalid_request_error: prompt is too long",
                RequestError,
            ),
            ("Responses upstream content_filter: blocked", RequestError),
            (
                "Responses upstream rate_limit_exceeded: slow down",
                RateLimited,
            ),
            (
                "Responses upstream insufficient_quota: out",
                PaymentRequired,
            ),
            (
                "Anthropic upstream authentication_error: bad key",
                AuthFailed,
            ),
            ("Responses upstream model_not_found: gone", NotFound),
            (
                "Responses upstream server_error: stream overloaded",
                ServerError,
            ),
        ] {
            assert_eq!(FailureClass::from_semantic(summary, ""), class, "{summary}");
        }
        assert_eq!(FailureClass::from_status(402), PaymentRequired);
        assert_eq!(PaymentRequired.cooldown_seconds(None, 0), 1800);
        assert_eq!(FirstByteTimeout.cooldown_seconds(None, 0), 0);
        assert_eq!(FirstByteTimeout.cooldown_seconds(None, 2), 120);
    }

    #[test]
    fn a_model_may_override_its_connections_protocols() {
        let connection = Connection {
            id: "agg".into(),
            label: "agg".into(),
            kind: ConnectionKind::ApiKey,
            vendor: Vendor::Other,
            base_url: "https://agg.example/v1".into(),
            protocols: vec![WireProtocol::ChatCompletions],
            use_global_proxy: false,
            models: vec![],
            catalog_revision: 1,
            enabled: true,
            rest_on_limit: true,
        };
        let entry = ModelEntry {
            upstream_id: "claude-x".into(),
            mapped_name: String::new(),
            display_name: String::new(),
            source: ModelSource::Manual,
            context_window: None,
            max_output: None,
            inputs: vec![],
            reasoning_tiers: vec![],
            protocols: vec![WireProtocol::Messages],
            enabled: true,
        };
        assert_eq!(
            reach_protocols(Harness::Claude, model_protocols(&connection, &entry)),
            Ok(Reach::Direct)
        );
        assert!(matches!(
            reach(Harness::Claude, &connection),
            Ok(Reach::ViaGateway { .. })
        ));
    }
}

/// 2026-10-27: saved connections are offered in the new-session picker as
/// derived `via-<harness>-<id>` configurations, never stored.
#[test]
fn every_saved_connection_is_offered_per_gateway_harness() {
    let mut doc = deepseek_doc();
    let derived = doc.connection_configs();
    let via = ConnectionsDocument::via_id;
    let ids: Vec<String> = derived.iter().map(|c| c.id.clone()).collect();
    // 2026-10-29: every harness, 「全部第三方模型」 first, then per connection.
    const ALL: [Harness; 4] = [Harness::Codex, Harness::Claude, Harness::Omp, Harness::Dsh];
    let mut expected: Vec<String> = ALL
        .iter()
        .map(|h| ConnectionsDocument::via_all_id(*h))
        .collect();
    for id in ["a", "b", "c"] {
        expected.extend(ALL.iter().map(|h| via(*h, id)));
    }
    assert_eq!(ids, expected);
    let all = doc
        .config(&ConnectionsDocument::via_all_id(Harness::Dsh))
        .expect("resolvable");
    assert_eq!(all.target, ConfigTarget::Route { pool_id: None });
    assert_eq!(all.label, "全部第三方模型");
    for id in ids.iter().filter(|id| !id.ends_with("-all")) {
        // `via-<harness>-<16 hex>`: the connection id itself is not exposed.
        let hash = id.rsplit('-').next().unwrap();
        assert!(
            hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{id}"
        );
    }
    let codex_a = doc
        .config(&via(Harness::Codex, "a"))
        .expect("resolvable by id");
    assert_eq!(codex_a.harness, Harness::Codex);
    assert_eq!(codex_a.label, "a");
    assert_eq!(codex_a.default_model, "deepseek-v4.1-flash");
    assert_eq!(
        codex_a.target,
        ConfigTarget::Connection {
            connection_id: "a".into()
        }
    );
    assert!(doc.configs.is_empty(), "nothing is written");

    // Gone with the connection, never for a typo; omp gets them too since it
    // reaches the gateway (2026-10-27), and dsh since 2026-10-29.
    assert_eq!(
        doc.config(&via(Harness::Omp, "b")).map(|c| c.harness),
        Some(Harness::Omp)
    );
    doc.connections[0].enabled = false;
    assert!(doc.config(&via(Harness::Codex, "a")).is_none());
    assert_eq!(
        doc.config(&via(Harness::Dsh, "b")).map(|c| c.harness),
        Some(Harness::Dsh)
    );
    assert!(doc.config("via-codex-missing").is_none());
    assert!(doc.config("via-").is_none());
    // A connection without a routable model offers nothing.
    doc.connections[1].models.clear();
    assert!(doc.config(&via(Harness::Claude, "b")).is_none());
    // A stored configuration still wins over the derived namespace.
    doc.configs.push(AgentConfig {
        id: via(Harness::Codex, "c"),
        label: "mine".into(),
        ..route_config(Harness::Codex, None)
    });
    assert_eq!(doc.config(&via(Harness::Codex, "c")).unwrap().label, "mine");
}
