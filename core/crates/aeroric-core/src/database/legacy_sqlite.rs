//! Legacy (v1) SQLite 文件命令的业务主体（切片九十四刀零上收；与
//! `src-tauri/src/database/legacy_sqlite.rs` 同源 verbatim——endpoint 的
//! Ssh 分支与连接文件路径留在 Tauri/宿主适配层，这里只有「拿到已校验的
//! 本地 .db 路径之后」的纯 rusqlite 逻辑）。宿主 `database_services.rs`
//! 消费这些函数；wire DTO（camelCase）也在这里。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params, Connection, OpenFlags, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
const MAX_PAGE_SIZE: i64 = 500;
const DEFAULT_PAGE_SIZE: i64 = 100;
const ROWID_KEY: &str = "__aeroric_rowid__";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub not_null: bool,
    pub primary_key: bool,
    pub primary_key_ordinal: i64,
    pub default_value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbIndex {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbForeignKey {
    pub table: String,
    pub from: String,
    pub to: String,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbTrigger {
    pub name: String,
    pub sql: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    pub name: String,
    pub object_type: String,
    pub columns: Vec<DbColumn>,
    pub indexes: Vec<DbIndex>,
    pub foreign_keys: Vec<DbForeignKey>,
    pub triggers: Vec<DbTrigger>,
    pub ddl: Option<String>,
    pub row_count: Option<i64>,
    pub editable: bool,
    pub primary_keys: Vec<String>,
    pub has_row_id: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbSchema {
    pub objects: Vec<DbObject>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRow {
    pub row_id: Option<i64>,
    pub key_values: Vec<DbKeyValue>,
    pub values: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbKeyValue {
    pub column: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbCellValue {
    pub column: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRowKey {
    #[serde(default)]
    pub row_id: Option<i64>,
    #[serde(default)]
    pub key_values: Vec<DbCellValue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<DbRow>,
    pub page: i64,
    pub page_size: i64,
    pub total_rows: Option<i64>,
    pub editable: bool,
    pub primary_keys: Vec<String>,
    pub has_row_id: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbExecuteResult {
    pub columns: Vec<String>,
    pub rows: Vec<DbRow>,
    pub rows_affected: usize,
    pub message: String,
}

pub fn normalize_page_size(page_size: Option<i64>) -> i64 {
    page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE)
}

pub fn quote_identifier(identifier: &str) -> Result<String, String> {
    if identifier.is_empty() {
        return Err("Identifier cannot be empty".to_string());
    }
    if identifier.contains('\0') {
        return Err("Identifier contains forbidden characters".to_string());
    }
    Ok(format!("\"{}\"", identifier.replace('"', "\"\"")))
}

pub fn validate_local_db_path(path: &str, project_root: Option<&str>) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Database path must be absolute".to_string());
    }
    if let Some(root) = project_root {
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let canonical = fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if !canonical.starts_with(&root) {
            return Err("Database path is outside the project root".to_string());
        }
        return Ok(canonical);
    }
    Ok(path)
}

pub fn value_ref_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Number(Number::from(value)),
        ValueRef::Real(value) => Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::String(format!("[BLOB {} bytes]", value.len())),
    }
}

pub fn cell_value_to_sql(value: Option<String>) -> SqlValue {
    match value {
        None => SqlValue::Null,
        Some(value) if value.trim().eq_ignore_ascii_case("NULL") => SqlValue::Null,
        Some(value) => SqlValue::Text(value),
    }
}

pub fn sql_params(values: &[SqlValue]) -> Vec<&dyn ToSql> {
    values.iter().map(|value| value as &dyn ToSql).collect()
}

pub fn table_columns(conn: &Connection, table: &str) -> Result<Vec<DbColumn>, String> {
    let pragma_sql = format!("PRAGMA table_info({})", quote_identifier(table)?);
    let mut column_stmt = conn.prepare(&pragma_sql).map_err(|e| e.to_string())?;
    let columns = column_stmt
        .query_map([], |row| {
            let not_null = row.get::<_, i64>(3)? != 0;
            let primary_key_ordinal = row.get::<_, i64>(5)?;
            Ok(DbColumn {
                name: row.get(1)?,
                data_type: row.get::<_, String>(2)?,
                nullable: !not_null,
                not_null,
                primary_key: primary_key_ordinal > 0,
                primary_key_ordinal,
                default_value: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(columns)
}

pub fn primary_keys(columns: &[DbColumn]) -> Vec<String> {
    let mut keys = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| (column.primary_key_ordinal, column.name.clone()))
        .collect::<Vec<_>>();
    keys.sort_by_key(|(ordinal, _)| *ordinal);
    keys.into_iter().map(|(_, name)| name).collect()
}

pub fn table_has_rowid(conn: &Connection, table: &str) -> bool {
    let sql = format!(
        "SELECT rowid FROM {} LIMIT 1",
        quote_identifier(table).unwrap_or_else(|_| "\"\"".to_string())
    );
    conn.prepare(&sql).is_ok()
}

pub fn table_indexes(conn: &Connection, table: &str) -> Result<Vec<DbIndex>, String> {
    let pragma_sql = format!("PRAGMA index_list({})", quote_identifier(table)?);
    let mut stmt = conn.prepare(&pragma_sql).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (name, unique) in raw {
        let detail_sql = format!("PRAGMA index_info({})", quote_identifier(&name)?);
        let mut detail = conn.prepare(&detail_sql).map_err(|e| e.to_string())?;
        let columns = detail
            .query_map([], |row| row.get::<_, String>(2))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        result.push(DbIndex {
            name,
            unique,
            columns,
        });
    }
    Ok(result)
}

pub fn table_foreign_keys(conn: &Connection, table: &str) -> Result<Vec<DbForeignKey>, String> {
    let pragma_sql = format!("PRAGMA foreign_key_list({})", quote_identifier(table)?);
    let mut stmt = conn.prepare(&pragma_sql).map_err(|e| e.to_string())?;
    let foreign_keys = stmt
        .query_map([], |row| {
            Ok(DbForeignKey {
                table: row.get(2)?,
                from: row.get(3)?,
                to: row.get(4)?,
                on_update: row.get(5)?,
                on_delete: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(foreign_keys)
}

pub fn table_triggers(conn: &Connection, table: &str) -> Result<Vec<DbTrigger>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, sql FROM sqlite_master \
             WHERE type = 'trigger' AND tbl_name = ? ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let triggers = stmt
        .query_map([table], |row| {
            Ok(DbTrigger {
                name: row.get(0)?,
                sql: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(triggers)
}

pub fn query_rows(
    conn: &Connection,
    sql: &str,
    page: i64,
    page_size: i64,
    rowid_column: bool,
    key_columns: &[String],
) -> Result<DbQueryResult, String> {
    let offset = page.saturating_sub(1) * page_size;
    let mut statement = conn.prepare(sql).map_err(|e| e.to_string())?;
    let all_columns = statement
        .column_names()
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let columns = if rowid_column {
        all_columns.iter().skip(1).cloned().collect()
    } else {
        all_columns.clone()
    };
    let column_count = statement.column_count();
    let mut rows = statement
        .query(params![page_size, offset])
        .map_err(|e| e.to_string())?;
    let key_indexes = key_columns
        .iter()
        .filter_map(|key| {
            columns
                .iter()
                .position(|column| column.eq_ignore_ascii_case(key))
                .map(|index| (key.clone(), index))
        })
        .collect::<Vec<_>>();
    let mut result_rows = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let row_id = if rowid_column {
            row.get::<_, Option<i64>>(0).unwrap_or(None)
        } else {
            None
        };
        let start = if rowid_column { 1 } else { 0 };
        let mut values = Vec::with_capacity(column_count.saturating_sub(start));
        for index in start..column_count {
            values.push(value_ref_to_json(
                row.get_ref(index).map_err(|e| e.to_string())?,
            ));
        }
        let mut key_values = Vec::new();
        if let Some(row_id) = row_id {
            key_values.push(DbKeyValue {
                column: ROWID_KEY.to_string(),
                value: Value::Number(Number::from(row_id)),
            });
        }
        for (column, value_index) in &key_indexes {
            if let Some(value) = values.get(*value_index) {
                key_values.push(DbKeyValue {
                    column: column.clone(),
                    value: value.clone(),
                });
            }
        }
        result_rows.push(DbRow {
            row_id,
            key_values,
            values,
        });
    }
    Ok(DbQueryResult {
        columns,
        rows: result_rows,
        page,
        page_size,
        total_rows: None,
        editable: false,
        primary_keys: key_columns.to_vec(),
        has_row_id: rowid_column,
    })
}

pub fn inspect_schema(path: &Path) -> Result<DbSchema, String> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT name, type, sql FROM sqlite_master \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )
        .map_err(|e| e.to_string())?;
    let objects = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for (name, object_type, ddl) in objects {
        let columns = table_columns(&conn, &name)?;
        let primary_keys = primary_keys(&columns);
        let has_row_id = object_type == "table" && table_has_rowid(&conn, &name);
        let row_count = if object_type == "table" {
            let count_sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(&name)?);
            conn.query_row(&count_sql, [], |row| row.get::<_, i64>(0))
                .ok()
        } else {
            None
        };
        let indexes = if object_type == "table" {
            table_indexes(&conn, &name).unwrap_or_default()
        } else {
            Vec::new()
        };
        let foreign_keys = if object_type == "table" {
            table_foreign_keys(&conn, &name).unwrap_or_default()
        } else {
            Vec::new()
        };
        let triggers = if object_type == "table" {
            table_triggers(&conn, &name).unwrap_or_default()
        } else {
            Vec::new()
        };
        result.push(DbObject {
            name,
            object_type,
            columns,
            indexes,
            foreign_keys,
            triggers,
            ddl,
            row_count,
            editable: has_row_id || !primary_keys.is_empty(),
            primary_keys,
            has_row_id,
        });
    }
    Ok(DbSchema { objects: result })
}

pub fn query_table(
    path: &Path,
    table: &str,
    page: i64,
    page_size: i64,
) -> Result<DbQueryResult, String> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let table_ident = quote_identifier(table)?;
    let columns_meta = table_columns(&conn, table)?;
    let primary_keys = primary_keys(&columns_meta);
    let has_row_id = table_has_rowid(&conn, table);
    let page = page.max(1);
    let page_size = normalize_page_size(Some(page_size));
    let total_rows = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table_ident}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .ok();
    let sql = if has_row_id {
        format!("SELECT rowid AS {ROWID_KEY}, * FROM {table_ident} LIMIT ? OFFSET ?")
    } else {
        format!("SELECT * FROM {table_ident} LIMIT ? OFFSET ?")
    };
    let mut result = query_rows(&conn, &sql, page, page_size, has_row_id, &primary_keys)?;
    result.total_rows = total_rows;
    result.editable = has_row_id || !primary_keys.is_empty();
    result.primary_keys = primary_keys;
    result.has_row_id = has_row_id;
    Ok(result)
}

/// 切片一百零三：legacy SQL 文件读取（Tauri `db_read_sql_file` verbatim——
/// 绝对路径＋`.sql` 后缀校验后整读；与 `ie_face::read_sql_file` 的 BOM 剥离
/// 版本是两条并存的历史语义，各自服务各自的调用面，不在此合并）。
pub async fn db_read_sql_file(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("SQL file path must be absolute".to_string());
        }
        if path.extension().and_then(|value| value.to_str()) != Some("sql") {
            return Err("Only .sql files can be executed from this action".to_string());
        }
        fs::read_to_string(path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn build_row_where_clause(row_key: DbRowKey) -> Result<(String, Vec<SqlValue>), String> {
    if let Some(row_id) = row_key.row_id {
        return Ok((
            format!("{} = ?", quote_identifier("rowid")?),
            vec![SqlValue::Integer(row_id)],
        ));
    }
    if row_key.key_values.is_empty() {
        return Err(
            "Row cannot be edited because no rowid or primary key is available".to_string(),
        );
    }
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    for key in row_key.key_values {
        if key.column == ROWID_KEY {
            let row_id = key
                .value
                .as_deref()
                .ok_or_else(|| "Invalid rowid value".to_string())?
                .parse::<i64>()
                .map_err(|e| e.to_string())?;
            clauses.push(format!("{} = ?", quote_identifier("rowid")?));
            values.push(SqlValue::Integer(row_id));
            continue;
        }
        let value = cell_value_to_sql(key.value);
        if matches!(value, SqlValue::Null) {
            clauses.push(format!("{} IS NULL", quote_identifier(&key.column)?));
        } else {
            clauses.push(format!("{} = ?", quote_identifier(&key.column)?));
            values.push(value);
        }
    }
    if clauses.is_empty() {
        return Err("Row key is empty".to_string());
    }
    Ok((clauses.join(" AND "), values))
}

pub fn ensure_writable(read_only: bool) -> Result<(), String> {
    if read_only {
        Err("Connection is read-only".to_string())
    } else {
        Ok(())
    }
}

pub fn update_cell(
    path: &Path,
    table: &str,
    row_key: DbRowKey,
    column: &str,
    value: Option<String>,
    read_only: bool,
) -> Result<(), String> {
    ensure_writable(read_only)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let (where_clause, mut values) = build_row_where_clause(row_key)?;
    values.insert(0, cell_value_to_sql(value));
    let sql = format!(
        "UPDATE {} SET {} = ? WHERE {}",
        quote_identifier(table)?,
        quote_identifier(column)?,
        where_clause
    );
    let params = sql_params(&values);
    conn.execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn insert_row(
    path: &Path,
    table: &str,
    values: Vec<DbCellValue>,
    read_only: bool,
) -> Result<(), String> {
    ensure_writable(read_only)?;
    if values.is_empty() {
        return Err("Insert requires at least one value".to_string());
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let columns = values
        .iter()
        .map(|value| quote_identifier(&value.column))
        .collect::<Result<Vec<_>, _>>()?;
    let placeholders = std::iter::repeat_n("?", values.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql_values = values
        .into_iter()
        .map(|value| cell_value_to_sql(value.value))
        .collect::<Vec<_>>();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_identifier(table)?,
        columns.join(", "),
        placeholders
    );
    let params = sql_params(&sql_values);
    conn.execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_row(
    path: &Path,
    table: &str,
    row_key: DbRowKey,
    read_only: bool,
) -> Result<(), String> {
    ensure_writable(read_only)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let (where_clause, values) = build_row_where_clause(row_key)?;
    let sql = format!(
        "DELETE FROM {} WHERE {}",
        quote_identifier(table)?,
        where_clause
    );
    let params = sql_params(&values);
    conn.execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape_next = false;
    for ch in sql.chars() {
        current.push(ch);
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' {
            escape_next = true;
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => {
                let statement = current.trim().trim_end_matches(';').trim();
                if !statement.is_empty() {
                    statements.push(statement.to_string());
                }
                current.clear();
            }
            _ => {}
        }
    }
    let statement = current.trim().trim_end_matches(';').trim();
    if !statement.is_empty() {
        statements.push(statement.to_string());
    }
    statements
}

pub fn strip_leading_sql_comments(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if let Some(comment) = sql.strip_prefix("--") {
            sql = comment.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
            continue;
        }
        if let Some(comment) = sql.strip_prefix("/*") {
            sql = comment
                .find("*/")
                .map(|end| &comment[end + 2..])
                .unwrap_or("");
            continue;
        }
        return sql;
    }
}

pub fn first_sql_word(sql: &str) -> String {
    strip_leading_sql_comments(sql)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Blank out string literals, quoted identifiers and comments so keyword
/// matching only ever sees SQL code. Without this a read-only query such as
/// `WITH x AS (SELECT * FROM logs WHERE action = 'delete') SELECT * FROM x`
/// is misread as a mutation and rejected. Each removed byte becomes a space so
/// neighbouring words cannot fuse into a new token.
pub fn strip_sql_literals(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut output = String::with_capacity(sql.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let closing = match byte {
            b'\'' => Some(b'\''),
            b'"' => Some(b'"'),
            b'`' => Some(b'`'),
            b'[' => Some(b']'),
            _ => None,
        };
        if let Some(closing) = closing {
            output.push(' ');
            index += 1;
            while index < bytes.len() {
                // '' inside a single-quoted literal is an escaped quote.
                if bytes[index] == closing {
                    if closing == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                        output.push_str("  ");
                        index += 2;
                        continue;
                    }
                    output.push(' ');
                    index += 1;
                    break;
                }
                output.push(' ');
                index += 1;
            }
            continue;
        }
        if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
            while index < bytes.len() && bytes[index] != b'\n' {
                output.push(' ');
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            output.push_str("  ");
            index += 2;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output.push_str("  ");
                    index += 2;
                    break;
                }
                output.push(' ');
                index += 1;
            }
            continue;
        }
        // Keep the scan ASCII-only: a non-ASCII byte can only belong to an
        // identifier or literal, never to a keyword we look for.
        output.push(if byte.is_ascii() { byte as char } else { ' ' });
        index += 1;
    }
    output
}

pub fn is_query_statement(sql: &str) -> bool {
    match first_sql_word(sql).as_str() {
        "select" | "pragma" => true,
        // A WITH clause can prefix INSERT/UPDATE/DELETE/REPLACE. Treat those
        // statements as writes so read-only connections cannot bypass the
        // guard by hiding a mutation behind a CTE.
        "with" => !contains_sql_word(
            &strip_sql_literals(strip_leading_sql_comments(sql)),
            &["insert", "update", "delete", "replace"],
        ),
        _ => false,
    }
}

pub fn contains_sql_word(sql: &str, words: &[&str]) -> bool {
    let normalized = sql.to_ascii_lowercase();
    normalized
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|word| words.contains(&word))
}

pub fn validate_sqlite_statement(statement: &str) -> Result<(), String> {
    // Scan code only. `SELECT 'load_extension'` is an ordinary read, and a
    // literal containing `=` must not trip the PRAGMA rule.
    let normalized = strip_sql_literals(strip_leading_sql_comments(statement)).to_ascii_lowercase();
    let normalized = normalized.trim().to_string();
    let first = first_sql_word(&normalized);
    let dangerous = matches!(first.as_str(), "attach" | "detach")
        || (first == "vacuum" && contains_sql_word(&normalized, &["into"]))
        || normalized.contains("load_extension")
        || (first == "pragma"
            && (normalized.contains('=')
                || [
                    "pragma writable_schema",
                    "pragma trusted_schema",
                    "pragma journal_mode(",
                    "pragma locking_mode(",
                    "pragma wal_checkpoint(",
                ]
                .iter()
                .any(|prefix| normalized.starts_with(prefix))));
    if dangerous {
        return Err(
            "SQLite statement is blocked because it can load extensions or access files outside the selected database"
                .to_string(),
        );
    }
    Ok(())
}

pub fn execute_query_sql(
    conn: &Connection,
    statement: &str,
    page: i64,
    page_size: i64,
) -> Result<DbExecuteResult, String> {
    let first = first_sql_word(statement);
    let limited_sql = if matches!(first.as_str(), "select" | "with") {
        format!("SELECT * FROM ({statement}) AS aeroric_query LIMIT ? OFFSET ?")
    } else {
        format!("{statement} LIMIT ? OFFSET ?")
    };
    let result = query_rows(conn, &limited_sql, page, page_size, false, &[])?;
    Ok(DbExecuteResult {
        columns: result.columns,
        rows: result.rows,
        rows_affected: 0,
        message: "Query completed".to_string(),
    })
}

pub fn execute_sql(
    path: &Path,
    sql: &str,
    page: i64,
    page_size: Option<i64>,
    read_only: bool,
) -> Result<DbExecuteResult, String> {
    let statements = split_sql_statements(sql);
    if statements.is_empty() {
        return Err("SQL cannot be empty".to_string());
    }
    for statement in &statements {
        validate_sqlite_statement(statement)?;
    }
    // The statement classifier is deliberately conservative, but SQLite must
    // enforce read-only mode too: PRAGMA syntax has write-capable forms that
    // are easy to miss in a text-only guard.
    let conn = if read_only {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
    } else {
        Connection::open(path)
    }
    .map_err(|e| e.to_string())?;
    let page_size = normalize_page_size(page_size);
    if read_only
        && statements
            .iter()
            .any(|statement| !is_query_statement(statement))
    {
        return Err("Connection is read-only".to_string());
    }
    if statements.len() == 1 && is_query_statement(&statements[0]) {
        return execute_query_sql(&conn, &statements[0], page.max(1), page_size);
    }

    let mut rows_affected = 0usize;
    for statement in statements.iter().take(statements.len().saturating_sub(1)) {
        rows_affected += conn.execute(statement, []).map_err(|e| e.to_string())?;
    }
    let Some(last) = statements.last() else {
        return Err("SQL cannot be empty".to_string());
    };
    if is_query_statement(last) {
        let mut result = execute_query_sql(&conn, last, page.max(1), page_size)?;
        result.rows_affected = rows_affected;
        result.message = if rows_affected > 0 {
            format!("{rows_affected} row(s) affected; query completed")
        } else {
            "Query completed".to_string()
        };
        return Ok(result);
    }
    rows_affected += conn.execute(last, []).map_err(|e| e.to_string())?;
    Ok(DbExecuteResult {
        columns: Vec::new(),
        rows: Vec::new(),
        rows_affected,
        message: format!("{rows_affected} row(s) affected"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn queries_and_updates_sqlite_table() {
        let path = std::env::temp_dir().join(format!("aeroric-test-{}.db", Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("CREATE TABLE notes (title TEXT, body TEXT)", [])
                .unwrap();
            conn.execute(
                "INSERT INTO notes (title, body) VALUES ('one', 'draft')",
                [],
            )
            .unwrap();
        }

        let first = query_table(&path, "notes", 1, 100).unwrap();
        assert_eq!(first.columns, vec!["title".to_string(), "body".to_string()]);
        let row_id = first.rows[0].row_id.unwrap();
        update_cell(
            &path,
            "notes",
            DbRowKey {
                row_id: Some(row_id),
                key_values: Vec::new(),
            },
            "body",
            Some("done".to_string()),
            false,
        )
        .unwrap();
        let second = query_table(&path, "notes", 1, 100).unwrap();
        assert_eq!(second.rows[0].values[1], Value::String("done".to_string()));
        insert_row(
            &path,
            "notes",
            vec![
                DbCellValue {
                    column: "title".to_string(),
                    value: Some("two".to_string()),
                },
                DbCellValue {
                    column: "body".to_string(),
                    value: Some("new".to_string()),
                },
            ],
            false,
        )
        .unwrap();
        let third = query_table(&path, "notes", 1, 100).unwrap();
        assert_eq!(third.rows.len(), 2);
        delete_row(
            &path,
            "notes",
            DbRowKey {
                row_id: third.rows[1].row_id,
                key_values: Vec::new(),
            },
            false,
        )
        .unwrap();
        let fourth = query_table(&path, "notes", 1, 100).unwrap();
        assert_eq!(fourth.rows.len(), 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn updates_without_rowid_table_by_primary_key() {
        let path = std::env::temp_dir().join(format!("aeroric-test-{}.db", Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('theme', 'dark')",
                [],
            )
            .unwrap();
        }

        let first = query_table(&path, "settings", 1, 100).unwrap();
        assert!(!first.has_row_id);
        assert_eq!(first.primary_keys, vec!["key".to_string()]);
        update_cell(
            &path,
            "settings",
            DbRowKey {
                row_id: None,
                key_values: vec![DbCellValue {
                    column: "key".to_string(),
                    value: Some("theme".to_string()),
                }],
            },
            "value",
            Some("light".to_string()),
            false,
        )
        .unwrap();
        let second = query_table(&path, "settings", 1, 100).unwrap();
        assert_eq!(second.rows[0].values[1], Value::String("light".to_string()));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn executes_multi_statement_script_and_returns_last_query() {
        let path = std::env::temp_dir().join(format!("aeroric-test-{}.db", Uuid::new_v4()));
        let result = execute_sql(
            &path,
            "CREATE TABLE logs (message TEXT); INSERT INTO logs VALUES ('ok'); SELECT message FROM logs;",
            1,
            Some(100),
            false,
        )
        .unwrap();
        assert_eq!(result.columns, vec!["message".to_string()]);
        assert_eq!(result.rows[0].values[0], Value::String("ok".to_string()));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_only_rejects_write_operations() {
        let path = std::env::temp_dir().join(format!("aeroric-test-{}.db", Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("CREATE TABLE notes (title TEXT)", []).unwrap();
        }
        assert!(execute_sql(
            &path,
            "INSERT INTO notes VALUES ('nope')",
            1,
            Some(100),
            true,
        )
        .is_err());
        assert!(execute_sql(&path, "PRAGMA user_version (123)", 1, Some(100), true).is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_only_rejects_mutating_ctes() {
        let path = std::env::temp_dir().join(format!("aeroric-test-{}.db", Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("CREATE TABLE notes (title TEXT)", []).unwrap();
        }
        assert!(execute_sql(
            &path,
            "WITH target AS (SELECT rowid FROM notes) DELETE FROM notes WHERE rowid IN (SELECT rowid FROM target)",
            1,
            Some(100),
            true,
        )
        .is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn blocks_sqlite_file_and_extension_escape_hatches() {
        for statement in [
            "ATTACH DATABASE '/tmp/other.db' AS other",
            "/* user input */ DETACH DATABASE other",
            "VACUUM INTO '/tmp/backup.db'",
            "SELECT load_extension('/tmp/extension')",
            "PRAGMA writable_schema = ON",
        ] {
            let error = validate_sqlite_statement(statement).unwrap_err();
            assert!(error.contains("blocked"), "statement: {statement}");
        }
    }

    #[test]
    fn permits_normal_sqlite_queries_and_schema_changes() {
        for statement in [
            "-- read\nSELECT * FROM notes",
            "PRAGMA table_info('notes')",
            "CREATE TABLE notes (title TEXT)",
            "UPDATE notes SET title = 'ok'",
        ] {
            assert!(
                validate_sqlite_statement(statement).is_ok(),
                "statement: {statement}"
            );
        }
    }

    #[test]
    fn keyword_guards_ignore_string_literals_and_quoted_identifiers() {
        // Keywords that only appear as data must not trip the guards.
        for statement in [
            "SELECT 'load_extension' AS label",
            "SELECT * FROM notes WHERE title = 'attach database'",
            "INSERT INTO notes (title) VALUES ('vacuum into backup')",
            "SELECT \"load_extension\" FROM notes",
        ] {
            assert!(
                validate_sqlite_statement(statement).is_ok(),
                "statement: {statement}"
            );
        }

        // A read-only CTE whose literal mentions a write verb stays a query.
        assert!(is_query_statement(
            "WITH x AS (SELECT * FROM logs WHERE action = 'delete') SELECT * FROM x"
        ));
        assert!(is_query_statement(
            "WITH x AS (SELECT * FROM logs WHERE action IN ('insert', 'update')) SELECT * FROM x"
        ));

        // The real mutation behind a CTE is still classified as a write.
        assert!(!is_query_statement(
            "WITH target AS (SELECT rowid FROM notes) DELETE FROM notes WHERE rowid IN (SELECT rowid FROM target)"
        ));

        // And the escape hatches remain blocked when they are real code.
        assert!(validate_sqlite_statement("SELECT load_extension('/tmp/x')").is_err());
        assert!(validate_sqlite_statement("ATTACH DATABASE '/tmp/o.db' AS o").is_err());
    }

    #[test]
    fn split_sql_keeps_semicolon_inside_string() {
        let parts = split_sql_statements("select ';'; select 2;");
        assert_eq!(parts, vec!["select ';'", "select 2"]);
    }

    #[test]
    fn quote_identifier_escapes_double_quotes() {
        assert_eq!(quote_identifier("a\"b").unwrap(), "\"a\"\"b\"");
    }
}
