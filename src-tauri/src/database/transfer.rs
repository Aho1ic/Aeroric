// ── 切片一百零三（database）：transfer／差异比较面 7 条改宿主转发壳。业务主体
// 在 core `database::transfer_face`（自目标拒绝/FK 排序与推迟补挂/三种收尾/
// 纯函数四条 verbatim 上收；既有 8 条单测随体迁 core `transfer_face_tests`）。
// Embedded 本地闭包与宿主服务调同一函数，External 绝不本地回退；进度事件在
// Embedded 仍是窗口双发（legacy 通道 `dbx-transfer-progress`＋v1 topic，
// TransferProgressSink 原样），宿主侧 append 进 EventLog 的 v1 topic。 ──

use std::sync::Arc;

use tauri::Manager;

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::transfer_face::{
    self, DatabaseType, TransferProgress, TransferProgressSink,
};

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

/// Embedded 本地路径的传输进度出口：窗口双发（legacy 通道逐字 `dbx-transfer-
/// progress`，React `databaseApi.ts` 按它过滤；v1 topic 与宿主 EventLog 同名）。
struct WindowTransferProgressSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TransferProgressSink for WindowTransferProgressSink<R> {
    fn report(&self, progress: &TransferProgress) {
        if let Ok(payload) = serde_json::to_value(progress) {
            let _ = tauri::Emitter::emit(&self.app, "dbx-transfer-progress", payload.clone());
            let _ =
                tauri::Emitter::emit(&self.app, transfer_face::TRANSFER_PROGRESS_TOPIC, payload);
        }
    }
}

#[tauri::command]
pub async fn dbx_start_transfer<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: transfer_face::TransferRequest,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_start_transfer",
        serde_json::json!({ "request": &request }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            let sink: Arc<dyn TransferProgressSink> =
                Arc::new(WindowTransferProgressSink { app: app.clone() });
            transfer_face::start_transfer(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                sink,
                request,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_cancel_transfer<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    transfer_id: String,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_cancel_transfer",
        serde_json::json!({ "transferId": transfer_id }),
        async {
            transfer_face::cancel_transfer(transfer_id)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value::<()>(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_prepare_schema_diff<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: transfer_face::SchemaDiffPreparationOptions,
) -> Result<transfer_face::SchemaDiffPreparation, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_prepare_schema_diff",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(transfer_face::prepare_schema_diff(options))
                .map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn dbx_generate_schema_sync_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    diffs: Vec<transfer_face::TableDiff>,
    function_diffs: Option<Vec<transfer_face::FunctionDiff>>,
    sequence_diffs: Option<Vec<transfer_face::SequenceDiff>>,
    rule_diffs: Option<Vec<transfer_face::RuleDiff>>,
    owner_diffs: Option<Vec<transfer_face::OwnerDiff>>,
    database_type: DatabaseType,
    target_schema: Option<String>,
    cascade_delete: Option<bool>,
) -> Result<String, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_generate_schema_sync_sql",
        serde_json::json!({
            "diffs": diffs,
            "functionDiffs": function_diffs,
            "sequenceDiffs": sequence_diffs,
            "ruleDiffs": rule_diffs,
            "ownerDiffs": owner_diffs,
            "databaseType": database_type,
            "targetSchema": target_schema,
            "cascadeDelete": cascade_delete,
        }),
        async {
            transfer_face::generate_schema_sync_sql(
                diffs,
                function_diffs,
                sequence_diffs,
                rule_diffs,
                owner_diffs,
                database_type,
                target_schema,
                cascade_delete,
            )
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_prepare_data_compare<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: transfer_face::DataComparePreparationOptions,
) -> Result<transfer_face::DataComparePreparation, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_prepare_data_compare",
        serde_json::json!({ "options": &options }),
        async {
            transfer_face::prepare_data_compare(options)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_compare_sync_plan<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: transfer_face::DataCompareSyncPlanOptions,
) -> Result<transfer_face::DataCompareSyncPlan, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_compare_sync_plan",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(transfer_face::build_data_compare_sync_plan(options))
                .map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_prepare_data_compare_from_tables<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: transfer_face::DataCompareFromTablesOptions,
) -> Result<transfer_face::DataCompareFromTablesPreparation, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_prepare_data_compare_from_tables",
        serde_json::json!({ "options": &options }),
        async {
            let state = app.state::<DbxState>();
            let paths = local_connection_paths()?;
            transfer_face::prepare_data_compare_from_tables(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                options,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::WindowTransferProgressSink;
    use aeroric_core::database::transfer_face::{TransferProgress, TransferProgressSink};
    use dbx_core::transfer::TransferStatus;
    use tauri::{Emitter, Listener};

    #[test]
    fn embedded_progress_reaches_the_frontend_channel_and_records_the_v1_limit() {
        // React `databaseApi.ts` listens on "dbx-transfer-progress" and folds
        // status==="error" into a terminal event, so that channel must carry
        // the payload byte-for-byte. The v1 topic half mirrors the host
        // EventLog name, but Tauri's event-name validation rejects dots
        // (`IllegalEventName`) — the same reality as the rag progress sink
        // since slice ninety-nine, whose dotted emit is likewise swallowed by
        // `let _ =`. Pin both facts: the legacy channel delivers, and the
        // dotted topic is rejected by the runtime rather than delivered.
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let sink = received.clone();
        let _listener = handle.listen("dbx-transfer-progress", move |event| {
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                sink.lock().expect("progress sink").push(payload);
            }
        });
        let progress = TransferProgress {
            transfer_id: "transfer-7".to_string(),
            table: "orders".to_string(),
            table_index: 1,
            total_tables: 2,
            rows_transferred: 40,
            total_rows: Some(100),
            status: TransferStatus::Error,
            error: Some("target table is read-only".to_string()),
            terminal: true,
        };
        WindowTransferProgressSink {
            app: handle.clone(),
        }
        .report(&progress);
        let events = received.lock().expect("progress sink");
        assert_eq!(
            events.len(),
            1,
            "exactly the legacy channel fires: {events:?}"
        );
        let payload = &events[0];
        assert_eq!(payload["transferId"], "transfer-7");
        assert_eq!(payload["table"], "orders");
        assert_eq!(payload["rowsTransferred"], 40);
        assert_eq!(payload["status"], "error");
        assert_eq!(payload["terminal"], true);
        let topic = "dbx.transfer_progress";
        assert!(Emitter::emit(&handle, topic, payload.clone()).is_err());
    }
}
