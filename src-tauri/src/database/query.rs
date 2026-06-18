use dbx_core::db;
use dbx_core::query::QueryExecutionOptions;
use serde::Deserialize;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

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
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        execution_id: non_empty(request.execution_id.clone()),
    }
}

fn options_from_multi_request(request: &ExecuteMultiRequest) -> QueryExecutionOptions {
    QueryExecutionOptions {
        max_rows: request.max_rows,
        fetch_size: request.fetch_size,
        page_size: request.page_size,
        result_session_id: non_empty(request.result_session_id.clone()),
        client_session_id: non_empty(request.client_session_id.clone()),
        timeout_secs: request.timeout_secs,
        execution_id: non_empty(request.execution_id.clone()),
    }
}

#[tauri::command]
pub async fn dbx_execute_query(
    state: State<'_, DbxState>,
    request: ExecuteQueryRequest,
) -> Result<db::QueryResult, String> {
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
    state: State<'_, DbxState>,
    request: ExecuteMultiRequest,
) -> Result<Vec<db::QueryResult>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
