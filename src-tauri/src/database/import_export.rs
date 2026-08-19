use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use dbx_core::db;
use dbx_core::query::QueryExecutionOptions;
use dbx_core::table_export::TableExportRequest;
#[cfg(test)]
use dbx_core::table_export::{ExportStatus, TableExportProgress};
use dbx_core::table_import::{TableImportPreview, TableImportRequest, TableImportSummary};
use serde::Deserialize;
use tauri::{AppHandle, State};
use zip::ZipArchive;

use super::connections;
use super::dbx_state::DbxState;
use super::query;

const MAX_EXCEL_ZIP_ENTRIES: usize = 2_048;
const MAX_EXCEL_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EXCEL_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXCEL_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MIN_RATIO_CHECK_BYTES: u64 = 1024 * 1024;
const MAX_EXCEL_COMPRESSION_RATIO: u64 = 200;

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

/// 读取 SQL 文件，处理 UTF-8 BOM 和编码问题
fn read_sql_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    // 去除 UTF-8 BOM (0xEF 0xBB 0xBF)
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        &bytes
    };

    String::from_utf8(bytes.to_vec()).map_err(|_| {
        "SQL file is not valid UTF-8. Please ensure the file is saved as UTF-8 encoding."
            .to_string()
    })
}

fn unsupported_update_sql_export() -> String {
    "UPDATE SQL table export is not supported by the bundled DBX Core exporter yet".to_string()
}

fn is_excel_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("xlsx") || extension.eq_ignore_ascii_case("xlsm")
        })
}

fn is_excel_metadata_entry(name: &str) -> bool {
    let name = name.replace('\\', "/").to_ascii_lowercase();
    name == "[content_types].xml"
        || name == "xl/workbook.xml"
        || name == "xl/styles.xml"
        || name == "xl/theme/theme1.xml"
        || name.starts_with("_rels/")
        || name.contains("/_rels/")
        || name.starts_with("docprops/")
}

fn validate_excel_zip_entry(
    name: &str,
    compressed_size: u64,
    uncompressed_size: u64,
) -> Result<(), String> {
    if name.len() > 1_024 {
        return Err("Excel archive contains an excessively long entry name".to_string());
    }
    if uncompressed_size > MAX_EXCEL_ENTRY_BYTES {
        return Err(format!(
            "Excel archive entry is too large after decompression: {name}"
        ));
    }
    if is_excel_metadata_entry(name) && uncompressed_size > MAX_EXCEL_METADATA_BYTES {
        return Err(format!("Excel metadata entry is too large: {name}"));
    }
    if uncompressed_size >= MIN_RATIO_CHECK_BYTES
        && (compressed_size == 0
            || uncompressed_size > compressed_size.saturating_mul(MAX_EXCEL_COMPRESSION_RATIO))
    {
        return Err(format!(
            "Excel archive entry has a suspicious compression ratio: {name}"
        ));
    }
    Ok(())
}

fn validate_excel_zip_archive(path: &Path) -> Result<(), String> {
    if !is_excel_zip_path(path) {
        return Ok(());
    }

    let file = fs::File::open(path)
        .map_err(|error| format!("Could not open Excel import file: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Invalid Excel ZIP archive: {error}"))?;
    if archive.len() > MAX_EXCEL_ZIP_ENTRIES {
        return Err(format!(
            "Excel archive contains too many entries (maximum {MAX_EXCEL_ZIP_ENTRIES})"
        ));
    }

    let mut total_uncompressed = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not inspect Excel ZIP entry: {error}"))?;
        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();
        let declared_size = entry.size();
        validate_excel_zip_entry(&name, entry.compressed_size(), declared_size)?;
        total_uncompressed = total_uncompressed
            .checked_add(declared_size)
            .ok_or_else(|| "Excel archive decompressed size overflowed".to_string())?;
        if total_uncompressed > MAX_EXCEL_TOTAL_BYTES {
            return Err("Excel archive is too large after decompression".to_string());
        }

        // Do not trust only central-directory sizes. Stream each entry through
        // the decoder under the same cap so forged descriptors and corrupt
        // compressed data are rejected before DBX parses the workbook.
        let mut limited = (&mut entry).take(MAX_EXCEL_ENTRY_BYTES + 1);
        let actual_size = std::io::copy(&mut limited, &mut std::io::sink())
            .map_err(|error| format!("Could not validate Excel ZIP entry {name}: {error}"))?;
        if actual_size > MAX_EXCEL_ENTRY_BYTES {
            return Err(format!(
                "Excel archive entry exceeded the decompression limit: {name}"
            ));
        }
        if actual_size != declared_size {
            return Err(format!("Excel archive entry size is inconsistent: {name}"));
        }
    }
    Ok(())
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
    validate_excel_zip_archive(Path::new(&file_path))?;
    dbx_core::table_import::preview_table_import_file_core(&file_path).await
}

#[tauri::command]
pub async fn dbx_import_table_file(
    state: State<'_, DbxState>,
    request: TableImportRequest,
) -> Result<TableImportSummary, String> {
    validate_excel_zip_archive(Path::new(&request.file_path))?;
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
    app: AppHandle,
    state: State<'_, DbxState>,
    request: ExecuteSqlFileRequest,
) -> Result<Vec<db::QueryResult>, String> {
    let path = validate_sql_file_path(&request.path)?;
    let sql = read_sql_file(&path)?;
    query::enforce_production_sql_confirmation(
        &app,
        &state,
        &request.connection_id,
        request.database.clone(),
        &sql,
    )
    .await?;
    connections::ensure_connected(&state, &request.connection_id).await?;
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
        validate_excel_zip_archive, validate_excel_zip_entry, validate_sql_file_path,
        MAX_EXCEL_ENTRY_BYTES, MAX_EXCEL_METADATA_BYTES,
    };
    use dbx_core::database_export::ExportStatus;
    use dbx_core::table_export::TableExportRequest;
    use std::io::Write;
    use std::path::Path;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn request() -> TableExportRequest {
        TableExportRequest {
            export_id: "e".to_string(),
            connection_id: "c".to_string(),
            database: "main".to_string(),
            schema: None,
            identifier_quote: None,
            table_name: "users".to_string(),
            file_path: "/tmp/users.csv".to_string(),
            format: "json".to_string(),
            columns: None,
            column_types: None,
            primary_keys: None,
            where_input: None,
            order_by: None,
            row_limit: None,
            skip_count: true,
            batch_size: None,
            date_time_format: None,
            numeric_column_right_align: false,
            column_comments: None,
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

    #[test]
    fn rejects_oversized_excel_entries_and_metadata() {
        assert!(validate_excel_zip_entry(
            "xl/worksheets/sheet1.xml",
            1024,
            MAX_EXCEL_ENTRY_BYTES + 1
        )
        .unwrap_err()
        .contains("too large"));
        assert!(
            validate_excel_zip_entry("xl/workbook.xml", 1024, MAX_EXCEL_METADATA_BYTES + 1)
                .unwrap_err()
                .contains("metadata")
        );
    }

    #[test]
    fn rejects_high_ratio_excel_zip_bomb_before_import() {
        let path =
            std::env::temp_dir().join(format!("aeroric-excel-bomb-{}.xlsx", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer
            .start_file("xl/worksheets/sheet1.xml", options)
            .unwrap();
        writer.write_all(&vec![b'a'; 2 * 1024 * 1024]).unwrap();
        writer.finish().unwrap();

        let error = validate_excel_zip_archive(Path::new(&path))
            .expect_err("high-ratio archive must be rejected");
        assert!(error.contains("suspicious compression ratio"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn skips_zip_validation_for_non_excel_imports() {
        assert!(validate_excel_zip_archive(Path::new("/tmp/not-a-zip.csv")).is_ok());
    }
}
