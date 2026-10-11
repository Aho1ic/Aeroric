//! Real CLI clients, real authenticated host services, injected memory secrets.
//! Deliberately do not spawn production `serve` (its credential port is OS-wide).

use std::process::{Output, Stdio};
use std::time::Duration;

use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::DbxDatabaseType;
use serde_json::{json, Value};

#[path = "../../aeroric-host/tests/host_harness/mod.rs"]
mod host_harness;
use host_harness::Harness;

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
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

#[tokio::test]
async fn cli_dbx_five_services_and_explicit_rejections() {
    let host = Harness::start("dbxcli").await;
    assert_eq!(
        success(rpc(&host, "database.dbx_list_connections", json!({})).await),
        json!([])
    );
    let dbx = default_connection_config(
        "cli-one",
        "cli database",
        DbxDatabaseType::Sqlite,
        host.home.join("cli.sqlite").to_string_lossy().into_owned(),
        0,
        None,
        false,
    );
    // A new file may be opened by the SQLite driver; it is inside our owned root.
    std::fs::write(host.home.join("cli.sqlite"), []).unwrap();
    let connection = json!({
        "id":"cli-one", "name":"cli database", "dbType":"sqlite",
        "readOnly":false, "projectScope":null, "dbx":dbx,
        "createdAt":1, "lastOpenedAt":null, "migratedFromLegacy":null
    });
    success(
        rpc(
            &host,
            "database.dbx_save_connection",
            json!({"connection":connection}),
        )
        .await,
    );
    success(
        rpc(
            &host,
            "database.dbx_test_connection",
            json!({"connection":connection}),
        )
        .await,
    );
    success(
        rpc(
            &host,
            "database.dbx_connect",
            json!({"connectionId":"cli-one"}),
        )
        .await,
    );
    let list = success(rpc(&host, "database.dbx_list_connections", json!({})).await);
    assert_eq!(list[0]["id"], "cli-one");
    success(
        rpc(
            &host,
            "database.dbx_delete_connection",
            json!({"connectionId":"cli-one"}),
        )
        .await,
    );
    assert_eq!(
        success(rpc(&host, "database.dbx_list_connections", json!({})).await),
        json!([])
    );
    let output = rpc(
        &host,
        "database.dbx_connect",
        json!({"connectionId":"cli-one"}),
    )
    .await;
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Connection not found"));
    let output = rpc(&host, "database.dbx_save_connection", json!({})).await;
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("InvalidArgument"));
}

#[tokio::test]
async fn cli_dbx_readonly_host_refuses_before_database_initialization() {
    let host = Harness::start_read_only("dbxcliro").await;
    let output = rpc(&host, "database.dbx_list_connections", json!({})).await;
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("lease"));
    assert!(output.stdout.is_empty());
    assert!(!host.data_dir.join("dbx-core.db").exists());
    assert!(!host.data_dir.join("database-connections-v2.json").exists());
}
