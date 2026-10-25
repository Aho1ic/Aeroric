//! N6a B3: the router resolves a config-pinned task from the data root its host
//! passes in, not from the process-wide default. The host's data dir is rarely
//! `~/.aeroric` (the App uses Application Support), so falling back to the
//! global made every configuration "no longer exist".
//!
//! Its own test binary because it sets process globals (secret store, the
//! `AERORIC_HOME` fallback) that the router reads.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use aeroric_core::agent_connections::model::ConnectionsDocument;
use aeroric_core::agent_connections::store;
use aeroric_core::context::MemorySecrets;
use aeroric_core::local_router::{
    task_tokens, LocalRouterState, RouterRuntimeConfig, RouterUpstreams, ROUTER_TOKEN_HEADER,
};
use axum::http::StatusCode;
use axum::routing::any;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn upstream() -> SocketAddr {
    let app = Router::new().fallback(any(|Json(body): Json<Value>| async move {
        Json(json!({"id":"resp","model":body["model"],"output":[],"usage":{}}))
    }));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    address
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_config_task_resolves_from_the_hosts_data_dir() {
    let scratch = std::env::temp_dir().join(format!("aeroric-gw-dd-{}", uuid::Uuid::new_v4()));
    let host_data = scratch.join("host-data");
    let global = scratch.join("global");
    std::fs::create_dir_all(&host_data).unwrap();
    std::fs::create_dir_all(&global).unwrap();
    // The process-wide fallback points somewhere with no configurations.
    std::env::set_var("AERORIC_HOME", &global);
    let secrets = Arc::new(MemorySecrets::default());
    aeroric_core::settings::secret_fields::configure_secret_store(secrets.clone());

    let address = upstream().await;
    let doc: ConnectionsDocument = serde_json::from_value(json!({
        "version": 1,
        "connections": [{
            "id": "up", "label": "up", "kind": "api_key", "vendor": "other",
            "base_url": format!("http://{address}/v1"),
            "protocols": ["responses"],
            "models": [{ "upstream_id": "ds-up", "mapped_name": "deepseek-v4.1-flash" }]
        }],
        "configs": [{ "id": "gw", "label": "gw", "harness": "codex",
            "target": { "kind": "connection", "connection_id": "up" },
            "default_model": "deepseek-v4.1-flash" }]
    }))
    .unwrap();
    store::save(&store::document_path(&host_data), &doc).unwrap();
    store::set_credential(secrets.as_ref(), "up", "sk-test").unwrap();

    let router = LocalRouterState::with_database_path(scratch.join("usage.sqlite3"));
    let info = router
        .start(
            RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                false,
                RouterUpstreams::default(),
            )
            .with_config_data_dir(&host_data),
        )
        .await
        .unwrap();

    let token = task_tokens::issue("dd-task", Some("gw".into()));
    let response = reqwest::Client::new()
        .post(format!("{}/codex/v1/responses", info.base_url))
        .header(ROUTER_TOKEN_HEADER, &token)
        .json(&json!({"model":"deepseek-v4.1-flash","input":"hello"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "{body}");
    task_tokens::revoke("dd-task");
    let _ = std::fs::remove_dir_all(&scratch);
}
