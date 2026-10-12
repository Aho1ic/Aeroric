//! Slice 102: real authenticated UDS, scratch files/SQLite and memory secrets.
//! A registered method is not proof of writer admission or working file IO.

use std::path::Path;
use std::time::Duration;

use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::DbxDatabaseType;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

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
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "ie").await);
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request("ie", method, params)).await
}

fn export_request(host: &Harness, format: &str) -> Value {
    json!({
        "exportId":"scratch-export", "connectionId":"scratch", "database":"",
        "tableName":"items", "filePath":host.home.join(format!("export.{format}")),
        "format":"deliberately-overridden", "skipCount":true,
        "numericColumnRightAlign":false
    })
}

fn assert_no_database_initialization(host: &Harness) {
    assert!(
        !host.data_dir.join("dbx-core.db").exists(),
        "initialized DBX"
    );
    assert!(!host.data_dir.join("database-connections-v2.json").exists());
    assert!(!host.data_dir.join("database-connections.json").exists());
    assert!(!host.data_dir.join("dbx-plugins").exists());
    assert!(!host.data_dir.join("dbx-agents").exists());
}

fn import_request(path: &Path) -> Value {
    json!({
        "importId":"scratch-import", "connectionId":"scratch", "database":"",
        "schema":"", "table":"items", "filePath":path,
        "mappings":[
            {"sourceColumn":"id", "targetColumn":"id", "targetDataType":"INTEGER"},
            {"sourceColumn":"name", "targetColumn":"name", "targetDataType":"TEXT"}
        ],
        "mode":"append", "batchSize":2
    })
}

fn database_export_request(host: &Harness) -> Value {
    json!({
        "exportId":"scratch-database", "connectionId":"scratch", "database":"",
        "schema":"", "filePath":host.home.join("database.sql"),
        "selectedTables":["items"], "includeStructure":true, "includeData":true,
        "includeObjects":false, "batchSize":2, "failOnError":true
    })
}

fn stateful_requests(host: &Harness) -> Vec<(&'static str, Value)> {
    let mut requests = vec![];
    for method in [METHODS[0], METHODS[1], METHODS[2], METHODS[3], METHODS[5]] {
        requests.push((method, json!({"request":export_request(host, "csv")})));
    }
    requests.extend([
        (METHODS[6], json!({"request":database_export_request(host)})),
        (
            METHODS[8],
            json!({"request":import_request(&host.home.join("input.csv"))}),
        ),
        (
            METHODS[9],
            json!({"request":{
                "connectionId":"scratch", "path":host.home.join("query.sql")
            }}),
        ),
    ]);
    requests
}

fn rows(connection: &rusqlite::Connection) -> Vec<(i64, String)> {
    connection
        .prepare("SELECT id, name FROM items ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn file_rows(host: &Harness) -> Vec<(i64, String)> {
    rows(&rusqlite::Connection::open(host.home.join("scratch.sqlite")).unwrap())
}

async fn save_sqlite(
    host: &Harness,
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    read_only: bool,
    production: bool,
) {
    let path = host.home.join("scratch.sqlite");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO items VALUES (1, '测试'), (2, 'O''Reilly,quoted');",
        )
        .unwrap();
    let mut config = default_connection_config(
        "scratch",
        "Scratch database",
        DbxDatabaseType::Sqlite,
        path.to_string_lossy().into_owned(),
        0,
        None,
        read_only,
    );
    config.is_production = production;
    success(
        call(
            reader,
            writer,
            "database.dbx_save_connection",
            json!({"connection":{
                "id":"scratch", "name":"Scratch database", "dbType":"sqlite",
                "readOnly":read_only, "createdAt":1, "dbx":config
            }}),
        )
        .await,
    );
}

#[tokio::test]
async fn malformed_import_export_requests_do_not_initialize_database_state() {
    let host = Harness::start("iebad").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    for method in METHODS {
        failure(
            call(&mut reader, &mut writer, method, json!({})).await,
            ErrorCode::InvalidArgument,
        );
    }
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn readonly_export_refuses_before_initialization_or_file_effects() {
    let host = Harness::start_read_only("iero").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let request = export_request(&host, "csv");
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            METHODS[0],
            json!({"request":request}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("lease"), "{message}");
    assert_no_database_initialization(&host);
    assert!(!host.home.join("export.csv").exists());
}

#[tokio::test]
async fn csv_preview_is_file_only_and_keeps_unicode_rows() {
    let host = Harness::start("iepreview").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let path = host.home.join("导入 预览.csv");
    std::fs::write(&path, "id,name\n1,测试\n2,plain\n").unwrap();
    let preview = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_preview_table_import_file",
            json!({"filePath":path}),
        )
        .await,
    );
    assert_eq!(preview["columns"], json!(["id", "name"]));
    assert_eq!(preview["rows"], json!([["1", "测试"], ["2", "plain"]]));
    assert_eq!(preview["totalRows"], 2);
    assert_eq!(preview["filePath"], json!(path));
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn unauthenticated_import_export_calls_have_no_side_effects() {
    let host = Harness::start("ieauth").await;
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
async fn readonly_host_refuses_every_stateful_operation_before_file_effects() {
    let host = Harness::start_read_only("ieallro").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let output = host.home.join("export.csv");
    std::fs::write(&output, "do not overwrite").unwrap();
    std::fs::write(host.home.join("input.csv"), "id,name\n3,extra\n").unwrap();
    std::fs::write(host.home.join("query.sql"), "DELETE FROM items;").unwrap();
    for (method, params) in stateful_requests(&host) {
        let message = failure(
            call(&mut reader, &mut writer, method, params).await,
            ErrorCode::IoError,
        );
        assert!(message.contains("lease"), "{method}: {message}");
        assert_no_database_initialization(&host);
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "do not overwrite"
        );
        assert!(!host.home.join("database.sql").exists());
        assert!(!host.home.join("scratch.sqlite").exists());
    }
    // Reading a user-selected file does not require the DBX writer.
    let preview = success(
        call(
            &mut reader,
            &mut writer,
            METHODS[7],
            json!({"filePath":host.home.join("input.csv")}),
        )
        .await,
    );
    assert_eq!(preview["rows"], json!([["3", "extra"]]));
    assert_no_database_initialization(&host);
}

#[tokio::test]
async fn update_sql_export_is_explicitly_unsupported_without_database_initialization() {
    let host = Harness::start("ieupdate").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            METHODS[4],
            json!({"request":export_request(&host, "sql")}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert_eq!(
        message,
        aeroric_core::database::ie_face::unsupported_update_sql_export()
    );
    assert_no_database_initialization(&host);
    assert!(!host.home.join("export.sql").exists());
}

#[tokio::test]
async fn five_table_exports_use_the_selected_format_and_preserve_sqlite_rows() {
    let host = Harness::start("ieformats").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, false).await;
    let original = file_rows(&host);
    for (method, format) in [
        (METHODS[0], "csv"),
        (METHODS[1], "json"),
        (METHODS[2], "md"),
        (METHODS[3], "sql"),
        (METHODS[5], "xlsx"),
    ] {
        let mut request = export_request(&host, format);
        request["orderBy"] = json!("id");
        assert_eq!(
            success(call(&mut reader, &mut writer, method, json!({"request":request})).await),
            Value::Null
        );
        let path = host.home.join(format!("export.{format}"));
        assert!(path.is_file(), "{method}");
        match format {
            "csv" => {
                let preview = success(
                    call(
                        &mut reader,
                        &mut writer,
                        METHODS[7],
                        json!({"filePath":path}),
                    )
                    .await,
                );
                assert_eq!(preview["columns"], json!(["id", "name"]));
                assert_eq!(
                    preview["rows"],
                    json!([["1", "测试"], ["2", "O'Reilly,quoted"]])
                );
            }
            "json" => {
                let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                assert_eq!(
                    value,
                    json!([
                        {"id":1, "name":"测试"}, {"id":2, "name":"O'Reilly,quoted"}
                    ])
                );
            }
            "md" => {
                let markdown = std::fs::read_to_string(path).unwrap();
                assert!(markdown.contains("| id | name |"), "{markdown}");
                assert!(markdown.contains("| 1 | 测试 |"), "{markdown}");
                assert!(markdown.contains("| 2 | O'Reilly,quoted |"), "{markdown}");
            }
            "sql" => {
                let replay = rusqlite::Connection::open_in_memory().unwrap();
                replay
                    .execute_batch("CREATE TABLE items (id INTEGER, name TEXT);")
                    .unwrap();
                replay
                    .execute_batch(&std::fs::read_to_string(path).unwrap())
                    .unwrap();
                assert_eq!(rows(&replay), original);
            }
            "xlsx" => {
                let preview = success(
                    call(
                        &mut reader,
                        &mut writer,
                        METHODS[7],
                        json!({"filePath":path}),
                    )
                    .await,
                );
                assert_eq!(preview["columns"], json!(["id", "name"]));
                assert_eq!(
                    preview["rows"],
                    json!([[1, "测试"], [2, "O'Reilly,quoted"]])
                );
                assert_eq!(preview["totalRows"], 2);
                assert!(!preview["sheets"].as_array().unwrap().is_empty());
            }
            _ => unreachable!(),
        }
        assert_eq!(file_rows(&host), original, "export mutated its source");
    }
}

#[tokio::test]
async fn database_export_contains_replayable_structure_and_data() {
    let host = Harness::start("iedatabase").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, false).await;
    assert_eq!(
        success(
            call(
                &mut reader,
                &mut writer,
                METHODS[6],
                json!({"request":database_export_request(&host)}),
            )
            .await
        ),
        Value::Null
    );
    let sql = std::fs::read_to_string(host.home.join("database.sql")).unwrap();
    let replay = rusqlite::Connection::open_in_memory().unwrap();
    replay.execute_batch(&sql).unwrap();
    assert_eq!(rows(&replay), file_rows(&host));
}

#[tokio::test]
async fn csv_import_appends_rows_and_returns_the_legacy_summary() {
    let host = Harness::start("ieimport").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, false).await;
    let path = host.home.join("导入.csv");
    let content = "id,name\n3,\"追加,文本\"\n4,plain\n";
    std::fs::write(&path, content).unwrap();
    let summary = success(
        call(
            &mut reader,
            &mut writer,
            METHODS[8],
            json!({"request":import_request(&path)}),
        )
        .await,
    );
    assert_eq!(summary["importId"], "scratch-import");
    assert_eq!(summary["rowsImported"], 2);
    assert_eq!(summary["totalRows"], 2);
    assert!(summary["elapsedMs"].is_u64());
    assert_eq!(
        file_rows(&host),
        vec![
            (1, "测试".to_string()),
            (2, "O'Reilly,quoted".to_string()),
            (3, "追加,文本".to_string()),
            (4, "plain".to_string())
        ]
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), content);
}

#[tokio::test]
async fn import_and_sql_file_respect_saved_connection_readonly_protection() {
    let host = Harness::start("ieconnro").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, true, false).await;
    let original = file_rows(&host);
    let input = host.home.join("input.csv");
    let sql = host.home.join("query.sql");
    std::fs::write(&input, "id,name\n3,blocked\n").unwrap();
    std::fs::write(&sql, "DELETE FROM items;").unwrap();
    for (method, request) in [
        (METHODS[8], import_request(&input)),
        (METHODS[9], json!({"connectionId":"scratch", "path":sql})),
    ] {
        let message = failure(
            call(&mut reader, &mut writer, method, json!({"request":request})).await,
            ErrorCode::IoError,
        );
        assert!(message.to_lowercase().contains("read-only"), "{message}");
        assert_eq!(file_rows(&host), original);
    }
}

#[tokio::test]
async fn sql_file_runs_bom_multistatement_and_refuses_bad_paths_or_encoding() {
    let host = Harness::start("iesql").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, false).await;
    let path = host.home.join("脚本.sql");
    std::fs::write(
        &path,
        "\u{feff}INSERT INTO items VALUES (3, 'SQL文本'); SELECT name FROM items WHERE id = 3;",
    )
    .unwrap();
    let result = success(
        call(
            &mut reader,
            &mut writer,
            METHODS[9],
            json!({"request":{"connectionId":"scratch", "path":path, "timeoutSecs":2}}),
        )
        .await,
    );
    assert_eq!(result.as_array().unwrap().len(), 2);
    assert_eq!(result[1]["rows"], json!([["SQL文本"]]));
    let original = file_rows(&host);
    let bad_encoding = host.home.join("bad.sql");
    std::fs::write(&bad_encoding, [0xff, 0xfe]).unwrap();
    for (path, reason) in [
        ("relative.sql".to_string(), "must be absolute"),
        (
            host.home.join("wrong.txt").to_string_lossy().into_owned(),
            "Only .sql",
        ),
        (
            bad_encoding.to_string_lossy().into_owned(),
            "not valid UTF-8",
        ),
        (
            host.home.join("missing.sql").to_string_lossy().into_owned(),
            "Failed to read",
        ),
    ] {
        let message = failure(
            call(
                &mut reader,
                &mut writer,
                METHODS[9],
                json!({"request":{"connectionId":"scratch", "path":path}}),
            )
            .await,
            ErrorCode::IoError,
        );
        assert!(message.contains(reason), "{message}");
        assert_eq!(file_rows(&host), original);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sql_file_production_confirmation_can_refuse_then_approve_over_two_uds_connections() {
    // This proves host routing, not the still-unimplemented desktop event pump.
    let host = Harness::start("ieconfirm").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, true).await;
    let path = host.home.join("production.sql");
    let sql = "INSERT INTO items VALUES (3, 'confirmed');";
    std::fs::write(&path, format!("\u{feff}{sql}")).unwrap();
    host.services.log.append("test-sentinel", json!({}));
    for approved in [false, true] {
        let mut after =
            success(call(&mut reader, &mut writer, "events.since", json!({"after":0})).await)
                ["latestSeq"]
                .as_u64()
                .unwrap();
        assert!(after > 0, "since(0) is a fresh snapshot, not event replay");
        let (mut execute_reader, mut execute_writer) = authenticated(&host).await;
        let params = json!({"request":{"connectionId":"scratch", "path":path}});
        let pending = tokio::spawn(async move {
            call(&mut execute_reader, &mut execute_writer, METHODS[9], params).await
        });
        let confirm = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let batch = success(
                    call(
                        &mut reader,
                        &mut writer,
                        "events.since",
                        json!({"after":after}),
                    )
                    .await,
                );
                after = batch["latestSeq"].as_u64().unwrap();
                if let Some(event) = batch["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|event| event["event"] == "dbx-production-confirm-requested")
                {
                    break event["data"].clone();
                }
                assert!(
                    !pending.is_finished(),
                    "SQL execution must wait for confirmation"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("confirmation must be published promptly");
        assert_eq!(confirm["sql"], sql);
        assert_eq!(confirm["connection"], "Scratch database");
        assert_eq!(file_rows(&host).len(), 2);
        success(
            call(
                &mut reader,
                &mut writer,
                "database.respond_dbx_production_confirmation",
                json!({"requestId":confirm["requestId"], "approved":approved}),
            )
            .await,
        );
        let reply = pending.await.unwrap();
        if approved {
            assert_eq!(success(reply).as_array().unwrap().len(), 1);
            assert_eq!(
                file_rows(&host).last().unwrap(),
                &(3, "confirmed".to_string())
            );
        } else {
            assert_eq!(
                failure(reply, ErrorCode::IoError),
                "Production database operation cancelled."
            );
            assert_eq!(file_rows(&host).len(), 2);
        }
    }
}

#[tokio::test]
async fn initialized_import_export_services_recheck_a_lost_writer_lease() {
    let host = Harness::start("ielost").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_sqlite(&host, &mut reader, &mut writer, false, false).await;
    success(
        call(
            &mut reader,
            &mut writer,
            METHODS[0],
            json!({"request":export_request(&host, "csv")}),
        )
        .await,
    );
    let original = file_rows(&host);
    let registry_path = host.data_dir.join("database-connections-v2.json");
    let registry = std::fs::read(&registry_path).unwrap();
    let output_path = host.home.join("export.csv");
    std::fs::write(&output_path, "keep output").unwrap();
    std::fs::write(host.home.join("input.csv"), "id,name\n3,forbidden\n").unwrap();
    std::fs::write(host.home.join("query.sql"), "DELETE FROM items;").unwrap();
    let lock = host.data_dir.join(".write.lock");
    std::fs::rename(&lock, lock.with_extension("retired")).unwrap();
    std::fs::write(&lock, b"replacement inode").unwrap();
    for (method, params) in stateful_requests(&host) {
        let message = failure(
            call(&mut reader, &mut writer, method, params).await,
            ErrorCode::IoError,
        );
        assert!(
            message.contains("lost the writer lease"),
            "{method}: {message}"
        );
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "keep output"
        );
        assert_eq!(file_rows(&host), original);
    }
    assert_eq!(std::fs::read(registry_path).unwrap(), registry);
    assert!(!host.home.join("database.sql").exists());
}

#[tokio::test]
async fn invalid_excel_archive_preview_is_rejected_without_initializing_database() {
    let host = Harness::start("iezip").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let path = host.home.join("hostile.xlsx");
    // Core's moved tests exercise an actual high-ratio ZIP; the host boundary
    // needs no extra dependency to prove archive validation precedes DBX IO.
    std::fs::write(&path, b"PK\x03\x04truncated ZIP archive").unwrap();
    let original = std::fs::read(&path).unwrap();
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            METHODS[7],
            json!({"filePath":path}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("Invalid Excel ZIP archive"), "{message}");
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert_no_database_initialization(&host);
}
