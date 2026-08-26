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

fn normalized_transfer_part(value: &str) -> &str {
    value.trim()
}

/// Return an error when a transfer would use the same physical object as both
/// its source and target.  Keep this check at Aeroric's wrapper boundary: the
/// DBX core also serves other callers, while this command owns the connection
/// and database identity supplied by the UI.
fn reject_self_target_transfer(
    request: &dbx_core::transfer::TransferRequest,
) -> Result<(), String> {
    let same_connection = normalized_transfer_part(&request.source_connection_id)
        == normalized_transfer_part(&request.target_connection_id);
    let same_database = normalized_transfer_part(&request.source_database)
        == normalized_transfer_part(&request.target_database);
    let same_schema = normalized_transfer_part(&request.source_schema)
        == normalized_transfer_part(&request.target_schema);
    let same_catalog = request.source_catalog.as_deref().unwrap_or("").trim()
        == request.target_catalog.as_deref().unwrap_or("").trim();

    if same_connection && same_database && same_schema && same_catalog {
        if let Some(source_table) = request.tables.iter().find(|table| {
            let target_table = request.target_table_name(table);
            normalized_transfer_part(table) == normalized_transfer_part(&target_table)
        }) {
            return Err(format!(
                "Cannot transfer table '{source_table}' onto itself"
            ));
        }
    }
    Ok(())
}

/// 执行 `transfer_table` 推迟下来的外键 ALTER,返回失败的目标表名(已排序去重)。
///
/// MySQL 系目标上 dbx 会把内联外键从 CREATE TABLE 里剥掉(外键成环时不存在任何
/// 合法建表顺序),改成让调用方在所有表都建完之后补上。调用方不执行这批语句就等于
/// 静默丢掉外键,所以每条退出路径都要过一遍这里。
async fn apply_pending_fk_alters(
    app_state: &dbx_core::connection::AppState,
    target_pool_key: &str,
    pending: &[(String, String)],
) -> Vec<String> {
    let mut failed: Vec<String> = Vec::new();
    for (table, alter_sql) in pending {
        if let Err(error) =
            dbx_core::transfer::execute_on_pool(app_state, target_pool_key, alter_sql).await
        {
            eprintln!("[transfer] failed to add deferred foreign key for {table}: {error}");
            failed.push(table.clone());
        }
    }
    failed.sort();
    failed.dedup();
    failed
}

/// 把外键补挂结果折成最后那条终止事件的 status / error。
///
/// 不能像 dbx-web 那样另发一条非终止的 Error 行:前端 `databaseApi.ts` 的
/// `isTerminalDbxTransferProgress` 把 `status === "error"` 当终止事件,多发一条会让它
/// 提前 unlisten,真正的 Done 就收不到了。所以外键失败只能并进这一条里报。
fn terminal_status_for_fk_result(
    failed_fk_tables: &[String],
) -> (dbx_core::transfer::TransferStatus, Option<String>) {
    if failed_fk_tables.is_empty() {
        (dbx_core::transfer::TransferStatus::Done, None)
    } else {
        (
            dbx_core::transfer::TransferStatus::Error,
            Some(format!(
                "Failed to add deferred foreign key constraints on: {}",
                failed_fk_tables.join(", ")
            )),
        )
    }
}

/// 一次传输的三种收尾方式。
///
/// 抽出来是为了让「补建外键 + 终止事件怎么报」只有一处实现。之前两条取消路径各自
/// `return`,谁都没调用 `apply_pending_fk_alters`,而它的文档写的是「每条退出路径都
/// 要过一遍这里」——已经传完的那些表就永久少了外键,而且没人告诉用户。
enum TransferExit {
    /// 全部表都处理完了。
    Completed,
    /// 用户取消。已经建好的表会留在目标库里,不回滚。
    Cancelled,
    /// 某张表失败,整次传输中止。带上原始错误。
    Failed(String),
}

fn terminal_status_for_exit(
    exit: &TransferExit,
    failed_fk_tables: &[String],
) -> (dbx_core::transfer::TransferStatus, Option<String>) {
    match exit {
        // 原始错误比外键补挂失败更有诊断价值,不覆盖它。外键失败已经进日志。
        TransferExit::Failed(error) => (
            dbx_core::transfer::TransferStatus::Error,
            Some(error.clone()),
        ),
        // 取消时还没轮到的表根本不存在,指向它们的外键 ALTER 必然失败——这是取消的
        // 正常结果而不是故障。所以照样补(能挂上的要挂上),但不把用户主动的取消
        // 报成错误,失败只留在日志里。
        TransferExit::Cancelled => (dbx_core::transfer::TransferStatus::Cancelled, None),
        TransferExit::Completed => terminal_status_for_fk_result(failed_fk_tables),
    }
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
    reject_self_target_transfer(&request)?;

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
        // 按外键依赖排序,被引用表先传,否则目标库上内联外键会因被引用表还不存在而
        // 失败(MySQL 系走推迟外键不受影响,Postgres 等仍是内联)。返回的外键元数据
        // 同时当 transfer_table 的预取缓存用,省掉它按表各查一次。
        // 外部 Doris/StarRocks catalog 跳过:库名不在默认 catalog 里,排序既查不到也没必要。
        let (tables, known_foreign_keys) = {
            let skip_fk_sort = {
                let configs = app_state.configs.read().await;
                configs
                    .get(&request.source_connection_id)
                    .and_then(|config| {
                        dbx_core::transfer::resolve_external_transfer_catalog_for_config(
                            request.source_catalog.as_deref(),
                            config,
                        )
                    })
                    .is_some()
            };
            if skip_fk_sort {
                (request.tables.clone(), std::collections::HashMap::new())
            } else {
                dbx_core::transfer::sort_tables_by_fk_dependency_with_foreign_keys(
                    &app_state,
                    &request.source_connection_id,
                    &request.source_database,
                    &request.source_schema,
                    &request.tables,
                    true,
                )
                .await
                .unwrap_or_else(|error| {
                    // 排序失败不该让整次传输失败,退回请求顺序即原有行为。
                    eprintln!(
                        "[transfer] failed to sort tables by FK dependency, using original order: {error}"
                    );
                    (request.tables.clone(), std::collections::HashMap::new())
                })
            }
        };
        let mut pending_fk_alters: Vec<(String, String)> = Vec::new();

        for (index, table) in tables.iter().enumerate() {
            if dbx_core::transfer::is_cancelled(&transfer_id).await {
                // 取消同样要补外键:已经传完的表不会被回滚,少了外键就是静默的
                // 数据模型损坏。
                let failed_fk_tables =
                    apply_pending_fk_alters(&app_state, &target_pool_key, &pending_fk_alters).await;
                let (status, error) =
                    terminal_status_for_exit(&TransferExit::Cancelled, &failed_fk_tables);
                emit_transfer_progress(
                    &app,
                    dbx_core::transfer::TransferProgress {
                        transfer_id: transfer_id.clone(),
                        table: table.clone(),
                        table_index: index,
                        total_tables,
                        rows_transferred: 0,
                        total_rows: None,
                        status,
                        error,
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
                &known_foreign_keys,
                &mut pending_fk_alters,
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
                    let exit = if error == "Cancelled" {
                        TransferExit::Cancelled
                    } else {
                        // 这一张表失败会中止整次传输,但前面已经建好的表还缺被剥掉的
                        // 外键。补建对失败和取消都要做,区别只在终止事件怎么报。
                        TransferExit::Failed(error)
                    };
                    let failed_fk_tables =
                        apply_pending_fk_alters(&app_state, &target_pool_key, &pending_fk_alters)
                            .await;
                    let (status, error) = terminal_status_for_exit(&exit, &failed_fk_tables);
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
                            error,
                            terminal: true,
                        },
                    );
                    dbx_core::transfer::clear_cancelled(&transfer_id).await;
                    return;
                }
            }
        }

        // 所有表都建完了,补上推迟的外键。
        let failed_fk_tables =
            apply_pending_fk_alters(&app_state, &target_pool_key, &pending_fk_alters).await;
        let (status, error) = terminal_status_for_exit(&TransferExit::Completed, &failed_fk_tables);

        emit_transfer_progress(
            &app,
            dbx_core::transfer::TransferProgress {
                transfer_id: transfer_id.clone(),
                table: String::new(),
                table_index: total_tables,
                total_tables,
                rows_transferred: 0,
                total_rows: None,
                status,
                error,
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
        None,
        &[],
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
    use super::{
        emit_transfer_progress, reject_self_target_transfer, terminal_status_for_exit,
        terminal_status_for_fk_result, TransferExit,
    };
    use dbx_core::transfer::{TransferRequest, TransferStatus};

    fn request() -> TransferRequest {
        TransferRequest {
            transfer_id: "test".to_string(),
            source_connection_id: "conn".to_string(),
            source_database: "db".to_string(),
            source_schema: "public".to_string(),
            source_catalog: None,
            target_connection_id: "conn".to_string(),
            target_database: "db".to_string(),
            target_schema: "public".to_string(),
            target_catalog: None,
            tables: vec!["users".to_string()],
            create_table: true,
            content: Default::default(),
            objects: Vec::new(),
            mode: Default::default(),
            target_table_name_case: Default::default(),
            ownership_policy: Default::default(),
            batch_size: 100,
        }
    }

    #[test]
    fn rejects_an_identical_source_and_target_object() {
        let error = reject_self_target_transfer(&request()).expect_err("self-target must fail");
        assert!(error.contains("users"));
    }

    #[test]
    fn allows_a_different_database_or_schema() {
        let mut different_database = request();
        different_database.target_database = "other".to_string();
        assert!(reject_self_target_transfer(&different_database).is_ok());

        let mut different_schema = request();
        different_schema.target_schema = "archive".to_string();
        assert!(reject_self_target_transfer(&different_schema).is_ok());
    }

    #[test]
    fn allows_a_different_target_table_or_connection() {
        let mut different_table = request();
        different_table.tables = vec!["Users".to_string()];
        different_table.target_table_name_case = dbx_core::transfer::TransferTableNameCase::Upper;
        assert!(reject_self_target_transfer(&different_table).is_ok());

        let mut different_connection = request();
        different_connection.target_connection_id = "other-conn".to_string();
        assert!(reject_self_target_transfer(&different_connection).is_ok());
    }

    #[test]
    fn transfer_progress_event_helper_is_callable() {
        let _ =
            emit_transfer_progress as fn(&tauri::AppHandle, dbx_core::transfer::TransferProgress);
    }

    #[test]
    fn reports_done_when_no_deferred_foreign_key_failed() {
        let (status, error) = terminal_status_for_fk_result(&[]);
        assert!(matches!(status, TransferStatus::Done), "got {status:?}");
        assert_eq!(error, None);
    }

    #[test]
    fn folds_foreign_key_failures_into_the_terminal_event() {
        // 前端把 status === "error" 当终止事件,所以外键失败必须走 Error 而不是
        // 另发一条非终止事件,否则用户只会看到"传输完成"、外键却没挂上。
        let (status, error) =
            terminal_status_for_fk_result(&["orders".to_string(), "users".to_string()]);
        assert!(matches!(status, TransferStatus::Error), "got {status:?}");
        let error = error.expect("failed foreign keys must surface an error message");
        assert!(
            error.contains("orders"),
            "message should name the table: {error}"
        );
        assert!(
            error.contains("users"),
            "message should name the table: {error}"
        );
    }

    #[test]
    fn cancelling_never_reports_deferred_foreign_key_failures_as_errors() {
        // 取消时还没轮到的表不存在,指向它们的 ALTER 必然失败。把这个报成 Error
        // 会让每次取消都弹一条假故障。
        let (status, error) = terminal_status_for_exit(
            &TransferExit::Cancelled,
            &["orders".to_string(), "users".to_string()],
        );
        assert!(
            matches!(status, TransferStatus::Cancelled),
            "got {status:?}"
        );
        assert_eq!(error, None);
    }

    #[test]
    fn a_failed_table_keeps_its_own_error_instead_of_the_foreign_key_one() {
        // 原始失败原因比"外键没挂上"更有诊断价值。
        let (status, error) = terminal_status_for_exit(
            &TransferExit::Failed("target table is read-only".to_string()),
            &["orders".to_string()],
        );
        assert!(matches!(status, TransferStatus::Error), "got {status:?}");
        assert_eq!(error.as_deref(), Some("target table is read-only"));
    }

    #[test]
    fn only_a_completed_transfer_turns_foreign_key_failures_into_an_error() {
        let (status, error) = terminal_status_for_exit(&TransferExit::Completed, &[]);
        assert!(matches!(status, TransferStatus::Done), "got {status:?}");
        assert_eq!(error, None);

        let (status, error) =
            terminal_status_for_exit(&TransferExit::Completed, &["orders".to_string()]);
        assert!(matches!(status, TransferStatus::Error), "got {status:?}");
        assert!(error.is_some_and(|message| message.contains("orders")));
    }
}
