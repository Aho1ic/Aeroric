use std::fs;
use std::path::PathBuf;

use dbx_core::db;
use dbx_core::query::QueryExecutionOptions;
use dbx_core::table_export::TableExportRequest;
#[cfg(test)]
use dbx_core::table_export::{ExportStatus, TableExportProgress};
use dbx_core::table_import::{TableImportPreview, TableImportRequest, TableImportSummary};
use serde::Deserialize;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteSqlFileRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    path: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

fn export_request_with_format(mut request: TableExportRequest, format: &str) -> TableExportRequest {
    request.format = format.to_string();
    request
}

async fn export_table(state: &DbxState, request: TableExportRequest) -> Result<(), String> {
    connections::ensure_connected(state, &request.connection_id).await?;
    let export_id = request.export_id.clone();
    dbx_core::table_export::export_table_data_core(state.app_state.as_ref(), &request, |_| {})
        .await?;
    dbx_core::database_export::clear_export_cancelled(&export_id).await;
    Ok(())
}

fn validate_sql_file_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("SQL file path must be absolute".to_string());
    }
    if path.extension().and_then(|value| value.to_str()) != Some("sql") {
        return Err("Only .sql files can be executed from this action".to_string());
    }
    Ok(path)
}

fn unsupported_update_sql_export() -> String {
    "UPDATE SQL table export is not supported by the bundled DBX Core exporter yet".to_string()
}

#[tauri::command]
pub async fn dbx_export_table_csv(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    export_table(&state, export_request_with_format(request, "csv")).await
}

#[tauri::command]
pub async fn dbx_export_table_json(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    export_table(&state, export_request_with_format(request, "json")).await
}

#[tauri::command]
pub async fn dbx_export_table_markdown(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    export_table(&state, export_request_with_format(request, "markdown")).await
}

#[tauri::command]
pub async fn dbx_export_table_insert_sql(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    export_table(&state, export_request_with_format(request, "sql")).await
}

#[tauri::command]
pub async fn dbx_export_table_update_sql(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    let _ = (state, request);
    Err(unsupported_update_sql_export())
}

#[tauri::command]
pub async fn dbx_export_table_xlsx(
    state: State<'_, DbxState>,
    request: TableExportRequest,
) -> Result<(), String> {
    export_table(&state, export_request_with_format(request, "xlsx")).await
}

#[tauri::command]
pub async fn dbx_export_database(
    state: State<'_, DbxState>,
    request: dbx_core::database_export::DatabaseExportRequest,
) -> Result<(), String> {
    connections::ensure_connected(&state, &request.connection_id).await?;
    let export_id = request.export_id.clone();
    dbx_core::database_export::export_database_sql_core(state.app_state.as_ref(), &request, |_| {})
        .await?;
    dbx_core::database_export::clear_export_cancelled(&export_id).await;
    Ok(())
}

#[tauri::command]
pub async fn dbx_preview_table_import_file(
    file_path: String,
) -> Result<TableImportPreview, String> {
    dbx_core::table_import::preview_table_import_file_core(&file_path).await
}

#[tauri::command]
pub async fn dbx_import_table_file(
    state: State<'_, DbxState>,
    request: TableImportRequest,
) -> Result<TableImportSummary, String> {
    connections::ensure_connected(&state, &request.connection_id).await?;
    connections::ensure_writable(&state, &request.connection_id, "Import").await?;
    let db_type = state
        .app_state
        .configs
        .read()
        .await
        .get(&request.connection_id)
        .map(|config| config.db_type)
        .ok_or_else(|| "Connection config not found".to_string())?;
    let pool_key = if request.database.trim().is_empty() {
        request.connection_id.clone()
    } else {
        state
            .app_state
            .get_or_create_pool(&request.connection_id, Some(&request.database))
            .await?
    };

    dbx_core::table_import::import_table_file_core(
        &state.app_state,
        &request,
        &db_type,
        &pool_key,
        |_import_id| Box::pin(async { false }),
        |_| {},
    )
    .await
}

#[tauri::command]
pub async fn dbx_execute_sql_file(
    state: State<'_, DbxState>,
    request: ExecuteSqlFileRequest,
) -> Result<Vec<db::QueryResult>, String> {
    connections::ensure_connected(&state, &request.connection_id).await?;
    let path = validate_sql_file_path(&request.path)?;
    let sql = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let execution_id = format!("sql-file:{}", uuid::Uuid::new_v4());
    let registered_query = state
        .app_state
        .running_queries
        .register(execution_id.clone());
    let database = request.database.unwrap_or_default();
    dbx_core::query::execute_multi_core_with_options(
        &state.app_state,
        &request.connection_id,
        &database,
        &sql,
        request.schema.as_deref(),
        Some(registered_query.token()),
        QueryExecutionOptions {
            timeout_secs: request.timeout_secs,
            execution_id: Some(execution_id),
            ..Default::default()
        },
    )
    .await
}

#[cfg(test)]
pub(crate) fn error_progress(export_id: String, error: String) -> TableExportProgress {
    TableExportProgress {
        export_id,
        table_name: String::new(),
        rows_exported: 0,
        total_rows: None,
        status: ExportStatus::Error,
        error_message: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        error_progress, export_request_with_format, unsupported_update_sql_export,
        validate_sql_file_path,
    };
    use dbx_core::database_export::ExportStatus;
    use dbx_core::table_export::TableExportRequest;

    fn request() -> TableExportRequest {
        TableExportRequest {
            export_id: "e".to_string(),
            connection_id: "c".to_string(),
            database: "main".to_string(),
            schema: None,
            table_name: "users".to_string(),
            file_path: "/tmp/users.csv".to_string(),
            format: "json".to_string(),
            columns: None,
            column_types: None,
            primary_keys: None,
            where_input: None,
            order_by: None,
            skip_count: true,
            batch_size: None,
        }
    }

    #[test]
    fn overrides_table_export_format() {
        assert_eq!(export_request_with_format(request(), "csv").format, "csv");
    }

    #[test]
    fn rejects_non_sql_file_for_sql_runner() {
        assert!(validate_sql_file_path("/tmp/a.txt").is_err());
    }

    #[test]
    fn reports_update_sql_export_as_unsupported() {
        assert!(unsupported_update_sql_export().contains("not supported"));
    }

    #[test]
    fn builds_error_progress_payload() {
        let progress = error_progress("e".to_string(), "failed".to_string());
        assert!(matches!(progress.status, ExportStatus::Error));
        assert_eq!(progress.error_message.as_deref(), Some("failed"));
    }
}
