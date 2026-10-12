use aeroric_core::database::dbx_connections::{self as biz, DbxConnectionPaths};
use tauri::Manager;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use super::types::AeroricDbConnectionConfig;

fn connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_list_connections<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<AeroricDbConnectionConfig>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_list_connections",
        serde_json::json!({}),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            biz::list_connections(&state, &paths, &KeyringDbxConnectionSecretStore)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_save_connection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection: AeroricDbConnectionConfig,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_save_connection",
        serde_json::json!({ "connection": connection }),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            biz::save_connection(&state, &paths, &KeyringDbxConnectionSecretStore, connection)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_delete_connection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection_id: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_delete_connection",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            biz::delete_connection(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                &connection_id,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_test_connection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection: AeroricDbConnectionConfig,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_test_connection",
        serde_json::json!({ "connection": connection }),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            biz::test_connection(&state, &paths, &KeyringDbxConnectionSecretStore, connection)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_connect<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection_id: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_connect",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            biz::connect(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                &connection_id,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

/// 切片一百零三：池生命周期上收 core（宿主与 Embedded 调 `dbx_connections::
/// disconnect` 同一主体）；此壳只做转发判定。
#[tauri::command]
pub async fn dbx_disconnect<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection_id: String,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_disconnect",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let state = app.state::<DbxState>();
            aeroric_core::database::dbx_connections::disconnect(&state, &connection_id)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

/// 切片一百零三：SQLite 备份主体上收 core `dbx_connections::
/// backup_sqlite_database`（spawn_blocking/只读源库/rusqlite backup verbatim）。
#[tauri::command]
pub async fn dbx_backup_sqlite_database<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connection_id: String,
    destination_path: String,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_backup_sqlite_database",
        serde_json::json!({ "connectionId": connection_id, "destinationPath": destination_path }),
        async {
            let state = app.state::<DbxState>();
            let paths = connection_paths()?;
            aeroric_core::database::dbx_connections::backup_sqlite_database(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                &connection_id,
                &destination_path,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}
