//! Slice 98: schema / driver manifest face over live UDS. The six connection
//! face methods run against a real scratch SQLite file (saved through the
//! real save service); the driver manifest is a static asset. Unregistered
//! redis/mongo/transfer faces stay UnsupportedCapability.

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

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

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request("schema-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "schema").await);
    (reader, writer)
}

/// 一个真 SQLite 文件（含一张 users 表），通过真实的保存服务注册连接。
async fn seed_sqlite_connection(host: &Harness) -> String {
    let db_path = host.home.join("schema.sqlite");
    let connection = rusqlite::Connection::open(&db_path).expect("open scratch sqlite");
    connection
        .execute_batch("CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);")
        .expect("create table");

    let (mut reader, mut writer) = authenticated(host).await;
    let body = json!({
        "id": "schema-1",
        "name": "Schema",
        "dbType": "sqlite",
        "readOnly": false,
        "dbx": {
            "id": "schema-1",
            "name": "Schema",
            "db_type": "sqlite",
            "host": db_path.to_string_lossy(),
            "port": 0,
            "username": "",
            "password": "",
            "is_production": false
        },
        "createdAt": 0
    });
    success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({ "connection": body }),
        )
        .await,
    );
    "schema-1".to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn driver_manifest_is_served_statically() {
    let host = Harness::start("schema-manifest").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let manifest = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_driver_manifest",
            json!({}),
        )
        .await,
    );
    let driver_types = manifest["drivers"]
        .as_array()
        .expect("drivers array")
        .iter()
        .filter_map(|driver| driver["dbType"].as_str())
        .collect::<Vec<_>>();
    for expected in ["sqlite", "postgres", "mysql", "redis", "mongodb"] {
        assert!(driver_types.contains(&expected), "{expected} in manifest");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn schema_face_reads_a_real_sqlite_connection() {
    let host = Harness::start("schema-face").await;
    let connection_id = seed_sqlite_connection(&host).await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let databases = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_list_databases",
            json!({ "connectionId": connection_id }),
        )
        .await,
    );
    assert!(
        databases.as_array().is_some(),
        "list_databases returns an array"
    );

    let columns = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_get_columns",
            json!({ "connectionId": connection_id, "table": "users" }),
        )
        .await,
    );
    let names = columns
        .as_array()
        .expect("columns array")
        .iter()
        .filter_map(|column| column["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["id", "email"]);

    let objects = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_list_objects",
            json!({ "connectionId": connection_id, "objectTypes": ["table"] }),
        )
        .await,
    );
    let tables = objects
        .as_array()
        .expect("objects array")
        .iter()
        .filter_map(|object| object["name"].as_str())
        .collect::<Vec<_>>();
    assert!(tables.contains(&"users"), "users table listed: {tables:?}");

    let ddl = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_get_table_ddl",
            json!({ "connectionId": connection_id, "table": "users" }),
        )
        .await,
    );
    assert!(ddl.as_str().is_some(), "ddl is a string: {ddl:?}");

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_get_columns",
            json!({ "connectionId": "missing", "table": "users" }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(!message.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_params_refuse_and_unregistered_family_stays_unsupported() {
    let host = Harness::start("schema-boundary").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_get_columns",
            json!({}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("connectionId"));

    // 切片一百零三上收 transfer／生命周期面——database 族缺口清零；此后这
    // 条控制组守「未知方法名必须保持不可达」：宿主不虚报能力。
    for method in ["database.dbx_not_a_registered_method"] {
        let reply = call(&mut reader, &mut writer, method, json!({})).await;
        assert_eq!(
            failure(reply, ErrorCode::UnsupportedCapability),
            "this host does not offer the requested method",
            "{method}"
        );
    }
}
