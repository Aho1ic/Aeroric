use std::collections::HashMap;
use std::time::Duration;

use dbx_core::db;
use dbx_core::query::QueryExecutionOptions;
use dbx_core::sql_risk::SqlRisk;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

use super::connections;
use super::dbx_state::DbxState;

/// 生产库确认请求的事件名。前端在 `src/App.tsx` 里监听。
const PRODUCTION_CONFIRM_EVENT: &str = "dbx-production-confirm-requested";

/// 等前端答复的上限。取值偏宽松 —— 这是要人读 SQL 再决定的确认框,
/// 但不能没有上限,否则前端不答会把查询永久挂住。
const PRODUCTION_CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

/// 发给前端的生产库确认请求。文案由前端 i18n 拼,这里只给事实。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductionConfirmRequest {
    request_id: String,
    /// 连接名(已过滤控制字符)。
    connection: String,
    /// 被判定为生产库的库名;空数组表示整个连接都是生产环境。
    databases: Vec<String>,
    sql: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteQueryRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
    sql: String,
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    max_rows: Option<usize>,
    #[serde(default)]
    fetch_size: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
    #[serde(default)]
    result_session_id: Option<String>,
    #[serde(default)]
    client_session_id: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    execution_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteMultiRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
    sql: String,
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    max_rows: Option<usize>,
    #[serde(default)]
    fetch_size: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
    #[serde(default)]
    result_session_id: Option<String>,
    #[serde(default)]
    client_session_id: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    execution_id: Option<String>,
    #[serde(default)]
    use_transaction: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssessProductionSqlRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
    sql: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssessProductionTargetRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionSqlAssessment {
    requires_confirmation: bool,
    is_mutation: bool,
    production_databases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionTargetAssessment {
    requires_confirmation: bool,
    production_databases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DuckDbAttachDatabaseSqlOptions {
    path: String,
    name: String,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn options_from_request(request: &ExecuteQueryRequest) -> QueryExecutionOptions {
    QueryExecutionOptions {
        max_rows: request.max_rows,
        fetch_size: request.fetch_size,
        page_size: request.page_size,
        catalog: None,
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        execution_id: non_empty(request.execution_id.clone()),
        use_transaction: None,
        continue_on_error: false,
        execution_mode: Default::default(),
    }
}

fn options_from_multi_request(request: &ExecuteMultiRequest) -> QueryExecutionOptions {
    QueryExecutionOptions {
        max_rows: request.max_rows,
        fetch_size: request.fetch_size,
        page_size: request.page_size,
        catalog: None,
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        execution_id: non_empty(request.execution_id.clone()),
        use_transaction: request.use_transaction,
        continue_on_error: false,
        execution_mode: Default::default(),
    }
}

fn assess_production_sql(
    config: &dbx_core::models::connection::ConnectionConfig,
    database: Option<String>,
    sql: &str,
) -> Result<ProductionSqlAssessment, String> {
    let risk = dbx_core::sql_risk::classify_sql_risk_for_database(sql, config.db_type)?;
    let is_mutation = risk != SqlRisk::ReadOnly;
    let active_database = non_empty(database)
        .or_else(|| config.database.clone())
        .unwrap_or_default();
    let requires_confirmation = is_mutation
        && dbx_core::production_safety::targets_production_database(config, &active_database, sql);
    let production_databases = if !requires_confirmation || config.is_production {
        Vec::new()
    } else if dbx_core::production_safety::is_production_database(config, &active_database) {
        vec![active_database]
    } else {
        config.production_databases.clone()
    };

    Ok(ProductionSqlAssessment {
        requires_confirmation,
        is_mutation,
        production_databases,
    })
}

fn assess_production_target(
    config: &dbx_core::models::connection::ConnectionConfig,
    database: Option<String>,
) -> ProductionTargetAssessment {
    let active_database = non_empty(database).or_else(|| config.database.clone());
    let requires_confirmation = config.is_production
        || active_database
            .as_deref()
            .map(|database| dbx_core::production_safety::is_production_database(config, database))
            .unwrap_or_else(|| !config.production_databases.is_empty());
    let production_databases = if !requires_confirmation || config.is_production {
        Vec::new()
    } else if let Some(database) = active_database {
        vec![database]
    } else {
        config.production_databases.clone()
    };

    ProductionTargetAssessment {
        requires_confirmation,
        production_databases,
    }
}

async fn production_connection_config(
    state: &DbxState,
    connection_id: &str,
) -> Result<dbx_core::models::connection::ConnectionConfig, String> {
    if let Some(config) = state
        .app_state
        .configs
        .read()
        .await
        .get(connection_id)
        .cloned()
    {
        return Ok(config);
    }

    connections::ensure_loaded(state).await?;
    let connection = state
        .connections
        .read()
        .await
        .get(connection_id)
        .cloned()
        .ok_or_else(|| "Connection config not found".to_string())?;
    connections::parse_core_config(&connection)
}

fn safe_dialog_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

pub(crate) async fn enforce_production_sql_confirmation(
    app: &AppHandle,
    state: &DbxState,
    connection_id: &str,
    database: Option<String>,
    sql: &str,
) -> Result<(), String> {
    let config = production_connection_config(state, connection_id).await?;
    let assessment = assess_production_sql(&config, database, sql)?;
    if !assessment.requires_confirmation {
        return Ok(());
    }

    let connection = safe_dialog_label(&config.name);
    let databases = if assessment.production_databases.is_empty() {
        Vec::new()
    } else {
        assessment
            .production_databases
            .iter()
            .map(|database| safe_dialog_label(database))
            .collect::<Vec<_>>()
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let (responder, response) = oneshot::channel::<bool>();
    state
        .pending_production_confirmations
        .lock()
        .await
        .insert(request_id.clone(), responder);

    // 发结构化字段而不是拼好的提示语:文案交给前端 i18n,不再在 Rust 里
    // 硬编码中英混排。sql 原样带上——前端弹窗要把它显示出来给人核对。
    let emitted = app.emit(
        PRODUCTION_CONFIRM_EVENT,
        ProductionConfirmRequest {
            request_id: request_id.clone(),
            connection,
            databases,
            sql: sql.to_string(),
        },
    );
    if let Err(error) = emitted {
        state
            .pending_production_confirmations
            .lock()
            .await
            .remove(&request_id);
        return Err(format!("Production confirmation dispatch failed: {error}"));
    }

    // 超时是必需的:前端窗口关掉、事件监听没挂上、或渲染进程崩了,
    // 都会让这个 await 永久挂住,连带把查询卡死。超时按拒绝处理。
    let approved = match tokio::time::timeout(PRODUCTION_CONFIRM_TIMEOUT, response).await {
        Ok(Ok(approved)) => approved,
        // sender 被 drop(前端没答就重启了之类)同样按拒绝处理。
        Ok(Err(_)) | Err(_) => false,
    };

    // 无论走哪条路都要清理条目,否则 map 会随失败的确认无限增长。
    state
        .pending_production_confirmations
        .lock()
        .await
        .remove(&request_id);

    if approved {
        Ok(())
    } else {
        Err("Production database operation cancelled.".to_string())
    }
}

/// 取出并唤醒等待中的确认。抽成独立函数是为了能单测 —— `State<'_, DbxState>`
/// 在测试里构造不出来,而这里的取出/唤醒/未知 id 三条分支才是要守住的逻辑。
pub(crate) fn resolve_pending_production_confirmation(
    pending: &mut HashMap<String, oneshot::Sender<bool>>,
    request_id: &str,
    approved: bool,
) -> Result<(), String> {
    match pending.remove(request_id) {
        // send 失败只意味着等待方已经超时走了,不是调用方的错。
        Some(responder) => {
            let _ = responder.send(approved);
            Ok(())
        }
        None => Err(format!(
            "No pending production confirmation for request {request_id}"
        )),
    }
}

/// 前端答复生产库确认。`approved` 为 false 时上面的 await 会返回拒绝。
#[tauri::command]
pub async fn respond_dbx_production_confirmation(
    state: State<'_, DbxState>,
    request_id: String,
    approved: bool,
) -> Result<(), String> {
    let mut pending = state.pending_production_confirmations.lock().await;
    resolve_pending_production_confirmation(&mut pending, &request_id, approved)
}

#[tauri::command]
pub async fn dbx_assess_production_sql(
    state: State<'_, DbxState>,
    request: AssessProductionSqlRequest,
) -> Result<ProductionSqlAssessment, String> {
    let config = production_connection_config(&state, &request.connection_id).await?;
    assess_production_sql(&config, request.database, &request.sql)
}

#[tauri::command]
pub async fn dbx_assess_production_target(
    state: State<'_, DbxState>,
    request: AssessProductionTargetRequest,
) -> Result<ProductionTargetAssessment, String> {
    let config = production_connection_config(&state, &request.connection_id).await?;
    Ok(assess_production_target(&config, request.database))
}

#[tauri::command]
pub async fn dbx_execute_query(
    app: AppHandle,
    state: State<'_, DbxState>,
    request: ExecuteQueryRequest,
) -> Result<db::QueryResult, String> {
    enforce_production_sql_confirmation(
        &app,
        &state,
        &request.connection_id,
        request.database.clone(),
        &request.sql,
    )
    .await?;
    connections::ensure_connected(&state, &request.connection_id).await?;
    let execution_id = non_empty(request.execution_id.clone());
    let registered_query = execution_id
        .as_ref()
        .map(|id| state.app_state.running_queries.register(id.clone()));
    let cancel_token = registered_query.as_ref().map(|query| query.token());
    let database = request.database.clone().unwrap_or_default();
    dbx_core::query::execute_sql_statement_with_options(
        &state.app_state,
        &request.connection_id,
        &database,
        &request.sql,
        request.schema.as_deref(),
        cancel_token,
        options_from_request(&request),
    )
    .await
}

#[tauri::command]
pub async fn dbx_execute_multi(
    app: AppHandle,
    state: State<'_, DbxState>,
    request: ExecuteMultiRequest,
) -> Result<Vec<db::QueryResult>, String> {
    enforce_production_sql_confirmation(
        &app,
        &state,
        &request.connection_id,
        request.database.clone(),
        &request.sql,
    )
    .await?;
    connections::ensure_connected(&state, &request.connection_id).await?;
    let execution_id = non_empty(request.execution_id.clone());
    let registered_query = execution_id
        .as_ref()
        .map(|id| state.app_state.running_queries.register(id.clone()));
    let cancel_token = registered_query.as_ref().map(|query| query.token());
    let database = request.database.clone().unwrap_or_default();
    dbx_core::query::execute_multi_core_with_options(
        &state.app_state,
        &request.connection_id,
        &database,
        &request.sql,
        request.schema.as_deref(),
        cancel_token,
        options_from_multi_request(&request),
    )
    .await
}

#[tauri::command]
pub async fn dbx_cancel_query(
    state: State<'_, DbxState>,
    execution_id: String,
) -> Result<(), String> {
    state.app_state.running_queries.cancel(&execution_id);
    Ok(())
}

#[tauri::command]
pub async fn dbx_close_result_session(
    state: State<'_, DbxState>,
    connection_id: String,
    #[allow(unused_variables)] session_id: String,
    database: Option<String>,
    client_session_id: Option<String>,
) -> Result<(), String> {
    let database = database.unwrap_or_default();
    dbx_core::query::close_query_session(
        &state.app_state,
        &connection_id,
        &database,
        &session_id,
        client_session_id.as_deref(),
        None,
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub fn dbx_build_table_structure_change_sql(
    options: dbx_core::table_structure_sql::TableStructureSqlOptions,
) -> Result<dbx_core::table_structure_sql::TableStructureSqlResult, String> {
    Ok(dbx_core::table_structure_sql::build_table_structure_change_sql(options))
}

#[tauri::command]
pub fn dbx_build_single_column_alter_sql(
    options: dbx_core::table_structure_sql::SingleColumnAlterSqlOptions,
) -> Result<dbx_core::table_structure_sql::TableStructureSqlResult, String> {
    Ok(dbx_core::table_structure_sql::build_single_column_alter_sql(options))
}

#[tauri::command]
pub fn dbx_build_create_database_sql(
    options: dbx_core::db_admin_sql::CreateDatabaseSqlOptions,
) -> Result<String, String> {
    dbx_core::db_admin_sql::build_create_database_sql(options)
}

fn quote_duckdb_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[tauri::command]
pub fn dbx_build_duckdb_attach_database_sql(
    options: DuckDbAttachDatabaseSqlOptions,
) -> Result<String, String> {
    Ok(format!(
        "ATTACH {} AS {};",
        quote_sql_string(&options.path),
        quote_duckdb_identifier(&options.name)
    ))
}

#[tauri::command]
pub fn dbx_build_rename_object_sql(
    options: dbx_core::db_admin_sql::RenameObjectSqlOptions,
) -> Result<String, String> {
    dbx_core::db_admin_sql::build_rename_object_sql(options)
}

#[tauri::command]
pub fn dbx_build_drop_database_sql(
    options: dbx_core::db_admin_sql::DatabaseNameSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_drop_database_sql(options))
}

#[tauri::command]
pub fn dbx_build_create_schema_sql(
    options: dbx_core::db_admin_sql::SchemaNameSqlOptions,
) -> Result<String, String> {
    dbx_core::db_admin_sql::build_create_schema_sql(options)
}

#[tauri::command]
pub fn dbx_build_drop_schema_sql(
    options: dbx_core::db_admin_sql::SchemaNameSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_drop_schema_sql(options))
}

#[tauri::command]
pub fn dbx_build_drop_table_sql(
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_drop_table_sql(options))
}

#[tauri::command]
pub fn dbx_build_truncate_table_sql(
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_truncate_table_sql(options))
}

#[tauri::command]
pub fn dbx_build_empty_table_sql(
    options: dbx_core::db_admin_sql::TableAdminSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_empty_table_sql(options))
}

#[tauri::command]
pub fn dbx_build_drop_object_sql(
    options: dbx_core::db_admin_sql::DropObjectSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_drop_object_sql(options))
}

#[tauri::command]
pub fn dbx_build_drop_table_child_object_sql(
    options: dbx_core::db_admin_sql::DropTableChildObjectSqlOptions,
) -> Result<String, String> {
    dbx_core::db_admin_sql::build_drop_table_child_object_sql(options)
}

#[tauri::command]
pub fn dbx_build_duplicate_table_structure_sql(
    options: dbx_core::db_admin_sql::DuplicateTableStructureSqlOptions,
) -> Result<String, String> {
    Ok(dbx_core::db_admin_sql::build_duplicate_table_structure_sql(
        options,
    ))
}

#[tauri::command]
pub fn dbx_build_database_search_sql(
    options: dbx_core::database_search_sql::DatabaseSearchSqlOptions,
) -> Result<Option<dbx_core::database_search_sql::DatabaseSearchSql>, String> {
    Ok(dbx_core::database_search_sql::build_database_search_sql(
        options,
    ))
}

#[tauri::command]
pub fn dbx_build_search_result_where(
    options: dbx_core::database_search_sql::SearchResultWhereOptions,
) -> Result<String, String> {
    Ok(dbx_core::database_search_sql::build_search_result_where(
        options,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbx_core::models::connection::ConnectionConfig;
    use serde_json::json;

    fn production_config(is_production: bool, production_databases: &[&str]) -> ConnectionConfig {
        serde_json::from_value(json!({
            "id": "c",
            "name": "Production",
            "db_type": "mysql",
            "host": "localhost",
            "port": 3306,
            "username": "root",
            "password": "",
            "database": "staging",
            "is_production": is_production,
            "production_databases": production_databases,
        }))
        .unwrap()
    }

    #[test]
    fn blank_execution_id_is_ignored_in_options() {
        let request = ExecuteQueryRequest {
            connection_id: "c".to_string(),
            database: None,
            sql: "select 1".to_string(),
            schema: None,
            max_rows: Some(10),
            fetch_size: None,
            page_size: None,
            result_session_id: None,
            client_session_id: None,
            timeout_secs: Some(3),
            execution_id: Some(" ".to_string()),
        };

        let options = options_from_request(&request);

        assert_eq!(options.max_rows, Some(10));
        assert_eq!(options.timeout_secs, Some(3));
        assert_eq!(options.execution_id, None);
        assert_eq!(options.use_transaction, None);
    }

    #[test]
    fn multi_query_transaction_option_is_forwarded() {
        let request = ExecuteMultiRequest {
            connection_id: "c".to_string(),
            database: Some("main".to_string()),
            sql: "insert into audit values (1); insert into audit values (2);".to_string(),
            schema: None,
            max_rows: None,
            fetch_size: None,
            page_size: None,
            result_session_id: None,
            client_session_id: None,
            timeout_secs: None,
            execution_id: None,
            use_transaction: Some(true),
        };

        assert_eq!(
            options_from_multi_request(&request).use_transaction,
            Some(true)
        );
    }

    #[test]
    fn production_assessment_allows_read_only_sql() {
        let assessment = assess_production_sql(
            &production_config(true, &[]),
            Some("main".to_string()),
            "select * from users",
        )
        .unwrap();

        assert_eq!(
            assessment,
            ProductionSqlAssessment {
                requires_confirmation: false,
                is_mutation: false,
                production_databases: vec![],
            }
        );
    }

    #[test]
    fn production_assessment_blocks_connection_wide_mutations() {
        let assessment = assess_production_sql(
            &production_config(true, &[]),
            Some("main".to_string()),
            "delete from users where id = 1",
        )
        .unwrap();

        assert!(assessment.requires_confirmation);
        assert!(assessment.is_mutation);
        assert!(assessment.production_databases.is_empty());
    }

    #[test]
    fn production_assessment_detects_active_and_qualified_databases() {
        let config = production_config(false, &["prod_app"]);
        let active = assess_production_sql(
            &config,
            Some("prod_app".to_string()),
            "update users set active = 0",
        )
        .unwrap();
        let qualified = assess_production_sql(
            &config,
            Some("staging".to_string()),
            "delete from prod_app.users where id = 1",
        )
        .unwrap();

        assert_eq!(active.production_databases, vec!["prod_app"]);
        assert!(active.requires_confirmation);
        assert_eq!(qualified.production_databases, vec!["prod_app"]);
        assert!(qualified.requires_confirmation);
    }

    #[test]
    fn production_target_assessment_uses_explicit_and_default_databases() {
        let config = production_config(false, &["prod_app"]);
        let explicit = assess_production_target(&config, Some("prod_app".to_string()));
        assert!(explicit.requires_confirmation);
        assert_eq!(explicit.production_databases, vec!["prod_app"]);

        let mut default_config = config;
        default_config.database = Some("prod_app".to_string());
        let defaulted = assess_production_target(&default_config, Some(" ".to_string()));
        assert!(defaulted.requires_confirmation);
        assert_eq!(defaulted.production_databases, vec!["prod_app"]);
    }

    #[test]
    fn production_target_assessment_is_conservative_without_a_database() {
        let mut config = production_config(false, &["prod_app", "prod_analytics"]);
        config.database = None;

        let assessment = assess_production_target(&config, None);

        assert!(assessment.requires_confirmation);
        assert_eq!(
            assessment.production_databases,
            vec!["prod_app", "prod_analytics"]
        );
    }

    #[test]
    fn builds_create_database_sql_with_dbx_core() {
        let sql = dbx_build_create_database_sql(dbx_core::db_admin_sql::CreateDatabaseSqlOptions {
            database_type: Some(dbx_core::models::connection::DatabaseType::Mysql),
            driver_profile: Some("mysql".to_string()),
            target: None,
            parent: None,
            name: "app".to_string(),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_unicode_ci".to_string()),
        })
        .unwrap();

        assert_eq!(
            sql,
            "CREATE DATABASE `app` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"
        );
    }

    #[test]
    fn builds_duckdb_attach_sql_without_duckdb_bundled_feature() {
        let sql = dbx_build_duckdb_attach_database_sql(DuckDbAttachDatabaseSqlOptions {
            path: "/tmp/app's.duckdb".to_string(),
            name: "app db".to_string(),
        })
        .unwrap();

        assert_eq!(sql, "ATTACH '/tmp/app''s.duckdb' AS \"app db\";");
    }

    #[test]
    fn builds_drop_database_and_schema_sql_with_dbx_core() {
        let database_sql =
            dbx_build_drop_database_sql(dbx_core::db_admin_sql::DatabaseNameSqlOptions {
                database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
                name: "app".to_string(),
            })
            .unwrap();
        let create_schema_sql =
            dbx_build_create_schema_sql(dbx_core::db_admin_sql::SchemaNameSqlOptions {
                database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
                name: "analytics".to_string(),
            })
            .unwrap();
        let drop_schema_sql =
            dbx_build_drop_schema_sql(dbx_core::db_admin_sql::SchemaNameSqlOptions {
                database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
                name: "analytics".to_string(),
            })
            .unwrap();

        assert_eq!(database_sql, "DROP DATABASE \"app\";");
        assert_eq!(create_schema_sql, "CREATE SCHEMA \"analytics\";");
        assert_eq!(drop_schema_sql, "DROP SCHEMA \"analytics\" CASCADE;");
    }

    #[test]
    fn builds_rename_object_sql_with_dbx_core() {
        let sql = dbx_build_rename_object_sql(dbx_core::db_admin_sql::RenameObjectSqlOptions {
            database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
            object_type: dbx_core::db_admin_sql::DatabaseObjectType::Table,
            schema: Some("public".to_string()),
            old_name: "users".to_string(),
            new_name: "app_users".to_string(),
        })
        .unwrap();

        assert_eq!(
            sql,
            "ALTER TABLE \"public\".\"users\" RENAME TO \"app_users\";"
        );
    }

    #[test]
    fn builds_database_search_sql_with_dbx_core() {
        let query = dbx_build_database_search_sql(
            dbx_core::database_search_sql::DatabaseSearchSqlOptions {
                database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                columns: vec![
                    dbx_core::database_search_sql::DatabaseSearchColumn {
                        name: "id".to_string(),
                        data_type: "integer".to_string(),
                        is_primary_key: true,
                    },
                    dbx_core::database_search_sql::DatabaseSearchColumn {
                        name: "email".to_string(),
                        data_type: "text".to_string(),
                        is_primary_key: false,
                    },
                ],
                term: "alice".to_string(),
                limit: Some(10),
            },
        )
        .unwrap()
        .unwrap();

        assert!(query.sql.contains("FROM \"public\".\"users\""));
        assert_eq!(query.searchable_columns, vec!["email"]);

        let where_input = dbx_build_search_result_where(
            dbx_core::database_search_sql::SearchResultWhereOptions {
                database_type: Some(dbx_core::models::connection::DatabaseType::Postgres),
                columns: vec![dbx_core::database_search_sql::DatabaseSearchColumn {
                    name: "id".to_string(),
                    data_type: "integer".to_string(),
                    is_primary_key: true,
                }],
                result_columns: vec!["id".to_string(), "email".to_string()],
                row: vec![
                    serde_json::json!(42),
                    serde_json::json!("alice@example.com"),
                ],
                matched_columns: vec!["email".to_string()],
            },
        )
        .unwrap();

        assert_eq!(where_input, "\"id\" = 42");
    }

    /* 生产库确认的往返记账。
     *
     * 这条闸是后端边界:前端即便被 devtools 直接驱动 IPC 也绕不过它。
     * 弹窗表面从原生 MessageBox 换成应用内弹窗后,「谁来唤醒等待方」
     * 变成了一条 IPC 回调,下面守住它的三条分支。 */

    #[tokio::test]
    async fn answering_a_pending_confirmation_wakes_the_waiter() {
        let mut pending = HashMap::new();
        let (sender, receiver) = oneshot::channel::<bool>();
        pending.insert("req-1".to_string(), sender);

        resolve_pending_production_confirmation(&mut pending, "req-1", true).unwrap();

        assert!(receiver.await.unwrap());
        // 答复过的条目必须摘掉,否则同一个 id 能被答第二次。
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn rejecting_forwards_false_to_the_waiter() {
        let mut pending = HashMap::new();
        let (sender, receiver) = oneshot::channel::<bool>();
        pending.insert("req-2".to_string(), sender);

        resolve_pending_production_confirmation(&mut pending, "req-2", false).unwrap();

        assert!(!receiver.await.unwrap());
    }

    #[test]
    fn answering_an_unknown_request_is_an_error() {
        let mut pending = HashMap::new();

        let error = resolve_pending_production_confirmation(&mut pending, "nope", true)
            .expect_err("unknown request id must not silently succeed");

        assert!(error.contains("nope"));
    }

    #[tokio::test]
    async fn answering_after_the_waiter_gave_up_is_not_an_error() {
        let mut pending = HashMap::new();
        let (sender, receiver) = oneshot::channel::<bool>();
        pending.insert("req-3".to_string(), sender);
        // 等待方超时走了 —— send 会失败,但那不是前端的错,不该报给调用方。
        drop(receiver);

        resolve_pending_production_confirmation(&mut pending, "req-3", true).unwrap();
    }

    #[test]
    fn confirm_request_payload_is_camel_case_for_the_frontend() {
        let payload = ProductionConfirmRequest {
            request_id: "req-4".to_string(),
            connection: "prod".to_string(),
            databases: vec!["orders".to_string()],
            sql: "DELETE FROM orders".to_string(),
        };

        let json = serde_json::to_value(&payload).unwrap();

        // 前端按 requestId 读;写成 request_id 会静默取到 undefined,
        // 于是永远回不了这条确认,查询卡到超时。
        assert_eq!(json["requestId"], "req-4");
        assert_eq!(json["databases"][0], "orders");
        assert_eq!(json["sql"], "DELETE FROM orders");
    }

    #[test]
    fn dialog_labels_drop_control_characters() {
        // 连接名/库名会原样进弹窗文案,控制字符要先滤掉。
        assert_eq!(safe_dialog_label("prod\u{0007}db"), "proddb");
        assert_eq!(safe_dialog_label("orders\npayments"), "orderspayments");
    }
}
