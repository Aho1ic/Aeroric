use dbx_core::db::redis_driver::{
    RedisCommandResult, RedisCommandSafety, RedisDatabaseInfo, RedisScanResult, RedisValue,
};
use serde::Deserialize;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

fn normalize_pattern(pattern: Option<String>) -> String {
    pattern
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_else(|| "*".to_string())
}

fn normalize_count(count: Option<usize>) -> usize {
    count.unwrap_or(200).clamp(1, 5000)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedisCreateKeyRequest {
    connection_id: String,
    db: u32,
    key_raw: String,
    key_type: String,
    value: String,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    entry_id: Option<String>,
    #[serde(default)]
    ttl: Option<i64>,
}

fn normalize_key_type(value: &str) -> String {
    value.trim().to_ascii_lowercase()
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

#[tauri::command]
pub async fn dbx_redis_list_databases(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<RedisDatabaseInfo>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::redis_ops::redis_list_databases_core(&state.app_state, &connection_id).await
}

#[tauri::command]
pub async fn dbx_redis_scan_keys(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    cursor: u64,
    pattern: Option<String>,
    count: Option<usize>,
) -> Result<RedisScanResult, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let pattern = normalize_pattern(pattern);
    dbx_core::redis_ops::redis_scan_keys_core(
        &state.app_state,
        &connection_id,
        db,
        cursor,
        &pattern,
        normalize_count(count),
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_get_value(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
) -> Result<RedisValue, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::redis_ops::redis_get_value_in_db_core(&state.app_state, &connection_id, db, &key_raw)
        .await
}

#[tauri::command]
pub async fn dbx_redis_load_more(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    key_type: String,
    cursor: u64,
    count: Option<usize>,
) -> Result<RedisValue, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::redis_ops::redis_load_more_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &key_type,
        cursor,
        normalize_count(count),
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_set_value(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "SET").await?;
    dbx_core::redis_ops::redis_set_string_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &value,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_delete_key(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "Delete key").await?;
    dbx_core::redis_ops::redis_delete_key_in_db_core(&state.app_state, &connection_id, db, &key_raw)
        .await
}

#[tauri::command]
pub async fn dbx_redis_set_ttl(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    ttl: i64,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "EXPIRE").await?;
    dbx_core::redis_ops::redis_set_ttl_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_create_key(
    state: State<'_, DbxState>,
    request: RedisCreateKeyRequest,
) -> Result<(), String> {
    connections::ensure_connected(&state, &request.connection_id).await?;
    connections::ensure_writable(&state, &request.connection_id, "Create key").await?;
    let key_type = normalize_key_type(&request.key_type);
    match key_type.as_str() {
        "string" => {
            dbx_core::redis_ops::redis_set_string_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &request.value,
                request.ttl,
            )
            .await
        }
        "hash" => {
            let field = non_empty(request.field).unwrap_or_else(|| "field".to_string());
            dbx_core::redis_ops::redis_hash_set_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &field,
                &request.value,
                request.ttl,
            )
            .await
        }
        "list" => {
            dbx_core::redis_ops::redis_list_push_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &request.value,
                request.ttl,
            )
            .await
        }
        "set" => {
            dbx_core::redis_ops::redis_set_add_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &request.value,
                request.ttl,
            )
            .await
        }
        "zset" => {
            dbx_core::redis_ops::redis_zadd_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &request.value,
                request.score.unwrap_or(0.0),
                request.ttl,
            )
            .await
        }
        "stream" => {
            let field = non_empty(request.field).unwrap_or_else(|| "field".to_string());
            let entry_id = non_empty(request.entry_id).unwrap_or_else(|| "*".to_string());
            dbx_core::redis_ops::redis_stream_add_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &entry_id,
                vec![(field, request.value)],
                request.ttl,
            )
            .await
        }
        "json" => {
            dbx_core::redis_ops::redis_json_set_in_db_core(
                &state.app_state,
                &request.connection_id,
                request.db,
                &request.key_raw,
                &request.value,
                request.ttl,
            )
            .await
        }
        _ => Err(format!("Unsupported Redis key type: {}", request.key_type)),
    }
}

#[tauri::command]
pub async fn dbx_redis_hash_del(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    field: String,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "HDEL").await?;
    dbx_core::redis_ops::redis_hash_del_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &field,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_hash_set(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    field: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "HSET").await?;
    dbx_core::redis_ops::redis_hash_set_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &field,
        &value,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_list_remove(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    index: i64,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "LREM").await?;
    dbx_core::redis_ops::redis_list_remove_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        index,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_list_push(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "RPUSH").await?;
    dbx_core::redis_ops::redis_list_push_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &value,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_list_set(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    index: i64,
    value: String,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "LSET").await?;
    dbx_core::redis_ops::redis_list_set_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        index,
        &value,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_set_remove(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "SREM").await?;
    dbx_core::redis_ops::redis_set_remove_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &member,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_set_add(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "SADD").await?;
    dbx_core::redis_ops::redis_set_add_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &member,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_zrem(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "ZREM").await?;
    dbx_core::redis_ops::redis_zrem_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &member,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_zadd(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    key_raw: String,
    member: String,
    score: f64,
    ttl: Option<i64>,
) -> Result<(), String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "ZADD").await?;
    dbx_core::redis_ops::redis_zadd_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &member,
        score,
        ttl,
    )
    .await
}

#[tauri::command]
pub async fn dbx_redis_execute_command(
    state: State<'_, DbxState>,
    connection_id: String,
    db: u32,
    command: String,
    skip_safety_check: Option<bool>,
) -> Result<RedisCommandResult, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let command_name = command.split_whitespace().next().unwrap_or("");
    let safety = dbx_core::db::redis_driver::classify_command(command_name);
    if safety != RedisCommandSafety::Allowed {
        connections::ensure_writable(&state, &connection_id, command_name).await?;
    }
    dbx_core::redis_ops::redis_execute_command_core(
        &state.app_state,
        &connection_id,
        db,
        &command,
        skip_safety_check.unwrap_or(false),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{non_empty, normalize_count, normalize_key_type, normalize_pattern};

    #[test]
    fn redis_scan_defaults_are_bounded() {
        assert_eq!(normalize_pattern(None), "*");
        assert_eq!(
            normalize_pattern(Some("  users:*  ".to_string())),
            "users:*"
        );
        assert_eq!(normalize_count(None), 200);
        assert_eq!(normalize_count(Some(0)), 1);
        assert_eq!(normalize_count(Some(10_000)), 5000);
    }

    #[test]
    fn redis_create_key_type_is_normalized() {
        assert_eq!(normalize_key_type(" String "), "string");
        assert_eq!(normalize_key_type("ZSET"), "zset");
    }

    #[test]
    fn blank_create_key_field_is_ignored() {
        assert_eq!(
            non_empty(Some(" field ".to_string())),
            Some("field".to_string())
        );
        assert_eq!(non_empty(Some("   ".to_string())), None);
        assert_eq!(non_empty(None), None);
    }
}
