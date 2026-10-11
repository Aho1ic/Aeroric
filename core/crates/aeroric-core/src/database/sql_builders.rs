//! P8-05 切片九十六（database 刀二）：SQL builders 纯函数族的宿主依赖点。
//!
//! 业务主体全部在 dbx-core（`table_structure_sql`／`db_admin_sql`／
//! `database_search_sql`）；这里把宿主服务需要的类型与函数 re-export 成
//! core 单一依赖点（宿主不直接依赖 dbx-core），并把 Tauri 侧的 DuckDB
//! ATTACH 两行构造 verbatim 上收——dbx-core 的同名函数被 `duckdb-sidecar`
//! feature 门控（core 未启用该 feature），Tauri 本地实现才是生产路径。

pub use dbx_core::database_search_sql::{
    build_database_search_sql, build_search_result_where, DatabaseSearchSql,
    DatabaseSearchSqlOptions, SearchResultWhereOptions,
};
pub use dbx_core::db_admin_sql::{
    build_create_database_sql, build_create_schema_sql, build_drop_database_sql,
    build_drop_object_sql, build_drop_schema_sql, build_drop_table_child_object_sql,
    build_drop_table_sql, build_duplicate_table_structure_sql, build_empty_table_sql,
    build_rename_object_sql, build_truncate_table_sql, CreateDatabaseSqlOptions,
    DatabaseNameSqlOptions, DatabaseObjectType, DropObjectSqlOptions,
    DropTableChildObjectSqlOptions, DuplicateTableStructureSqlOptions, RenameObjectSqlOptions,
    SchemaNameSqlOptions, TableAdminSqlOptions, TableChildObjectType,
};
pub use dbx_core::models::connection::DatabaseType;
pub use dbx_core::table_structure_sql::{
    build_single_column_alter_sql, build_table_structure_change_sql, EditableStructureColumn,
    SingleColumnAlterSqlOptions, TableStructureSqlOptions, TableStructureSqlResult,
};

use serde::{Deserialize, Serialize};

/// DuckDB ATTACH 请求（与 Tauri `DuckDbAttachDatabaseSqlOptions` 同源
/// verbatim——camelCase wire 由前端 IPC 形状决定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuckDbAttachDatabaseSqlOptions {
    pub path: String,
    pub name: String,
}

fn quote_duckdb_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `ATTACH '<path>' AS "<name>";`（Tauri query.rs 本地构造逐字——路径经
/// SQL 字符串转义、库名经 DuckDB 双引号标识符转义）。
pub fn build_duckdb_attach_database_sql(options: DuckDbAttachDatabaseSqlOptions) -> String {
    format!(
        "ATTACH {} AS {};",
        quote_sql_string(&options.path),
        quote_duckdb_identifier(&options.name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbx_core::models::connection::DatabaseType;

    #[test]
    fn builds_create_database_sql_with_dbx_core() {
        let sql = build_create_database_sql(CreateDatabaseSqlOptions {
            database_type: Some(DatabaseType::Mysql),
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
        let sql = build_duckdb_attach_database_sql(DuckDbAttachDatabaseSqlOptions {
            path: "/tmp/app's.duckdb".to_string(),
            name: "app db".to_string(),
        });

        assert_eq!(sql, "ATTACH '/tmp/app''s.duckdb' AS \"app db\";");
    }

    #[test]
    fn builds_drop_database_and_schema_sql_with_dbx_core() {
        let database_sql = build_drop_database_sql(DatabaseNameSqlOptions {
            database_type: Some(DatabaseType::Postgres),
            name: "app".to_string(),
        });
        let create_schema_sql = build_create_schema_sql(SchemaNameSqlOptions {
            database_type: Some(DatabaseType::Postgres),
            name: "analytics".to_string(),
        })
        .unwrap();
        let drop_schema_sql = build_drop_schema_sql(SchemaNameSqlOptions {
            database_type: Some(DatabaseType::Postgres),
            name: "analytics".to_string(),
        });

        assert_eq!(database_sql, "DROP DATABASE \"app\";");
        assert_eq!(create_schema_sql, "CREATE SCHEMA \"analytics\";");
        assert_eq!(drop_schema_sql, "DROP SCHEMA \"analytics\" CASCADE;");
    }

    #[test]
    fn builds_rename_object_sql_with_dbx_core() {
        let sql = build_rename_object_sql(RenameObjectSqlOptions {
            database_type: Some(DatabaseType::Postgres),
            object_type: DatabaseObjectType::Table,
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
        let query = build_database_search_sql(DatabaseSearchSqlOptions {
            database_type: Some(DatabaseType::Postgres),
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
        })
        .unwrap();

        assert!(query.sql.contains("FROM \"public\".\"users\""));
        assert_eq!(query.searchable_columns, vec!["email"]);

        let where_input = build_search_result_where(SearchResultWhereOptions {
            database_type: Some(DatabaseType::Postgres),
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
        });

        assert_eq!(where_input, "\"id\" = 42");
    }
}
