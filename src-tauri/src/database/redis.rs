use dbx_core::db::redis_driver::{RedisDatabaseInfo, RedisScanResult, RedisValue};
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

#[cfg(test)]
mod tests {
    use super::{normalize_count, normalize_pattern};

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
}
