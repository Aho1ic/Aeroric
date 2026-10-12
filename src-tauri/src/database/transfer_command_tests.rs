//! Slice 103: call all ten transfer/lifecycle command adapters through
//! MockApp + ScriptedHost. External apps have no DbxState and touch nothing
//! locally; Embedded controls cover the pure faces, a real SQLite backup and
//! the refusal shapes.

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::database::transfer_face::TransferRequest;
use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use super::{connections, legacy_sqlite, transfer};
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;

/// Same rationale as `import_export_command_tests::command_test!`: the
/// scripted UDS rig plus the ten-command dispatch chain polls near the 2 MiB
/// libtest thread default, so every command-shell test gets an explicit
/// oversized stack.
macro_rules! command_test {
    ($name:ident, $body:expr) => {
        #[test]
        fn $name() {
            std::thread::Builder::new()
                .stack_size(16 * 1024 * 1024)
                .spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("command test runtime")
                        .block_on(async move { $body })
                })
                .expect("command test thread")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        }
    };
}

const TOKEN: &str = "synthetic-dbx-transfer-token";
const REFUSAL: &str = "fixture transfer writer lease refusal";
const METHODS: [&str; 10] = [
    "database.dbx_start_transfer",
    "database.dbx_cancel_transfer",
    "database.dbx_prepare_schema_diff",
    "database.dbx_generate_schema_sync_sql",
    "database.dbx_prepare_data_compare",
    "database.dbx_build_data_compare_sync_plan",
    "database.dbx_prepare_data_compare_from_tables",
    "database.dbx_disconnect",
    "database.dbx_backup_sqlite_database",
    "database.db_read_sql_file",
];
type MockApp = tauri::App<tauri::test::MockRuntime>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("dbxtr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn assert_untouched(&self) {
        assert!(
            std::fs::read_dir(&self.0).unwrap().next().is_none(),
            "External must not create local input, output or state files"
        );
    }

    fn assert_no_local_dbx(&self) {
        assert!(!self.0.join(".aeroric").exists(), "local DBX IO ran");
        assert!(!self.0.join("backup.sqlite").exists());
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn transfer_request() -> TransferRequest {
    serde_json::from_value(json!({
        "transferId": "wire-transfer", "sourceConnectionId": "wire-source",
        "sourceDatabase": "wire database", "sourceSchema": "wire schema",
        "targetConnectionId": "wire-target", "targetDatabase": "wire target db",
        "targetSchema": "wire schema", "tables": ["wire table"],
        "createTable": true, "batchSize": 9
    }))
    .unwrap()
}

fn params(method: &str) -> Value {
    match method {
        "database.dbx_start_transfer" => json!({"request": transfer_request()}),
        "database.dbx_cancel_transfer" => json!({"transferId": "wire-transfer"}),
        "database.dbx_prepare_schema_diff" => json!({"options": {
            "sourceTables": [], "targetTables": [], "sourceDetails": [],
            "targetDetails": [], "sourceFunctions": [], "targetFunctions": [],
            "sourceSequences": [], "targetSequences": [], "sourceRules": [],
            "targetRules": [], "sourceOwners": [], "targetOwners": [],
            "databaseType": "sqlite", "ignoreComments": false,
            "cascadeDelete": false, "compareColumnOrder": false,
            "detectRenames": false, "detectTableRenames": false,
            "renameThreshold": 0.0, "enableRollback": false,
            "compatibilityThreshold": 0.0
        }}),
        "database.dbx_generate_schema_sync_sql" => json!({
            "diffs": [], "databaseType": "sqlite", "targetSchema": null,
            "cascadeDelete": null, "functionDiffs": null, "sequenceDiffs": null,
            "ruleDiffs": null, "ownerDiffs": null
        }),
        "database.dbx_prepare_data_compare" => json!({"options": {
            "tableName": "items", "columns": ["id"], "keyColumns": ["id"],
            "sourceRows": [], "targetRows": []
        }}),
        "database.dbx_build_data_compare_sync_plan" => json!({"options": {"tables": []}}),
        "database.dbx_prepare_data_compare_from_tables" => json!({"options": {
            "sourceConnectionId": "wire-source", "sourceDatabase": "wire database",
            "sourceSchema": "wire schema", "sourceTable": "wire table",
            "targetConnectionId": "wire-target", "targetDatabase": "wire target db",
            "targetSchema": "wire schema", "targetTable": "wire table",
            "columns": ["id"], "keyColumns": ["id"]
        }}),
        "database.dbx_disconnect" => json!({"connectionId": "wire-source"}),
        "database.dbx_backup_sqlite_database" => json!({
            "connectionId": "wire-source", "destinationPath": "/tmp/wire-backup.sqlite"
        }),
        "database.db_read_sql_file" => json!({"path": "/tmp/wire-script.sql"}),
        _ => panic!("unlisted fixture method"),
    }
}

fn host_value(method: &str) -> Value {
    match method {
        "database.dbx_prepare_schema_diff" => json!({
            "diffs": [], "syncSql": "", "rollbackCompleteness": "complete"
        }),
        "database.dbx_generate_schema_sync_sql" => json!("wire sync sql"),
        "database.dbx_prepare_data_compare" => json!({
            "result": {"added": [], "removed": [], "modified": []},
            "syncStatements": [], "syncSql": ""
        }),
        "database.dbx_build_data_compare_sync_plan" => json!({
            "insertCount": 0, "updateCount": 0, "deleteCount": 0,
            "statementCount": 0, "syncStatements": [], "syncSql": ""
        }),
        "database.dbx_prepare_data_compare_from_tables" => json!({
            "result": {"added": [], "removed": [], "modified": []},
            "syncStatements": [], "syncSql": "", "preSyncStatements": [],
            "sourceRowCount": 0, "targetRowCount": 0,
            "sourceTruncated": false, "targetTruncated": false
        }),
        "database.db_read_sql_file" => json!("SELECT wire;"),
        _ => Value::Null,
    }
}

async fn call(app: &MockApp, method: &str) -> Result<Value, String> {
    let app = app.handle().clone();
    fn encode<T: serde::Serialize>(value: Result<T, String>) -> Result<Value, String> {
        value.and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
    }
    match method {
        "database.dbx_start_transfer" => {
            encode(transfer::dbx_start_transfer(app, transfer_request()).await)
        }
        "database.dbx_cancel_transfer" => {
            encode(transfer::dbx_cancel_transfer(app, "wire-transfer".to_string()).await)
        }
        "database.dbx_prepare_schema_diff" => encode(
            transfer::dbx_prepare_schema_diff(
                app,
                serde_json::from_value(json!({"databaseType": "sqlite"})).unwrap(),
            )
            .await,
        ),
        "database.dbx_generate_schema_sync_sql" => encode(
            transfer::dbx_generate_schema_sync_sql(
                app,
                Vec::new(),
                None,
                None,
                None,
                None,
                serde_json::from_value(json!("sqlite")).unwrap(),
                None,
                None,
            )
            .await,
        ),
        "database.dbx_prepare_data_compare" => encode(
            transfer::dbx_prepare_data_compare(
                app,
                serde_json::from_value(
                    json!({"tableName": "items", "columns": ["id"], "keyColumns": ["id"]}),
                )
                .unwrap(),
            )
            .await,
        ),
        "database.dbx_build_data_compare_sync_plan" => encode(
            transfer::dbx_build_data_compare_sync_plan(
                app,
                serde_json::from_value(json!({"tables": []})).unwrap(),
            )
            .await,
        ),
        "database.dbx_prepare_data_compare_from_tables" => encode(
            transfer::dbx_prepare_data_compare_from_tables(
                app,
                serde_json::from_value(json!({
                    "sourceConnectionId": "wire-source", "sourceDatabase": "wire database",
                    "sourceSchema": "wire schema", "sourceTable": "wire table",
                    "targetConnectionId": "wire-target", "targetDatabase": "wire target db",
                    "targetSchema": "wire schema", "targetTable": "wire table",
                    "columns": ["id"], "keyColumns": ["id"]
                }))
                .unwrap(),
            )
            .await,
        ),
        "database.dbx_disconnect" => {
            encode(connections::dbx_disconnect(app, "wire-source".to_string()).await)
        }
        "database.dbx_backup_sqlite_database" => encode(
            connections::dbx_backup_sqlite_database(
                app,
                "wire-source".to_string(),
                "/tmp/wire-backup.sqlite".to_string(),
            )
            .await,
        ),
        "database.db_read_sql_file" => {
            encode(legacy_sqlite::db_read_sql_file(app, "/tmp/wire-script.sql".to_string()).await)
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
        let host = ScriptedHost::start("dbxtr", script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "dbx-transfer-test")
            .await
            .unwrap();
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);
        let app = tauri::test::mock_app();
        app.manage(gateway);
        Self { app, host, calls }
    }
}

command_test!(
    external_ten_commands_preserve_wire_without_evaluating_local_state,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let rig = External::new(|method| Some(success(host_value(method)))).await;
        for method in METHODS {
            let result = call(&rig.app, method)
                .await
                .unwrap_or_else(|error| panic!("{method}: {error}"));
            assert_eq!(result, host_value(method), "{method}");
            scratch.assert_untouched();
        }
        let expected: Vec<_> = METHODS
            .iter()
            .map(|method| (method.to_string(), params(method)))
            .collect();
        assert_eq!(*rig.calls.lock(), expected);
    }
);

command_test!(external_host_refusals_never_fall_back_to_local_transfer, {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let rig = External::new(|_| Some(failure(ErrorCode::IoError, REFUSAL))).await;
    for method in METHODS {
        let error = call(&rig.app, method).await.unwrap_err();
        assert!(error.contains(REFUSAL), "{method}: {error}");
        assert!(error.contains(method), "{error}");
        scratch.assert_untouched();
    }
    assert_eq!(rig.calls.lock().len(), METHODS.len());
    // The scripted host is kept alive by the rig until this point.
    assert!(rig
        .host
        .methods()
        .iter()
        .any(|method| method == "handshake"));
});

struct AbsentProbe;

impl PlacementProbe for AbsentProbe {
    fn availability(&self, _: &std::path::Path) -> HostAvailability {
        HostAvailability::Absent
    }

    fn attach(&self, _: &std::path::Path) -> Result<(HostClient, bool), String> {
        panic!("absent probe must not attach");
    }
}

async fn embedded(scratch: &Scratch) -> MockApp {
    let gateway_dir = scratch.0.join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "dbx-transfer-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    // The transfer/lifecycle Embedded closures read the local DbxState, unlike
    // the file-only import preview: manage the adapter wrapper over the
    // gateway dir (its core state lives under the same root).
    let (core_state, _) =
        aeroric_core::database::dbx_state::DbxState::new(&gateway_dir, "dbx-transfer-test").await;
    app.manage(super::dbx_state::DbxState(core_state));
    app
}

command_test!(
    embedded_pure_faces_read_a_real_script_without_local_state,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let script = scratch.0.join("脚本.sql");
        std::fs::write(&script, "SELECT 1;").unwrap();
        let app = embedded(&scratch).await;
        let read = legacy_sqlite::db_read_sql_file(
            app.handle().clone(),
            script.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        assert_eq!(read, "SELECT 1;");
        let preparation = call(&app, "database.dbx_prepare_schema_diff")
            .await
            .unwrap();
        assert!(preparation.is_object(), "{preparation}");
        let sql = call(&app, "database.dbx_generate_schema_sync_sql")
            .await
            .unwrap();
        assert!(sql.is_string(), "{sql}");
        let plan = call(&app, "database.dbx_build_data_compare_sync_plan")
            .await
            .unwrap();
        assert_eq!(plan["insertCount"], 0);
        assert_eq!(
            call(&app, "database.dbx_cancel_transfer").await.unwrap(),
            Value::Null
        );
        scratch.assert_no_local_dbx();
    }
);

command_test!(
    embedded_backup_round_trips_rows_through_a_saved_connection,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let source = scratch.0.join("source.sqlite");
        rusqlite::Connection::open(&source)
            .unwrap()
            .execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY); INSERT INTO items VALUES (7);",
            )
            .unwrap();
        let app = embedded(&scratch).await;
        connections::dbx_save_connection(
            app.handle().clone(),
            serde_json::from_value(json!({
                "id": "bk", "name": "Backup", "dbType": "sqlite",
                "readOnly": false, "createdAt": 1,
                "dbx": {
                    "id": "bk", "name": "Backup", "db_type": "sqlite",
                    "host": source.to_string_lossy(), "port": 0,
                    "username": "", "password": "", "is_production": false
                }
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        let destination = scratch.0.join("backup.sqlite");
        connections::dbx_backup_sqlite_database(
            app.handle().clone(),
            "bk".to_string(),
            destination.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        let rows: i64 = rusqlite::Connection::open(&destination)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        // The pure faces ran; only the connection registry was written locally.
        assert!(scratch.0.join(".aeroric").exists() || scratch.0.join("gateway").exists());
        assert!(!scratch.0.join("dbx-core.db").exists(), "DBX init ran");
    }
);

command_test!(
    embedded_start_transfer_refuses_unknown_connections_without_state,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let app = embedded(&scratch).await;
        let error = call(&app, "database.dbx_start_transfer").await.unwrap_err();
        // The registry lookup message is ensure_connected's own ("Connection not
        // found"), not a connection-file IO error: nothing was read or written.
        assert_eq!(error, "Connection not found", "{error}");
        scratch.assert_no_local_dbx();
    }
);
