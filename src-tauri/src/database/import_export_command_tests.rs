//! Slice 102: call all ten actual command adapters through MockApp + ScriptedHost.
//! External apps have no DbxState and their requested files do not exist.
//! Embedded preview/update controls also need neither state nor credentials.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::database::ie_face::{
    DatabaseExportRequest, ExecuteSqlFileRequest, TableExportRequest, TableImportPreview,
    TableImportRequest, TableImportSummary,
};
use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use dbx_core::db;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use super::{dbx_state::DbxState, import_export};
use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;

const TOKEN: &str = "synthetic-dbx-import-export-token";
const UPDATE_UNSUPPORTED: &str =
    "UPDATE SQL table export is not supported by the bundled DBX Core exporter yet";

/// The scripted UDS rig plus the ten-command dispatch chain poll near the 2 MiB
/// libtest thread default (observed flaking between runs), the same class of
/// heavy poll as `ie_face::on_database_export_stack`, so every command-shell
/// test gets an explicit oversized stack instead of an env-var gate patch.
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
// Six table exports, database export, then the three non-unit replies.
const METHODS: [&str; 10] = [
    "database.dbx_export_table_csv",
    "database.dbx_export_table_json",
    "database.dbx_export_table_markdown",
    "database.dbx_export_table_insert_sql",
    "database.dbx_export_table_update_sql",
    "database.dbx_export_table_xlsx",
    "database.dbx_export_database",
    "database.dbx_preview_table_import_file",
    "database.dbx_import_table_file",
    "database.dbx_execute_sql_file",
];
type MockApp = tauri::App<tauri::test::MockRuntime>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("dbxie-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn assert_no_local_dbx(&self) {
        assert!(!self.0.join(".aeroric").exists(), "local DBX IO ran");
        assert!(!self.0.join("table-export.csv").exists());
        assert!(!self.0.join("database-export.sql").exists());
    }

    fn assert_external_untouched(&self) {
        self.assert_no_local_dbx();
        assert!(
            std::fs::read_dir(&self.0).unwrap().next().is_none(),
            "External must not create local input, output or state files"
        );
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

struct Requests {
    table_export: TableExportRequest,
    database_export: DatabaseExportRequest,
    table_import: TableImportRequest,
    execute_sql: ExecuteSqlFileRequest,
}

impl Requests {
    fn new(scratch: &Scratch) -> Self {
        // Deserialize real request types, then serialize them for wire assertions.
        // In particular, preserve caller format on the wire; only the executing
        // core branch may override it according to the selected export command.
        Self {
            table_export: serde_json::from_value(json!({
                "exportId": "wire-export", "connectionId": "wire-connection",
                "database": "wire database", "schema": "wire schema",
                "identifierQuote": "\"", "tableName": "wire table",
                "filePath": scratch.0.join("table-export.csv"),
                "format": "caller-format-must-not-select-exporter",
                "columns": ["id", "name"], "columnTypes": ["INTEGER", null],
                "primaryKeys": ["id"], "whereInput": "id > 3", "orderBy": "name DESC",
                "skipCount": true, "batchSize": 7, "rowLimit": 19,
                "dateTimeFormat": "%Y-%m-%d", "numericColumnRightAlign": true,
                "columnComments": [null, "wire comment"]
            }))
            .unwrap(),
            database_export: serde_json::from_value(json!({
                "exportId": "wire-database-export", "connectionId": "wire-connection",
                "database": "wire database", "schema": "wire schema",
                "filePath": scratch.0.join("database-export.sql"),
                "selectedTables": ["wire table"], "excludedTables": ["excluded table"],
                "includeStructure": true, "includeData": true, "includeObjects": false,
                "includeCreateDatabase": true, "dropTableIfExists": true,
                "omitAutoIncrement": true, "failOnError": true,
                "snapshotSessionId": "wire-snapshot", "batchSize": 11
            }))
            .unwrap(),
            table_import: serde_json::from_value(json!({
                "importId": "wire-import", "connectionId": "wire-connection",
                "database": "wire database", "schema": "wire schema", "table": "wire table",
                "filePath": scratch.0.join("source.csv"), "sourceRef": "wire-source",
                "sourceFormat": "csv",
                "parseOptions": {
                    "delimiter": ";", "encoding": "utf8", "hasHeader": true,
                    "titleRow": 2, "dataStartRow": 3, "lastDataRow": 5,
                    "trimValues": true, "emptyStringAsNull": false,
                    "sheetName": null, "sheetIndex": null, "jsonShape": "objects"
                },
                "mappings": [
                    {"sourceColumn": "id", "targetColumn": "target_id", "targetDataType": "INTEGER"},
                    {"sourceColumn": "name", "targetColumn": "target_name", "targetDataType": null}
                ],
                "mode": "append", "createTable": true, "batchSize": 13,
                "dateTimeFormat": "%Y-%m-%d", "retainSource": true
            }))
            .unwrap(),
            execute_sql: serde_json::from_value(json!({
                "connectionId": "wire-connection", "database": "wire database",
                "schema": "wire schema", "path": scratch.0.join("queries.sql"),
                "timeoutSecs": 17
            }))
            .unwrap(),
        }
    }

    fn params(&self, method: &str) -> Value {
        match method {
            method if METHODS[..6].contains(&method) => json!({"request": self.table_export}),
            "database.dbx_export_database" => json!({"request": self.database_export}),
            "database.dbx_preview_table_import_file" => {
                json!({"filePath": self.table_import.file_path})
            }
            "database.dbx_import_table_file" => json!({"request": self.table_import}),
            "database.dbx_execute_sql_file" => json!({"request": self.execute_sql}),
            _ => panic!("unlisted fixture method"),
        }
    }
}

fn encode<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

async fn call(app: &MockApp, method: &str, requests: &Requests) -> Result<Value, String> {
    let app = app.handle().clone();
    match method {
        "database.dbx_export_table_csv" => {
            import_export::dbx_export_table_csv(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_table_json" => {
            import_export::dbx_export_table_json(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_table_markdown" => {
            import_export::dbx_export_table_markdown(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_table_insert_sql" => {
            import_export::dbx_export_table_insert_sql(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_table_update_sql" => {
            import_export::dbx_export_table_update_sql(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_table_xlsx" => {
            import_export::dbx_export_table_xlsx(app, requests.table_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_export_database" => {
            import_export::dbx_export_database(app, requests.database_export.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_preview_table_import_file" => import_export::dbx_preview_table_import_file(
            app,
            requests.table_import.file_path.clone(),
        )
        .await
        .and_then(encode),
        "database.dbx_import_table_file" => {
            import_export::dbx_import_table_file(app, requests.table_import.clone())
                .await
                .and_then(encode)
        }
        "database.dbx_execute_sql_file" => {
            import_export::dbx_execute_sql_file(app, requests.execute_sql.clone())
                .await
                .and_then(encode)
        }
        _ => panic!("unlisted fixture method"),
    }
}

fn host_value(method: &str, file_path: &str) -> Value {
    match method {
        "database.dbx_preview_table_import_file" => {
            let preview: TableImportPreview = serde_json::from_value(json!({
                "fileName": "source.csv", "filePath": file_path, "sourceRef": "host-source",
                "fileType": "csv", "sizeBytes": 31, "columns": ["host-column"],
                "rows": [["host-only"]], "totalRows": 5, "totalRowsExact": false,
                "sourceFingerprint": "host-fingerprint", "effectiveEncoding": "utf8",
                "sheets": ["host-sheet"]
            }))
            .unwrap();
            encode(preview).unwrap()
        }
        "database.dbx_import_table_file" => encode(TableImportSummary {
            import_id: "wire-import".to_string(),
            rows_imported: 3,
            total_rows: 5,
            elapsed_ms: 23,
        })
        .unwrap(),
        "database.dbx_execute_sql_file" => {
            let results: Vec<db::QueryResult> = serde_json::from_value(json!([{
                "columns": ["host-column"], "column_types": ["TEXT"],
                "column_sortables": [true], "rows": [["host-only"]],
                "affected_rows": 1, "execution_time_ms": 29,
                "truncated": true, "session_id": "host-query", "has_more": true
            }]))
            .unwrap();
            encode(results).unwrap()
        }
        _ => Value::Null,
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
        // ScriptedHost owns and cleans its own exclusive short /tmp scratch.
        let host = ScriptedHost::start("dbxie", script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "dbx-import-export-test")
            .await
            .unwrap();
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);
        let app = tauri::test::mock_app();
        app.manage(gateway);
        assert!(app.try_state::<DbxState>().is_none());
        Self { app, host, calls }
    }
}

command_test!(
    external_ten_commands_preserve_wire_without_evaluating_local_state,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let requests = Requests::new(&scratch);
        let source_path = requests.table_import.file_path.clone();
        let rig = External::new(move |method| {
            Some(if method == METHODS[4] {
                failure(ErrorCode::IoError, UPDATE_UNSUPPORTED)
            } else {
                success(host_value(method, &source_path))
            })
        })
        .await;
        for method in METHODS {
            let result = call(&rig.app, method, &requests).await;
            if method == METHODS[4] {
                let error = result.unwrap_err();
                assert!(error.contains(UPDATE_UNSUPPORTED), "{error}");
                assert!(error.contains(method), "{error}");
            } else {
                assert_eq!(
                    result.unwrap(),
                    host_value(method, &requests.table_import.file_path),
                    "{method}"
                );
            }
            scratch.assert_external_untouched();
        }
        let expected: Vec<_> = METHODS
            .iter()
            .map(|method| (method.to_string(), requests.params(method)))
            .collect();
        assert_eq!(*rig.calls.lock(), expected);
        assert!(rig
            .host
            .methods()
            .iter()
            .any(|method| method == "handshake"));
    }
);

command_test!(
    external_host_refusals_never_fall_back_to_local_import_export,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let requests = Requests::new(&scratch);
        let rig = External::new(|_| {
            Some(failure(
                ErrorCode::IoError,
                "fixture host writer lease refusal",
            ))
        })
        .await;
        for method in METHODS {
            let error = call(&rig.app, method, &requests).await.unwrap_err();
            assert!(
                error.contains("fixture host writer lease refusal"),
                "{method}: {error}"
            );
            assert!(error.contains(method), "{error}");
            scratch.assert_external_untouched();
        }
        assert_eq!(rig.calls.lock().len(), METHODS.len());
    }
);

command_test!(external_transport_failures_never_open_local_state, {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let requests = Requests::new(&scratch);
    for method in METHODS {
        let rig = External::new(|_| None).await;
        let error = call(&rig.app, method, &requests).await.unwrap_err();
        assert!(
            error.contains("the host closed the connection"),
            "{method}: {error}"
        );
        assert_eq!(
            *rig.calls.lock(),
            vec![(method.to_string(), requests.params(method))]
        );
        scratch.assert_external_untouched();
    }
});

command_test!(external_typed_commands_reject_malformed_replies, {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let requests = Requests::new(&scratch);
    let source_path = requests.table_import.file_path.clone();
    let rig = External::new(move |method| {
        let mut value = host_value(method, &source_path);
        match method {
            "database.dbx_preview_table_import_file" => value["totalRows"] = json!("not a count"),
            "database.dbx_import_table_file" => value["rowsImported"] = json!("not a count"),
            "database.dbx_execute_sql_file" => value[0]["affected_rows"] = json!("not a count"),
            _ => panic!("not a typed reply fixture"),
        }
        Some(success(value))
    })
    .await;
    for &method in &METHODS[7..] {
        let error = call(&rig.app, method, &requests).await.unwrap_err();
        assert!(error.contains("invalid type"), "{method}: {error}");
        scratch.assert_external_untouched();
    }
    assert_eq!(rig.calls.lock().len(), 3);
});

command_test!(external_unit_commands_reject_malformed_success_replies, {
    let scratch = Scratch::new();
    let _home = crate::platform::install_scratch_home(scratch.0.clone());
    let requests = Requests::new(&scratch);
    for malformed in [
        json!({}),
        json!([]),
        json!(false),
        json!(1),
        json!("not unit"),
    ] {
        let rig = External::new(move |_| Some(success(malformed.clone()))).await;
        for &method in &METHODS[..7] {
            let error = call(&rig.app, method, &requests).await.unwrap_err();
            assert!(error.contains("unit"), "{method}: {error}");
            scratch.assert_external_untouched();
        }
        assert_eq!(rig.calls.lock().len(), 7);
    }
});

struct AbsentProbe;

impl PlacementProbe for AbsentProbe {
    fn availability(&self, _: &Path) -> HostAvailability {
        HostAvailability::Absent
    }

    fn attach(&self, _: &Path) -> Result<(HostClient, bool), String> {
        panic!("absent probe must not attach");
    }
}

fn embedded(scratch: &Scratch) -> MockApp {
    let gateway_dir = scratch.0.join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "dbx-import-export-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    assert!(app.try_state::<DbxState>().is_none());
    app
}

command_test!(
    embedded_preview_reads_scratch_csv_without_local_state_or_credentials,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let path = scratch.0.join("source.csv");
        let csv = "name,role\nAda,local-only\nGrace,fixture\n";
        std::fs::write(&path, csv).unwrap();
        let app = embedded(&scratch);
        let preview = import_export::dbx_preview_table_import_file(
            app.handle().clone(),
            path.to_str().unwrap().to_string(),
        )
        .await
        .unwrap();
        assert_eq!(preview.file_path, path.to_str().unwrap());
        assert_eq!(preview.file_name, "source.csv");
        assert_eq!(preview.file_type, "csv");
        assert_eq!(preview.size_bytes, csv.len() as u64);
        assert_eq!(preview.columns, vec!["name", "role"]);
        assert_eq!(
            preview.rows,
            vec![
                vec![json!("Ada"), json!("local-only")],
                vec![json!("Grace"), json!("fixture")],
            ]
        );
        assert_eq!(preview.total_rows, 2);
        // Bounded delimited previews never parse the tail, so the row count is
        // always approximate in dbx-core (its own suite pins this shape too).
        assert!(!preview.total_rows_exact);
        assert!(!preview.source_fingerprint.is_empty());
        assert!(preview.sheets.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), csv);
        scratch.assert_no_local_dbx();
    }
);

command_test!(
    embedded_update_sql_preserves_unsupported_without_local_state_or_credentials,
    {
        let scratch = Scratch::new();
        let _home = crate::platform::install_scratch_home(scratch.0.clone());
        let requests = Requests::new(&scratch);
        let app = embedded(&scratch);
        assert_eq!(
            import_export::dbx_export_table_update_sql(app.handle().clone(), requests.table_export)
                .await
                .unwrap_err(),
            UPDATE_UNSUPPORTED
        );
        scratch.assert_no_local_dbx();
    }
);
