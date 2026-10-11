// ── 切片一百零一（database）：mongo 面 6 条改宿主转发壳。业务主体在 core
// `database::mongo_face`（run_cancellable 取消登记/可写闸/mongo_ops verbatim
// 上收）；Embedded 本地闭包与宿主服务调同一函数，External 绝不本地回退。

use dbx_core::db::mongo_driver::MongoDocumentResult;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::mongo_face;

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_mongo_list_databases<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_list_databases",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_list_databases(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_mongo_list_collections<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    database: String,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_list_collections",
        serde_json::json!({ "connectionId": connection_id, "database": database }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_list_collections(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                database,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn dbx_mongo_find_documents<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    skip: Option<u64>,
    limit: Option<i64>,
    filter: Option<String>,
    projection: Option<String>,
    sort: Option<String>,
    execution_id: Option<String>,
) -> Result<MongoDocumentResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_find_documents",
        serde_json::json!({ "connectionId": connection_id, "database": database, "collection": collection, "skip": skip, "limit": limit, "filter": filter, "projection": projection, "sort": sort, "executionId": execution_id }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_find_documents(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, database, collection, skip, limit, filter, projection, sort, execution_id)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_mongo_insert_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    doc_json: String,
) -> Result<String, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_insert_document",
        serde_json::json!({ "connectionId": connection_id, "database": database, "collection": collection, "docJson": doc_json }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_insert_document(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, database, collection, doc_json)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_mongo_update_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    doc_json: String,
    routing: Option<String>,
) -> Result<u64, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_update_document",
        serde_json::json!({ "connectionId": connection_id, "database": database, "collection": collection, "id": id, "docJson": doc_json, "routing": routing }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_update_document(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, database, collection, id, doc_json, routing)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_mongo_delete_documents<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    many: Option<bool>,
) -> Result<u64, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_mongo_delete_documents",
        serde_json::json!({ "connectionId": connection_id, "database": database, "collection": collection, "filterJson": filter_json, "many": many }),
        async {
            let paths = local_connection_paths()?;
            mongo_face::dbx_mongo_delete_documents(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, database, collection, filter_json, many)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
