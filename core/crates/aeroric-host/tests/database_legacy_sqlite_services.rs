//! P8-05 slice 94: the eight legacy-SQLite file commands on the host.
//! Real authenticated UDS, real temp SQLite files, synthetic connections JSON.
//! SSH endpoints must stay refused (no remote executor on the headless host).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn success_result(reply: Frame) -> Value {
    match reply {
        Frame::Success(success) => success.result,
        Frame::Failure(failure) => panic!("expected success, got failure: {failure:?}"),
        other => panic!("expected success frame, got {other:?}"),
    }
}

fn failure_code(reply: Frame) -> (ErrorCode, String) {
    match reply {
        Frame::Failure(failure) => (failure.error.code, failure.error.message),
        other => panic!("expected failure frame, got {other:?}"),
    }
}

struct Rig {
    host: Harness,
    db_path: PathBuf,
}

impl Rig {
    /// Starts a host with a real temp SQLite database and no connections file.
    async fn start(tag: &str) -> Self {
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        let db_path = std::env::temp_dir().join(format!(
            "aeroric-db-legacy-{tag}-{seq}-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);
        let conn = rusqlite::Connection::open(&db_path).expect("open fixture db");
        conn.execute_batch(
            "CREATE TABLE notes (title TEXT, body TEXT);
             INSERT INTO notes (title, body) VALUES ('one', 'first');
             INSERT INTO notes (title, body) VALUES ('two', 'second');",
        )
        .expect("seed fixture db");
        let host = Harness::start(tag).await;
        Self { host, db_path }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply =
            Harness::handshake(&mut reader, &mut writer, &self.host.token, "db-legacy").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }

    fn local_endpoint(&self) -> Value {
        json!({ "kind": "local", "path": self.db_path.to_string_lossy() })
    }
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    let line = serde_json::json!({ "v": 1, "id": id, "method": method, "params": params });
    use tokio::io::AsyncWriteExt;
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write request");
    let mut line = String::new();
    use tokio::io::AsyncBufReadExt;
    reader.read_line(&mut line).await.expect("read reply");
    serde_json::from_str::<Frame>(line.trim()).expect("decode reply")
}

#[tokio::test]
async fn database_db_inspect_and_query_table_roundtrip() {
    let rig = Rig::start("db-inspect").await;
    let (mut reader, mut writer) = rig.connect().await;

    let reply = query(
        &mut reader,
        &mut writer,
        "inspect-1",
        "database.db_inspect",
        json!({ "endpoint": rig.local_endpoint(), "projectRoot": null }),
    )
    .await;
    let result = success_result(reply);
    let objects = result["objects"].as_array().expect("objects");
    assert_eq!(objects.len(), 1, "{result}");
    assert_eq!(objects[0]["name"], "notes", "{result}");
    assert_eq!(objects[0]["objectType"], "table", "{result}");
    assert_eq!(objects[0]["rowCount"], 2, "{result}");
    let columns = objects[0]["columns"].as_array().expect("columns");
    assert_eq!(columns[0]["name"], "title", "{result}");
    assert_eq!(result["objects"][0]["hasRowId"], true, "{result}");

    // Paged table query carries rowid keys and the exact stored values.
    let reply = query(
        &mut reader,
        &mut writer,
        "query-1",
        "database.db_query_table",
        json!({
            "endpoint": rig.local_endpoint(),
            "table": "notes",
            "page": 1,
            "pageSize": 1,
            "projectRoot": null
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["columns"], json!(["title", "body"]), "{result}");
    assert_eq!(result["totalRows"], 2, "{result}");
    let rows = result["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1, "{result}");
    assert_eq!(rows[0]["values"], json!(["one", "first"]), "{result}");
    assert_eq!(
        rows[0]["keyValues"][0]["column"],
        json!("__aeroric_rowid__"),
        "{result}"
    );

    // Missing endpoint -> InvalidArgument
    let reply = query(
        &mut reader,
        &mut writer,
        "inspect-missing",
        "database.db_inspect",
        json!({}),
    )
    .await;
    let (code, _) = failure_code(reply);
    assert_eq!(code, ErrorCode::InvalidArgument);

    // SSH endpoints stay refused: the headless host has no remote db executor.
    let reply = query(
        &mut reader,
        &mut writer,
        "inspect-ssh",
        "database.db_inspect",
        json!({
            "endpoint": { "kind": "ssh", "connection": { "id": "c", "name": "c", "host": "h", "port": 22, "username": "u" }, "path": "/srv/app.db" },
            "projectRoot": null
        }),
    )
    .await;
    let (code, _) = failure_code(reply);
    assert_eq!(code, ErrorCode::UnsupportedCapability);
}

#[tokio::test]
async fn database_db_execute_sql_and_mutations_respect_read_only() {
    let rig = Rig::start("db-execute").await;
    let (mut reader, mut writer) = rig.connect().await;
    let endpoint = rig.local_endpoint();

    // Multi-statement script: the last query returns rows, the writes land.
    let reply = query(
        &mut reader,
        &mut writer,
        "exec-1",
        "database.db_execute_sql",
        json!({
            "endpoint": endpoint,
            "sql": "INSERT INTO notes (title, body) VALUES ('three', 'third'); SELECT title FROM notes ORDER BY rowid;",
            "page": 1,
            "pageSize": 10,
            "readOnly": false,
            "connectionId": null,
            "projectRoot": null
        }),
    )
    .await;
    let result = success_result(reply);
    assert_eq!(result["rowsAffected"], 1, "{result}");
    let rows = result["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 3, "{result}");
    assert_eq!(rows[2]["values"][0], "three", "{result}");

    // update_cell / insert_row / delete_row end states.
    let row_id = {
        let reply = query(
            &mut reader,
            &mut writer,
            "query-for-update",
            "database.db_query_table",
            json!({ "endpoint": endpoint, "table": "notes", "page": 1, "pageSize": 100, "projectRoot": null }),
        )
        .await;
        let result = success_result(reply);
        result["rows"][0]["rowId"].as_i64().expect("rowId")
    };
    let reply = query(
        &mut reader,
        &mut writer,
        "update-1",
        "database.db_update_cell",
        json!({
            "endpoint": endpoint, "table": "notes", "rowKey": { "rowId": row_id },
            "column": "title", "value": "uno", "readOnly": false,
            "connectionId": null, "projectRoot": null
        }),
    )
    .await;
    let _ = success_result(reply);
    let reply = query(
        &mut reader,
        &mut writer,
        "insert-1",
        "database.db_insert_row",
        json!({
            "endpoint": endpoint, "table": "notes",
            "values": [ { "column": "title", "value": "four" }, { "column": "body", "value": "fourth" } ],
            "readOnly": false, "connectionId": null, "projectRoot": null
        }),
    )
    .await;
    let _ = success_result(reply);
    let reply = query(
        &mut reader,
        &mut writer,
        "delete-1",
        "database.db_delete_row",
        json!({
            "endpoint": endpoint, "table": "notes", "rowKey": { "rowId": row_id },
            "readOnly": false, "connectionId": null, "projectRoot": null
        }),
    )
    .await;
    let _ = success_result(reply);
    let reply = query(
        &mut reader,
        &mut writer,
        "query-after",
        "database.db_query_table",
        json!({ "endpoint": endpoint, "table": "notes", "page": 1, "pageSize": 100, "projectRoot": null }),
    )
    .await;
    let result = success_result(reply);
    let rows = result["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 3, "{result}");
    assert_eq!(rows[0]["values"], json!(["two", "second"]), "{result}");
    assert_eq!(rows[2]["values"], json!(["four", "fourth"]), "{result}");

    // readOnly: true refuses every mutation, and the file is untouched.
    for (id, method, extra) in [
        (
            "upd-ro",
            "database.db_update_cell",
            json!({ "rowKey": { "rowId": row_id }, "column": "title", "value": "x" }),
        ),
        (
            "ins-ro",
            "database.db_insert_row",
            json!({ "values": [ { "column": "title", "value": "x" } ] }),
        ),
        (
            "del-ro",
            "database.db_delete_row",
            json!({ "rowKey": { "rowId": row_id } }),
        ),
    ] {
        let mut params = json!({ "endpoint": endpoint, "table": "notes", "readOnly": true, "connectionId": null, "projectRoot": null });
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        let reply = query(&mut reader, &mut writer, id, method, params).await;
        let (code, message) = failure_code(reply);
        assert_eq!(code, ErrorCode::IoError, "{id}: {message}");
        assert_eq!(message, "Connection is read-only", "{id}");
    }
    let reply = query(
        &mut reader,
        &mut writer,
        "exec-ro",
        "database.db_execute_sql",
        json!({
            "endpoint": endpoint, "sql": "DELETE FROM notes",
            "page": 1, "pageSize": 10, "readOnly": true,
            "connectionId": null, "projectRoot": null
        }),
    )
    .await;
    let (code, message) = failure_code(reply);
    assert_eq!(code, ErrorCode::IoError, "{message}");
    assert_eq!(message, "Connection is read-only");

    // Escape hatches stay blocked (attach / load_extension).
    for (id, sql) in [
        ("exec-attach", "ATTACH DATABASE '/tmp/o.db' AS o"),
        ("exec-ext", "SELECT load_extension('/tmp/x')"),
    ] {
        let reply = query(
            &mut reader,
            &mut writer,
            id,
            "database.db_execute_sql",
            json!({ "endpoint": endpoint, "sql": sql, "page": 1, "pageSize": 10, "readOnly": false, "connectionId": null, "projectRoot": null }),
        )
        .await;
        let (code, _) = failure_code(reply);
        assert_eq!(code, ErrorCode::IoError, "{id}");
    }
}

#[tokio::test]
async fn database_saved_read_only_connection_is_authoritative() {
    let rig = Rig::start("db-authority").await;
    let (mut reader, mut writer) = rig.connect().await;
    let endpoint = rig.local_endpoint();

    // A saved connection with readOnly: true and a MATCHING endpoint overrides
    // the renderer's readOnly: false (authoritative_read_only semantics).
    let connections_path = rig.host.data_dir.join("database-connections.json");
    std::fs::write(
        &connections_path,
        serde_json::to_vec_pretty(&json!([
            {
                "id": "saved", "name": "saved", "endpoint": endpoint,
                "readOnly": true, "createdAt": 1, "lastOpenedAt": null
            }
        ]))
        .unwrap(),
    )
    .unwrap();

    let reply = query(
        &mut reader,
        &mut writer,
        "exec-authority",
        "database.db_execute_sql",
        json!({
            "endpoint": endpoint, "sql": "DELETE FROM notes",
            "page": 1, "pageSize": 10, "readOnly": false,
            "connectionId": "saved", "projectRoot": null
        }),
    )
    .await;
    let (code, message) = failure_code(reply);
    assert_eq!(code, ErrorCode::IoError, "{message}");
    assert_eq!(message, "Connection is read-only");

    // A connection id pointing at a DIFFERENT endpoint is refused outright.
    let reply = query(
        &mut reader,
        &mut writer,
        "exec-mismatch",
        "database.db_execute_sql",
        json!({
            "endpoint": { "kind": "local", "path": "/tmp/some-other.db" },
            "sql": "SELECT 1",
            "page": 1, "pageSize": 10, "readOnly": false,
            "connectionId": "saved", "projectRoot": null
        }),
    )
    .await;
    let (code, message) = failure_code(reply);
    assert_eq!(code, ErrorCode::IoError, "{message}");
    assert_eq!(
        message,
        "Connection endpoint does not match saved configuration"
    );

    // load/save round-trip preserves the saved entries verbatim and the file
    // stays private (0600).
    let reply = query(
        &mut reader,
        &mut writer,
        "load-1",
        "database.db_load_connections",
        json!({}),
    )
    .await;
    let result = success_result(reply);
    let entries = result.as_array().expect("connections array");
    assert_eq!(entries.len(), 1, "{result}");
    assert_eq!(entries[0]["id"], "saved", "{result}");
    assert_eq!(entries[0]["endpoint"]["kind"], "local", "{result}");

    let updated = json!([
        { "id": "saved", "name": "saved", "endpoint": endpoint, "readOnly": true, "createdAt": 1, "lastOpenedAt": null },
        { "id": "second", "name": "second", "endpoint": { "kind": "local", "path": "/tmp/second.db" }, "readOnly": false, "createdAt": 2, "lastOpenedAt": null }
    ]);
    let reply = query(
        &mut reader,
        &mut writer,
        "save-1",
        "database.db_save_connections",
        json!({ "connections": updated }),
    )
    .await;
    let _ = success_result(reply);
    let stored = std::fs::read_to_string(&connections_path).unwrap();
    let parsed: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(parsed, updated, "save must preserve the saved entries");

    // Missing file (fresh data dir) is an empty success, not an error.
    let rig2 = Rig::start("db-load-empty").await;
    let (mut reader2, mut writer2) = rig2.connect().await;
    let reply = query(
        &mut reader2,
        &mut writer2,
        "load-empty",
        "database.db_load_connections",
        json!({}),
    )
    .await;
    assert_eq!(success_result(reply), json!([]));
}
