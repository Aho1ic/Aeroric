use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::execution;
use aeroric_core::database::grid::DataGridSaveStatementOptions;
use aeroric_core::database::grid::SqlPreviewResponse;
use serde_json::Value;
use tauri::State;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;

// ── 切片九十七（database 刀三）：grid 数据/保存四条改宿主转发壳。业务主体
// 在 core `database::execution`（query_table_data/execute_grid_save）；
// Embedded 本地闭包与宿主服务调同一函数，External 绝不本地回退。

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_preview_grid_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: DataGridSaveStatementOptions,
) -> Result<SqlPreviewResponse, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_preview_grid_sql",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(aeroric_core::database::grid::preview_grid_sql(options))
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_insert_statement<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridCopyInsertStatementOptions,
) -> Result<Option<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_copy_insert_statement",
        serde_json::json!({ "options": &options }),
        async {
            Ok(
                aeroric_core::database::grid::build_data_grid_copy_insert_statement(options)
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_update_statements<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridCopyUpdateStatementOptions,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_copy_update_statements",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(
                aeroric_core::database::grid::build_data_grid_copy_update_statements(options),
            )
            .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_context_filter_condition<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridContextFilterConditionOptions,
) -> Result<Option<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_context_filter_condition",
        serde_json::json!({ "options": &options }),
        async {
            Ok(
                aeroric_core::database::grid::build_data_grid_context_filter_condition(options)
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_query_table_data<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: execution::GridTableDataRequest,
) -> Result<Value, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_query_table_data",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::query_table_data(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
        },
    )
    .await?;
    Ok(value)
}

#[tauri::command]
pub async fn dbx_update_cell<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: execution::GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_update_cell",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::execute_grid_save(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_insert_row<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: execution::GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_insert_row",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::execute_grid_save(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_delete_rows<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: execution::GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_delete_rows",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::execute_grid_save(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
