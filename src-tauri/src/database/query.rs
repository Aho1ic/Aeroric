use std::sync::Arc;

use dbx_core::db;
use tauri::{Emitter, State};

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::execution;

pub(crate) use aeroric_core::database::execution::{
    AssessProductionSqlRequest, AssessProductionTargetRequest, ExecuteMultiRequest,
    ExecuteQueryRequest,
};

// ── 切片九十七（database 刀三）：执行面 7 条改宿主转发壳。业务主体在 core
// `database::execution`；Embedded 本地闭包与宿主服务调同一函数，External
// 绝不本地回退。确认事件的 Embedded 出口仍是 Tauri 窗口 emit（legacy 通道
// 名逐字），宿主侧出口是 EventLog append。 ──

fn confirm_emit_sink<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> aeroric_core::database::execution::ConfirmEventSink {
    use aeroric_core::database::execution::ProductionConfirmRequest;
    let app = app.clone();
    Arc::new(move |request: &ProductionConfirmRequest| {
        if let Ok(payload) = serde_json::to_value(request) {
            let _ = app.emit(
                aeroric_core::database::execution::PRODUCTION_CONFIRM_EVENT,
                payload,
            );
        }
    })
}

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_assess_production_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: AssessProductionSqlRequest,
) -> Result<aeroric_core::database::query::ProductionSqlAssessment, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_assess_production_sql",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::assess_sql(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_assess_production_target<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: AssessProductionTargetRequest,
) -> Result<aeroric_core::database::query::ProductionTargetAssessment, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_assess_production_target",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            execution::assess_target(&state, &paths, &KeyringDbxConnectionSecretStore, request)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn respond_dbx_production_confirmation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request_id: String,
    approved: bool,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.respond_dbx_production_confirmation",
        serde_json::json!({ "requestId": request_id, "approved": approved }),
        async {
            let mut pending = state.pending_production_confirmations.lock().await;
            execution::resolve_pending_production_confirmation(&mut pending, &request_id, approved)
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_execute_query<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: ExecuteQueryRequest,
) -> Result<db::QueryResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_execute_query",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            let sink = confirm_emit_sink(&app);
            execution::execute_query(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                Some(&sink),
                request,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_execute_multi<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    request: ExecuteMultiRequest,
) -> Result<Vec<db::QueryResult>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_execute_multi",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            let sink = confirm_emit_sink(&app);
            execution::execute_multi(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                Some(&sink),
                request,
            )
            .await
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_cancel_query<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    execution_id: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_cancel_query",
        serde_json::json!({ "executionId": execution_id }),
        async { execution::cancel_query(&state, execution_id).map(|_| serde_json::Value::Null) },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_close_result_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, DbxState>,
    connection_id: String,
    #[allow(unused_variables)] session_id: String,
    database: Option<String>,
    client_session_id: Option<String>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_close_result_session",
        serde_json::json!({
            "connectionId": connection_id,
            "sessionId": session_id,
            "database": database,
            "clientSessionId": client_session_id
        }),
        async {
            execution::close_result_session(
                &state,
                connection_id,
                session_id,
                database,
                client_session_id,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

// ── 切片九十六（database 刀二）：16 条 SQL builders 改宿主转发壳。业务主体
// 在 dbx-core，core `database::sql_builders` 是宿主侧的同一依赖点；Embedded
// 本地闭包与宿主服务调同一个函数，External 绝不本地回退。 ──

#[tauri::command]
pub async fn dbx_build_table_structure_change_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::table_structure_sql::TableStructureSqlOptions,
) -> Result<dbx_core::table_structure_sql::TableStructureSqlResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_table_structure_change_sql",
        serde_json::json!({ "options": &options }),
        async {
            let result =
                aeroric_core::database::sql_builders::build_table_structure_change_sql(options);
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_single_column_alter_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::table_structure_sql::SingleColumnAlterSqlOptions,
) -> Result<dbx_core::table_structure_sql::TableStructureSqlResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_single_column_alter_sql",
        serde_json::json!({ "options": &options }),
        async {
            let result =
                aeroric_core::database::sql_builders::build_single_column_alter_sql(options);
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_create_database_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::CreateDatabaseSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_create_database_sql",
        serde_json::json!({ "options": &options }),
        async {
            aeroric_core::database::sql_builders::build_create_database_sql(options)
                .map(serde_json::Value::String)
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_duckdb_attach_database_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::sql_builders::DuckDbAttachDatabaseSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_duckdb_attach_database_sql",
        serde_json::json!({ "options": &options }),
        async {
            // dbx-core 的同名函数被 duckdb-sidecar feature 门控；这行本地构造
            // 是 feature 关闭时的生产路径（core 逐字上收同一实现）。
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_duckdb_attach_database_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_rename_object_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::RenameObjectSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_rename_object_sql",
        serde_json::json!({ "options": &options }),
        async {
            aeroric_core::database::sql_builders::build_rename_object_sql(options)
                .map(serde_json::Value::String)
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_drop_database_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::DatabaseNameSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_drop_database_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_drop_database_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_create_schema_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::SchemaNameSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_create_schema_sql",
        serde_json::json!({ "options": &options }),
        async {
            aeroric_core::database::sql_builders::build_create_schema_sql(options)
                .map(serde_json::Value::String)
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_drop_schema_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::SchemaNameSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_drop_schema_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_drop_schema_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_drop_table_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_drop_table_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_drop_table_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_truncate_table_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_truncate_table_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_truncate_table_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_empty_table_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_empty_table_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_empty_table_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_drop_object_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::DropObjectSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_drop_object_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_drop_object_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_drop_table_child_object_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::DropTableChildObjectSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_drop_table_child_object_sql",
        serde_json::json!({ "options": &options }),
        async {
            aeroric_core::database::sql_builders::build_drop_table_child_object_sql(options)
                .map(serde_json::Value::String)
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_duplicate_table_structure_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::db_admin_sql::DuplicateTableStructureSqlOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_duplicate_table_structure_sql",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_duplicate_table_structure_sql(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}

#[tauri::command]
pub async fn dbx_build_database_search_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::database_search_sql::DatabaseSearchSqlOptions,
) -> Result<Option<dbx_core::database_search_sql::DatabaseSearchSql>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_database_search_sql",
        serde_json::json!({ "options": &options }),
        async {
            match aeroric_core::database::sql_builders::build_database_search_sql(options) {
                Some(result) => serde_json::to_value(result).map_err(|error| error.to_string()),
                None => Ok(serde_json::Value::Null),
            }
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_search_result_where<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: dbx_core::database_search_sql::SearchResultWhereOptions,
) -> Result<String, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_search_result_where",
        serde_json::json!({ "options": &options }),
        async {
            Ok(serde_json::Value::String(
                aeroric_core::database::sql_builders::build_search_result_where(options),
            ))
        },
    )
    .await
    .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
}
