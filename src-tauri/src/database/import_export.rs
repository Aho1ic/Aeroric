// ── 切片一百零二（database）：import/export 面 10 条改宿主转发壳。业务主体
// 在 core `database::ie_face`（Excel ZIP 炸弹校验层/导出导入/SQL 文件执行的
// 生产确认闸 verbatim 上收）；Embedded 本地闭包与宿主服务调同一函数，
// External 绝不本地回退。

use dbx_core::db;
use tauri::Manager;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::ie_face::{
    self, DatabaseExportRequest, TableExportRequest, TableImportPreview, TableImportRequest,
    TableImportSummary,
};

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

/// Embedded 本地路径的生产确认出口：窗口 emit（legacy 通道名逐字）。
fn window_confirm_sink<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> aeroric_core::database::execution::ConfirmEventSink {
    use aeroric_core::database::execution::ProductionConfirmRequest;
    let app = app.clone();
    std::sync::Arc::new(move |request: &ProductionConfirmRequest| {
        if let Ok(payload) = serde_json::to_value(request) {
            let _ = tauri::Emitter::emit(
                &app,
                aeroric_core::database::execution::PRODUCTION_CONFIRM_EVENT,
                payload,
            );
        }
    })
}

#[tauri::command]
pub async fn dbx_export_table_csv<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_csv",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_table(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                ie_face::export_request_with_format(request, "csv"),
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_table_json<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_json",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_table(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                ie_face::export_request_with_format(request, "json"),
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_table_markdown<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_markdown",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_table(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                ie_face::export_request_with_format(request, "markdown"),
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_table_insert_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_insert_sql",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_table(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                ie_face::export_request_with_format(request, "sql"),
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_table_update_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_update_sql",
        serde_json::json!({ "request": &request }),
        async { Err(ie_face::unsupported_update_sql_export()) },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_table_xlsx<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_table_xlsx",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_table(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                ie_face::export_request_with_format(request, "xlsx"),
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_export_database<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: DatabaseExportRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_export_database",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::export_database(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_preview_table_import_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    file_path: String,
) -> Result<TableImportPreview, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_preview_table_import_file",
        serde_json::json!({ "filePath": file_path }),
        async {
            ie_face::preview_table_import_file(file_path)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value::<TableImportPreview>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_import_table_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: TableImportRequest,
) -> Result<TableImportSummary, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_import_table_file",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            ie_face::import_table_file(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value::<TableImportSummary>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_execute_sql_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: ie_face::ExecuteSqlFileRequest,
) -> Result<Vec<db::QueryResult>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_execute_sql_file",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            let sink = window_confirm_sink(&app);
            ie_face::execute_sql_file(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                &sink,
                request,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value::<Vec<db::QueryResult>>(value).map_err(|error| error.to_string())
}
