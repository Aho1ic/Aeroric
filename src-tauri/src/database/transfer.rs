use dbx_core::models::connection::DatabaseType;
use tauri::{AppHandle, Emitter, State};

use super::connections;
use super::dbx_state::DbxState;

async fn db_type(state: &DbxState, connection_id: &str) -> Result<DatabaseType, String> {
    state
        .app_state
        .configs
        .read()
        .await
        .get(connection_id)
        .map(|config| config.db_type)
        .ok_or_else(|| format!("Connection config not found: {connection_id}"))
}

fn emit_transfer_progress(app: &AppHandle, progress: dbx_core::transfer::TransferProgress) {
    let _ = app.emit("dbx-transfer-progress", progress);
}

#[tauri::command]
pub async fn dbx_start_transfer(
    app: AppHandle,
    state: State<'_, DbxState>,
    request: dbx_core::transfer::TransferRequest,
) -> Result<(), String> {
    connections::ensure_connected(&state, &request.source_connection_id).await?;
    connections::ensure_connected(&state, &request.target_connection_id).await?;
    connections::ensure_writable(&state, &request.target_connection_id, "Transfer").await?;

    let app_state = state.app_state.clone();
    let source_db_type = db_type(&state, &request.source_connection_id).await?;
    let target_db_type = db_type(&state, &request.target_connection_id).await?;
    let source_pool_key = app_state
        .get_or_create_pool(
            &request.source_connection_id,
            Some(&request.source_database),
        )
        .await?;
    let target_pool_key = app_state
        .get_or_create_pool(
            &request.target_connection_id,
            Some(&request.target_database),
        )
        .await?;
    let transfer_id = request.transfer_id.clone();
    let total_tables = request.tables.len();

    tokio::spawn(async move {
        for (index, table) in request.tables.iter().enumerate() {
            if dbx_core::transfer::is_cancelled(&transfer_id).await {
                emit_transfer_progress(
                    &app,
                    dbx_core::transfer::TransferProgress {
                        transfer_id: transfer_id.clone(),
                        table: table.clone(),
                        table_index: index,
                        total_tables,
                        rows_transferred: 0,
                        total_rows: None,
                        status: dbx_core::transfer::TransferStatus::Cancelled,
                        error: None,
                        terminal: true,
                    },
                );
                dbx_core::transfer::clear_cancelled(&transfer_id).await;
                return;
            }

            let mut last_rows_transferred = 0;
            let mut last_total_rows = None;
            let result = dbx_core::transfer::transfer_table(
                &app_state,
                &request,
                table,
                index,
                &source_db_type,
                &target_db_type,
                &source_pool_key,
                &target_pool_key,
                |progress| {
                    last_rows_transferred = progress.rows_transferred;
                    last_total_rows = progress.total_rows;
                    emit_transfer_progress(&app, progress);
                },
            )
            .await;

            match result {
                Ok(rows) => emit_transfer_progress(
                    &app,
                    dbx_core::transfer::TransferProgress {
                        transfer_id: transfer_id.clone(),
                        table: table.clone(),
                        table_index: index,
                        total_tables,
                        rows_transferred: rows,
                        total_rows: last_total_rows.or(Some(rows)),
                        status: dbx_core::transfer::TransferStatus::TableDone,
                        error: None,
                        terminal: false,
                    },
                ),
                Err(error) => {
                    let status = if error == "Cancelled" {
                        dbx_core::transfer::TransferStatus::Cancelled
                    } else {
                        dbx_core::transfer::TransferStatus::Error
                    };
                    emit_transfer_progress(
                        &app,
                        dbx_core::transfer::TransferProgress {
                            transfer_id: transfer_id.clone(),
                            table: table.clone(),
                            table_index: index,
                            total_tables,
                            rows_transferred: last_rows_transferred,
                            total_rows: last_total_rows,
                            status,
                            error: (error != "Cancelled").then_some(error),
                            terminal: true,
                        },
                    );
                    dbx_core::transfer::clear_cancelled(&transfer_id).await;
                    return;
                }
            }
        }

        emit_transfer_progress(
            &app,
            dbx_core::transfer::TransferProgress {
                transfer_id: transfer_id.clone(),
                table: String::new(),
                table_index: total_tables,
                total_tables,
                rows_transferred: 0,
                total_rows: None,
                status: dbx_core::transfer::TransferStatus::Done,
                error: None,
                terminal: true,
            },
        );
        dbx_core::transfer::clear_cancelled(&transfer_id).await;
    });

    Ok(())
}

#[tauri::command]
pub async fn dbx_cancel_transfer(transfer_id: String) -> Result<(), String> {
    dbx_core::transfer::set_cancelled(&transfer_id).await;
    Ok(())
}

#[tauri::command]
pub fn dbx_prepare_schema_diff(
    options: dbx_core::schema_diff::SchemaDiffPreparationOptions,
) -> Result<dbx_core::schema_diff::SchemaDiffPreparation, String> {
    Ok(dbx_core::schema_diff::prepare_schema_diff(options))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn dbx_generate_schema_sync_sql(
    diffs: Vec<dbx_core::schema_diff::TableDiff>,
    function_diffs: Option<Vec<dbx_core::schema_diff::FunctionDiff>>,
    sequence_diffs: Option<Vec<dbx_core::schema_diff::SequenceDiff>>,
    rule_diffs: Option<Vec<dbx_core::schema_diff::RuleDiff>>,
    owner_diffs: Option<Vec<dbx_core::schema_diff::OwnerDiff>>,
    database_type: DatabaseType,
    target_schema: Option<String>,
    cascade_delete: Option<bool>,
) -> Result<String, String> {
    Ok(dbx_core::schema_diff::generate_schema_sync_sql(
        &diffs,
        function_diffs.as_deref().unwrap_or_default(),
        sequence_diffs.as_deref().unwrap_or_default(),
        rule_diffs.as_deref().unwrap_or_default(),
        owner_diffs.as_deref().unwrap_or_default(),
        database_type,
        target_schema.as_deref(),
        cascade_delete.unwrap_or(false),
    ))
}

#[tauri::command]
pub fn dbx_prepare_data_compare(
    options: dbx_core::data_compare::DataComparePreparationOptions,
) -> Result<dbx_core::data_compare::DataComparePreparation, String> {
    dbx_core::data_compare::prepare_data_compare(options)
}

#[tauri::command]
pub fn dbx_build_data_compare_sync_plan(
    options: dbx_core::data_compare::DataCompareSyncPlanOptions,
) -> Result<dbx_core::data_compare::DataCompareSyncPlan, String> {
    Ok(dbx_core::data_compare::build_data_compare_sync_plan(
        options,
    ))
}

#[tauri::command]
pub async fn dbx_prepare_data_compare_from_tables(
    state: State<'_, DbxState>,
    options: dbx_core::data_compare::DataCompareFromTablesOptions,
) -> Result<dbx_core::data_compare::DataCompareFromTablesPreparation, String> {
    connections::ensure_connected(&state, &options.source_connection_id).await?;
    connections::ensure_connected(&state, &options.target_connection_id).await?;
    dbx_core::data_compare::prepare_data_compare_from_tables(&state.app_state, options).await
}

#[cfg(test)]
mod tests {
    use super::emit_transfer_progress;

    #[test]
    fn transfer_progress_event_helper_is_callable() {
        let _ =
            emit_transfer_progress as fn(&tauri::AppHandle, dbx_core::transfer::TransferProgress);
    }
}
