//! P8-05 切片一百零二（database）：import/export 面＋SQL 文件执行的宿主依赖
//! 点。与 `src-tauri/src/database/import_export.rs` 同源 verbatim 上收：
//! Excel ZIP 炸弹校验层（条目数/单条大小/元数据大小/压缩比/总量）、导出与
//! 导入、SQL 文件执行（生产确认闸复用 execution::enforce，出口注入）。

use std::fs;
use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbx_core::db;
use dbx_core::query::QueryExecutionOptions;
pub use dbx_core::table_export::TableExportRequest;
pub use dbx_core::table_import::{TableImportRequest, TableImportTextEncoding};

pub use dbx_core::database_export::DatabaseExportRequest;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::database::dbx_connections::{ensure_connected, ensure_writable, DbxConnectionPaths};
use crate::database::dbx_secrets::DbxConnectionSecretStore;
use crate::database::dbx_state::DbxState;
use crate::database::execution;

const MAX_EXCEL_ZIP_ENTRIES: usize = 2_048;
const MAX_EXCEL_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EXCEL_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXCEL_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MIN_RATIO_CHECK_BYTES: u64 = 1024 * 1024;
const MAX_EXCEL_COMPRESSION_RATIO: u64 = 200;
const DATABASE_EXPORT_STACK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteSqlFileRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    pub path: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// DBX's reply types are Serialize-only. Own the bidirectional wire projection
/// here so both adapters use one schema without changing the DBX dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableImportPreview {
    pub file_name: String,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    pub file_type: String,
    pub size_bytes: u64,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total_rows: usize,
    pub total_rows_exact: bool,
    pub source_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_encoding: Option<TableImportTextEncoding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<String>,
}

impl From<dbx_core::table_import::TableImportPreview> for TableImportPreview {
    fn from(value: dbx_core::table_import::TableImportPreview) -> Self {
        Self {
            file_name: value.file_name,
            file_path: value.file_path,
            source_ref: value.source_ref,
            file_type: value.file_type,
            size_bytes: value.size_bytes,
            columns: value.columns,
            rows: value.rows,
            total_rows: value.total_rows,
            total_rows_exact: value.total_rows_exact,
            source_fingerprint: value.source_fingerprint,
            effective_encoding: value.effective_encoding,
            sheets: value.sheets,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableImportSummary {
    pub import_id: String,
    pub rows_imported: usize,
    pub total_rows: usize,
    pub elapsed_ms: u128,
}

impl From<dbx_core::table_import::TableImportSummary> for TableImportSummary {
    fn from(value: dbx_core::table_import::TableImportSummary) -> Self {
        Self {
            import_id: value.import_id,
            rows_imported: value.rows_imported,
            total_rows: value.total_rows,
            elapsed_ms: value.elapsed_ms,
        }
    }
}

pub fn export_request_with_format(
    mut request: TableExportRequest,
    format: &str,
) -> TableExportRequest {
    request.format = format.to_string();
    request
}

pub async fn export_table(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: TableExportRequest,
) -> Result<(), String> {
    ensure_connected(state, paths, store, &request.connection_id).await?;
    let export_id = request.export_id.clone();
    dbx_core::table_export::export_table_data_core(state.app_state.as_ref(), &request, |_| {})
        .await?;
    dbx_core::database_export::clear_export_cancelled(&export_id).await;
    Ok(())
}

pub fn validate_sql_file_path(path: &str) -> Result<PathBuf, String> {
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
pub fn read_sql_file(path: &Path) -> Result<String, String> {
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

pub fn unsupported_update_sql_export() -> String {
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

pub async fn export_database(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: DatabaseExportRequest,
) -> Result<(), String> {
    ensure_connected(state, paths, store, &request.connection_id).await?;
    let export_id = request.export_id.clone();
    let app_state = Arc::clone(&state.app_state);
    on_database_export_stack(move || async move {
        dbx_core::database_export::export_database_sql_core(&app_state, &request, |_| {}).await
    })
    .await?;
    dbx_core::database_export::clear_export_cancelled(&export_id).await;
    Ok(())
}

/// The bundled DBX exporter has a large *poll stack*, even though its future
/// is already boxed upstream. Isolate just this operation on a bounded stack;
/// use the caller's live Tokio runtime, not a second runtime with stale pools.
/// Dropping the caller closes the channel and drops the export future, so this
/// worker cannot silently continue a cancelled request in the background.
async fn on_database_export_stack<T, F, Fut>(build: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, String>>,
{
    let runtime = tokio::runtime::Handle::current();
    let (mut sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("aeroric-db-export".to_string())
        .stack_size(DATABASE_EXPORT_STACK_BYTES)
        .spawn(move || {
            runtime.block_on(async {
                let result = tokio::select! {
                    biased;
                    _ = sender.closed() => return,
                    result = build() => result,
                };
                let _ = sender.send(result);
            });
        })
        .map_err(|error| format!("Could not start database export worker: {error}"))?;
    receiver
        .await
        .map_err(|_| "Database export worker stopped before replying".to_string())?
}

pub async fn preview_table_import_file(file_path: String) -> Result<TableImportPreview, String> {
    validate_excel_zip_archive(Path::new(&file_path))?;
    dbx_core::table_import::preview_table_import_file_core(&file_path)
        .await
        .map(Into::into)
}

pub async fn import_table_file(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: TableImportRequest,
) -> Result<TableImportSummary, String> {
    validate_excel_zip_archive(Path::new(&request.file_path))?;
    ensure_connected(state, paths, store, &request.connection_id).await?;
    ensure_writable(state, &request.connection_id, "Import").await?;
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
    .map(Into::into)
}

/// SQL 文件执行（Tauri `dbx_execute_sql_file` verbatim：路径校验 → BOM 剥离
/// → 生产确认闸（出口注入）→ 连接 → 注册取消令牌 → dbx-core 多语句执行）。
pub async fn execute_sql_file(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    sink: &execution::ConfirmEventSink,
    request: ExecuteSqlFileRequest,
) -> Result<Vec<db::QueryResult>, String> {
    let path = validate_sql_file_path(&request.path)?;
    let sql = read_sql_file(&path)?;
    execution::enforce_production_sql_confirmation(
        state,
        paths,
        store,
        &request.connection_id,
        request.database.clone(),
        &sql,
        Some(sink),
        execution::PRODUCTION_CONFIRM_TIMEOUT,
    )
    .await?;
    ensure_connected(state, paths, store, &request.connection_id).await?;
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
fn error_progress(export_id: String, error: String) -> dbx_core::table_export::TableExportProgress {
    dbx_core::table_export::TableExportProgress {
        export_id,
        table_name: String::new(),
        rows_exported: 0,
        total_rows: None,
        status: dbx_core::table_export::ExportStatus::Error,
        error_message: Some(error),
    }
}

#[cfg(test)]
#[path = "ie_face_tests.rs"]
mod tests;
