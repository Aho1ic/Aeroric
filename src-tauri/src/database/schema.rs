// ── 切片九十八（database 收尾）：schema 面 6 条改宿主转发壳。业务主体在
// core `database::schema_face`；Embedded 本地闭包与宿主服务调同一函数，
// External 绝不本地回退。

use dbx_core::db;
use tauri::State;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::schema_face;

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_list_databases<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<db::DatabaseInfo>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_list_databases",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let paths = local_connection_paths()?;
            schema_face::list_databases(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_list_schemas<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_list_schemas",
        serde_json::json!({ "connectionId": connection_id, "database": database }),
        async {
            let paths = local_connection_paths()?;
            schema_face::list_schemas(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn dbx_list_objects<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    filter: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    object_types: Option<Vec<String>>,
) -> Result<Vec<db::ObjectInfo>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_list_objects",
        serde_json::json!({
            "connectionId": connection_id,
            "database": database,
            "schema": schema,
            "filter": filter,
            "limit": limit,
            "offset": offset,
            "objectTypes": object_types
        }),
        async {
            let paths = local_connection_paths()?;
            schema_face::list_objects(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
                schema,
                filter,
                limit,
                offset,
                object_types,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_get_columns<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<Vec<db::ColumnInfo>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_get_columns",
        serde_json::json!({
            "connectionId": connection_id,
            "database": database,
            "schema": schema,
            "table": table
        }),
        async {
            let paths = local_connection_paths()?;
            schema_face::get_columns(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
                schema,
                table,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_get_table_ddl<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<String, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_get_table_ddl",
        serde_json::json!({
            "connectionId": connection_id,
            "database": database,
            "schema": schema,
            "table": table
        }),
        async {
            let paths = local_connection_paths()?;
            schema_face::get_table_ddl(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
                schema,
                table,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_get_object_source<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    name: String,
    object_type: db::ObjectSourceKind,
    signature: Option<String>,
) -> Result<db::ObjectSource, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_get_object_source",
        serde_json::json!({
            "connectionId": connection_id,
            "database": database,
            "schema": schema,
            "name": name,
            "objectType": object_type,
            "signature": signature
        }),
        async {
            let paths = local_connection_paths()?;
            schema_face::get_object_source(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
                schema,
                name,
                object_type,
                signature,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
