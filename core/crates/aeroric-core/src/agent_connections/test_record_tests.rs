use super::*;
use crate::agent_connections::model::{ConnectionKind, Vendor};

fn connection(proxy: bool) -> Connection {
    Connection {
        id: "c1".into(),
        label: "C1".into(),
        kind: ConnectionKind::ApiKey,
        vendor: Vendor::Other,
        base_url: "https://api.example/v1/".into(),
        protocols: vec![WireProtocol::ChatCompletions, WireProtocol::Messages],
        use_global_proxy: proxy,
        models: vec![],
        catalog_revision: 1,
        enabled: true,
        rest_on_limit: true,
    }
}

fn global() -> ProxySettings {
    ProxySettings {
        url: "http://me:secret@127.0.0.1:7890".into(),
        no_proxy: "corp.example".into(),
        username: "me".into(),
        password: "secret".into(),
    }
}

fn data_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "aeroric-c5-tests-{}",
        uuid::Uuid::new_v4().simple()
    ))
}

fn record(
    connection: &Connection,
    secret: &str,
    global: &ProxySettings,
    status: u16,
) -> TestRecord {
    TestRecord {
        connection_id: connection.id.clone(),
        model: "m1".into(),
        at_ms: 5,
        status,
        elapsed_ms: 120,
        excerpt: "pong".into(),
        egress: Egress::of(connection, test_protocol(connection), secret, global).unwrap(),
    }
}

/// The exit is described without any secret: the proxy loses its user and
/// password, the key is a 12-digit fingerprint.
#[test]
fn the_exit_keeps_no_secret() {
    let egress = Egress::of(
        &connection(true),
        "chat_completions",
        "sk-live-123",
        &global(),
    )
    .unwrap();
    assert_eq!(egress.base_url, "https://api.example/v1");
    assert_eq!(egress.proxy, "http://127.0.0.1:7890");
    assert_eq!(egress.proxy_user, "me");
    assert_eq!(egress.credential.len(), 12);
    let text = serde_json::to_string(&egress).unwrap();
    assert!(
        !text.contains("secret") && !text.contains("sk-live"),
        "{text}"
    );
    assert_eq!(
        Egress::of(&connection(false), "chat_completions", "k", &global())
            .unwrap()
            .proxy,
        "",
        "not opted in: direct"
    );
}

/// A kept test reads as current until the exit changes; then it says what
/// changed — the endpoint, the proxy, the key or the protocol.
#[test]
fn a_test_goes_stale_when_its_exit_changes_and_says_why() {
    let dir = data_dir();
    let mut c = connection(true);
    save(&dir, record(&c, "sk-1", &global(), 200)).unwrap();
    let key = |_: &str| Some("sk-1".to_string());

    let now = views(&dir, std::slice::from_ref(&c), &global(), key);
    assert_eq!(now.len(), 1);
    assert!(now[0].passed);
    assert_eq!(now[0].stale, None);

    let stale = |c: &Connection, global: &ProxySettings, secret: &'static str| {
        views(&dir, std::slice::from_ref(c), global, move |_| {
            Some(secret.to_string())
        })[0]
            .stale
            .clone()
    };
    assert_eq!(
        stale(&c, &global(), "sk-2").as_deref(),
        Some("配置已变，需要重测（凭据换过）")
    );
    let other_proxy = ProxySettings {
        url: "http://127.0.0.1:9999".into(),
        ..global()
    };
    assert_eq!(
        stale(&c, &other_proxy, "sk-1").as_deref(),
        Some("配置已变，需要重测（代理设置改过）")
    );
    assert!(stale(&c, &ProxySettings::default(), "sk-1")
        .unwrap()
        .contains("Global proxy is not configured"));
    c.use_global_proxy = false;
    assert_eq!(
        stale(&c, &global(), "sk-1").as_deref(),
        Some("配置已变，需要重测（代理设置改过）"),
        "now direct"
    );
    c.use_global_proxy = true;
    c.base_url = "https://other.example/v1".into();
    assert_eq!(
        stale(&c, &global(), "sk-1").as_deref(),
        Some("配置已变，需要重测（地址改过）")
    );
    c.base_url = "https://api.example/v1".into();
    c.protocols = vec![WireProtocol::Responses];
    assert_eq!(
        stale(&c, &global(), "sk-1").as_deref(),
        Some("配置已变，需要重测（协议改过）")
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// One record per connection and model, the latest kept; a deleted
/// connection's tests go, and tests of connections no longer there are
/// not shown.
#[test]
fn the_latest_test_per_model_is_kept_and_a_deleted_connection_forgets() {
    let dir = data_dir();
    let c = connection(false);
    save(&dir, record(&c, "k", &global(), 500)).unwrap();
    save(&dir, record(&c, "k", &global(), 200)).unwrap();
    let mut other = record(&c, "k", &global(), 401);
    other.model = "m2".into();
    save(&dir, other).unwrap();
    let all = views(&dir, std::slice::from_ref(&c), &global(), |_| {
        Some("k".into())
    });
    let seen: Vec<(String, u16, bool)> = all
        .iter()
        .map(|v| (v.record.model.clone(), v.record.status, v.passed))
        .collect();
    assert_eq!(seen, [("m1".into(), 200, true), ("m2".into(), 401, false)]);
    assert!(views(&dir, &[], &global(), |_| None).is_empty());
    forget(&dir, "c1").unwrap();
    assert!(views(&dir, std::slice::from_ref(&c), &global(), |_| None).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
