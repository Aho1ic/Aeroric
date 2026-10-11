//! Slice 95: actual desktop command adapters. External apps intentionally have
//! no DbxState: eagerly evaluating any local branch would panic, not pass.
//! Embedded controls use an empty scratch registry (no OS credential calls).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use super::{connections, dbx_state::DbxState, types::AeroricDbConnectionConfig};
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;

const TOKEN: &str = "synthetic-dbx-command-token";
const METHODS: [&str; 5] = [
    "database.dbx_list_connections",
    "database.dbx_save_connection",
    "database.dbx_delete_connection",
    "database.dbx_test_connection",
    "database.dbx_connect",
];
type MockApp = tauri::App<tauri::test::MockRuntime>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("dbxc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn assert_no_local_dbx(&self) {
        assert!(
            !self.0.join(".aeroric").exists(),
            "External evaluated local IO"
        );
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn connection() -> AeroricDbConnectionConfig {
    serde_json::from_value(json!({
        "id":"wire-id", "name":"wire name", "dbType":"mysql", "readOnly":true,
        "projectScope":null, "createdAt":17, "lastOpenedAt":23,
        "migratedFromLegacy":false, "connectionGroup":"group", "pinned":true,
        "dbx":{"password":"", "save_password":true, "futureField":{"keep":[1,2]}}
    }))
    .unwrap()
}

async fn call(
    app: &MockApp,
    method: &str,
    connection: &AeroricDbConnectionConfig,
) -> Result<Value, String> {
    match method {
        "database.dbx_list_connections" => connections::dbx_list_connections(app.handle().clone())
            .await
            .and_then(|list| serde_json::to_value(list).map_err(|error| error.to_string())),
        "database.dbx_save_connection" => {
            connections::dbx_save_connection(app.handle().clone(), connection.clone())
                .await
                .map(|()| Value::Null)
        }
        "database.dbx_delete_connection" => {
            connections::dbx_delete_connection(app.handle().clone(), connection.id.clone())
                .await
                .map(|()| Value::Null)
        }
        "database.dbx_test_connection" => {
            connections::dbx_test_connection(app.handle().clone(), connection.clone())
                .await
                .map(|()| Value::Null)
        }
        "database.dbx_connect" => {
            connections::dbx_connect(app.handle().clone(), connection.id.clone())
                .await
                .map(|()| Value::Null)
        }
        _ => panic!("unlisted fixture method"),
    }
}

struct External {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl External {
    async fn new(reply: impl Fn(&str) -> Option<Frame> + Send + Sync + 'static) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if METHODS.contains(&method) {
                seen.lock().push((method.to_string(), params.clone()));
                reply(method)
            } else {
                handshake(method, params)
            }
        });
        let host = ScriptedHost::start("dbxcmd", script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "dbx-command-test")
            .await
            .unwrap();
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);
        let app = tauri::test::mock_app();
        app.manage(gateway);
        // No local state and no keyring adapter are installed.
        Self { app, host, calls }
    }
}

#[tokio::test]
async fn external_five_commands_preserve_wire_without_evaluating_local_state() {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let rig = External::new(|method| {
        Some(success(if method == METHODS[0] {
            json!([connection()])
        } else {
            Value::Null
        }))
    })
    .await;
    let connection = connection();
    for method in METHODS {
        let result = call(&rig.app, method, &connection).await.unwrap();
        if method == METHODS[0] {
            assert_eq!(result, json!([connection]));
        } else {
            assert_eq!(result, Value::Null);
        }
        scratch.assert_no_local_dbx();
    }
    assert_eq!(
        *rig.calls.lock(),
        vec![
            (METHODS[0].to_string(), json!({})),
            (METHODS[1].to_string(), json!({"connection":connection})),
            (METHODS[2].to_string(), json!({"connectionId":"wire-id"})),
            (METHODS[3].to_string(), json!({"connection":connection})),
            (METHODS[4].to_string(), json!({"connectionId":"wire-id"})),
        ]
    );
    assert!(rig
        .host
        .methods()
        .iter()
        .any(|method| method == "handshake"));
}

#[tokio::test]
async fn external_host_refusals_never_fall_back_to_local_database() {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let rig = External::new(|_| Some(failure(ErrorCode::IoError, "fixture-db-refusal"))).await;
    for method in METHODS {
        assert!(call(&rig.app, method, &connection())
            .await
            .unwrap_err()
            .contains("fixture-db-refusal"));
        scratch.assert_no_local_dbx();
    }
    assert_eq!(rig.calls.lock().len(), METHODS.len());
}

#[tokio::test]
async fn external_transport_failures_never_open_local_state() {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    for method in METHODS {
        let rig = External::new(|_| None).await;
        assert!(call(&rig.app, method, &connection()).await.is_err());
        assert_eq!(rig.calls.lock().len(), 1);
        scratch.assert_no_local_dbx();
    }
}

#[tokio::test]
async fn external_list_rejects_malformed_reply_instead_of_reading_local_json() {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let rig = External::new(|_| Some(success(json!({"not":"an array"})))).await;
    assert!(call(&rig.app, METHODS[0], &connection()).await.is_err());
    scratch.assert_no_local_dbx();
}

struct AbsentProbe;
impl PlacementProbe for AbsentProbe {
    fn availability(&self, _: &Path) -> HostAvailability {
        HostAvailability::Absent
    }
    fn attach(&self, _: &Path) -> Result<(HostClient, bool), String> {
        panic!("absent probe must not attach");
    }
}

#[tokio::test]
async fn embedded_empty_registry_uses_shared_state_without_any_credential_lookup() {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let gateway_dir = scratch.0.join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "dbx-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let (shared, degradation) =
        aeroric_core::database::dbx_state::DbxState::new(&scratch.0.join(".aeroric"), "test").await;
    assert!(degradation.is_none());
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app.manage(DbxState(shared));
    assert_eq!(
        call(&app, METHODS[0], &connection()).await.unwrap(),
        json!([])
    );
    // Missing ID fails before any secret hydration or connection attempt.
    assert_eq!(
        call(&app, METHODS[4], &connection()).await.unwrap_err(),
        "Connection not found"
    );
    assert!(!scratch
        .0
        .join(".aeroric/database-connections-v2.json")
        .exists());
}
