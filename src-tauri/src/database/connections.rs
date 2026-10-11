use std::fs;
use std::path::PathBuf;

use aeroric_core::database::dbx_connections::{self as biz, DbxConnectionPaths};
use dbx_core::models::connection::DatabaseType;
use tauri::{Manager, State};

pub(crate) use aeroric_core::database::dbx_connections::{ensure_writable, parse_core_config};

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use super::types::AeroricDbConnectionConfig;

fn connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

pub(crate) async fn ensure_loaded(state: &DbxState) -> Result<(), String> {
    let paths = connection_paths()?;
    biz::ensure_loaded(state, &paths, &KeyringDbxConnectionSecretStore).await
}

pub(crate) async fn ensure_connected(state: &DbxState, connection_id: &str) -> Result<(), String> {
    let paths = connection_paths()?;
    biz::ensure_connected(
        state,
        &paths,
        &KeyringDbxConnectionSecretStore,
        connection_id,
    )
    .await
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

#[tauri::command]
pub async fn dbx_disconnect(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<(), String> {
    state
        .app_state
        .remove_connection_pools(&connection_id)
        .await;
    state
        .app_state
        .reset_connection_transport(&connection_id)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn dbx_backup_sqlite_database(
    state: State<'_, DbxState>,
    connection_id: String,
    destination_path: String,
) -> Result<(), String> {
    ensure_loaded(&state).await?;
    let connection = state
        .connections
        .read()
        .await
        .get(&connection_id)
        .cloned()
        .ok_or_else(|| "Connection not found".to_string())?;
    let config = parse_core_config(&connection)?;
    if config.db_type != DatabaseType::Sqlite {
        return Err("Only SQLite connections can be backed up".to_string());
    }
    let source_path = config.host.trim().to_string();
    if source_path.is_empty() || source_path.eq_ignore_ascii_case(":memory:") {
        return Err("SQLite backup requires a file-backed database".to_string());
    }
    let destination_path = destination_path.trim().to_string();
    if destination_path.is_empty() {
        return Err("Backup destination path is required".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        if !source.exists() {
            return Err("SQLite database file not found".to_string());
        }
        let destination = PathBuf::from(&destination_path);
        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let source_connection = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        source_connection
            .backup(rusqlite::DatabaseName::Main, &destination, None)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
