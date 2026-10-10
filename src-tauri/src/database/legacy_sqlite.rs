use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::path_guard::remote_path_has_relative_components;
use crate::ssh::SshConnection;

// 切片九十四刀零：纯业务主体（DTO＋六个 rusqlite 函数＋SQL 卫士）上收
// core `database::legacy_sqlite`，这里 glob re-export 保持命令体逐字不改。
use aeroric_core::database::legacy_sqlite::*;

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

/// Legacy (v1) connection file path.
///
/// This file stores `DbConnectionConfig` endpoints only — no dbx passwords.
/// Nested `SshConnection.password` is `skip_serializing` and lives in the SSH
/// keyring (`crate::ssh`); reconnect/scp paths must call
/// `crate::ssh::hydrate_ssh_password` before use. Modern dbx secrets are
/// handled by `database::connection_secrets` against
/// `database-connections-v2.json`.
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
    command.arg("-o").arg("StrictHostKeyChecking=yes");
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

fn remote_file_fingerprint(
    connection: &SshConnection,
    remote_path: &str,
) -> Result<Option<String>, String> {
    let quoted = crate::ssh::shell_quote_posix(remote_path);
    let command_text = format!(
        "if [ -f {quoted} ]; then if stat -c '%s:%Y' {quoted} 2>/dev/null; then :; elif stat -f '%z:%m' {quoted} 2>/dev/null; then :; else exit 2; fi; else exit 3; fi"
    );
    let mut command = crate::ssh::std_ssh_command_for_remote_command(connection, command_text);
    crate::subprocess::configure_background_command(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        let fingerprint = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if fingerprint.is_empty() {
            return Err("Remote database metadata is empty".to_string());
        }
        return Ok(Some(fingerprint));
    }
    if output.status.code() == Some(3) {
        return Ok(None);
    }
    Err(crate::ssh::annotate_ssh_error(
        connection,
        String::from_utf8_lossy(&output.stderr).trim(),
    ))
}

fn ensure_remote_sqlite_snapshot_is_safe(
    connection: &SshConnection,
    remote_path: &str,
    initial_fingerprint: &str,
) -> Result<(), String> {
    for suffix in ["-wal", "-shm"] {
        if remote_file_fingerprint(connection, &format!("{remote_path}{suffix}"))?.is_some() {
            return Err(
                "Remote SQLite is using WAL sidecar files; refusing an unsafe stale snapshot"
                    .to_string(),
            );
        }
    }
    let current = remote_file_fingerprint(connection, remote_path)?
        .ok_or_else(|| "Remote SQLite database disappeared during the operation".to_string())?;
    if current != initial_fingerprint {
        return Err(
            "Remote SQLite database changed during the operation; no local snapshot was uploaded"
                .to_string(),
        );
    }
    Ok(())
}

fn temp_db_path() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("aeroric-db");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tighten_temp_directory_permissions(&dir)?;
    Ok(dir.join(format!("{}.db", Uuid::new_v4())))
}

fn tighten_temp_directory_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn download_remote_db(
    connection: &SshConnection,
    remote_path: &str,
) -> Result<(PathBuf, String), String> {
    let fingerprint = remote_file_fingerprint(connection, remote_path)?
        .ok_or_else(|| "Remote SQLite database does not exist".to_string())?;
    ensure_remote_sqlite_snapshot_is_safe(connection, remote_path, &fingerprint)?;
    let local_path = temp_db_path()?;
    let mut command = scp_command(connection);
    command.arg(remote_target(connection, remote_path));
    command.arg(&local_path);
    run_scp(command)?;
    crate::storage::ensure_private_file_permissions(&local_path)?;
    Ok((local_path, fingerprint))
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
            let (local_path, fingerprint) = download_remote_db(&connection, &remote_path)?;
            let result = action(&local_path);
            if result.is_ok() {
                if let Err(err) =
                    ensure_remote_sqlite_snapshot_is_safe(&connection, &remote_path, &fingerprint)
                {
                    let _ = fs::remove_file(&local_path);
                    return Err(err);
                }
                if write {
                    if let Err(err) = upload_remote_db(&connection, &local_path, &remote_path) {
                        let _ = fs::remove_file(&local_path);
                        return Err(err);
                    }
                }
            }
            let _ = fs::remove_file(&local_path);
            result
        }
    }
}

pub async fn local_db_load_connections() -> Result<Vec<DbConnectionConfig>, String> {
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
pub async fn db_load_connections<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<DbConnectionConfig>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.db_load_connections",
        serde_json::json!({}),
        async {
            local_db_load_connections()
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Persist legacy connection list. `SshConnection.password` never serializes
/// (`skip_serializing`); SSH secrets stay in the SSH keyring. Do not add
/// plaintext credential fields here — route new secrets through
/// `database::connection_secrets` instead.
pub async fn local_db_save_connections(connections: Vec<DbConnectionConfig>) -> Result<(), String> {
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
pub async fn db_save_connections<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    connections: Vec<DbConnectionConfig>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.db_save_connections",
        serde_json::json!({ "connections": connections }),
        async {
            local_db_save_connections(connections)
                .await
                .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
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

pub async fn local_db_inspect(
    endpoint: DbEndpoint,
    project_root: Option<String>,
) -> Result<DbSchema, String> {
    tokio::task::spawn_blocking(move || with_sqlite(endpoint, project_root, false, inspect_schema))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn db_inspect<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    project_root: Option<String>,
) -> Result<DbSchema, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.db_inspect",
        serde_json::json!({ "endpoint": endpoint, "projectRoot": project_root }),
        async {
            local_db_inspect(endpoint, project_root)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

pub async fn local_db_query_table(
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
pub async fn db_query_table<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    table: String,
    page: i64,
    page_size: Option<i64>,
    project_root: Option<String>,
) -> Result<DbQueryResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.db_query_table",
        serde_json::json!({ "endpoint": endpoint, "table": table, "page": page, "pageSize": page_size, "projectRoot": project_root }),
        async { local_db_query_table(endpoint, table, page, page_size, project_root).await.and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())) },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

pub async fn local_db_update_cell(
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
pub async fn db_update_cell<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    table: String,
    row_key: DbRowKey,
    column: String,
    value: Option<String>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.db_update_cell",
        serde_json::json!({ "endpoint": endpoint, "table": table, "rowKey": row_key, "column": column, "value": value, "readOnly": read_only, "connectionId": connection_id, "projectRoot": project_root }),
        async { local_db_update_cell(endpoint, table, row_key, column, value, read_only, connection_id, project_root).await.map(|_| serde_json::Value::Null) },
    )
    .await
    .map(|_| ())
}

pub async fn local_db_insert_row(
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
pub async fn db_insert_row<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    table: String,
    values: Vec<DbCellValue>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.db_insert_row",
        serde_json::json!({ "endpoint": endpoint, "table": table, "values": values, "readOnly": read_only, "connectionId": connection_id, "projectRoot": project_root }),
        async { local_db_insert_row(endpoint, table, values, read_only, connection_id, project_root).await.map(|_| serde_json::Value::Null) },
    )
    .await
    .map(|_| ())
}

pub async fn local_db_delete_row(
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
pub async fn db_delete_row<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    table: String,
    row_key: DbRowKey,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.db_delete_row",
        serde_json::json!({ "endpoint": endpoint, "table": table, "rowKey": row_key, "readOnly": read_only, "connectionId": connection_id, "projectRoot": project_root }),
        async { local_db_delete_row(endpoint, table, row_key, read_only, connection_id, project_root).await.map(|_| serde_json::Value::Null) },
    )
    .await
    .map(|_| ())
}

pub async fn local_db_execute_sql(
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

#[tauri::command]
pub async fn db_execute_sql<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    endpoint: DbEndpoint,
    sql: String,
    page: Option<i64>,
    page_size: Option<i64>,
    read_only: Option<bool>,
    connection_id: Option<String>,
    project_root: Option<String>,
) -> Result<DbExecuteResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.db_execute_sql",
        serde_json::json!({ "endpoint": endpoint, "sql": sql, "page": page, "pageSize": page_size, "readOnly": read_only, "connectionId": connection_id, "projectRoot": project_root }),
        async { local_db_execute_sql(endpoint, sql, page, page_size, read_only, connection_id, project_root).await.and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())) },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_core::database::legacy_sqlite::quote_identifier;

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
}
