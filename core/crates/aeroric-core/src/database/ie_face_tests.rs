//! Original import/export regression tests moved intact from the desktop adapter.

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
    let base = request();
    let mut out = export_request_with_format(base.clone(), "csv");
    assert_eq!(out.format, "csv");
    // 这个函数真正会出的错是顺手改坏别的字段:file_path 被清掉,
    // 导出会写到空路径;skip_count 被改回 false,多跑一次 count 查询。
    // 把 format 改回去之后整份请求必须与输入逐字段相同。
    out.format = base.format.clone();
    assert_eq!(
        serde_json::to_value(&out).unwrap(),
        serde_json::to_value(&base).unwrap(),
        "export_request_with_format must not touch any field other than format"
    );
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
    assert!(
        validate_excel_zip_entry("xl/worksheets/sheet1.xml", 1024, MAX_EXCEL_ENTRY_BYTES + 1)
            .unwrap_err()
            .contains("too large")
    );
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

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("aeroric-ie-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn preview_projection_preserves_dbx_wire_with_present_and_omitted_fields() {
    use super::{TableImportPreview, TableImportTextEncoding};
    for populated in [false, true] {
        let original = dbx_core::table_import::TableImportPreview {
            file_name: "导入.csv".to_string(),
            file_path: "/synthetic/导入.csv".to_string(),
            source_ref: populated.then(|| "source:opaque".to_string()),
            file_type: "csv".to_string(),
            size_bytes: 47,
            columns: vec!["id".to_string(), "value".to_string()],
            rows: vec![vec![serde_json::json!(1), serde_json::json!("文本")]],
            total_rows: 17,
            total_rows_exact: populated,
            source_fingerprint: "fixture-fingerprint".to_string(),
            effective_encoding: populated.then_some(TableImportTextEncoding::Utf8),
            sheets: if populated {
                vec!["工作表".to_string()]
            } else {
                vec![]
            },
        };
        let expected = serde_json::to_vec(&original).unwrap();
        let projected = TableImportPreview::from(original);
        assert_eq!(serde_json::to_vec(&projected).unwrap(), expected);
        let decoded: TableImportPreview = serde_json::from_slice(&expected).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), expected);
    }
}

#[test]
fn import_summary_projection_preserves_dbx_wire_and_rejects_malformed_replies() {
    use super::TableImportSummary;
    let original = dbx_core::table_import::TableImportSummary {
        import_id: "import-1".to_string(),
        rows_imported: 9,
        total_rows: 13,
        elapsed_ms: 123_456,
    };
    let expected = serde_json::to_vec(&original).unwrap();
    let projected = TableImportSummary::from(original);
    assert_eq!(serde_json::to_vec(&projected).unwrap(), expected);
    let decoded: TableImportSummary = serde_json::from_slice(&expected).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), expected);
    assert!(serde_json::from_str::<TableImportSummary>("{}").is_err());
    assert!(
        serde_json::from_value::<TableImportSummary>(serde_json::json!({
            "importId":"bad", "rowsImported":"9", "totalRows":13, "elapsedMs":1
        }))
        .is_err()
    );
}

#[test]
fn preview_projection_does_not_default_required_metadata_or_rows() {
    assert!(serde_json::from_str::<super::TableImportPreview>("{}").is_err());
    assert!(
        serde_json::from_value::<super::TableImportPreview>(serde_json::json!({
            "fileName":"x.csv", "filePath":"/synthetic/x.csv", "fileType":"csv",
            "sizeBytes":2, "columns":[], "rows":[], "totalRows":0,
            "sourceFingerprint":"fixture"
        }))
        .is_err(),
        "totalRowsExact is required on the wire"
    );
}

#[test]
fn sql_file_request_roundtrips_optional_fields_without_renaming_wire() {
    let minimal: super::ExecuteSqlFileRequest = serde_json::from_value(serde_json::json!({
        "connectionId":"one", "path":"/synthetic/query.sql"
    }))
    .unwrap();
    assert!(minimal.database.is_none());
    assert!(minimal.schema.is_none());
    assert!(minimal.timeout_secs.is_none());
    let wire = serde_json::json!({
        "connectionId":"one", "database":"main", "schema":"app",
        "path":"/synthetic/query.sql", "timeoutSecs":17
    });
    let request: super::ExecuteSqlFileRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), wire);
}

#[test]
fn sql_reader_strips_only_utf8_bom_and_refuses_invalid_encoding() {
    let scratch = Scratch::new();
    let path = scratch.0.join("SQL 文件.sql");
    for bytes in [
        "select '文本';".as_bytes(),
        "\u{feff}select '文本';".as_bytes(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(super::read_sql_file(&path).unwrap(), "select '文本';");
    }
    std::fs::write(&path, [0xff, 0xfe, 0x41, 0]).unwrap();
    assert!(super::read_sql_file(&path)
        .unwrap_err()
        .contains("not valid UTF-8"));
    assert!(super::read_sql_file(&scratch.0.join("missing.sql"))
        .unwrap_err()
        .contains("Failed to read file"));
}

#[test]
fn sql_runner_requires_absolute_path_and_exact_sql_extension() {
    assert_eq!(
        validate_sql_file_path("relative.sql").unwrap_err(),
        "SQL file path must be absolute"
    );
    assert!(validate_sql_file_path("/synthetic/query.SQL").is_err());
    assert_eq!(
        validate_sql_file_path("/synthetic/文本.sql").unwrap(),
        Path::new("/synthetic/文本.sql")
    );
}

#[test]
fn excel_entry_limits_keep_boundary_and_name_checks() {
    assert!(validate_excel_zip_entry(&"a".repeat(1_025), 1, 1)
        .unwrap_err()
        .contains("entry name"));
    let size = super::MIN_RATIO_CHECK_BYTES;
    assert!(validate_excel_zip_entry("sheet.xml", size, size).is_ok());
    assert!(validate_excel_zip_entry("sheet.xml", 0, size)
        .unwrap_err()
        .contains("compression ratio"));
    assert!(
        validate_excel_zip_entry("XL\\WORKBOOK.XML", size, MAX_EXCEL_METADATA_BYTES + 1)
            .unwrap_err()
            .contains("metadata")
    );
}

#[test]
fn excel_archive_rejects_excess_entries_and_corrupt_zip_before_parsing() {
    let scratch = Scratch::new();
    let path = scratch.0.join("too-many.XLSX");
    let mut writer = ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for index in 0..=super::MAX_EXCEL_ZIP_ENTRIES {
        writer
            .start_file(format!("entry-{index}"), options)
            .unwrap();
    }
    writer.finish().unwrap();
    assert!(validate_excel_zip_archive(&path)
        .unwrap_err()
        .contains("too many entries"));
    std::fs::write(&path, "not a ZIP archive").unwrap();
    assert!(validate_excel_zip_archive(&path)
        .unwrap_err()
        .contains("Invalid Excel ZIP archive"));
}

#[tokio::test]
async fn database_export_worker_keeps_the_callers_runtime_and_propagates_errors() {
    let result = super::on_database_export_stack(|| async {
        // A current-thread test runtime must keep driving timers while the
        // export worker uses its handle; no nested replacement runtime.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        Err::<(), _>("fixture exporter refusal".to_string())
    })
    .await;
    assert_eq!(result.unwrap_err(), "fixture exporter refusal");
}

#[tokio::test]
async fn dropping_database_export_caller_cancels_its_worker_future() {
    struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            let _ = self.0.take().unwrap().send(());
        }
    }
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let (dropped, dropped_rx) = tokio::sync::oneshot::channel();
    let caller = tokio::spawn(super::on_database_export_stack(move || async move {
        let _dropped = Dropped(Some(dropped));
        let _ = started.send(());
        std::future::pending::<Result<(), String>>().await
    }));
    tokio::time::timeout(std::time::Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(3), dropped_rx)
        .await
        .expect("the worker must not outlive a dropped export request")
        .unwrap();
}
