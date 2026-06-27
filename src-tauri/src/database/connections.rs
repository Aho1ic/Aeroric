use std::fs;
use std::path::PathBuf;

use dbx_core::connection::metadata_connection_config;
use dbx_core::models::connection::{
    default_connect_timeout_secs, default_idle_timeout_secs, default_keepalive_interval_secs,
    default_query_timeout_secs, default_redis_key_separator, ConnectionConfig, DatabaseType,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use super::dbx_state::DbxState;
use super::types::{AeroricDbConnectionConfig, DbxDatabaseType, ProjectScope};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum LegacyEndpoint {
    Local {
        path: String,
    },
    Ssh {
        connection: LegacySshConnection,
        path: String,
        #[serde(default)]
        project_path: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySshConnection {
    id: String,
    #[serde(default)]
    remote_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyConnection {
    id: String,
    name: String,
    endpoint: LegacyEndpoint,
    #[serde(default)]
    read_only: Option<bool>,
    created_at: i64,
    #[serde(default)]
    last_opened_at: Option<i64>,
}

fn v2_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("database-connections-v2.json"))
}

fn legacy_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("database-connections.json"))
}

fn dbx_type_to_core(db_type: DbxDatabaseType) -> DatabaseType {
    match db_type {
        DbxDatabaseType::Sqlite => DatabaseType::Sqlite,
        DbxDatabaseType::Mysql => DatabaseType::Mysql,
        DbxDatabaseType::Postgres => DatabaseType::Postgres,
        DbxDatabaseType::Duckdb => DatabaseType::DuckDb,
        DbxDatabaseType::Redis => DatabaseType::Redis,
        DbxDatabaseType::Mongodb => DatabaseType::MongoDb,
        DbxDatabaseType::Sqlserver => DatabaseType::SqlServer,
        DbxDatabaseType::Oracle => DatabaseType::Oracle,
        DbxDatabaseType::Clickhouse => DatabaseType::ClickHouse,
    }
}

fn core_type_to_dbx(db_type: DatabaseType) -> Option<DbxDatabaseType> {
    match db_type {
        DatabaseType::Sqlite => Some(DbxDatabaseType::Sqlite),
        DatabaseType::Mysql => Some(DbxDatabaseType::Mysql),
        DatabaseType::Postgres => Some(DbxDatabaseType::Postgres),
        DatabaseType::DuckDb => Some(DbxDatabaseType::Duckdb),
        DatabaseType::Redis => Some(DbxDatabaseType::Redis),
        DatabaseType::MongoDb => Some(DbxDatabaseType::Mongodb),
        DatabaseType::SqlServer => Some(DbxDatabaseType::Sqlserver),
        DatabaseType::Oracle => Some(DbxDatabaseType::Oracle),
        DatabaseType::ClickHouse => Some(DbxDatabaseType::Clickhouse),
        _ => None,
    }
}

fn default_connection_config(
    id: &str,
    name: &str,
    db_type: DbxDatabaseType,
    host: String,
    port: u16,
    database: Option<String>,
    read_only: bool,
) -> ConnectionConfig {
    ConnectionConfig {
        id: id.to_string(),
        name: name.to_string(),
        db_type: dbx_type_to_core(db_type),
        driver_profile: None,
        driver_label: None,
        url_params: None,
        host,
        port,
        username: String::new(),
        password: String::new(),
        database,
        visible_databases: None,
        visible_schemas: None,
        attached_databases: Vec::new(),
        color: None,
        transport_layers: Vec::new(),
        connect_timeout_secs: default_connect_timeout_secs(),
        query_timeout_secs: default_query_timeout_secs(),
        idle_timeout_secs: default_idle_timeout_secs(),
        keepalive_interval_secs: default_keepalive_interval_secs(),
        ssl: false,
        ca_cert_path: String::new(),
        client_cert_path: String::new(),
        client_key_path: String::new(),
        sysdba: false,
        oracle_connection_type: None,
        connection_string: None,
        redis_connection_mode: None,
        redis_sentinel_master: String::new(),
        redis_sentinel_nodes: String::new(),
        redis_sentinel_username: String::new(),
        redis_sentinel_password: String::new(),
        redis_sentinel_tls: false,
        redis_cluster_nodes: String::new(),
        redis_key_separator: default_redis_key_separator(),
        redis_scan_page_size: None,
        etcd_endpoints: String::new(),
        gbase_server: String::new(),
        informix_server: String::new(),
        external_config: None,
        jdbc_driver_class: None,
        jdbc_driver_paths: Vec::new(),
        one_time: false,
        read_only,
    }
}

fn legacy_to_aeroric(connection: LegacyConnection) -> AeroricDbConnectionConfig {
    let read_only = connection.read_only.unwrap_or(false);
    let (host, project_scope) = match connection.endpoint {
        LegacyEndpoint::Local { path } => (
            path,
            Some(ProjectScope {
                kind: "local".to_string(),
                project_root: None,
                remote_project_path: None,
                ssh_connection_id: None,
            }),
        ),
        LegacyEndpoint::Ssh {
            connection,
            path,
            project_path,
        } => {
            let remote_project_path = project_path.or(connection.remote_path);
            (
                path,
                Some(ProjectScope {
                    kind: "ssh".to_string(),
                    project_root: None,
                    remote_project_path,
                    ssh_connection_id: Some(connection.id),
                }),
            )
        }
    };
    let dbx = serde_json::to_value(default_connection_config(
        &connection.id,
        &connection.name,
        DbxDatabaseType::Sqlite,
        host,
        0,
        None,
        read_only,
    ))
    .unwrap_or_else(|_| json!({}));
    AeroricDbConnectionConfig {
        id: connection.id,
        name: connection.name,
        db_type: DbxDatabaseType::Sqlite,
        read_only,
        project_scope,
        dbx,
        created_at: connection.created_at,
        last_opened_at: connection.last_opened_at,
        migrated_from_legacy: Some(true),
        connection_group: None,
        pinned: None,
    }
}

fn redact_sensitive_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let key = key.to_ascii_lowercase();
                if key.contains("password") || key.contains("passphrase") || key == "client_key" {
                    *value = Value::String(String::new());
                } else {
                    redact_sensitive_json(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_json(item);
            }
        }
        _ => {}
    }
}

fn sanitized(connection: &AeroricDbConnectionConfig) -> AeroricDbConnectionConfig {
    let mut next = connection.clone();
    redact_sensitive_json(&mut next.dbx);
    next
}

pub(crate) fn parse_core_config(
    connection: &AeroricDbConnectionConfig,
) -> Result<ConnectionConfig, String> {
    let mut config: ConnectionConfig =
        serde_json::from_value(connection.dbx.clone()).map_err(|e| e.to_string())?;
    config.id = connection.id.clone();
    config.name = connection.name.clone();
    config.db_type = dbx_type_to_core(connection.db_type);
    config.read_only = connection.read_only;
    Ok(config.canonicalized())
}

fn normalize_incoming(
    mut connection: AeroricDbConnectionConfig,
) -> Result<AeroricDbConnectionConfig, String> {
    if connection.id.trim().is_empty() {
        return Err("Connection id is required".to_string());
    }
    if connection.name.trim().is_empty() {
        return Err("Connection name is required".to_string());
    }
    if let Ok(config) = parse_core_config(&connection) {
        if let Some(db_type) = core_type_to_dbx(config.db_type) {
            connection.db_type = db_type;
        }
        connection.dbx = serde_json::to_value(config).map_err(|e| e.to_string())?;
    }
    Ok(connection)
}

async fn load_connections_from_disk() -> Result<Vec<AeroricDbConnectionConfig>, String> {
    let v2_path = v2_connections_path()?;
    if v2_path.exists() {
        let data = fs::read_to_string(v2_path).map_err(|e| e.to_string())?;
        return serde_json::from_str(&data).map_err(|e| e.to_string());
    }

    let legacy_path = legacy_connections_path()?;
    if !legacy_path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(legacy_path).map_err(|e| e.to_string())?;
    let legacy: Vec<LegacyConnection> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    let migrated: Vec<_> = legacy.into_iter().map(legacy_to_aeroric).collect();
    save_connections_to_disk(&migrated).await?;
    Ok(migrated)
}

async fn save_connections_to_disk(connections: &[AeroricDbConnectionConfig]) -> Result<(), String> {
    crate::storage::ensure_aeroric_dirs()?;
    let data = connections_disk_json(connections)?;
    fs::write(v2_connections_path()?, format!("{data}\n")).map_err(|e| e.to_string())
}

fn connections_disk_json(connections: &[AeroricDbConnectionConfig]) -> Result<String, String> {
    serde_json::to_string_pretty(connections).map_err(|e| e.to_string())
}

pub(crate) async fn ensure_loaded(state: &DbxState) -> Result<(), String> {
    if *state.loaded_connections.read().await {
        return Ok(());
    }
    let loaded = load_connections_from_disk().await?;
    let mut map = state.connections.write().await;
    if !*state.loaded_connections.read().await {
        map.clear();
        for connection in loaded {
            map.insert(connection.id.clone(), connection);
        }
        *state.loaded_connections.write().await = true;
    }
    Ok(())
}

pub(crate) async fn ensure_connected(state: &DbxState, connection_id: &str) -> Result<(), String> {
    ensure_loaded(state).await?;
    let connection = state
        .connections
        .read()
        .await
        .get(connection_id)
        .cloned()
        .ok_or_else(|| "Connection not found".to_string())?;
    let config = parse_core_config(&connection)?;
    let metadata_config = metadata_connection_config(&config);
    state
        .app_state
        .configs
        .write()
        .await
        .insert(connection_id.to_string(), metadata_config);
    state
        .app_state
        .get_or_create_pool(connection_id, None)
        .await
        .map(|_| ())
}

pub(crate) async fn ensure_writable(
    state: &DbxState,
    connection_id: &str,
    action: &str,
) -> Result<(), String> {
    if let Some(name) =
        dbx_core::query::connection_readonly_name(&state.app_state, connection_id).await
    {
        return Err(format!(
            "Read-only mode: connection '{}' has read-only protection enabled. {} blocked.",
            name, action
        ));
    }
    Ok(())
}

async fn persist_state_connections(
    state: &DbxState,
) -> Result<Vec<AeroricDbConnectionConfig>, String> {
    let mut connections: Vec<_> = state.connections.read().await.values().cloned().collect();
    connections.sort_by(|a, b| {
        b.last_opened_at
            .cmp(&a.last_opened_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    save_connections_to_disk(&connections).await?;
    Ok(connections
        .into_iter()
        .map(|connection| sanitized(&connection))
        .collect())
}

#[tauri::command]
pub async fn dbx_list_connections(
    state: State<'_, DbxState>,
) -> Result<Vec<AeroricDbConnectionConfig>, String> {
    ensure_loaded(&state).await?;
    let mut connections: Vec<_> = state.connections.read().await.values().cloned().collect();
    connections.sort_by(|a, b| {
        b.last_opened_at
            .cmp(&a.last_opened_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    Ok(connections
        .into_iter()
        .map(|connection| sanitized(&connection))
        .collect())
}

#[tauri::command]
pub async fn dbx_save_connection(
    state: State<'_, DbxState>,
    connection: AeroricDbConnectionConfig,
) -> Result<(), String> {
    ensure_loaded(&state).await?;
    let connection = normalize_incoming(connection)?;
    let connection = preserve_existing_secrets(&state, connection).await;
    let core_config = parse_core_config(&connection)?;
    state
        .app_state
        .configs
        .write()
        .await
        .insert(connection.id.clone(), core_config);
    state
        .connections
        .write()
        .await
        .insert(connection.id.clone(), connection);
    persist_state_connections(&state).await?;
    Ok(())
}

async fn preserve_existing_secrets(
    state: &DbxState,
    incoming: AeroricDbConnectionConfig,
) -> AeroricDbConnectionConfig {
    let map = state.connections.read().await;
    let existing = match map.get(&incoming.id) {
        Some(e) => e.clone(),
        None => return incoming,
    };
    drop(map);

    preserve_existing_secrets_from_existing(incoming, &existing)
}

fn preserve_existing_secrets_from_existing(
    mut incoming: AeroricDbConnectionConfig,
    existing: &AeroricDbConnectionConfig,
) -> AeroricDbConnectionConfig {
    merge_sensitive_json(&mut incoming.dbx, &existing.dbx);
    incoming
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password") || key.contains("passphrase") || key == "client_key"
}

fn merge_sensitive_json(incoming: &mut Value, existing: &Value) {
    match (incoming, existing) {
        (Value::Object(incoming_map), Value::Object(existing_map)) => {
            for (key, existing_value) in existing_map {
                if is_sensitive_key(key) {
                    let incoming_empty = incoming_map
                        .get(key)
                        .and_then(|value| value.as_str())
                        .map(|value| value.is_empty())
                        .unwrap_or(true);
                    if incoming_empty {
                        if let Some(existing_text) = existing_value.as_str() {
                            if !existing_text.is_empty() {
                                incoming_map
                                    .insert(key.clone(), Value::String(existing_text.to_string()));
                            }
                        }
                    }
                    continue;
                }
                if let Some(incoming_value) = incoming_map.get_mut(key) {
                    merge_sensitive_json(incoming_value, existing_value);
                }
            }
        }
        (Value::Array(incoming_items), Value::Array(existing_items)) => {
            for (incoming_item, existing_item) in incoming_items.iter_mut().zip(existing_items) {
                merge_sensitive_json(incoming_item, existing_item);
            }
        }
        _ => {}
    }
}

#[tauri::command]
pub async fn dbx_delete_connection(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<(), String> {
    ensure_loaded(&state).await?;
    state
        .app_state
        .remove_connection_pools(&connection_id)
        .await;
    state
        .app_state
        .reset_connection_transport(&connection_id)
        .await;
    state.app_state.configs.write().await.remove(&connection_id);
    state.connections.write().await.remove(&connection_id);
    persist_state_connections(&state).await?;
    Ok(())
}

#[tauri::command]
pub async fn dbx_test_connection(
    state: State<'_, DbxState>,
    connection: AeroricDbConnectionConfig,
) -> Result<(), String> {
    let connection = normalize_incoming(connection)?;
    let config = parse_core_config(&connection)?;
    let id = format!("{}:test", config.id);
    let mut test_config = config.clone();
    test_config.id = id.clone();
    state
        .app_state
        .configs
        .write()
        .await
        .insert(id.clone(), test_config);
    let result = state
        .app_state
        .get_or_create_pool(&id, None)
        .await
        .map(|_| ());
    state.app_state.remove_connection_pools(&id).await;
    state.app_state.reset_connection_transport(&id).await;
    state.app_state.configs.write().await.remove(&id);
    result
}

#[tauri::command]
pub async fn dbx_connect(state: State<'_, DbxState>, connection_id: String) -> Result<(), String> {
    ensure_connected(&state, &connection_id).await
}

#[tauri::command]
pub async fn dbx_disconnect(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<(), String> {
    state
        .app_state
        .remove_connection_pools(&connection_id)
        .await;
    state
        .app_state
        .reset_connection_transport(&connection_id)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn dbx_backup_sqlite_database(
    state: State<'_, DbxState>,
    connection_id: String,
    destination_path: String,
) -> Result<(), String> {
    ensure_loaded(&state).await?;
    let connection = state
        .connections
        .read()
        .await
        .get(&connection_id)
        .cloned()
        .ok_or_else(|| "Connection not found".to_string())?;
    let config = parse_core_config(&connection)?;
    if config.db_type != DatabaseType::Sqlite {
        return Err("Only SQLite connections can be backed up".to_string());
    }
    let source_path = config.host.trim().to_string();
    if source_path.is_empty() || source_path.eq_ignore_ascii_case(":memory:") {
        return Err("SQLite backup requires a file-backed database".to_string());
    }
    let destination_path = destination_path.trim().to_string();
    if destination_path.is_empty() {
        return Err("Backup destination path is required".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        if !source.exists() {
            return Err("SQLite database file not found".to_string());
        }
        let destination = PathBuf::from(&destination_path);
        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let source_connection = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        source_connection
            .backup(rusqlite::DatabaseName::Main, &destination, None)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mysql_connection_with_dbx(dbx: Value) -> AeroricDbConnectionConfig {
        AeroricDbConnectionConfig {
            id: "mysql-1".to_string(),
            name: "mysql".to_string(),
            db_type: DbxDatabaseType::Mysql,
            read_only: false,
            project_scope: None,
            dbx,
            created_at: 1,
            last_opened_at: None,
            migrated_from_legacy: None,
            connection_group: None,
            pinned: None,
        }
    }

    #[test]
    fn legacy_local_connection_migrates_to_sqlite_dbx_config() {
        let legacy = LegacyConnection {
            id: "one".to_string(),
            name: "local".to_string(),
            endpoint: LegacyEndpoint::Local {
                path: "/tmp/a.db".to_string(),
            },
            read_only: Some(true),
            created_at: 1,
            last_opened_at: Some(2),
        };

        let migrated = legacy_to_aeroric(legacy);
        let core = parse_core_config(&migrated).unwrap();

        assert_eq!(migrated.db_type, DbxDatabaseType::Sqlite);
        assert!(migrated.migrated_from_legacy.unwrap());
        assert_eq!(core.db_type, DatabaseType::Sqlite);
        assert_eq!(core.host, "/tmp/a.db");
        assert!(core.read_only);
    }

    #[test]
    fn sanitized_connection_removes_password_fields() {
        let connection = AeroricDbConnectionConfig {
            id: "secret".to_string(),
            name: "secret".to_string(),
            db_type: DbxDatabaseType::Mysql,
            read_only: false,
            project_scope: None,
            dbx: json!({
                "id": "secret",
                "name": "secret",
                "db_type": "mysql",
                "host": "127.0.0.1",
                "port": 3306,
                "username": "root",
                "password": "pw",
                "database": null,
                "transport_layers": [{ "type": "ssh", "password": "ssh", "key_passphrase": "key" }]
            }),
            created_at: 1,
            last_opened_at: None,
            migrated_from_legacy: None,
            connection_group: None,
            pinned: None,
        };

        let sanitized = sanitized(&connection);

        assert_eq!(sanitized.dbx["password"], "");
        assert_eq!(sanitized.dbx["transport_layers"][0]["password"], "");
        assert_eq!(sanitized.dbx["transport_layers"][0]["key_passphrase"], "");
    }

    #[test]
    fn disk_connection_json_keeps_password_for_restart_reconnect() {
        let connection = mysql_connection_with_dbx(json!({
            "id": "mysql-1",
            "name": "mysql",
            "db_type": "mysql",
            "host": "127.0.0.1",
            "port": 3306,
            "username": "root",
            "password": "root-secret",
            "database": null
        }));

        let api_connection = sanitized(&connection);
        let disk_json = connections_disk_json(&[connection]).unwrap();

        assert_eq!(api_connection.dbx["password"], "");
        assert!(disk_json.contains("\"password\": \"root-secret\""));
    }

    #[test]
    fn sanitized_edit_preserves_existing_sensitive_values() {
        let existing = mysql_connection_with_dbx(json!({
            "id": "mysql-1",
            "name": "mysql",
            "db_type": "mysql",
            "host": "127.0.0.1",
            "port": 3306,
            "username": "root",
            "password": "root-secret",
            "redis_sentinel_password": "sentinel-secret",
            "transport_layers": [{
                "id": "transport-1",
                "type": "ssh",
                "password": "ssh-secret",
                "key_passphrase": "key-secret"
            }]
        }));
        let incoming = mysql_connection_with_dbx(json!({
            "id": "mysql-1",
            "name": "mysql",
            "db_type": "mysql",
            "host": "127.0.0.1",
            "port": 3306,
            "username": "root",
            "password": "",
            "redis_sentinel_password": "",
            "transport_layers": [{
                "id": "transport-1",
                "type": "ssh",
                "password": "",
                "key_passphrase": ""
            }]
        }));

        let preserved = preserve_existing_secrets_from_existing(incoming, &existing);

        assert_eq!(preserved.dbx["password"], "root-secret");
        assert_eq!(preserved.dbx["redis_sentinel_password"], "sentinel-secret");
        assert_eq!(
            preserved.dbx["transport_layers"][0]["password"],
            "ssh-secret"
        );
        assert_eq!(
            preserved.dbx["transport_layers"][0]["key_passphrase"],
            "key-secret"
        );
    }
}
