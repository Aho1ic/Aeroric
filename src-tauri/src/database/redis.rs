use base64::Engine as _;
use dbx_core::db::redis_driver::{
    RedisBlob, RedisBlobEncoding, RedisCollectionPage, RedisCommandResult, RedisCommandSafety,
    RedisDatabaseInfo, RedisScanResult, RedisValue, RedisValueData,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AeroricRedisValue {
    key_display: String,
    key_raw: String,
    key_type: String,
    ttl: i64,
    value_is_binary: bool,
    value: Value,
    total: Option<u64>,
    scan_cursor: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AeroricRedisCollectionPage {
    List {
        items: Vec<Value>,
        scan_cursor: Option<u64>,
    },
    Set {
        items: Vec<Value>,
        scan_cursor: Option<u64>,
    },
    Hash {
        items: Vec<Value>,
        scan_cursor: Option<u64>,
    },
    Zset {
        items: Vec<Value>,
        scan_cursor: Option<u64>,
    },
}

fn redis_blob_bytes(blob: &RedisBlob) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(&blob.raw_base64)
        .unwrap_or_default()
}

fn redis_blob_display(blob: &RedisBlob) -> String {
    let bytes = redis_blob_bytes(blob);
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return text.replace('\\', "\\\\");
    }
    let mut output = String::new();
    for byte in bytes {
        match byte {
            b'\\' => output.push_str("\\\\"),
            0x20..=0x7e => output.push(byte as char),
            _ => output.push_str(&format!("\\x{byte:02x}")),
        }
    }
    output
}

fn redis_blob_value(blob: &RedisBlob) -> Value {
    Value::String(redis_blob_display(blob))
}

fn redis_blob_is_binary(blob: &RedisBlob) -> bool {
    matches!(blob.encoding, RedisBlobEncoding::Binary)
}

fn redis_list_items(items: Vec<dbx_core::db::redis_driver::RedisListItem>) -> Vec<Value> {
    items
        .into_iter()
        .map(|item| redis_blob_value(&item.value))
        .collect()
}

fn redis_set_items(items: Vec<dbx_core::db::redis_driver::RedisSetItem>) -> Vec<Value> {
    items
        .into_iter()
        .map(|item| redis_blob_value(&item.member))
        .collect()
}

fn redis_hash_items(items: Vec<dbx_core::db::redis_driver::RedisHashItem>) -> Vec<Value> {
    items
        .into_iter()
        .map(|item| {
            json!({
                "field": redis_blob_display(&item.field),
                "value": redis_blob_display(&item.value),
            })
        })
        .collect()
}

fn redis_zset_items(items: Vec<dbx_core::db::redis_driver::RedisZsetItem>) -> Vec<Value> {
    items
        .into_iter()
        .map(|item| {
            json!({
                "score": item.score,
                "member": redis_blob_display(&item.member),
            })
        })
        .collect()
}

fn adapt_redis_value(value: RedisValue) -> AeroricRedisValue {
    let RedisValue {
        key_display,
        key_raw,
        ttl,
        redis_type,
        data,
    } = value;
    let (value, value_is_binary, total, scan_cursor) = match data {
        RedisValueData::String { content } => (
            redis_blob_value(&content),
            redis_blob_is_binary(&content),
            None,
            None,
        ),
        RedisValueData::Json { value } => (value, false, None, None),
        RedisValueData::List {
            items,
            total,
            scan_cursor,
        } => (
            Value::Array(redis_list_items(items)),
            false,
            Some(total),
            scan_cursor,
        ),
        RedisValueData::Set {
            items,
            total,
            scan_cursor,
        } => (
            Value::Array(redis_set_items(items)),
            false,
            Some(total),
            scan_cursor,
        ),
        RedisValueData::Hash {
            items,
            total,
            scan_cursor,
        } => (
            Value::Array(redis_hash_items(items)),
            false,
            Some(total),
            scan_cursor,
        ),
        RedisValueData::Zset {
            items,
            total,
            scan_cursor,
        } => (
            Value::Array(redis_zset_items(items)),
            false,
            Some(total),
            scan_cursor,
        ),
        RedisValueData::Stream { entries } => (
            Value::Array(
                entries
                    .into_iter()
                    .map(|entry| {
                        let fields = entry
                            .fields
                            .into_iter()
                            .map(|field| (field.field, Value::String(field.value)))
                            .collect::<serde_json::Map<_, _>>();
                        json!({ "id": entry.id, "fields": fields })
                    })
                    .collect(),
            ),
            false,
            None,
            None,
        ),
        RedisValueData::Unknown => (Value::Null, false, None, None),
    };
    AeroricRedisValue {
        key_display,
        key_raw,
        key_type: redis_type,
        ttl,
        value_is_binary,
        value,
        total,
        scan_cursor,
    }
}

fn adapt_redis_collection_page(page: RedisCollectionPage) -> AeroricRedisCollectionPage {
    match page {
        RedisCollectionPage::List { items, scan_cursor } => AeroricRedisCollectionPage::List {
            items: redis_list_items(items),
            scan_cursor,
        },
        RedisCollectionPage::Set { items, scan_cursor } => AeroricRedisCollectionPage::Set {
            items: redis_set_items(items),
            scan_cursor,
        },
        RedisCollectionPage::Hash { items, scan_cursor } => AeroricRedisCollectionPage::Hash {
            items: redis_hash_items(items),
            scan_cursor,
        },
        RedisCollectionPage::Zset { items, scan_cursor } => AeroricRedisCollectionPage::Zset {
            items: redis_zset_items(items),
            scan_cursor,
        },
    }
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
) -> Result<AeroricRedisValue, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::redis_ops::redis_get_value_in_db_core(&state.app_state, &connection_id, db, &key_raw)
        .await
        .map(adapt_redis_value)
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
    filter: Option<String>,
) -> Result<AeroricRedisCollectionPage, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::redis_ops::redis_load_more_in_db_core(
        &state.app_state,
        &connection_id,
        db,
        &key_raw,
        &key_type,
        cursor,
        normalize_count(count),
        filter.as_deref(),
    )
    .await
    .map(adapt_redis_collection_page)
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
    use super::{
        adapt_redis_collection_page, adapt_redis_value, non_empty, normalize_count,
        normalize_key_type, normalize_pattern, AeroricRedisCollectionPage,
    };
    use base64::Engine as _;
    use dbx_core::db::redis_driver::{
        RedisBlob, RedisBlobEncoding, RedisCollectionPage, RedisHashItem, RedisValue,
        RedisValueData,
    };
    use serde_json::json;

    fn blob(bytes: &[u8], encoding: RedisBlobEncoding) -> RedisBlob {
        RedisBlob {
            raw_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            encoding,
        }
    }

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

    #[test]
    fn adapts_new_redis_value_shape_to_the_existing_frontend_contract() {
        let value = adapt_redis_value(RedisValue {
            key_display: "path".to_string(),
            key_raw: "cGF0aA==".to_string(),
            ttl: 60,
            redis_type: "string".to_string(),
            data: RedisValueData::String {
                content: blob(br"C:\tmp", RedisBlobEncoding::Utf8),
            },
        });

        assert_eq!(value.key_type, "string");
        assert_eq!(value.ttl, 60);
        assert!(!value.value_is_binary);
        assert_eq!(value.value, json!(r"C:\\tmp"));
        assert_eq!(value.total, None);
        assert_eq!(value.scan_cursor, None);
    }

    #[test]
    fn marks_binary_string_values_and_escapes_non_utf8_bytes() {
        let value = adapt_redis_value(RedisValue {
            key_display: "binary".to_string(),
            key_raw: "YmluYXJ5".to_string(),
            ttl: -1,
            redis_type: "string".to_string(),
            data: RedisValueData::String {
                content: blob(&[0xff, b'\\'], RedisBlobEncoding::Binary),
            },
        });

        assert!(value.value_is_binary);
        assert_eq!(value.value, json!("\\xff\\\\"));
    }

    #[test]
    fn adapts_typed_hash_pages_without_fabricating_key_metadata() {
        let page = adapt_redis_collection_page(RedisCollectionPage::Hash {
            items: vec![RedisHashItem {
                field: blob(b"role", RedisBlobEncoding::Utf8),
                value: blob(b"admin", RedisBlobEncoding::Utf8),
            }],
            scan_cursor: Some(42),
        });

        match page {
            AeroricRedisCollectionPage::Hash { items, scan_cursor } => {
                assert_eq!(items, vec![json!({ "field": "role", "value": "admin" })]);
                assert_eq!(scan_cursor, Some(42));
            }
            other => panic!("expected hash page, got {other:?}"),
        }
    }
}
