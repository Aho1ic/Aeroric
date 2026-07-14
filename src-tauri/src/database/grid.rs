use dbx_core::data_grid_sql::{
    self, DataGridContextFilterConditionOptions, DataGridCopyInsertStatementOptions,
    DataGridCopyUpdateStatementOptions, DataGridSavePreparation, DataGridSaveStatementOptions,
};
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SqlPreviewResponse {
    statements: Vec<String>,
    rollback_statements: Vec<String>,
    validation_error: Option<String>,
    execution_schema: Option<String>,
    executed: bool,
    rows_affected: u64,
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

fn preview_response(
    preparation: DataGridSavePreparation,
    executed: bool,
    rows_affected: u64,
) -> SqlPreviewResponse {
    SqlPreviewResponse {
        statements: preparation.statements,
        rollback_statements: preparation.rollback_statements,
        validation_error: preparation.validation_error,
        execution_schema: preparation.execution_schema,
        executed,
        rows_affected,
    }
}

fn copy_insert_statement(options: DataGridCopyInsertStatementOptions) -> Option<String> {
    data_grid_sql::build_data_grid_copy_insert_statement(options)
}

fn copy_update_statements(options: DataGridCopyUpdateStatementOptions) -> Vec<String> {
    data_grid_sql::build_data_grid_copy_update_statements(options)
}

fn context_filter_condition(options: DataGridContextFilterConditionOptions) -> Option<String> {
    data_grid_sql::build_data_grid_context_filter_condition(options)
}

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
        identifier_quote: identifier_quote.clone(),
        schema: schema.clone(),
        table_name: request.table.clone(),
        catalog: request.catalog.clone(),
        database: qualified_database.clone(),
        table_type: None,
        primary_keys,
        columns: column_names,
        fallback_order_columns: Vec::new(),
        order_by: request.order_by.clone(),
        limit: Some(page_size),
        offset: Some(offset),
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

#[tauri::command]
pub async fn dbx_preview_grid_sql(
    options: DataGridSaveStatementOptions,
) -> Result<SqlPreviewResponse, String> {
    Ok(preview_response(
        data_grid_sql::prepare_data_grid_save(options),
        false,
        0,
    ))
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_insert_statement(
    options: DataGridCopyInsertStatementOptions,
) -> Result<Option<String>, String> {
    Ok(copy_insert_statement(options))
}

#[tauri::command]
pub async fn dbx_build_data_grid_copy_update_statements(
    options: DataGridCopyUpdateStatementOptions,
) -> Result<Vec<String>, String> {
    Ok(copy_update_statements(options))
}

#[tauri::command]
pub async fn dbx_build_data_grid_context_filter_condition(
    options: DataGridContextFilterConditionOptions,
) -> Result<Option<String>, String> {
    Ok(context_filter_condition(options))
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
    use super::{
        context_filter_condition, copy_insert_statement, copy_update_statements, first_u64_cell,
        normalize_page_size,
    };
    use dbx_core::data_grid_sql::{
        DataGridColumnInfo, DataGridContextFilterConditionOptions, DataGridContextFilterMode,
        DataGridCopyInsertStatementOptions, DataGridCopyUpdateStatementOptions, DataGridTableMeta,
    };
    use dbx_core::db::QueryResult;
    use dbx_core::models::connection::DatabaseType;
    use serde_json::json;

    fn test_column(name: &str, data_type: &str, is_primary_key: bool) -> DataGridColumnInfo {
        DataGridColumnInfo {
            name: name.to_string(),
            data_type: data_type.to_string(),
            is_nullable: !is_primary_key,
            is_primary_key,
            column_default: None,
            extra: None,
        }
    }

    fn test_table_meta() -> DataGridTableMeta {
        DataGridTableMeta {
            catalog: None,
            database: None,
            schema: Some("public".to_string()),
            table_name: "users".to_string(),
            primary_keys: vec!["id".to_string()],
            columns: Some(vec![
                test_column("id", "integer", true),
                test_column("email", "text", false),
            ]),
        }
    }

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
            rows: vec![vec![json!(42)]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            session_id: None,
            has_more: false,
        };

        assert_eq!(first_u64_cell(&result), Some(42));
    }

    #[test]
    fn builds_copy_insert_statement_with_dbx_core() {
        let statement = copy_insert_statement(DataGridCopyInsertStatementOptions {
            database_type: Some(DatabaseType::Postgres),
            table_meta: Some(test_table_meta()),
            columns: vec!["id".to_string(), "email".to_string()],
            column_types: None,
            source_columns: Some(vec![Some("id".to_string()), Some("email".to_string())]),
            rows: vec![vec![json!(1), json!("alice@example.com")]],
            exclude_primary_keys: false,
            insert_mode: Default::default(),
        });

        assert_eq!(
            statement.as_deref(),
            Some(
                "INSERT INTO \"public\".\"users\" (\"id\", \"email\") VALUES (1, 'alice@example.com');",
            )
        );
    }

    #[test]
    fn builds_copy_update_statements_with_dbx_core() {
        let statements = copy_update_statements(DataGridCopyUpdateStatementOptions {
            database_type: Some(DatabaseType::Postgres),
            table_meta: test_table_meta(),
            columns: vec!["id".to_string(), "email".to_string()],
            source_columns: Some(vec![Some("id".to_string()), Some("email".to_string())]),
            rows: vec![vec![json!(1), json!("alice@example.com")]],
        });

        assert_eq!(
            statements,
            vec![
                "UPDATE \"public\".\"users\" SET \"email\" = 'alice@example.com' WHERE \"id\" = 1;",
            ]
        );
    }

    #[test]
    fn builds_context_filter_condition_with_dbx_core() {
        let condition = context_filter_condition(DataGridContextFilterConditionOptions {
            database_type: Some(DatabaseType::Postgres),
            column_name: "email".to_string(),
            mode: DataGridContextFilterMode::Like,
            value: json!("alice@example.com"),
            values: Vec::new(),
            end_value: None,
            column_info: Some(test_column("email", "text", false)),
        });

        assert_eq!(
            condition.as_deref(),
            Some("\"email\" LIKE '%alice@example.com%'"),
        );
    }
}
