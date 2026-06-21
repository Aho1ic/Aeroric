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
    Ok(dbx_core::db_admin_sql::build_create_database_sql(options))
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
    Ok(dbx_core::db_admin_sql::build_create_schema_sql(options))
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

    #[test]
    fn builds_create_database_sql_with_dbx_core() {
        let sql = dbx_build_create_database_sql(dbx_core::db_admin_sql::CreateDatabaseSqlOptions {
            database_type: Some(dbx_core::models::connection::DatabaseType::Mysql),
            driver_profile: Some("mysql".to_string()),
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
}
