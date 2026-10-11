use aeroric_core::database::grid as grid_biz;
use aeroric_core::database::grid::{DataGridSaveStatementOptions, SqlPreviewResponse};
use dbx_core::data_grid_sql;
use dbx_core::db;
use dbx_core::models::connection::DatabaseType;
use dbx_core::query::QueryExecutionOptions;
use dbx_core::sql_dialect::{build_table_data_select_sql, TableDataSelectSqlOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TableDataRequest {
    connection_id: String,
    #[serde(default)]
    catalog: Option<String>,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    table: String,
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    where_input: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TableDataResponse {
    result: db::QueryResult,
    total_rows: Option<u64>,
    sql: String,
    count_sql: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GridSaveRequest {
    connection_id: String,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    options: DataGridSaveStatementOptions,
    #[serde(default)]
    execute: bool,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    100
}

fn normalize_page_size(page_size: usize) -> usize {
    page_size.clamp(1, 1000)
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

async fn execute_grid_save(
    state: &DbxState,
    request: GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    connections::ensure_connected(state, &request.connection_id).await?;
    let db_type = database_type(state, &request.connection_id).await;
    let preparation = data_grid_sql::prepare_data_grid_save(request.options);
    if preparation.validation_error.is_some()
        || !request.execute
        || preparation.statements.is_empty()
    {
        return Ok(preview_response(preparation, false, 0));
    }

    let database = request.database.unwrap_or_default();
    let schema = preparation
        .execution_schema
        .as_deref()
        .or(request.schema.as_deref());
    let result = dbx_core::query::execute_statements(
        &state.app_state,
        &request.connection_id,
        &database,
        &preparation.statements,
        schema,
        None,
    )
    .await
    .map_err(|error| data_grid_sql::normalize_data_grid_save_error(db_type, &error))?;

    Ok(preview_response(preparation, true, result.affected_rows))
}

use grid_biz::preview_response;

#[tauri::command]
pub async fn dbx_query_table_data(
    state: State<'_, DbxState>,
    request: TableDataRequest,
) -> Result<TableDataResponse, String> {
    connections::ensure_connected(&state, &request.connection_id).await?;
    let database = request.database.clone().unwrap_or_default();
    let qualified_database = (!database.is_empty()).then_some(database.clone());
    let identifier_quote = state
        .app_state
        .connection_identifier_quote(&request.connection_id, qualified_database.as_deref())
        .await?;
    let schema = request.schema.clone();
    let page_size = normalize_page_size(request.page_size);
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
    let db_type = database_type(&state, &request.connection_id).await;
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

    let result = dbx_core::query::execute_sql_statement_with_options(
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
    let count_result = dbx_core::query::execute_sql_statement_with_options(
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

    Ok(TableDataResponse {
        result,
        total_rows,
        sql,
        count_sql,
    })
}

// ── 切片九十六（database 刀二）：grid 预览/复制四条改宿主转发壳。业务主体
// 在 dbx-core data_grid_sql，core `database::grid` re-export 为宿主同一依赖
// 点；Embedded 本地闭包与宿主服务调同一个函数，External 绝不本地回退。 ──

#[tauri::command]
pub async fn dbx_preview_grid_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: DataGridSaveStatementOptions,
) -> Result<SqlPreviewResponse, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_preview_grid_sql",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(grid_biz::preview_grid_sql(options))
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_insert_statement<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridCopyInsertStatementOptions,
) -> Result<Option<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_copy_insert_statement",
        serde_json::json!({ "options": &options }),
        async {
            Ok(
                aeroric_core::database::grid::build_data_grid_copy_insert_statement(options)
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_update_statements<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridCopyUpdateStatementOptions,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_copy_update_statements",
        serde_json::json!({ "options": &options }),
        async {
            serde_json::to_value(
                aeroric_core::database::grid::build_data_grid_copy_update_statements(options),
            )
            .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_build_data_grid_context_filter_condition<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: aeroric_core::database::grid::DataGridContextFilterConditionOptions,
) -> Result<Option<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_build_data_grid_context_filter_condition",
        serde_json::json!({ "options": &options }),
        async {
            Ok(
                aeroric_core::database::grid::build_data_grid_context_filter_condition(options)
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_update_cell(
    state: State<'_, DbxState>,
    request: GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    execute_grid_save(&state, request).await
}

#[tauri::command]
pub async fn dbx_insert_row(
    state: State<'_, DbxState>,
    request: GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    execute_grid_save(&state, request).await
}

#[tauri::command]
pub async fn dbx_delete_rows(
    state: State<'_, DbxState>,
    request: GridSaveRequest,
) -> Result<SqlPreviewResponse, String> {
    execute_grid_save(&state, request).await
}

#[cfg(test)]
mod tests {
    use super::{first_u64_cell, normalize_page_size};
    use dbx_core::db::QueryResult;
    use serde_json::json;

    #[test]
    fn normalizes_grid_page_size() {
        assert_eq!(normalize_page_size(0), 1);
        assert_eq!(normalize_page_size(100), 100);
        assert_eq!(normalize_page_size(5000), 1000);
    }

    #[test]
    fn reads_total_count_from_first_cell() {
        let result = QueryResult {
            columns: vec!["count".to_string()],
            column_types: Vec::new(),
            column_sortables: Vec::new(),
            spatial_columns: Vec::new(),
            spatial_values: Vec::new(),
            rows: vec![vec![json!(42)]],
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
}
