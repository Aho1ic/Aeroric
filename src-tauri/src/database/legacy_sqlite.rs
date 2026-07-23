use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params, Connection, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use uuid::Uuid;

use crate::ssh::SshConnection;

const MAX_PAGE_SIZE: i64 = 500;
const DEFAULT_PAGE_SIZE: i64 = 100;
const ROWID_KEY: &str = "__aeroric_rowid__";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum DbEndpoint {
    Local {
        path: String,
    },
    Ssh {
        connection: SshConnection,
        path: String,
        #[serde(rename = "projectPath", skip_serializing_if = "Option::is_none")]
        project_path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbConnectionConfig {
    id: String,
    name: String,
    endpoint: DbEndpoint,
    #[serde(default)]
    read_only: bool,
    created_at: i64,
    last_opened_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbColumn {
    name: String,
    data_type: String,
    nullable: bool,
    not_null: bool,
    primary_key: bool,
    primary_key_ordinal: i64,
    default_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbIndex {
    name: String,
    unique: bool,
    columns: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbForeignKey {
    table: String,
    from: String,
    to: String,
    on_update: String,
    on_delete: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbTrigger {
    name: String,
    sql: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbObject {
    name: String,
    object_type: String,
    columns: Vec<DbColumn>,
    indexes: Vec<DbIndex>,
    foreign_keys: Vec<DbForeignKey>,
    triggers: Vec<DbTrigger>,
    ddl: Option<String>,
    row_count: Option<i64>,
    editable: bool,
    primary_keys: Vec<String>,
    has_row_id: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbSchema {
    objects: Vec<DbObject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbRow {
    row_id: Option<i64>,
    key_values: Vec<DbKeyValue>,
    values: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbKeyValue {
    column: String,
    value: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbCellValue {
    column: String,
    value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbRowKey {
    #[serde(default)]
    row_id: Option<i64>,
    #[serde(default)]
    key_values: Vec<DbCellValue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbQueryResult {
    columns: Vec<String>,
    rows: Vec<DbRow>,
    page: i64,
    page_size: i64,
    total_rows: Option<i64>,
    editable: bool,
    primary_keys: Vec<String>,
    has_row_id: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbExecuteResult {
    columns: Vec<String>,
    rows: Vec<DbRow>,
    rows_affected: usize,
    message: String,
}

fn database_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("database-connections.json"))
}

fn endpoints_match(left: &DbEndpoint, right: &DbEndpoint) -> bool {
    match (left, right) {
        (DbEndpoint::Local { path: left }, DbEndpoint::Local { path: right }) => {
            let left = fs::canonicalize(left).unwrap_or_else(|_| PathBuf::from(left));
            let right = fs::canonicalize(right).unwrap_or_else(|_| PathBuf::from(right));
            left == right
        }
        (
            DbEndpoint::Ssh {
                connection: left_connection,
                path: left_path,
                ..
            },
            DbEndpoint::Ssh {
                connection: right_connection,
                path: right_path,
                ..
            },
        ) => {
            left_connection.host == right_connection.host
                && left_connection.port == right_connection.port
                && left_connection.username == right_connection.username
                && left_path.trim_end_matches('/') == right_path.trim_end_matches('/')
        }
        _ => false,
    }
}

fn resolve_connection_read_only(
    connections: &[DbConnectionConfig],
    endpoint: &DbEndpoint,
    connection_id: Option<&str>,
    requested_read_only: bool,
) -> Result<bool, String> {
    if let Some(connection_id) = connection_id.filter(|value| !value.trim().is_empty()) {
        if let Some(connection) = connections
            .iter()
            .find(|connection| connection.id == connection_id)
        {
            if !endpoints_match(&connection.endpoint, endpoint) {
                return Err("Connection endpoint does not match saved configuration".to_string());
            }
            return Ok(connection.read_only);
        }
    }

    let matching = connections
        .iter()
        .filter(|connection| endpoints_match(&connection.endpoint, endpoint))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        Ok(requested_read_only)
    } else {
        Ok(matching.iter().any(|connection| connection.read_only))
    }
}

fn authoritative_read_only(
    endpoint: &DbEndpoint,
    connection_id: Option<&str>,
    requested_read_only: bool,
) -> Result<bool, String> {
    let path = database_connections_path()?;
    if !path.exists() {
        return Ok(requested_read_only);
    }
    crate::storage::ensure_private_file_permissions(&path)?;
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let connections: Vec<DbConnectionConfig> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    resolve_connection_read_only(&connections, endpoint, connection_id, requested_read_only)
}

fn normalize_page_size(page_size: Option<i64>) -> i64 {
    page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE)
}

pub(crate) fn quote_identifier(identifier: &str) -> Result<String, String> {
    if identifier.is_empty() {
        return Err("Identifier cannot be empty".to_string());
    }
    if identifier.contains('\0') {
        return Err("Identifier contains forbidden characters".to_string());
    }
    Ok(format!("\"{}\"", identifier.replace('"', "\"\"")))
}

fn validate_local_db_path(path: &str, project_root: Option<&str>) -> Result<PathBuf, String> {
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

fn remote_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn validate_remote_db_path(path: &str, project_root: Option<&str>) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err("Remote database path must be absolute".to_string());
    }
    if path.contains('\0') || remote_path_has_relative_components(path) {
        return Err("Remote database path is invalid".to_string());
    }
    let path = if path == "/" {
        "/".to_string()
    } else {
        path.trim_end_matches('/').to_string()
    };
    if let Some(root) = project_root {
        if !root.starts_with('/') || remote_path_has_relative_components(root) {
            return Err("Remote project path is invalid".to_string());
        }
        let root = if root == "/" {
            "/".to_string()
        } else {
            root.trim_end_matches('/').to_string()
        };
        let prefix = format!("{}/", root.trim_end_matches('/'));
        if path != root && !path.starts_with(&prefix) {
            return Err("Remote database path is outside the project root".to_string());
        }
    }
    Ok(path)
}

fn sshpass_program() -> String {
    let detected = crate::platform::detect_path("sshpass");
    if detected.is_empty() {
        "sshpass".to_string()
    } else {
        detected
    }
}

fn scp_command(connection: &SshConnection) -> Command {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let mut command = if password.is_some() {
        let mut cmd = Command::new(sshpass_program());
        cmd.arg("-e").arg("scp");
        cmd
    } else {
        Command::new("scp")
    };
    command.arg("-P").arg(connection.port.to_string());
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        command.arg("-i").arg(identity_file);
    }
    if let Some(password) = password {
        command.env("SSHPASS", password);
    }
    command.env("PATH", crate::app_settings::get_login_shell_path());
    crate::subprocess::configure_background_command(&mut command);
    command
}

fn remote_target(connection: &SshConnection, remote_path: &str) -> String {
    format!(
        "{}@{}:{}",
        connection.username,
        connection.host,
        crate::ssh::shell_quote_posix(remote_path)
    )
}

fn run_scp(mut command: Command) -> Result<(), String> {
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn temp_db_path() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("aeroric-db");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{}.db", Uuid::new_v4())))
}

fn download_remote_db(connection: &SshConnection, remote_path: &str) -> Result<PathBuf, String> {
    let local_path = temp_db_path()?;
    let mut command = scp_command(connection);
    command.arg(remote_target(connection, remote_path));
    command.arg(&local_path);
    run_scp(command)?;
    Ok(local_path)
}

fn upload_remote_db(
    connection: &SshConnection,
    local_path: &Path,
    remote_path: &str,
) -> Result<(), String> {
    let mut command = scp_command(connection);
    command.arg(local_path);
    command.arg(remote_target(connection, remote_path));
    run_scp(command)
}

fn with_sqlite<T>(
    endpoint: DbEndpoint,
    project_root: Option<String>,
    write: bool,
    action: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    match endpoint {
        DbEndpoint::Local { path } => {
            let path = validate_local_db_path(&path, project_root.as_deref())?;
            action(&path)
        }
        DbEndpoint::Ssh {
            connection,
            path,
            project_path,
        } => {
            let remote_path = validate_remote_db_path(
                &path,
                project_path.as_deref().or(project_root.as_deref()),
            )?;
            let local_path = download_remote_db(&connection, &remote_path)?;
            let result = action(&local_path);
            if result.is_ok() && write {
                if let Err(err) = upload_remote_db(&connection, &local_path, &remote_path) {
                    let _ = fs::remove_file(&local_path);
                    return Err(err);
                }
            }
            let _ = fs::remove_file(&local_path);
            result
        }
    }
}

fn value_ref_to_json(value: ValueRef<'_>) -> Value {
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

fn cell_value_to_sql(value: Option<String>) -> SqlValue {
    match value {
        None => SqlValue::Null,
        Some(value) if value.trim().eq_ignore_ascii_case("NULL") => SqlValue::Null,
        Some(value) => SqlValue::Text(value),
    }
}

fn sql_params(values: &[SqlValue]) -> Vec<&dyn ToSql> {
    values.iter().map(|value| value as &dyn ToSql).collect()
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<DbColumn>, String> {
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

fn primary_keys(columns: &[DbColumn]) -> Vec<String> {
    let mut keys = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| (column.primary_key_ordinal, column.name.clone()))
        .collect::<Vec<_>>();
    keys.sort_by_key(|(ordinal, _)| *ordinal);
    keys.into_iter().map(|(_, name)| name).collect()
}

fn table_has_rowid(conn: &Connection, table: &str) -> bool {
    let sql = format!(
        "SELECT rowid FROM {} LIMIT 1",
        quote_identifier(table).unwrap_or_else(|_| "\"\"".to_string())
    );
    conn.prepare(&sql).is_ok()
}

fn table_indexes(conn: &Connection, table: &str) -> Result<Vec<DbIndex>, String> {
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

fn table_foreign_keys(conn: &Connection, table: &str) -> Result<Vec<DbForeignKey>, String> {
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

fn table_triggers(conn: &Connection, table: &str) -> Result<Vec<DbTrigger>, String> {
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

fn query_rows(
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

fn inspect_schema(path: &Path) -> Result<DbSchema, String> {
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

fn query_table(
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

fn build_row_where_clause(row_key: DbRowKey) -> Result<(String, Vec<SqlValue>), String> {
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

fn ensure_writable(read_only: bool) -> Result<(), String> {
    if read_only {
        Err("Connection is read-only".to_string())
    } else {
        Ok(())
    }
}

fn update_cell(
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

fn insert_row(
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
    let placeholders = std::iter::repeat("?")
        .take(values.len())
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

fn delete_row(path: &Path, table: &str, row_key: DbRowKey, read_only: bool) -> Result<(), String> {
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

fn split_sql_statements(sql: &str) -> Vec<String> {
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

fn first_sql_word(sql: &str) -> String {
    sql.trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_query_statement(sql: &str) -> bool {
    matches!(first_sql_word(sql).as_str(), "select" | "with" | "pragma")
}

fn execute_query_sql(
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

fn execute_sql(
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
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
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
    let last = statements.last().expect("non-empty statements");
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

#[tauri::command]
pub async fn db_load_connections() -> Result<Vec<DbConnectionConfig>, String> {
    tokio::task::spawn_blocking(move || {
        let path = database_connections_path()?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        crate::storage::ensure_private_file_permissions(&path)?;
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_save_connections(connections: Vec<DbConnectionConfig>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let path = database_connections_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(&connections).map_err(|e| e.to_string())?;
        crate::storage::atomic_write_private(&path, &format!("{content}\n"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
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

#[tauri::command]
pub async fn db_inspect(
    endpoint: DbEndpoint,
    project_root: Option<String>,
) -> Result<DbSchema, String> {
    tokio::task::spawn_blocking(move || with_sqlite(endpoint, project_root, false, inspect_schema))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_query_table(
    endpoint: DbEndpoint,
    table: String,
    page: i64,
    page_size: Option<i64>,
    project_root: Option<String>,
) -> Result<DbQueryResult, String> {
    tokio::task::spawn_blocking(move || {
        with_sqlite(endpoint, project_root, false, |path| {
            query_table(path, &table, page, normalize_page_size(page_size))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_update_cell(
    endpoint: DbEndpoint,
    table: String,
    row_key: DbRowKey,
    column: String,
    value: Option<String>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let read_only = authoritative_read_only(
            &endpoint,
            connection_id.as_deref(),
            read_only.unwrap_or(false),
        )?;
        with_sqlite(endpoint, project_root, true, |path| {
            update_cell(path, &table, row_key, &column, value, read_only)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_insert_row(
    endpoint: DbEndpoint,
    table: String,
    values: Vec<DbCellValue>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let read_only = authoritative_read_only(
            &endpoint,
            connection_id.as_deref(),
            read_only.unwrap_or(false),
        )?;
        with_sqlite(endpoint, project_root, true, |path| {
            insert_row(path, &table, values, read_only)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_delete_row(
    endpoint: DbEndpoint,
    table: String,
    row_key: DbRowKey,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let read_only = authoritative_read_only(
            &endpoint,
            connection_id.as_deref(),
            read_only.unwrap_or(false),
        )?;
        with_sqlite(endpoint, project_root, true, |path| {
            delete_row(path, &table, row_key, read_only)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_execute_sql(
    endpoint: DbEndpoint,
    sql: String,
    page: Option<i64>,
    page_size: Option<i64>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<DbExecuteResult, String> {
    let is_write = split_sql_statements(&sql)
        .iter()
        .any(|statement| !is_query_statement(statement));
    tokio::task::spawn_blocking(move || {
        let read_only = authoritative_read_only(
            &endpoint,
            connection_id.as_deref(),
            read_only.unwrap_or(false),
        )?;
        with_sqlite(endpoint, project_root, is_write, |path| {
            execute_sql(path, &sql, page.unwrap_or(1), page_size, read_only)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_sql_identifiers() {
        assert_eq!(quote_identifier("users").unwrap(), "\"users\"");
        assert_eq!(
            quote_identifier("weird\"name").unwrap(),
            "\"weird\"\"name\""
        );
        assert!(quote_identifier("").is_err());
    }

    #[test]
    fn rejects_relative_remote_paths() {
        assert!(validate_remote_db_path("tmp/app.db", None).is_err());
        assert!(validate_remote_db_path("/srv/app/../x.db", None).is_err());
    }

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

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saved_read_only_connection_overrides_renderer_flag() {
        let endpoint = DbEndpoint::Local {
            path: "/tmp/aeroric-read-only.db".to_string(),
        };
        let connections = vec![DbConnectionConfig {
            id: "saved".to_string(),
            name: "saved".to_string(),
            endpoint: endpoint.clone(),
            read_only: true,
            created_at: 1,
            last_opened_at: None,
        }];

        assert!(
            resolve_connection_read_only(&connections, &endpoint, Some("saved"), false).unwrap()
        );
        assert!(resolve_connection_read_only(&connections, &endpoint, None, false).unwrap());
    }

    #[test]
    fn connection_id_cannot_be_reused_with_another_endpoint() {
        let saved_endpoint = DbEndpoint::Local {
            path: "/tmp/aeroric-saved.db".to_string(),
        };
        let requested_endpoint = DbEndpoint::Local {
            path: "/tmp/aeroric-other.db".to_string(),
        };
        let connections = vec![DbConnectionConfig {
            id: "saved".to_string(),
            name: "saved".to_string(),
            endpoint: saved_endpoint,
            read_only: true,
            created_at: 1,
            last_opened_at: None,
        }];

        assert!(resolve_connection_read_only(
            &connections,
            &requested_endpoint,
            Some("saved"),
            false,
        )
        .is_err());
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
