//! P8-05 切片九十七（database 刀三）：执行面＋生产确认回路的业务主体。
//!
//! 与 `src-tauri/src/database/{query,grid}.rs` 同源 verbatim 上收：执行 DTO、
//! 生产确认回路（emit → oneshot → 超时拒绝）、执行/取消/关会话、grid 数据页
//! 与保存。差异只有两处，都是可测性注入而不是行为变更：确认事件的出口是
//! 注入的 [`ConfirmEventSink`]（宿主 append 进 EventLog、Tauri emit 到窗口、
//! 测试可注入捕集），超时是参数（生产路径传 [`PRODUCTION_CONFIRM_TIMEOUT`]
//! 逐字常量，测试传短值）。硬底线：**无应答时超时拒绝，绝不静默放行**。

use std::collections::HashMap;
use std::time::Duration;

use dbx_core::data_grid_sql;
use dbx_core::db;
use dbx_core::models::connection::DatabaseType;
use dbx_core::query::{
    execute_sql_statement_with_options, execute_statements, QueryExecutionOptions,
};
use dbx_core::sql_dialect::{build_table_data_select_sql, TableDataSelectSqlOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::dbx_connections::{
    ensure_connected, ensure_loaded, parse_core_config, DbxConnectionPaths,
};
use crate::database::dbx_secrets::DbxConnectionSecretStore;
use crate::database::dbx_state::DbxState;
use crate::database::grid::preview_response;
use crate::database::query::{
    assess_production_sql, assess_production_target, ProductionSqlAssessment,
    ProductionTargetAssessment,
};

/// 旧桌面通道名（React 客户端 `listen` 它）。台账 v1 主题
/// `dbx.production_confirm_requested` 与它一一对应；宿主 EventLog 与 Tauri
/// 窗口 emit 都用这个名字（executor "task-status" 同款先例）。
pub const PRODUCTION_CONFIRM_EVENT: &str = "dbx-production-confirm-requested";

/// 等前端答复的上限。取值偏宽松——这是要人读 SQL 再决定的确认框,
/// 但不能没有上限,否则前端不答会把查询永久挂住。超时按拒绝处理。
pub const PRODUCTION_CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

/// 确认请求事件的出口。core 不认识窗口/连接：宿主把 payload append 进
/// EventLog（v1 面），Tauri emit 到前端（legacy 面），测试注入捕集器。
pub type ConfirmEventSink = Arc<dyn Fn(&ProductionConfirmRequest) + Send + Sync>;

use std::sync::Arc;

/// 发给前端的生产库确认请求。文案由前端 i18n 拼,这里只给事实。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionConfirmRequest {
    pub request_id: String,
    /// 连接名(已过滤控制字符)。
    pub connection: String,
    /// 被判定为生产库的库名;空数组表示整个连接都是生产环境。
    pub databases: Vec<String>,
    pub sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteQueryRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub sql: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub max_rows: Option<usize>,
    #[serde(default)]
    pub fetch_size: Option<usize>,
    #[serde(default)]
    pub page_size: Option<usize>,
    #[serde(default)]
    pub result_session_id: Option<String>,
    #[serde(default)]
    pub client_session_id: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub execution_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteMultiRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub sql: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub max_rows: Option<usize>,
    #[serde(default)]
    pub fetch_size: Option<usize>,
    #[serde(default)]
    pub page_size: Option<usize>,
    #[serde(default)]
    pub result_session_id: Option<String>,
    #[serde(default)]
    pub client_session_id: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub use_transaction: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessProductionSqlRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessProductionTargetRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
}

/// grid 数据页请求（与 Tauri `TableDataRequest` camelCase 字段对齐；与
/// `grid::TableDataRequest` 的区别是补了 catalog/schema 两个可选轴——那是
/// dbx 面的形状，legacy `db_query_table` 不带它们）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridTableDataRequest {
    pub connection_id: String,
    #[serde(default)]
    pub catalog: Option<String>,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    pub table: String,
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub order_by: Option<String>,
    #[serde(default)]
    pub where_input: Option<String>,
}

/// grid 保存请求（update/insert/delete 三条共用；execute=false 即纯预览）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSaveRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    pub options: data_grid_sql::DataGridSaveStatementOptions,
    #[serde(default)]
    pub execute: bool,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    100
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
        row_offset: None,
        max_result_bytes: None,
        result_key_columns: Vec::new(),
        // dbx 要求这个开关只用于它自己生成的表数据预览 SQL。这两个入口执行的是
        // 用户任意 SQL,置 true 会让 dbx 去解析并不存在的隐藏预览元数据列。
        table_data_preview: false,
        catalog: None,
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        await_cancel_completion: false,
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
        row_offset: None,
        max_result_bytes: None,
        result_key_columns: Vec::new(),
        table_data_preview: false,
        catalog: None,
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        await_cancel_completion: false,
        execution_id: non_empty(request.execution_id.clone()),
        use_transaction: request.use_transaction,
        continue_on_error: false,
        execution_mode: Default::default(),
    }
}

fn safe_dialog_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

/// 连接的 dbx-core 配置（Tauri `production_connection_config` verbatim）：
/// 先查连接池注册表，未命中再惰性加载连接文件。
pub async fn production_connection_config(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
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
    ensure_loaded(state, paths, store).await?;
    let connection = state
        .connections
        .read()
        .await
        .get(connection_id)
        .cloned()
        .ok_or_else(|| "Connection config not found".to_string())?;
    parse_core_config(&connection)
}

/// 生产库确认闸（Tauri `enforce_production_sql_confirmation` verbatim，出口
/// 与超时改为注入）。超时/发送端掉线/事件出口失败都按拒绝处理——**绝不
/// 静默放行生产写**。
pub async fn enforce_production_sql_confirmation(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: &str,
    database: Option<String>,
    sql: &str,
    sink: Option<&ConfirmEventSink>,
    timeout: Duration,
) -> Result<(), String> {
    let config = production_connection_config(state, paths, store, connection_id).await?;
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
    let (responder, response) = tokio::sync::oneshot::channel::<bool>();
    state
        .pending_production_confirmations
        .lock()
        .await
        .insert(request_id.clone(), responder);

    // 发结构化字段而不是拼好的提示语:文案交给前端 i18n,不再在 Rust 里
    // 硬编码中英混排。sql 原样带上——前端弹窗要把它显示出来给人核对。
    let request = ProductionConfirmRequest {
        request_id: request_id.clone(),
        connection,
        databases,
        sql: sql.to_string(),
    };
    match sink {
        Some(sink) => sink(&request),
        None => {
            state
                .pending_production_confirmations
                .lock()
                .await
                .remove(&request_id);
            return Err("Production confirmation dispatch failed: no event sink".to_string());
        }
    }

    // 超时是必需的:前端窗口关掉、事件监听没挂上、或渲染进程崩了,
    // 都会让这个 await 永久挂住,连带把查询卡死。超时按拒绝处理。
    let approved = match tokio::time::timeout(timeout, response).await {
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

/// 取出并唤醒等待中的确认。抽成独立函数是为了能单测——`DbxState` 的注册表
/// 在测试里可直接构造，这里的取出/唤醒/未知 id 三条分支才是要守住的逻辑。
pub fn resolve_pending_production_confirmation(
    pending: &mut HashMap<String, tokio::sync::oneshot::Sender<bool>>,
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

/// 评估一条 SQL 的生产写风险（含配置查询）。
pub async fn assess_sql(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: AssessProductionSqlRequest,
) -> Result<ProductionSqlAssessment, String> {
    let config = production_connection_config(state, paths, store, &request.connection_id).await?;
    assess_production_sql(&config, request.database, &request.sql)
}

/// 评估一个连接目标的生产属性（含配置查询）。
pub async fn assess_target(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: AssessProductionTargetRequest,
) -> Result<ProductionTargetAssessment, String> {
    let config = production_connection_config(state, paths, store, &request.connection_id).await?;
    Ok(assess_production_target(&config, request.database))
}

/// 单条 SQL 执行（Tauri `dbx_execute_query` verbatim：确认闸 → 连接 → 注册
/// 取消令牌 → dbx-core 执行）。
pub async fn execute_query(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    sink: Option<&ConfirmEventSink>,
    request: ExecuteQueryRequest,
) -> Result<db::QueryResult, String> {
    enforce_production_sql_confirmation(
        state,
        paths,
        store,
        &request.connection_id,
        request.database.clone(),
        &request.sql,
        sink,
        PRODUCTION_CONFIRM_TIMEOUT,
    )
    .await?;
    ensure_connected(state, paths, store, &request.connection_id).await?;
    let execution_id = non_empty(request.execution_id.clone());
    let registered_query = execution_id
        .as_ref()
        .map(|id| state.app_state.running_queries.register(id.clone()));
    let cancel_token = registered_query.as_ref().map(|query| query.token());
    let database = request.database.clone().unwrap_or_default();
    execute_sql_statement_with_options(
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

/// 多语句执行（Tauri `dbx_execute_multi` verbatim）。
pub async fn execute_multi(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    sink: Option<&ConfirmEventSink>,
    request: ExecuteMultiRequest,
) -> Result<Vec<db::QueryResult>, String> {
    enforce_production_sql_confirmation(
        state,
        paths,
        store,
        &request.connection_id,
        request.database.clone(),
        &request.sql,
        sink,
        PRODUCTION_CONFIRM_TIMEOUT,
    )
    .await?;
    ensure_connected(state, paths, store, &request.connection_id).await?;
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

/// 取消在飞查询（Tauri `dbx_cancel_query` verbatim）。
pub fn cancel_query(state: &DbxState, execution_id: String) -> Result<(), String> {
    state.app_state.running_queries.cancel(&execution_id);
    Ok(())
}

/// 关闭结果会话（Tauri `dbx_close_result_session` verbatim）。
pub async fn close_result_session(
    state: &DbxState,
    connection_id: String,
    session_id: String,
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

async fn database_type(state: &DbxState, connection_id: &str) -> Option<DatabaseType> {
    state
        .app_state
        .configs
        .read()
        .await
        .get(connection_id)
        .map(|config| config.db_type)
}

fn first_u64_cell(result: &db::QueryResult) -> Option<u64> {
    result.rows.first()?.first().and_then(|value| match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|v| u64::try_from(v).ok())),
        Value::String(value) => value.parse::<u64>().ok(),
        _ => None,
    })
}

/// 页大小归一（Tauri grid.rs verbatim：1..=1000 clamp——注意这与
/// `grid::normalize_page_size` 的 1..=500 是两条不同命令的契约，不合并）。
fn normalize_grid_page_size(page_size: usize) -> usize {
    page_size.clamp(1, 1000)
}

/// 表数据有界页（Tauri `dbx_query_table_data` verbatim：确认列 → 构造
/// select/count SQL → 各自带 max_rows 执行 → 总数取首格）。
#[allow(clippy::too_many_arguments)]
pub async fn query_table_data(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: GridTableDataRequest,
) -> Result<Value, String> {
    ensure_connected(state, paths, store, &request.connection_id).await?;
    let database = request.database.clone().unwrap_or_default();
    let qualified_database = (!database.is_empty()).then_some(database.clone());
    let identifier_quote = state
        .app_state
        .connection_identifier_quote(&request.connection_id, qualified_database.as_deref())
        .await?;
    let schema = request.schema.clone();
    let page_size = normalize_grid_page_size(request.page_size);
    let page = request.page.max(1);
    let offset = (page - 1) * page_size;
    let columns = dbx_core::schema::get_columns_core(
        &state.app_state,
        &request.connection_id,
        &database,
        schema.as_deref().unwrap_or_default(),
        &request.table,
    )
    .await
    .unwrap_or_default();
    let column_names = columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let primary_keys = columns
        .iter()
        .filter(|column| column.is_primary_key)
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let db_type = database_type(state, &request.connection_id).await;
    let sql = build_table_data_select_sql(TableDataSelectSqlOptions {
        database_type: db_type,
        // dbx 的 JDBC 驱动画像与服务端投影是可选优化;这里全部留空以保持
        // Aeroric 现有行为(SQL 分页 + 完整取值),避免静默改变结果集。
        driver_profile: None,
        identifier_quote: identifier_quote.clone(),
        schema: schema.clone(),
        table_name: request.table.clone(),
        catalog: request.catalog.clone(),
        database: qualified_database.clone(),
        table_type: None,
        primary_keys,
        columns: column_names,
        column_types: Vec::new(),
        large_value_preview_size: None,
        fallback_order_columns: Vec::new(),
        order_by: request.order_by.clone(),
        limit: Some(page_size),
        offset: Some(offset),
        use_driver_row_offset: false,
        where_input: request.where_input.clone(),
        include_row_id: false,
    });
    let count_sql =
        data_grid_sql::build_data_grid_count_sql(data_grid_sql::DataGridCountSqlOptions {
            database_type: db_type,
            identifier_quote,
            catalog: request.catalog,
            database: qualified_database,
            schema,
            table_name: request.table,
            where_input: request.where_input,
            count_hint: None,
        });

    let result = execute_sql_statement_with_options(
        &state.app_state,
        &request.connection_id,
        &database,
        &sql,
        None,
        None,
        QueryExecutionOptions {
            max_rows: Some(page_size),
            page_size: Some(page_size),
            ..Default::default()
        },
    )
    .await?;
    let count_result = execute_sql_statement_with_options(
        &state.app_state,
        &request.connection_id,
        &database,
        &count_sql,
        None,
        None,
        QueryExecutionOptions {
            max_rows: Some(1),
            ..Default::default()
        },
    )
    .await
    .ok();
    let total_rows = count_result.as_ref().and_then(first_u64_cell);

    serde_json::to_value(GridTableDataResponse {
        result,
        total_rows,
        sql,
        count_sql,
    })
    .map_err(|error| error.to_string())
}

/// 表数据回执（与 Tauri `TableDataResponse` camelCase 字段对齐）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridTableDataResponse {
    pub result: db::QueryResult,
    pub total_rows: Option<u64>,
    pub sql: String,
    pub count_sql: String,
}

/// grid 保存（Tauri `execute_grid_save` verbatim：连接 → prepare → 无校验错/
/// 不执行/空语句即纯预览 → 否则执行并归一错误 → 预览回执）。
pub async fn execute_grid_save(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    request: GridSaveRequest,
) -> Result<Value, String> {
    ensure_connected(state, paths, store, &request.connection_id).await?;
    let db_type = database_type(state, &request.connection_id).await;
    let preparation = data_grid_sql::prepare_data_grid_save(request.options);
    if preparation.validation_error.is_some()
        || !request.execute
        || preparation.statements.is_empty()
    {
        return serde_json::to_value(preview_response(preparation, false, 0))
            .map_err(|error| error.to_string());
    }

    let database = request.database.unwrap_or_default();
    let schema = preparation
        .execution_schema
        .as_deref()
        .or(request.schema.as_deref());
    let result = execute_statements(
        &state.app_state,
        &request.connection_id,
        &database,
        &preparation.statements,
        schema,
        None,
    )
    .await
    .map_err(|error| data_grid_sql::normalize_data_grid_save_error(db_type, &error))?;

    serde_json::to_value(preview_response(preparation, true, result.affected_rows))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dbx_secrets::test_support::MemoryDbxSecretStore;
    use dbx_core::models::connection::ConnectionConfig;

    fn full_production_config(
        is_production: bool,
        production_databases: &[&str],
    ) -> ConnectionConfig {
        serde_json::from_value(serde_json::json!({
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
    fn production_assessment_allows_read_only_sql() {
        let assessment = assess_production_sql(
            &full_production_config(true, &[]),
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
            &full_production_config(true, &[]),
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
        let config = full_production_config(false, &["prod_app"]);
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
        let config = full_production_config(false, &["prod_app"]);
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
        let mut config = full_production_config(false, &["prod_app", "prod_analytics"]);
        config.database = None;

        let assessment = assess_production_target(&config, None);

        assert!(assessment.requires_confirmation);
        assert_eq!(
            assessment.production_databases,
            vec!["prod_app", "prod_analytics"]
        );
    }

    /* 生产库确认的往返记账。 */

    #[tokio::test]
    async fn answering_a_pending_confirmation_wakes_the_waiter() {
        let mut pending = HashMap::new();
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        pending.insert("req-1".to_string(), sender);

        resolve_pending_production_confirmation(&mut pending, "req-1", true).unwrap();

        assert!(receiver.await.unwrap());
        // 答复过的条目必须摘掉,否则同一个 id 能被答第二次。
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn rejecting_forwards_false_to_the_waiter() {
        let mut pending = HashMap::new();
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
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
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
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

    #[tokio::test]
    async fn confirmation_without_a_sink_is_refused_immediately() {
        let (state, _degradation) =
            DbxState::new(&std::env::temp_dir().join("aeroric-exec-no-sink"), "test").await;
        let paths = DbxConnectionPaths::from_data_dir(&std::env::temp_dir());
        let store = MemoryDbxSecretStore::default();
        // 配置注册表为空：连配置都查不到时先报配置缺失——确认闸在此之前。
        let error = enforce_production_sql_confirmation(
            &state,
            &paths,
            &store,
            "missing-conn",
            None,
            "DELETE FROM x",
            None,
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(error.contains("Connection config not found") || error.contains("not found"));
    }

    #[tokio::test]
    async fn confirmation_times_out_as_refusal() {
        // 硬底线的机器形状：没人应答时超时拒绝，绝不静默放行生产写。
        let dir = std::env::temp_dir().join("aeroric-exec-timeout");
        let (state, _degradation) = DbxState::new(&dir, "test").await;
        // sqlite 生产连接 + DELETE：评估要求确认；无人应答 → 注入的短超时拒绝。
        // 直接插 dbx-core 注册表（快路径）：ensure_loaded 会用磁盘内容覆盖
        // 内存连接表，手插 state.connections 会被冲掉。
        let mut core_config = crate::database::dbx_connections::default_connection_config(
            "prod-conn",
            "Production",
            crate::database::dbx_types::DbxDatabaseType::Sqlite,
            dir.join("prod.sqlite").to_string_lossy().into_owned(),
            0,
            None,
            false,
        );
        core_config.is_production = true;
        state
            .app_state
            .configs
            .write()
            .await
            .insert("prod-conn".to_string(), core_config);
        let paths = DbxConnectionPaths::from_data_dir(&dir);
        let store = MemoryDbxSecretStore::default();
        let captured = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink: ConfirmEventSink = {
            let captured = Arc::clone(&captured);
            Arc::new(move |request: &ProductionConfirmRequest| {
                captured.lock().push(request.request_id.clone());
            })
        };
        let error = enforce_production_sql_confirmation(
            &state,
            &paths,
            &store,
            "prod-conn",
            None,
            "DELETE FROM users",
            Some(&sink),
            Duration::from_millis(80),
        )
        .await
        .unwrap_err();
        assert_eq!(error, "Production database operation cancelled.");
        // 请求确实发出去了（sink 收到 request id）且条目已清理。
        assert_eq!(captured.lock().len(), 1);
        assert!(state
            .pending_production_confirmations
            .lock()
            .await
            .is_empty());
    }

    #[test]
    fn normalizes_grid_page_size() {
        assert_eq!(normalize_grid_page_size(0), 1);
        assert_eq!(normalize_grid_page_size(100), 100);
        assert_eq!(normalize_grid_page_size(5000), 1000);
    }

    #[test]
    fn reads_total_count_from_first_cell() {
        let result = db::QueryResult {
            columns: vec!["count".to_string()],
            column_types: Vec::new(),
            column_sortables: Vec::new(),
            spatial_columns: Vec::new(),
            spatial_values: Vec::new(),
            rows: vec![vec![serde_json::json!(42)]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            session_id: None,
            has_more: false,
            elasticsearch_raw_body: None,
            messages: Vec::new(),
        };

        assert_eq!(first_u64_cell(&result), Some(42));
    }

    #[test]
    fn blank_execution_id_is_ignored_in_options() {
        let request = ExecuteQueryRequest {
            connection_id: "c".to_string(),
            database: None,
            sql: "SELECT 1".to_string(),
            schema: None,
            max_rows: None,
            fetch_size: None,
            page_size: None,
            result_session_id: Some("  ".to_string()),
            client_session_id: None,
            timeout_secs: None,
            execution_id: Some("   ".to_string()),
        };
        let options = options_from_request(&request);
        assert_eq!(options.result_session_id, None);
        assert_eq!(options.execution_id, None);
    }

    #[test]
    fn multi_query_transaction_option_is_forwarded() {
        let request = ExecuteMultiRequest {
            connection_id: "c".to_string(),
            database: None,
            sql: "SELECT 1;".to_string(),
            schema: None,
            max_rows: Some(10),
            fetch_size: None,
            page_size: None,
            result_session_id: None,
            client_session_id: None,
            timeout_secs: None,
            execution_id: None,
            use_transaction: Some(true),
        };
        let options = options_from_multi_request(&request);
        assert_eq!(options.max_rows, Some(10));
        assert_eq!(options.use_transaction, Some(true));
    }
}
