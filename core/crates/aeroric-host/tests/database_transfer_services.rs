//! Slice 103: real authenticated UDS, scratch SQLite files and memory secrets.
//! The transfer face closes the database family: a registered method must
//! really move rows, publish progress events and refuse before effects.

use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::DbxDatabaseType;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

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

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected failure, got {reply:?}");
    };
    assert_eq!(reply.error.code, code, "{reply:?}");
    reply.error.message
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "tr").await);
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request("tr", method, params)).await
}

fn assert_no_database_initialization(host: &Harness) {
    assert!(
        !host.data_dir.join("dbx-core.db").exists(),
        "initialized DBX"
    );
    assert!(!host.data_dir.join("database-connections-v2.json").exists());
}

fn malformed_params(method: &str) -> Value {
    match method {
        "database.dbx_start_transfer" => json!({"request": {"transferId": 7}}),
        "database.dbx_cancel_transfer" => json!({}),
        "database.dbx_prepare_schema_diff" => json!({"options": {"databaseType": 9}}),
        "database.dbx_generate_schema_sync_sql" => json!({"diffs": "no"}),
        "database.dbx_prepare_data_compare" => json!({"options": []}),
        "database.dbx_build_data_compare_sync_plan" => json!({}),
        "database.dbx_prepare_data_compare_from_tables" => json!({"options": {"columns": "no"}}),
        "database.dbx_disconnect" => json!({"connectionId": 3}),
        "database.dbx_backup_sqlite_database" => json!({"connectionId": "c"}),
        "database.db_read_sql_file" => json!({}),
        _ => unreachable!(),
    }
}

async fn save_sqlite(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    name: &str,
    path: &std::path::Path,
    read_only: bool,
) {
    let mut config = default_connection_config(
        id,
        name,
        DbxDatabaseType::Sqlite,
        path.to_string_lossy().into_owned(),
        0,
        None,
        read_only,
    );
    config.is_production = false;
    success(
        call(
            reader,
            writer,
            "database.dbx_save_connection",
            json!({"connection":{
                "id": id, "name": name, "dbType": "sqlite",
                "readOnly": read_only, "createdAt": 1, "dbx": config
            }}),
        )
        .await,
    );
}

fn sqlite_rows(path: &std::path::Path, table: &str) -> Vec<(i64, String)> {
    rusqlite::Connection::open(path)
        .unwrap()
        .prepare(&format!("SELECT id, name FROM {table} ORDER BY id"))
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

async fn terminal_progress(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    transfer_id: &str,
) -> Value {
    // since(0) is a fresh snapshot, not a replay from zero (slice-97 lesson):
    // a terminal event published before the snapshot lands is INSIDE it, so the
    // snapshot itself must be scanned before polling for newer batches.
    let matches = |event: &Value| {
        event["event"] == "dbx.transfer_progress"
            && event["data"]["transferId"] == transfer_id
            && event["data"]["terminal"] == true
    };
    let snapshot = success(call(reader, writer, "events.since", json!({"after": 0})).await);
    if let Some(event) = snapshot["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| matches(event))
    {
        return event["data"].clone();
    }
    let mut after = snapshot["latestSeq"].as_u64().unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let batch =
                success(call(reader, writer, "events.since", json!({"after": after})).await);
            after = batch["latestSeq"].as_u64().unwrap();
            if let Some(event) = batch["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| matches(event))
            {
                return event["data"].clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("terminal transfer progress must be published promptly")
}

#[tokio::test]
async fn malformed_transfer_requests_do_not_initialize_database_state() {
    let host = Harness::start("trbad").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    for method in METHODS {
        failure(
            call(&mut reader, &mut writer, method, malformed_params(method)).await,
            ErrorCode::InvalidArgument,
        );
    }
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn unauthenticated_transfer_calls_have_no_side_effects() {
    let host = Harness::start("trauth").await;
    for method in METHODS {
        let (mut reader, mut writer) = host.connect().await;
        failure(
            call(&mut reader, &mut writer, method, json!({})).await,
            ErrorCode::Unauthenticated,
        );
    }
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn sqlite_to_sqlite_transfer_moves_rows_and_publishes_progress() {
    let host = Harness::start("trmove").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-source.sqlite");
    let target = host.home.join("tr-target.sqlite");
    rusqlite::Connection::open(&source)
        .unwrap()
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO items VALUES (1, '一行'), (2, 'two, quoted'), (3, 'three');",
        )
        .unwrap();
    rusqlite::Connection::open(&target).unwrap(); // empty file-backed target
    save_sqlite(&mut reader, &mut writer, "src", "Source", &source, false).await;
    save_sqlite(&mut reader, &mut writer, "dst", "Target", &target, false).await;
    // Sentinel lifts the events.since watermark past 0 (slice-97 lesson): a
    // fresh since(0) snapshot is not a replay, so progress polling must read
    // incrementally from a nonzero cursor.
    host.services.log.append("test-sentinel", json!({}));
    let request = json!({
        "transferId": "tr-move-1", "sourceConnectionId": "src",
        "sourceDatabase": "", "sourceSchema": "",
        "targetConnectionId": "dst", "targetDatabase": "", "targetSchema": "",
        "tables": ["items"], "createTable": true, "batchSize": 2
    });
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_start_transfer",
                json!({"request": request})
            )
            .await
        ),
        Value::Null
    );
    let terminal = terminal_progress(&mut reader, &mut writer, "tr-move-1").await;
    assert_eq!(terminal["status"], "done", "{terminal}");
    assert_eq!(terminal["terminal"], true);
    assert_eq!(terminal["table"], "");
    assert_eq!(terminal["totalTables"], 1);
    assert_eq!(terminal["rowsTransferred"], 0);
    assert_eq!(
        sqlite_rows(&target, "items"),
        vec![
            (1, "一行".into()),
            (2, "two, quoted".into()),
            (3, "three".into())
        ]
    );
    assert_eq!(
        sqlite_rows(&source, "items").len(),
        3,
        "transfer mutated its source"
    );
}

#[tokio::test]
async fn pre_cancelled_transfer_reports_cancelled_without_copying() {
    // Cancel is registered before the spawned task runs its first iteration, so
    // the terminal event is deterministic instead of racing a fast SQLite copy.
    let host = Harness::start("trcancel").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-cancel-source.sqlite");
    let target = host.home.join("tr-cancel-target.sqlite");
    rusqlite::Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY); INSERT INTO items VALUES (1);")
        .unwrap();
    rusqlite::Connection::open(&target).unwrap(); // empty file-backed target
    save_sqlite(&mut reader, &mut writer, "src", "Source", &source, false).await;
    save_sqlite(&mut reader, &mut writer, "dst", "Target", &target, false).await;
    host.services.log.append("test-sentinel", json!({}));
    // Take the incremental cursor BEFORE the cancel fires: since() never
    // replays events that landed before this connection's first snapshot.
    let mut after = success(
        call(
            &mut reader,
            &mut writer,
            "events.since",
            json!({"after": 0}),
        )
        .await,
    )["latestSeq"]
        .as_u64()
        .unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_cancel_transfer",
                json!({"transferId": "tr-cancel-1"})
            )
            .await
        ),
        Value::Null
    );
    let request = json!({
        "transferId": "tr-cancel-1", "sourceConnectionId": "src",
        "sourceDatabase": "", "sourceSchema": "",
        "targetConnectionId": "dst", "targetDatabase": "", "targetSchema": "",
        "tables": ["items"], "createTable": true, "batchSize": 2
    });
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_start_transfer",
                json!({"request": request})
            )
            .await
        ),
        Value::Null
    );
    let terminal = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let batch = success(
                call(
                    &mut reader,
                    &mut writer,
                    "events.since",
                    json!({"after": after}),
                )
                .await,
            );
            after = batch["latestSeq"].as_u64().unwrap();
            if let Some(event) = batch["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["event"] == "dbx.transfer_progress")
            {
                return event["data"].clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("terminal transfer progress must be published promptly");
    assert_eq!(terminal["status"], "cancelled", "{terminal}");
    assert_eq!(terminal["error"], Value::Null);
    assert!(
        !target.exists()
            || rusqlite::Connection::open(&target)
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='items'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(1)
                == 0,
        "cancelled transfer must not have copied the table"
    );
}

#[tokio::test]
async fn self_target_transfer_is_rejected_without_initialization() {
    let host = Harness::start("trself").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-self.sqlite");
    rusqlite::Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY);")
        .unwrap();
    save_sqlite(&mut reader, &mut writer, "src", "Self", &source, false).await;
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_start_transfer",
            json!({"request":{
                "transferId": "tr-self-1", "sourceConnectionId": "src",
                "sourceDatabase": "", "sourceSchema": "",
                "targetConnectionId": "src", "targetDatabase": "", "targetSchema": "",
                "tables": ["items"], "createTable": true, "batchSize": 2
            }}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(
        message, "Cannot transfer table 'items' onto itself",
        "{message}"
    );
}

#[tokio::test]
async fn readonly_target_refuses_transfer_before_copying() {
    let host = Harness::start("trro").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-ro-source.sqlite");
    let target = host.home.join("tr-ro-target.sqlite");
    rusqlite::Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY); INSERT INTO items VALUES (1);")
        .unwrap();
    rusqlite::Connection::open(&target).unwrap(); // empty file-backed target
    save_sqlite(&mut reader, &mut writer, "src", "Source", &source, false).await;
    save_sqlite(&mut reader, &mut writer, "dst", "Target", &target, true).await;
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_start_transfer",
            json!({"request":{
                "transferId": "tr-ro-1", "sourceConnectionId": "src",
                "sourceDatabase": "", "sourceSchema": "",
                "targetConnectionId": "dst", "targetDatabase": "", "targetSchema": "",
                "tables": ["items"], "createTable": true, "batchSize": 2
            }}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(
        message.to_lowercase().contains("read-only") || message.contains("lease"),
        "{message}"
    );
    assert!(
        rusqlite::Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='items'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(1)
            == 0,
        "refused transfer must not create the target table"
    );
}

#[tokio::test]
async fn stateless_faces_serve_on_readonly_hosts_before_any_state() {
    // The four pure diff/plan faces, cancel and the SQL file reader must work
    // without the DBX writer: structure comparison and script reading have no
    // host state, exactly like the file-only import preview.
    let host = Harness::start_read_only("trstateless").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_cancel_transfer",
                json!({"transferId": "nothing-running"})
            )
            .await
        ),
        Value::Null
    );
    let prep = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_prepare_schema_diff",
            json!({"options": {"databaseType": "sqlite"}}),
        )
        .await,
    );
    assert!(prep.is_object(), "schema diff preparation: {prep}");
    let sql = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_generate_schema_sync_sql",
            json!({"diffs": [], "databaseType": "sqlite"}),
        )
        .await,
    );
    assert!(sql.is_string(), "sync sql: {sql}");
    let compare = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_prepare_data_compare",
            json!({"options": {"tableName": "items", "columns": ["id"], "keyColumns": ["id"]}}),
        )
        .await,
    );
    assert!(compare.is_object(), "data compare preparation: {compare}");
    let plan = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_build_data_compare_sync_plan",
            json!({"options": {"tables": []}}),
        )
        .await,
    );
    assert!(plan.is_object(), "sync plan: {plan}");
    let script = host.home.join("脚本.sql");
    std::fs::write(&script, "SELECT 1;").unwrap();
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.db_read_sql_file",
                json!({"path": script})
            )
            .await
        ),
        json!("SELECT 1;")
    );
    assert_no_database_initialization(&host);
    // Stateful faces still refuse on a read-only host, before any effect.
    // Params must decode first: admission happens after decode by design.
    for (method, params) in [
        (
            "database.dbx_start_transfer",
            json!({"request": {
                "transferId": "x", "sourceConnectionId": "a",
                "sourceDatabase": "", "sourceSchema": "",
                "targetConnectionId": "b", "targetDatabase": "", "targetSchema": "",
                "tables": ["items"], "createTable": true, "batchSize": 10
            }}),
        ),
        ("database.dbx_disconnect", json!({"connectionId": "c"})),
        (
            "database.dbx_backup_sqlite_database",
            json!({"connectionId": "c", "destinationPath": "/tmp/x"}),
        ),
        (
            "database.dbx_prepare_data_compare_from_tables",
            json!({"options": {
                "sourceConnectionId": "a", "sourceDatabase": "", "sourceSchema": "",
                "sourceTable": "items",
                "targetConnectionId": "b", "targetDatabase": "", "targetSchema": "",
                "targetTable": "items",
                "columns": ["id"], "keyColumns": ["id"]
            }}),
        ),
    ] {
        let message = failure(
            call(&mut reader, &mut writer, method, params).await,
            ErrorCode::IoError,
        );
        assert!(message.contains("lease"), "{method}: {message}");
    }
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn backup_copies_rows_and_refuses_non_sqlite_or_memory_sources() {
    let host = Harness::start("trbackup").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-backup-source.sqlite");
    rusqlite::Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES (1, '备份');")
        .unwrap();
    save_sqlite(
        &mut reader,
        &mut writer,
        "bksqlite",
        "Backup",
        &source,
        false,
    )
    .await;
    let destination = host.home.join("nested").join("backup.sqlite");
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_backup_sqlite_database",
                json!({"connectionId": "bksqlite", "destinationPath": destination})
            )
            .await
        ),
        Value::Null
    );
    assert_eq!(sqlite_rows(&destination, "items"), vec![(1, "备份".into())]);
    // A :memory: source and a missing destination parent both keep their legacy
    // messages; a non-SQLite connection cannot be backed up at all.
    for (connection_id, destination, reason) in [
        (
            "bkmissing",
            host.home.join("nope.sqlite").to_string_lossy().into_owned(),
            "Connection not found",
        ),
        (
            "bksqlite",
            "   ".to_string(),
            "destination path is required",
        ),
    ] {
        let message = failure(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_backup_sqlite_database",
                json!({"connectionId": connection_id, "destinationPath": destination}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert!(message.contains(reason), "{message}");
    }
}

#[tokio::test]
async fn data_compare_from_tables_finds_missing_target_rows() {
    let host = Harness::start("trcompare").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let source = host.home.join("tr-cmp-source.sqlite");
    let target = host.home.join("tr-cmp-target.sqlite");
    for (path, rows) in [(&source, 3), (&target, 1)] {
        let values: Vec<String> = (1..=rows).map(|id| format!("({id}, 'r{id}')")).collect();
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(&format!(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES {}",
                values.join(", ")
            ))
            .unwrap();
    }
    save_sqlite(&mut reader, &mut writer, "cs", "CmpSource", &source, false).await;
    save_sqlite(&mut reader, &mut writer, "ct", "CmpTarget", &target, false).await;
    let preparation = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_prepare_data_compare_from_tables",
            json!({"options":{
                "sourceConnectionId": "cs", "sourceDatabase": "", "sourceSchema": "",
                "sourceTable": "items",
                "targetConnectionId": "ct", "targetDatabase": "", "targetSchema": "",
                "targetTable": "items",
                "columns": ["id", "name"], "keyColumns": ["id"]
            }}),
        )
        .await,
    );
    assert!(
        serde_json::to_string(&preparation)
            .unwrap()
            .contains("items"),
        "preparation should reference the compared table: {preparation}"
    );
}

#[tokio::test]
async fn disconnect_and_sql_file_reader_keep_their_legacy_shapes() {
    let host = Harness::start("trlegacy").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    // Disconnect of an unknown connection is a no-op, exactly like the old
    // client: pools are only dropped, nothing is created.
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                "database.dbx_disconnect",
                json!({"connectionId": "unknown"})
            )
            .await
        ),
        Value::Null
    );
    let script = host.home.join("脚本.sql");
    std::fs::write(&script, "\u{feff}SELECT 1;").unwrap();
    let read = success(
        call(
            &mut reader,
            &mut writer,
            "database.db_read_sql_file",
            json!({"path": script}),
        )
        .await,
    );
    // The legacy reader keeps the file byte-for-byte (BOM included) — the BOM
    // stripping variant is ie_face's execute_sql_file, not this command.
    assert_eq!(read, json!("\u{feff}SELECT 1;"), "{read}");
    for (path, reason) in [
        ("relative.sql".to_string(), "must be absolute"),
        (
            host.home.join("wrong.txt").to_string_lossy().into_owned(),
            "Only .sql",
        ),
    ] {
        let message = failure(
            call(
                &mut reader,
                &mut writer,
                "database.db_read_sql_file",
                json!({"path": path}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert!(message.contains(reason), "{message}");
    }
}
