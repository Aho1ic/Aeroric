//! Slice 102: real CLI subprocesses, authenticated UDS and scratch SQLite/files.
//! Harness startup injects MemorySecrets; never spawn production `serve` or use
//! an OS credential store. Harness::home is the owned scratch root.

use std::process::{Output, Stdio};
use std::time::Duration;

use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::{AeroricDbConnectionConfig, DbxDatabaseType};
use aeroric_core::database::ie_face::{
    DatabaseExportRequest, ExecuteSqlFileRequest, TableExportRequest, TableImportRequest,
};
use serde_json::{json, Value};

#[path = "../../aeroric-host/tests/host_harness/mod.rs"]
mod host_harness;
use host_harness::Harness;

const CONNECTION_ID: &str = "cli-import-export";
const SEED_SQL: &str = concat!(
    "\u{feff}CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n",
    "INSERT INTO items (id, name) VALUES (1, '初始'), (2, 'comma, \"quoted\"');\n",
);
const APPEND_CSV: &str = "id,name\n3,追加\n";
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

async fn rpc(host: &Harness, method: &str, params: Value) -> Output {
    let input = host.home.join("cli-params.json");
    std::fs::write(&input, serde_json::to_vec(&params).unwrap()).unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_aeroric-cli"));
    command
        .env("HOME", &host.home)
        .env("AERORIC_HOME", host.home.join("cli-home"))
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .arg("--data-dir")
        .arg(&host.data_dir)
        .args(["rpc", method, "--params-file"])
        .arg(input);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("CLI must finish within 10 seconds")
        .expect("CLI subprocess")
}

fn success(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("CLI JSON result on stdout")
}

fn failure(output: Output, expected: &str) -> String {
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains(expected), "{stderr}");
    stderr
}

// Decode fixtures through the core re-exports, then serialize those DTOs for
// the CLI. Nested DBX connection config has its own, non-camelCase wire shape.
fn export_request(host: &Harness, file_name: &str, format: &str) -> TableExportRequest {
    serde_json::from_value(json!({
        "exportId":format!("cli-{file_name}"), "connectionId":CONNECTION_ID,
        "database":"", "schema":null, "tableName":"items",
        "filePath":host.home.join(file_name), "format":format,
        "columns":["id","name"], "orderBy":"id ASC", "batchSize":2,
        "skipCount":false, "numericColumnRightAlign":false
    }))
    .expect("core TableExportRequest fixture")
}

fn database_export_request(host: &Harness, file_name: &str) -> DatabaseExportRequest {
    serde_json::from_value(json!({
        "exportId":format!("cli-{file_name}"), "connectionId":CONNECTION_ID,
        "database":"", "schema":"", "filePath":host.home.join(file_name),
        "selectedTables":["items"], "includeStructure":true,
        "includeData":true, "includeObjects":false,
        "failOnError":true, "batchSize":2
    }))
    .expect("core DatabaseExportRequest fixture")
}

fn import_request(host: &Harness, file_name: &str) -> TableImportRequest {
    serde_json::from_value(json!({
        "importId":"cli-append", "connectionId":CONNECTION_ID,
        "database":"", "schema":"", "table":"items",
        "filePath":host.home.join(file_name), "sourceFormat":"csv",
        "mappings":[
            {"sourceColumn":"id","targetColumn":"id","targetDataType":"INTEGER"},
            {"sourceColumn":"name","targetColumn":"name","targetDataType":"TEXT"}
        ],
        "mode":"append", "batchSize":2
    }))
    .expect("core TableImportRequest fixture")
}

fn sql_request(host: &Harness, file_name: &str) -> ExecuteSqlFileRequest {
    ExecuteSqlFileRequest {
        connection_id: CONNECTION_ID.to_string(),
        database: None,
        schema: None,
        path: host.home.join(file_name).to_string_lossy().into_owned(),
        timeout_secs: Some(5),
    }
}

fn assert_no_database_initialization(host: &Harness) {
    for name in [
        "dbx-core.db",
        "dbx-core.db-wal",
        "dbx-core.db-shm",
        "database-connections-v2.json",
        "database-connections.json",
    ] {
        assert!(!host.data_dir.join(name).exists(), "created {name}");
    }
}

#[tokio::test]
async fn cli_import_export_round_trip_uses_sqlite_and_real_files() {
    let host = Harness::start("iecli").await;
    let sqlite = host.home.join("scratch.sqlite");
    // Only an empty scratch file is prepared here; the production SQL-file
    // method below creates the schema and inserts the initial rows.
    std::fs::write(&sqlite, []).unwrap();
    let dbx = default_connection_config(
        CONNECTION_ID,
        "CLI import/export",
        DbxDatabaseType::Sqlite,
        sqlite.to_string_lossy().into_owned(),
        0,
        None,
        false,
    );
    let connection = AeroricDbConnectionConfig {
        id: CONNECTION_ID.to_string(),
        name: "CLI import/export".to_string(),
        db_type: DbxDatabaseType::Sqlite,
        read_only: false,
        project_scope: None,
        dbx: serde_json::to_value(dbx).unwrap(),
        created_at: 1,
        last_opened_at: None,
        migrated_from_legacy: None,
        connection_group: None,
        pinned: None,
    };
    assert_eq!(
        success(
            rpc(
                &host,
                "database.dbx_save_connection",
                json!({"connection":connection}),
            )
            .await,
        ),
        Value::Null
    );
    std::fs::write(host.home.join("seed with bom.sql"), SEED_SQL).unwrap();
    let executed = success(
        rpc(
            &host,
            "database.dbx_execute_sql_file",
            json!({"request":sql_request(&host, "seed with bom.sql")}),
        )
        .await,
    );
    assert_eq!(executed.as_array().expect("SQL result array").len(), 2);
    assert_eq!(executed[1]["affected_rows"], 2);

    // The method, not the caller's conflicting format, selects the exporter.
    assert_eq!(
        success(
            rpc(
                &host,
                "database.dbx_export_table_csv",
                json!({"request":export_request(&host, "items.csv", "json")}),
            )
            .await,
        ),
        Value::Null
    );
    let csv = std::fs::read_to_string(host.home.join("items.csv")).unwrap();
    assert_eq!(
        csv.trim_end_matches('\n'),
        concat!(
            "\u{feff}\"id\",\"name\"\n",
            "\"1\",\"初始\"\n",
            "\"2\",\"comma, \"\"quoted\"\"\"",
        )
    );
    let preview = success(
        rpc(
            &host,
            "database.dbx_preview_table_import_file",
            json!({"filePath":host.home.join("items.csv")}),
        )
        .await,
    );
    assert_eq!(preview["columns"], json!(["id", "name"]));
    assert_eq!(
        preview["rows"],
        json!([["1", "初始"], ["2", "comma, \"quoted\""]])
    );
    assert_eq!(preview["totalRows"], 2);
    assert_eq!(preview["filePath"], json!(host.home.join("items.csv")));

    std::fs::write(host.home.join("append.csv"), APPEND_CSV).unwrap();
    let imported = success(
        rpc(
            &host,
            "database.dbx_import_table_file",
            json!({"request":import_request(&host, "append.csv")}),
        )
        .await,
    );
    assert_eq!(imported["importId"], "cli-append");
    assert_eq!(imported["rowsImported"], 1);
    assert_eq!(imported["totalRows"], 1);
    assert_eq!(
        success(
            rpc(
                &host,
                "database.dbx_export_table_json",
                json!({"request":export_request(&host, "items.json", "csv")}),
            )
            .await,
        ),
        Value::Null
    );
    let exported: Value =
        serde_json::from_slice(&std::fs::read(host.home.join("items.json")).unwrap()).unwrap();
    assert_eq!(exported.as_array().expect("exported JSON rows").len(), 3);
    assert_eq!(
        exported,
        json!([
            {"id":1,"name":"初始"},
            {"id":2,"name":"comma, \"quoted\""},
            {"id":3,"name":"追加"}
        ])
    );

    // Query the actual SQLite data too, not just exporter replies or files.
    std::fs::write(
        host.home.join("verify.sql"),
        "SELECT COUNT(*) AS total FROM items;\nSELECT id, name FROM items ORDER BY id;\n",
    )
    .unwrap();
    let verified = success(
        rpc(
            &host,
            "database.dbx_execute_sql_file",
            json!({"request":sql_request(&host, "verify.sql")}),
        )
        .await,
    );
    assert_eq!(verified.as_array().expect("SQL result array").len(), 2);
    assert_eq!(verified[0]["rows"], json!([[3]]));
    assert_eq!(verified[1]["columns"], json!(["id", "name"]));
    assert_eq!(
        verified[1]["rows"],
        json!([[1, "初始"], [2, "comma, \"quoted\""], [3, "追加"]])
    );

    assert_eq!(
        success(
            rpc(
                &host,
                "database.dbx_export_database",
                json!({"request":database_export_request(&host, "database.sql")}),
            )
            .await,
        ),
        Value::Null
    );
    let dump = std::fs::read_to_string(host.home.join("database.sql")).unwrap();
    assert!(dump.contains("CREATE TABLE"), "{dump}");
    assert!(dump.contains("items"), "{dump}");
    assert!(dump.contains("PRIMARY KEY"), "{dump}");
    assert!(dump.contains("INSERT INTO"), "{dump}");
    for row in ["(1, '初始')", "(2, 'comma, \"quoted\"')", "(3, '追加')"] {
        assert!(dump.contains(row), "missing {row}: {dump}");
    }
}

#[tokio::test]
async fn cli_import_export_readonly_rejects_writes_but_allows_preview() {
    let host = Harness::start_read_only("iecliro").await;
    std::fs::write(host.home.join("append.csv"), APPEND_CSV).unwrap();
    std::fs::write(host.home.join("seed.sql"), SEED_SQL).unwrap();
    assert_no_database_initialization(&host);

    // Valid DTOs and existing input files must reach writer admission, not a
    // missing-connection error or lazy initialization before the lease check.
    for (method, params) in [
        (
            "database.dbx_export_table_csv",
            json!({"request":export_request(&host, "refused.csv", "csv")}),
        ),
        (
            "database.dbx_export_database",
            json!({"request":database_export_request(&host, "refused.sql")}),
        ),
        (
            "database.dbx_import_table_file",
            json!({"request":import_request(&host, "append.csv")}),
        ),
        (
            "database.dbx_execute_sql_file",
            json!({"request":sql_request(&host, "seed.sql")}),
        ),
    ] {
        let stderr = failure(rpc(&host, method, params).await, "IoError");
        assert!(stderr.contains("lease"), "{method}: {stderr}");
        assert_no_database_initialization(&host);
        for name in ["refused.csv", "refused.sql", "scratch.sqlite"] {
            assert!(!host.home.join(name).exists(), "{method} created {name}");
        }
    }

    let preview = success(
        rpc(
            &host,
            "database.dbx_preview_table_import_file",
            json!({"filePath":host.home.join("append.csv")}),
        )
        .await,
    );
    assert_eq!(preview["columns"], json!(["id", "name"]));
    assert_eq!(preview["rows"], json!([["3", "追加"]]));
    assert_eq!(preview["totalRows"], 1);
    assert_eq!(preview["filePath"], json!(host.home.join("append.csv")));
    assert_no_database_initialization(&host);
    assert_eq!(
        std::fs::read_to_string(host.home.join("append.csv")).unwrap(),
        APPEND_CSV
    );
    assert_eq!(
        std::fs::read_to_string(host.home.join("seed.sql")).unwrap(),
        SEED_SQL
    );
}

#[tokio::test]
async fn cli_import_export_unsupported_and_invalid_do_not_initialize_dbx() {
    let host = Harness::start("ieclibad").await;
    let stderr = failure(
        rpc(
            &host,
            "database.dbx_export_table_update_sql",
            json!({"request":export_request(&host, "unsupported.sql", "sql")}),
        )
        .await,
        "UPDATE SQL table export is not supported by the bundled DBX Core exporter yet",
    );
    assert!(!stderr.contains("UnsupportedCapability"), "{stderr}");
    assert_no_database_initialization(&host);
    assert!(!host.home.join("unsupported.sql").exists());

    for method in METHODS {
        failure(rpc(&host, method, json!({})).await, "InvalidArgument");
        assert_no_database_initialization(&host);
        assert!(!host.home.join("unsupported.sql").exists(), "{method}");
    }
}
