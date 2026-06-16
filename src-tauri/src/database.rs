use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::types::ValueRef;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use uuid::Uuid;

use crate::ssh::SshConnection;

const MAX_PAGE_SIZE: i64 = 500;
const DEFAULT_PAGE_SIZE: i64 = 100;

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
    created_at: i64,
    last_opened_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbColumn {
    name: String,
    data_type: String,
    not_null: bool,
    primary_key: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbObject {
    name: String,
    object_type: String,
    columns: Vec<DbColumn>,
    row_count: Option<i64>,
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
    values: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DbQueryResult {
    columns: Vec<String>,
    rows: Vec<DbRow>,
    page: i64,
    page_size: i64,
    total_rows: Option<i64>,
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

fn query_rows(
    conn: &Connection,
    sql: &str,
    page: i64,
    page_size: i64,
    rowid_column: bool,
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
        result_rows.push(DbRow { row_id, values });
    }
    Ok(DbQueryResult {
        columns,
        rows: result_rows,
        page,
        page_size,
        total_rows: None,
    })
}

fn inspect_schema(path: &Path) -> Result<DbSchema, String> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )
        .map_err(|e| e.to_string())?;
    let objects = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for (name, object_type) in objects {
        let pragma_sql = format!("PRAGMA table_info({})", quote_identifier(&name)?);
        let mut column_stmt = conn.prepare(&pragma_sql).map_err(|e| e.to_string())?;
        let columns = column_stmt
            .query_map([], |row| {
                Ok(DbColumn {
                    name: row.get(1)?,
                    data_type: row.get::<_, String>(2)?,
                    not_null: row.get::<_, i64>(3)? != 0,
                    primary_key: row.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let row_count = if object_type == "table" {
            let count_sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(&name)?);
            conn.query_row(&count_sql, [], |row| row.get::<_, i64>(0))
                .ok()
        } else {
            None
        };
        result.push(DbObject {
            name,
            object_type,
            columns,
            row_count,
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
    let page = page.max(1);
    let page_size = normalize_page_size(Some(page_size));
    let total_rows = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table_ident}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .ok();
    let sql = format!("SELECT rowid AS __aeroric_rowid__, * FROM {table_ident} LIMIT ? OFFSET ?");
    match query_rows(&conn, &sql, page, page_size, true) {
        Ok(mut result) => {
            result.total_rows = total_rows;
            Ok(result)
        }
        Err(_) => {
            let sql = format!("SELECT * FROM {table_ident} LIMIT ? OFFSET ?");
            let mut result = query_rows(&conn, &sql, page, page_size, false)?;
            result.total_rows = total_rows;
            Ok(result)
        }
    }
}

fn update_cell(
    path: &Path,
    table: &str,
    row_id: i64,
    column: &str,
    value: Option<String>,
) -> Result<(), String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let sql = format!(
        "UPDATE {} SET {} = ?1 WHERE rowid = ?2",
        quote_identifier(table)?,
        quote_identifier(column)?
    );
    conn.execute(&sql, params![value, row_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn execute_sql(path: &Path, sql: &str, page_size: Option<i64>) -> Result<DbExecuteResult, String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Err("SQL cannot be empty".to_string());
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let page_size = normalize_page_size(page_size);
    let first = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(first.as_str(), "select" | "with") {
        let limited_sql = format!("SELECT * FROM ({trimmed}) AS aeroric_query LIMIT ? OFFSET ?");
        let result = query_rows(&conn, &limited_sql, 1, page_size, false)?;
        return Ok(DbExecuteResult {
            columns: result.columns,
            rows: result.rows,
            rows_affected: 0,
            message: "Query completed".to_string(),
        });
    }
    if first == "pragma" {
        let limited_sql = format!("{trimmed} LIMIT ? OFFSET ?");
        let result = query_rows(&conn, &limited_sql, 1, page_size, false)?;
        return Ok(DbExecuteResult {
            columns: result.columns,
            rows: result.rows,
            rows_affected: 0,
            message: "Query completed".to_string(),
        });
    }
    let rows_affected = conn.execute(trimmed, []).map_err(|e| e.to_string())?;
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
        fs::write(path, format!("{content}\n")).map_err(|e| e.to_string())
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
    row_id: i64,
    column: String,
    value: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        with_sqlite(endpoint, project_root, true, |path| {
            update_cell(path, &table, row_id, &column, value)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_execute_sql(
    endpoint: DbEndpoint,
    sql: String,
    page_size: Option<i64>,
    project_root: Option<String>,
) -> Result<DbExecuteResult, String> {
    let is_write = !matches!(
        sql.trim()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "select" | "with" | "pragma"
    );
    tokio::task::spawn_blocking(move || {
        with_sqlite(endpoint, project_root, is_write, |path| {
            execute_sql(path, &sql, page_size)
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
        update_cell(&path, "notes", row_id, "body", Some("done".to_string())).unwrap();
        let second = query_table(&path, "notes", 1, 100).unwrap();
        assert_eq!(second.rows[0].values[1], Value::String("done".to_string()));

        let _ = fs::remove_file(path);
    }
}
