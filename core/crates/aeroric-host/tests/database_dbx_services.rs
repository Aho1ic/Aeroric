//! Slice 95: authenticated DBX connection services. Every file is below the
//! harness scratch root; credentials use injected memory, never OS Keychain.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aeroric_core::context::{MemorySecrets, SecretKey, SecretStore};
use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::{AeroricDbConnectionConfig, DbxDatabaseType};
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

#[derive(Default)]
struct ObservedSecrets {
    memory: MemorySecrets,
    calls: AtomicUsize,
}

impl SecretStore for ObservedSecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.get(key)
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.set(key, value)
    }
    fn delete(&self, key: &SecretKey) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.delete(key)
    }
}

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

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request("dbx-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "dbx").await);
    (reader, writer)
}

fn sqlite_connection(host: &Harness, id: &str) -> Value {
    let path = host.home.join(format!("{id}.sqlite"));
    let config = default_connection_config(
        id,
        &format!("conn-{id}"),
        DbxDatabaseType::Sqlite,
        path.to_string_lossy().into_owned(),
        0,
        None,
        false,
    );
    serde_json::to_value(AeroricDbConnectionConfig {
        id: id.to_string(),
        name: format!("conn-{id}"),
        db_type: DbxDatabaseType::Sqlite,
        read_only: false,
        project_scope: None,
        // Inner DBX wire is snake_case; outer Aeroric DTO is camelCase.
        dbx: serde_json::to_value(config).unwrap(),
        created_at: 1,
        last_opened_at: None,
        migrated_from_legacy: None,
        connection_group: None,
        pinned: None,
    })
    .unwrap()
}

#[tokio::test]
async fn database_dbx_initializes_on_the_live_runtime() {
    let host = Harness::start("dbxi").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_list_connections",
                json!({})
            )
            .await
        ),
        json!([])
    );
    assert!(host.data_dir.join("dbx-core.db").is_file());
}

#[tokio::test]
async fn database_dbx_save_list_connect_roundtrip_and_delete() {
    let secrets = Arc::new(MemorySecrets::default());
    let host = Harness::start_with_secrets("dbxc", false, secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let mut connection = sqlite_connection(&host, "one");
    connection["dbx"]["password"] = json!("fixture-only-secret");
    rusqlite::Connection::open(host.home.join("one.sqlite")).unwrap();
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_save_connection",
                json!({"connection": connection})
            )
            .await
        ),
        Value::Null
    );
    let result = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["dbType"], "sqlite");
    assert_eq!(result[0]["dbx"]["db_type"], "sqlite");
    assert_eq!(result[0]["dbx"]["password"], "");
    assert!(secrets
        .peek("dbx-connection-secrets:one")
        .unwrap()
        .contains("fixture-only-secret"));
    let disk = std::fs::read_to_string(host.data_dir.join("database-connections-v2.json")).unwrap();
    assert!(!disk.contains("fixture-only-secret"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(host.data_dir.join("database-connections-v2.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    // A SQLite password means SQLCipher, which this build intentionally lacks.
    // Verify the real refusal rather than enabling a driver to suit the fixture.
    let message = failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_connect",
            json!({"connectionId":"one"}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("SQLCipher"), "{message}");
    let mut edited = result[0].clone();
    // Legacy list redacts every key containing "password", including this
    // non-secret flag. Preserve that output wire in this migration; edits must
    // supply the typed input flag rather than replaying the raw list verbatim.
    assert_eq!(edited["dbx"]["save_password"], "");
    edited["dbx"]["save_password"] = json!(true);
    edited["name"] = json!("renamed without exposing the password");
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":edited}),
        )
        .await,
    );
    assert!(secrets
        .peek("dbx-connection-secrets:one")
        .unwrap()
        .contains("fixture-only-secret"));
    edited["dbx"]["save_password"] = json!(false);
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":edited}),
        )
        .await,
    );
    assert!(secrets.peek("dbx-connection-secrets:one").is_none());
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_connect",
            json!({"connectionId":"one"}),
        )
        .await,
    );
    for _ in 0..2 {
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_delete_connection",
                json!({"connectionId":"one"}),
            )
            .await,
        );
    }
    assert!(secrets.peek("dbx-connection-secrets:one").is_none());
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_list_connections",
                json!({})
            )
            .await
        ),
        json!([])
    );
    assert_eq!(
        failure(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_connect",
                json!({"connectionId":"one"})
            )
            .await,
            ErrorCode::IoError
        ),
        "Connection not found"
    );
}

#[tokio::test]
async fn database_dbx_test_connection_opens_a_real_sqlite_pool() {
    let host = Harness::start("dbxt").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let db = rusqlite::Connection::open(host.home.join("test.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
    drop(db);
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_test_connection",
            json!({"connection":sqlite_connection(&host, "test")}),
        )
        .await,
    );
    // A temporary test connection is not saved or left in the public registry.
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_list_connections",
                json!({})
            )
            .await
        ),
        json!([])
    );
    assert!(!host.data_dir.join("database-connections-v2.json").exists());
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_test_connection",
            json!({}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
}

#[tokio::test]
async fn database_dbx_requires_writer_before_initialization_or_secret_io() {
    let secrets = Arc::new(ObservedSecrets {
        memory: MemorySecrets::with(&[("dbx-connection-secrets:sentinel", "keep")]),
        ..Default::default()
    });
    let host = Harness::start_with_secrets("dbxro", true, secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    for (method, params) in [
        ("database.dbx_list_connections", json!({})),
        (
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        ),
        (
            "database.dbx_delete_connection",
            json!({"connectionId":"sentinel"}),
        ),
        (
            "database.dbx_test_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        ),
        ("database.dbx_connect", json!({"connectionId":"one"})),
    ] {
        let message = failure(
            query(&mut reader, &mut writer, method, params).await,
            ErrorCode::IoError,
        );
        assert!(message.contains("lease"), "{message}");
        for file in [
            "dbx-core.db",
            "dbx-plugins",
            "dbx-agents",
            "database-connections-v2.json",
        ] {
            assert!(
                !host.data_dir.join(file).exists(),
                "{method} touched {file}"
            );
        }
    }
    assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        secrets.memory.keys(),
        vec!["dbx-connection-secrets:sentinel"]
    );
    assert_eq!(
        secrets
            .memory
            .peek("dbx-connection-secrets:sentinel")
            .as_deref(),
        Some("keep")
    );
}

#[tokio::test]
async fn database_dbx_invalid_wire_is_rejected_before_initialization() {
    let host = Harness::start("dbxbad").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    for (method, params) in [
        ("database.dbx_list_connections", json!([])),
        ("database.dbx_save_connection", json!({})),
        ("database.dbx_test_connection", json!({"connection":42})),
        (
            "database.dbx_delete_connection",
            json!({"connectionId":false}),
        ),
        ("database.dbx_connect", json!({})),
    ] {
        failure(
            query(&mut reader, &mut writer, method, params).await,
            ErrorCode::InvalidArgument,
        );
        assert!(
            !host.data_dir.join("dbx-core.db").exists(),
            "{method} initialized DBX"
        );
    }
}

#[tokio::test]
async fn database_dbx_unauthenticated_request_has_no_side_effects() {
    let host = Harness::start("dbxauth").await;
    let (mut reader, mut writer) = host.connect().await;
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        )
        .await,
        ErrorCode::Unauthenticated,
    );
    assert!(!host.data_dir.join("dbx-core.db").exists());
    assert!(!host.data_dir.join("database-connections-v2.json").exists());
}

#[tokio::test]
async fn database_dbx_two_hosts_never_share_registry_or_storage() {
    let a = Harness::start("dbxa").await;
    let b = Harness::start("dbxb").await;
    let (mut ar, mut aw) = authenticated(&a).await;
    let (mut br, mut bw) = authenticated(&b).await;
    success(
        query(
            &mut ar,
            &mut aw,
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&a, "same")}),
        )
        .await,
    );
    assert_eq!(
        success(query(&mut br, &mut bw, "database.dbx_list_connections", json!({})).await),
        json!([])
    );
    let mut other = sqlite_connection(&b, "same");
    other["name"] = json!("independent");
    success(
        query(
            &mut br,
            &mut bw,
            "database.dbx_save_connection",
            json!({"connection":other}),
        )
        .await,
    );
    let al = success(query(&mut ar, &mut aw, "database.dbx_list_connections", json!({})).await);
    let bl = success(query(&mut br, &mut bw, "database.dbx_list_connections", json!({})).await);
    assert_eq!(al[0]["name"], "conn-same");
    assert_eq!(bl[0]["name"], "independent");
    assert_ne!(al[0]["dbx"]["host"], bl[0]["dbx"]["host"]);
    for host in [&a, &b] {
        assert!(host.data_dir.join("dbx-core.db").is_file());
        assert!(host.data_dir.join("database-connections-v2.json").is_file());
    }
}

#[tokio::test]
async fn database_dbx_failed_persistence_does_not_publish_new_registry() {
    let host = Harness::start("dbxfail").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let original = sqlite_connection(&host, "one");
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":original}),
        )
        .await,
    );
    let path = host.data_dir.join("database-connections-v2.json");
    let original_disk = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap(); // deterministic rename failure, even as root
    let mut updated = original.clone();
    updated["name"] = json!("must-not-publish");
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":updated}),
        )
        .await,
        ErrorCode::IoError,
    );
    let live = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(live[0]["name"], original["name"]);
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_delete_connection",
            json!({"connectionId":"one"}),
        )
        .await,
        ErrorCode::IoError,
    );
    let live = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(live.as_array().unwrap().len(), 1);
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, original_disk).unwrap();
}

#[tokio::test]
async fn database_dbx_restart_hydrates_secrets_and_retains_metadata() {
    let secrets = Arc::new(MemorySecrets::default());
    let host = Harness::start_with_secrets("dbxrestart", false, secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let mut connection = sqlite_connection(&host, "persisted");
    connection["dbx"]["password"] = json!("restart-fixture-secret");
    connection["connectionGroup"] = json!("isolated");
    connection["pinned"] = json!(true);
    connection["lastOpenedAt"] = json!(23);
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":connection}),
        )
        .await,
    );
    let before = std::fs::read(host.data_dir.join("database-connections-v2.json")).unwrap();
    drop((reader, writer));
    let host = host.restart(secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let list = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(list[0]["connectionGroup"], "isolated");
    assert_eq!(list[0]["pinned"], true);
    assert_eq!(list[0]["lastOpenedAt"], 23);
    assert_eq!(list[0]["dbx"]["password"], "");
    assert_eq!(
        std::fs::read(host.data_dir.join("database-connections-v2.json")).unwrap(),
        before
    );
    let mut edited = list[0].clone();
    edited["dbx"]["save_password"] = json!(true);
    edited["name"] = json!("after restart");
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":edited}),
        )
        .await,
    );
    assert!(secrets
        .peek("dbx-connection-secrets:persisted")
        .unwrap()
        .contains("restart-fixture-secret"));
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_delete_connection",
            json!({"connectionId":"persisted"}),
        )
        .await,
    );
    assert!(secrets.peek("dbx-connection-secrets:persisted").is_none());
}

#[tokio::test]
async fn database_dbx_legacy_migration_and_plaintext_v2_load_are_preserved() {
    let secrets = Arc::new(MemorySecrets::default());
    let host = Harness::start_with_secrets("dbxmigrate", false, secrets.clone()).await;
    let legacy = json!([{
        "id":"legacy", "name":"old", "endpoint":{"kind":"local","path":host.home.join("legacy.sqlite")},
        "readOnly":true, "createdAt":5, "lastOpenedAt":7
    }]);
    let original = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(host.data_dir.join("database-connections.json"), &original).unwrap();
    let (mut reader, mut writer) = authenticated(&host).await;
    let list = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(list[0]["migratedFromLegacy"], true);
    assert_eq!(list[0]["readOnly"], true);
    assert_eq!(list[0]["dbx"]["read_only"], true);
    assert_eq!(list[0]["createdAt"], 5);
    assert_eq!(
        std::fs::read(host.data_dir.join("database-connections.json")).unwrap(),
        original
    );
    assert!(host.data_dir.join("database-connections-v2.json").is_file());
    drop((reader, writer));
    // Simulate a legacy plaintext v2 file; load migrates the store, the next save
    // blanks the file (not an invented eager disk-rewrite policy).
    let mut plaintext = list[0].clone();
    plaintext["dbx"]["save_password"] = json!(true);
    plaintext["dbx"]["password"] = json!("legacy-plaintext-fixture");
    std::fs::write(
        host.data_dir.join("database-connections-v2.json"),
        serde_json::to_vec(&json!([plaintext])).unwrap(),
    )
    .unwrap();
    let host = host.restart(secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let list = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(list[0]["dbx"]["password"], "");
    assert!(secrets
        .peek("dbx-connection-secrets:legacy")
        .unwrap()
        .contains("legacy-plaintext-fixture"));
    let mut edited = list[0].clone();
    edited["dbx"]["save_password"] = json!(true);
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":edited}),
        )
        .await,
    );
    assert!(
        !std::fs::read_to_string(host.data_dir.join("database-connections-v2.json"))
            .unwrap()
            .contains("legacy-plaintext-fixture")
    );
}

#[tokio::test]
async fn database_dbx_concurrent_saves_keep_every_connection_on_disk() {
    let host = Harness::start("dbxparallel").await;
    let mut calls = Vec::new();
    for n in 0..12 {
        let (mut reader, mut writer) = authenticated(&host).await;
        let mut connection = sqlite_connection(&host, &format!("item-{n}"));
        connection["createdAt"] = json!(n);
        calls.push(tokio::spawn(async move {
            success(
                query(
                    &mut reader,
                    &mut writer,
                    "database.dbx_save_connection",
                    json!({"connection":connection}),
                )
                .await,
            );
        }));
    }
    for call in calls {
        call.await.unwrap();
    }
    let disk: Value = serde_json::from_slice(
        &std::fs::read(host.data_dir.join("database-connections-v2.json")).unwrap(),
    )
    .unwrap();
    let (mut reader, mut writer) = authenticated(&host).await;
    let list = success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    assert_eq!(list.as_array().unwrap().len(), 12);
    assert_eq!(disk.as_array().unwrap().len(), 12);
    for (index, connection) in list.as_array().unwrap().iter().enumerate() {
        assert_eq!(connection["createdAt"], 11 - index);
        assert_eq!(connection["id"], disk[index]["id"]);
        assert_eq!(connection["name"], disk[index]["name"]);
        assert_eq!(connection["dbx"]["host"], disk[index]["dbx"]["host"]);
        assert_eq!(disk[index]["dbx"]["save_password"], true);
        assert_eq!(connection["dbx"]["save_password"], "");
    }
}

#[tokio::test]
async fn database_dbx_corrupt_connection_file_is_not_overwritten_and_can_retry() {
    let host = Harness::start("dbxcorrupt").await;
    let path = host.data_dir.join("database-connections-v2.json");
    let corrupt = b"[ { unfinished config";
    std::fs::write(&path, corrupt).unwrap();
    let (mut reader, mut writer) = authenticated(&host).await;
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
        ErrorCode::IoError,
    );
    failure(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    std::fs::write(&path, b"[]").unwrap();
    assert_eq!(
        success(
            query(
                &mut reader,
                &mut writer,
                "database.dbx_list_connections",
                json!({})
            )
            .await
        ),
        json!([])
    );
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        )
        .await,
    );
}

#[tokio::test]
async fn database_dbx_storage_degradation_is_queryable_once_and_keeps_json_root() {
    let host = Harness::start("dbxdegrade").await;
    std::fs::create_dir(host.data_dir.join("dbx-core.db")).unwrap();
    let (mut reader, mut writer) = authenticated(&host).await;
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":sqlite_connection(&host, "one")}),
        )
        .await,
    );
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_list_connections",
            json!({}),
        )
        .await,
    );
    let records = success(
        query(
            &mut reader,
            &mut writer,
            "diagnostics.list_startup_degradations",
            json!({}),
        )
        .await,
    );
    assert_eq!(records.as_array().unwrap().len(), 1, "{records}");
    assert_eq!(records[0]["component"], "dbx-state");
    assert_eq!(records[0]["fallback"], ":memory:");
    assert!(!records[0]["reason"].as_str().unwrap().is_empty());
    assert!(host.data_dir.join("database-connections-v2.json").is_file());
    assert!(host.data_dir.join("dbx-core.db").is_dir());
}

#[tokio::test]
async fn database_dbx_rechecks_lost_writer_on_an_existing_connection() {
    let secrets = Arc::new(ObservedSecrets::default());
    let host = Harness::start_with_secrets("dbxlost", false, secrets.clone()).await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let connection = sqlite_connection(&host, "one");
    success(
        query(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":connection}),
        )
        .await,
    );
    let path = host.data_dir.join("database-connections-v2.json");
    let bytes = std::fs::read(&path).unwrap();
    let calls = secrets.calls.load(Ordering::SeqCst);
    let lock = host.data_dir.join(".write.lock");
    std::fs::rename(&lock, lock.with_extension("retired")).unwrap();
    std::fs::write(&lock, b"replacement inode").unwrap();
    for (method, params) in [
        ("database.dbx_list_connections", json!({})),
        (
            "database.dbx_save_connection",
            json!({"connection":connection}),
        ),
        (
            "database.dbx_delete_connection",
            json!({"connectionId":"one"}),
        ),
        (
            "database.dbx_test_connection",
            json!({"connection":connection}),
        ),
        ("database.dbx_connect", json!({"connectionId":"one"})),
    ] {
        let message = failure(
            query(&mut reader, &mut writer, method, params).await,
            ErrorCode::IoError,
        );
        assert!(message.contains("lost the writer lease"), "{message}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(secrets.calls.load(Ordering::SeqCst), calls);
}

#[test]
fn database_dbx_default_library_startup_never_selects_os_keyring() {
    let source = include_str!("../src/startup.rs");
    let wrapper = source
        .split("pub async fn run_startup(")
        .nth(1)
        .unwrap()
        .split("pub async fn run_startup_with_secrets(")
        .next()
        .unwrap();
    assert!(wrapper.contains("run_startup_with_secrets("));
    assert!(wrapper.contains("MemorySecrets::default()"));
    assert!(!wrapper.contains("KeyringSecrets"));
    let composition = include_str!("../../aeroric-cli/src/main.rs");
    assert!(composition.contains("run_startup_with_secrets("));
    assert!(composition.contains("Arc::new(aeroric_host::platform_adapter::KeyringSecrets)"));
}
