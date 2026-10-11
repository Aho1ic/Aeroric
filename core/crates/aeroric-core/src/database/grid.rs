//! P7-04 Action 1/4：grid 分页纯函数（core 独占；**百万行结果只请求有界页，
//! 内存不加载全表**——分页/页大小 clamp 与 grid 纯函数；DataGrid SQL 构造在
//! dbx-core data_grid_sql，这里承载分页守卫）。P8-05 切片九十六（database
//! 刀二）起另承载 grid 预览/复制纯函数的宿主依赖点：`SqlPreviewResponse`
//! 与 executed=false 的预览映射 verbatim 上收（刀三的 save 执行半边复用同
//! DTO），data_grid_sql 的类型与函数在此 re-export 供宿主消费。
use serde::{Deserialize, Serialize};

pub use dbx_core::data_grid_sql::{
    build_data_grid_context_filter_condition, build_data_grid_copy_insert_statement,
    build_data_grid_copy_update_statements, prepare_data_grid_save, DataGridColumnInfo,
    DataGridContextFilterConditionOptions, DataGridContextFilterMode,
    DataGridCopyInsertStatementOptions, DataGridCopyUpdateStatementOptions,
    DataGridSavePreparation, DataGridSaveStatementOptions, DataGridTableMeta,
};

/// 表数据请求（与 Tauri `TableDataRequest` camelCase 字段对齐）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDataRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub table: String,
    #[serde(default)]
    pub page: usize,
    #[serde(default)]
    pub page_size: usize,
    #[serde(default)]
    pub order_by: Option<String>,
    #[serde(default)]
    pub where_input: Option<String>,
}

/// 页大小归一（verbatim：1..=500 clamp；百万行只请求有界页）。
pub fn normalize_page_size(page_size: usize) -> usize {
    page_size.clamp(1, 500)
}

/// 分页守卫（百万行结果只请求有界页，内存不加载全表——page/page_size clamp
/// 后的 offset 计算；无界请求拒绝）。
pub fn bounded_offset(page: usize, page_size: usize) -> Result<(usize, usize), String> {
    let size = normalize_page_size(page_size);
    if page == 0 {
        return Err("page must be >= 1".to_string());
    }
    let offset = (page - 1).saturating_mul(size);
    // 有界：offset 上限（防深分页 DoS；与宿主 MAX 上限同语义）。
    const MAX_OFFSET: usize = 10_000_000;
    if offset > MAX_OFFSET {
        return Err("page offset exceeds the bounded limit".to_string());
    }
    Ok((offset, size))
}

/// grid 条件保护（Action 4：编辑使用原 DBX 条件保护，不拼 Swift SQL——
/// where_input 的 NUL/分隔符守卫）。
pub fn validate_where_input(where_input: &str) -> Result<(), String> {
    if where_input.contains('\0') {
        return Err("where input contains NUL".to_string());
    }
    if where_input.contains(';') {
        return Err("where input contains multiple statements".to_string());
    }
    Ok(())
}

/// SQL 预览/保存回执（与 Tauri `SqlPreviewResponse` 同源 verbatim——
/// camelCase wire；executed=false 是纯预览，true 由刀三的执行半边发布）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlPreviewResponse {
    pub statements: Vec<String>,
    pub rollback_statements: Vec<String>,
    pub validation_error: Option<String>,
    pub execution_schema: Option<String>,
    pub executed: bool,
    pub rows_affected: u64,
}

/// 预览映射（Tauri grid.rs `preview_response` 逐字）。
pub fn preview_response(
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

/// grid SQL 预览（`dbx_preview_grid_sql` 的纯半边：只 prepare 不执行）。
pub fn preview_grid_sql(options: DataGridSaveStatementOptions) -> SqlPreviewResponse {
    preview_response(prepare_data_grid_save(options), false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbx_core::data_grid_sql::{
        DataGridColumnInfo, DataGridContextFilterMode, DataGridTableMeta,
    };
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
    fn builds_copy_insert_statement_with_dbx_core() {
        let statement = build_data_grid_copy_insert_statement(DataGridCopyInsertStatementOptions {
            database_type: Some(DatabaseType::Postgres),
            // None 表示沿用 database_type 的方言默认引号,Postgres 下仍是 `"`。
            identifier_quote: None,
            table_meta: Some(test_table_meta()),
            columns: vec!["id".to_string(), "email".to_string()],
            column_types: None,
            source_columns: Some(vec![Some("id".to_string()), Some("email".to_string())]),
            rows: vec![vec![json!(1), json!("alice@example.com")]],
            exclude_primary_keys: false,
            include_computed_columns: false,
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
        let statements =
            build_data_grid_copy_update_statements(DataGridCopyUpdateStatementOptions {
                database_type: Some(DatabaseType::Postgres),
                identifier_quote: None,
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
        let condition =
            build_data_grid_context_filter_condition(DataGridContextFilterConditionOptions {
                database_type: Some(DatabaseType::Postgres),
                identifier_quote: None,
                column_name: "email".to_string(),
                mode: DataGridContextFilterMode::Like,
                value: json!("alice@example.com"),
                values: Vec::new(),
                end_value: None,
                column_info: Some(test_column("email", "text", false)),
            });

        assert_eq!(
            condition.as_deref(),
            Some("\"email\" LIKE '%alice@example.com%'")
        );
    }

    #[test]
    fn previews_grid_sql_without_executing() {
        let response = preview_grid_sql(DataGridSaveStatementOptions {
            database_type: Some(DatabaseType::Postgres),
            identifier_quote: None,
            table_meta: test_table_meta(),
            columns: vec!["id".to_string(), "email".to_string()],
            source_columns: None,
            rows: Vec::new(),
            dirty_rows: Vec::new(),
            deleted_rows: Vec::new(),
            new_rows: Vec::new(),
        });

        assert!(!response.executed);
        assert_eq!(response.rows_affected, 0);
    }
}
