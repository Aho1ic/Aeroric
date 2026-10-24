//! B1 4b end to end: a task pinned to an agent configuration routes by mapped name,
//! fails over on a 503 and cools the failed channel down, but never fails over on a
//! request error (Q25).
//!
//! Its own test binary because it sets process globals (data dir, secret store) that
//! the router reads; inside the lib test binary they would leak into parallel tests.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use aeroric_core::agent_connections::model::ConnectionsDocument;
use aeroric_core::agent_connections::store;
use aeroric_core::context::MemorySecrets;
use aeroric_core::local_router::{
    config_routes, task_tokens, LocalRouterState, RouterRuntimeConfig, RouterUpstreams,
    ROUTER_TOKEN_HEADER,
};
use axum::http::StatusCode;
use axum::routing::any;
use axum::{Json, Router};
use serde_json::{json, Value};

type Seen = Arc<Mutex<Vec<String>>>;

/// A mock upstream answering `status`, recording the model of each request.
async fn upstream(status: StatusCode) -> (SocketAddr, Seen) {
    let seen: Seen = Arc::default();
    let record = Arc::clone(&seen);
    let app = Router::new().fallback(any(move |Json(body): Json<Value>| {
        let record = Arc::clone(&record);
        async move {
            record
                .lock()
                .unwrap()
                .push(body["model"].as_str().unwrap_or_default().to_string());
            (
                status,
                Json(json!({"id":"resp","model":body["model"],"output":[],"usage":{}})),
            )
        }
    }));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (address, seen)
}

fn connection(id: &str, address: SocketAddr, upstream_id: &str) -> Value {
    json!({
        "id": id, "label": id, "kind": "api_key", "vendor": "other",
        "base_url": format!("http://{address}/v1"),
        "protocols": ["responses"],
        "models": [{ "upstream_id": upstream_id, "mapped_name": "deepseek-v4.1-flash" }]
    })
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn send(base_url: &str, token: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base_url}/codex/v1/responses"))
        .header(ROUTER_TOKEN_HEADER, token)
        .json(&json!({"model":"deepseek-v4.1-flash","input":"hello"}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn mapped_name_fails_over_on_503_and_stops_on_422() {
    let root = std::env::temp_dir().join(format!("aeroric-gw-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    aeroric_core::settings::configure_data_dir(&root);
    let secrets = Arc::new(MemorySecrets::default());
    aeroric_core::settings::secret_fields::configure_secret_store(secrets.clone());

    let (down, down_seen) = upstream(StatusCode::SERVICE_UNAVAILABLE).await;
    let (up, up_seen) = upstream(StatusCode::OK).await;
    let (bad, bad_seen) = upstream(StatusCode::UNPROCESSABLE_ENTITY).await;
    let (spare, spare_seen) = upstream(StatusCode::OK).await;

    // Priority order with equal priority falls back to connection id, so `a-*`
    // is always tried before `b-*`.
    let doc: ConnectionsDocument = serde_json::from_value(json!({
        "version": 1,
        "connections": [
            connection("a-down", down, "ds-down"),
            connection("b-up", up, "ds-up"),
            connection("c-bad", bad, "ds-bad"),
            connection("d-spare", spare, "ds-spare"),
        ],
        "pools": [
            { "id": "p1", "label": "p1", "members": [
                { "connection_id": "a-down" }, { "connection_id": "b-up" } ] },
            { "id": "p2", "label": "p2", "members": [
                { "connection_id": "c-bad" }, { "connection_id": "d-spare" } ] }
        ],
        "configs": [
            { "id": "failover", "label": "failover", "harness": "codex",
              "target": { "kind": "pool", "pool_id": "p1" },
              "default_model": "deepseek-v4.1-flash" },
            { "id": "request-error", "label": "request-error", "harness": "codex",
              "target": { "kind": "pool", "pool_id": "p2" },
              "default_model": "deepseek-v4.1-flash" }
        ]
    }))
    .unwrap();
    store::save(&store::document_path(&root), &doc).unwrap();
    for id in ["a-down", "b-up", "c-bad", "d-spare"] {
        store::set_credential(secrets.as_ref(), id, "sk-test").unwrap();
    }

    let router = LocalRouterState::with_database_path(root.join("usage.sqlite3"));
    let info = router
        .start(RouterRuntimeConfig::new(
            "127.0.0.1",
            unused_port(),
            false,
            RouterUpstreams::default(),
        ))
        .await
        .unwrap();

    // A request without a live task token is refused, loopback or not.
    let anonymous = reqwest::Client::new()
        .post(format!("{}/codex/v1/responses", info.base_url))
        .json(&json!({"model":"deepseek-v4.1-flash","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    // 503 on the first channel: the router moves on to the second.
    let failover_token = task_tokens::issue("e2e-failover", Some("failover".into()));
    let response = send(&info.base_url, &failover_token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(*down_seen.lock().unwrap(), vec!["ds-down"]);
    assert_eq!(
        *up_seen.lock().unwrap(),
        vec!["ds-up"],
        "the upstream model id is sent, not the mapped name"
    );
    assert!(
        config_routes::cooldowns().contains_key("a-down"),
        "the failed channel cools down"
    );

    // Next request: the session sticks to the channel that worked.
    let response = send(&info.base_url, &failover_token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(down_seen.lock().unwrap().len(), 1, "cooling + sticky skip it");
    assert_eq!(up_seen.lock().unwrap().len(), 2);

    // 422 is a request error: another channel would fail the same way.
    let request_error_token =
        task_tokens::issue("e2e-request-error", Some("request-error".into()));
    let response = send(&info.base_url, &request_error_token).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad_seen.lock().unwrap().len(), 1);
    assert!(spare_seen.lock().unwrap().is_empty(), "no failover on 422");
    assert!(!config_routes::cooldowns().contains_key("c-bad"));

    // A revoked token stops working at once; another task's token is unaffected.
    task_tokens::revoke("e2e-failover");
    let response = send(&info.base_url, &failover_token).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "revoked token");
    let response = send(&info.base_url, &request_error_token).await;
    assert_ne!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "other task's token is still live"
    );

    router.stop().await.unwrap();
    std::fs::remove_dir_all(&root).ok();
}
