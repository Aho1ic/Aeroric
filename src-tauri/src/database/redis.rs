// ── 切片一百（database）：redis 面 18 条改宿主转发壳。业务主体在 core
// `database::redis_face`（ensure_connected/ensure_writable/适配器/安全闸
// verbatim 上收）；Embedded 本地闭包与宿主服务调同一函数，External 绝不
// 本地回退。

use super::connection_secrets::KeyringDbxConnectionSecretStore;
use super::dbx_state::DbxState;
use aeroric_core::database::dbx_connections::DbxConnectionPaths;
use aeroric_core::database::redis_face;
use dbx_core::db::redis_driver::{RedisCommandResult, RedisDatabaseInfo, RedisScanResult};

fn local_connection_paths() -> Result<DbxConnectionPaths, String> {
    Ok(DbxConnectionPaths::from_data_dir(
        &crate::storage::aeroric_dir()?,
    ))
}

#[tauri::command]
pub async fn dbx_redis_list_databases<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<RedisDatabaseInfo>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_list_databases",
        serde_json::json!({ "connectionId": connection_id }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_list_databases(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_redis_scan_keys<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    cursor: u64,
    pattern: Option<String>,
    count: Option<usize>,
) -> Result<RedisScanResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_scan_keys",
        serde_json::json!({ "connectionId": connection_id, "db": db, "cursor": cursor, "pattern": pattern, "count": count }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_scan_keys(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, cursor, pattern, count)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_redis_get_value<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
) -> Result<redis_face::AeroricRedisValue, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_get_value",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_get_value(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                db,
                key_raw,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_redis_load_more<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    key_type: String,
    cursor: u64,
    count: Option<usize>,
    filter: Option<String>,
) -> Result<redis_face::AeroricRedisCollectionPage, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_load_more",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "keyType": key_type, "cursor": cursor, "count": count, "filter": filter }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_load_more(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, key_type, cursor, count, filter)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_redis_set_value<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_set_value",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "value": value, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_set_value(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, value, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_delete_key<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_delete_key",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_delete_key(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                connection_id,
                db,
                key_raw,
            )
            .await
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_set_ttl<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    ttl: i64,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_set_ttl",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_set_ttl(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_create_key<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    request: redis_face::RedisCreateKeyRequest,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_create_key",
        serde_json::json!({ "request": &request }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_create_key(
                &state,
                &paths,
                &KeyringDbxConnectionSecretStore,
                request,
            )
            .await
            .map(|_| serde_json::Value::Null)
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_hash_del<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    field: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_hash_del",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "field": field }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_hash_del(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, field)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_hash_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    field: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_hash_set",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "field": field, "value": value, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_hash_set(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, field, value, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_list_remove<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    index: i64,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_list_remove",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "index": index }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_list_remove(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, index)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_list_push<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_list_push",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "value": value, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_list_push(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, value, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_list_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    index: i64,
    value: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_list_set",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "index": index, "value": value }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_list_set(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, index, value)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_set_remove<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_set_remove",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "member": member }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_set_remove(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, member)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_set_add<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_set_add",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "member": member, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_set_add(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, member, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_zrem<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_zrem",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "member": member }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_zrem(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, member)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_zadd<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
    score: f64,
    ttl: Option<i64>,
) -> Result<(), String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_zadd",
        serde_json::json!({ "connectionId": connection_id, "db": db, "keyRaw": key_raw, "member": member, "score": score, "ttl": ttl }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_zadd(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, key_raw, member, score, ttl)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dbx_redis_execute_command<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DbxState>,
    connection_id: String,
    db: u32,
    command: String,
    skip_safety_check: Option<bool>,
) -> Result<RedisCommandResult, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "database.dbx_redis_execute_command",
        serde_json::json!({ "connectionId": connection_id, "db": db, "command": command, "skipSafetyCheck": skip_safety_check }),
        async {
            let paths = local_connection_paths()?;
            redis_face::dbx_redis_execute_command(&state, &paths, &KeyringDbxConnectionSecretStore, connection_id, db, command, skip_safety_check)
                .await
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
