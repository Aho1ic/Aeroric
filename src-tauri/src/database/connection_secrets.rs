//! DB connection (`dbx`) password keyring migration.
//!
//! Mirrors the SSH design in `crate::ssh` + `crate::secrets`:
//! - Production backend is OS keyring via `crate::secrets` wrappers
//!   (`dbx-connection-secrets:{connection_id}`, one JSON blob per connection).
//! - Tests inject an in-memory store; the real keyring cannot be round-tripped.
//! - Keyring is preferred over plaintext on disk. migrate-on-read writes the
//!   keyring when disk still holds secrets; the next successful save blanks disk.
//! - `save_password: false` deletes the keyring entry and blanks disk fields.
//! - Keyring write failure keeps plaintext on disk (legacy fallback) so a
//!   restart can still reconnect.
//!
//! `legacy_sqlite.rs` / `database-connections.json` (v1) do not store dbx
//! passwords. Nested `SshConnection.password` is already `skip_serializing` and
//! goes through the SSH keyring path (`crate::ssh::hydrate_ssh_password`).
//!
//! This module must call `crate::secrets` DBX wrappers only — never the raw
//! keyring crate and never the plaintext `get` entry point on `crate::secrets`
//! (see source guards in `secrets.rs`).

use std::collections::BTreeMap;

use serde_json::Value;
use url::{form_urlencoded, Url};

use super::types::AeroricDbConnectionConfig;

/// Keyring backend for per-connection dbx secret blobs.
///
/// Production implementation is OS keyring. No Tauri command may dump these
/// blobs; readers go through `crate::secrets` wrappers only.
pub(crate) trait DbxConnectionSecretStore: Send + Sync {
    /// keyring read of the JSON blob. `Ok(None)` / empty string = unset.
    fn keyring_read(&self, connection_id: &str) -> Result<Option<String>, String>;
    fn keyring_write(&self, connection_id: &str, secrets_json: &str) -> Result<(), String>;
    fn keyring_delete(&self, connection_id: &str) -> Result<(), String>;
}

/// Production backend: OS keyring via `crate::secrets` DBX wrappers.
pub(crate) struct KeyringDbxConnectionSecretStore;

impl DbxConnectionSecretStore for KeyringDbxConnectionSecretStore {
    fn keyring_read(&self, connection_id: &str) -> Result<Option<String>, String> {
        crate::secrets::read_dbx_connection_secrets(connection_id)
    }

    fn keyring_write(&self, connection_id: &str, secrets_json: &str) -> Result<(), String> {
        crate::secrets::store_dbx_connection_secrets(connection_id, secrets_json)
    }

    fn keyring_delete(&self, connection_id: &str) -> Result<(), String> {
        crate::secrets::delete_dbx_connection_secrets(connection_id)
    }
}

/// Same detector as the frontend-sanitize / preserve-merge path.
pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password") || key.contains("passphrase") || key == "client_key"
}

/// URL-parameter names that must never sit in plaintext connection JSON.
pub(crate) fn is_sensitive_url_parameter(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    is_sensitive_key(&key)
        || matches!(
            key.as_str(),
            "passwd"
                | "pwd"
                | "token"
                | "access_token"
                | "refresh_token"
                | "api_key"
                | "apikey"
                | "client_secret"
                | "private_key"
                | "secret"
        )
}

fn is_connection_string_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("connection_string")
}

fn is_url_params_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("url_params")
}

/// Whether an opaque/structured credential string still carries secret material.
fn contains_sensitive_material(key: &str, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    if is_connection_string_key(key) {
        match Url::parse(text) {
            Ok(url) => {
                if url.password().is_some() {
                    return true;
                }
                if url.cannot_be_a_base() {
                    // jdbc:… and other opaque formats often embed credentials.
                    return true;
                }
                url.query()
                    .map(|query| {
                        form_urlencoded::parse(query.as_bytes())
                            .any(|(k, v)| is_sensitive_url_parameter(&k) && !v.is_empty())
                    })
                    .unwrap_or(false)
            }
            // Unparseable connection strings are treated as sensitive (same
            // conservative choice as `redact_connection_string`).
            Err(_) => true,
        }
    } else if is_url_params_key(key) {
        form_urlencoded::parse(text.as_bytes())
            .any(|(k, v)| is_sensitive_url_parameter(&k) && !v.is_empty())
    } else {
        false
    }
}

/// Stable field path for a secret inside `dbx`.
///
/// Transport layers prefer layer `id` (`transport_layers:{id}:password`);
/// fall back to index when the layer has no id. Ids must not contain `:`.
fn path_join(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}:{segment}")
    }
}

fn array_segment(item: &Value, index: usize) -> String {
    item.as_object()
        .and_then(|map| map.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && !id.contains(':'))
        .map(str::to_string)
        .unwrap_or_else(|| index.to_string())
}

/// Walk `dbx` and collect non-empty secret fields as `path → value`.
pub(crate) fn extract_secrets_from_dbx(dbx: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    collect_secrets("", dbx, &mut out);
    out
}

fn collect_secrets(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = path_join(prefix, key);
                if is_sensitive_key(key) {
                    if let Some(text) = child.as_str().filter(|text| !text.is_empty()) {
                        out.insert(path, text.to_string());
                    }
                    continue;
                }
                if is_connection_string_key(key) || is_url_params_key(key) {
                    if let Some(text) = child.as_str() {
                        if contains_sensitive_material(key, text) {
                            out.insert(path, text.to_string());
                        }
                    }
                    continue;
                }
                collect_secrets(&path, child, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_secrets(&path_join(prefix, &array_segment(item, index)), item, out);
            }
        }
        _ => {}
    }
}

fn parse_secrets_blob(blob: &str) -> Result<BTreeMap<String, String>, String> {
    serde_json::from_str(blob).map_err(|error| error.to_string())
}

pub(crate) fn secrets_blob(secrets: &BTreeMap<String, String>) -> Result<String, String> {
    serde_json::to_string(secrets).map_err(|error| error.to_string())
}

/// Mutably resolve a `:`-separated field path inside `dbx`.
fn resolve_path_mut<'a>(root: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let mut current = root;
    for segment in path.split(':').filter(|part| !part.is_empty()) {
        current = match current {
            Value::Object(map) => map.get_mut(segment)?,
            Value::Array(items) => {
                let mut found = None;
                for (index, item) in items.iter_mut().enumerate() {
                    if array_segment(item, index) == segment {
                        found = Some(item);
                        break;
                    }
                }
                found?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// Overwrite secret fields from a keyring blob (keyring preferred on load).
pub(crate) fn apply_secrets_to_dbx(dbx: &mut Value, secrets: &BTreeMap<String, String>) {
    for (path, secret) in secrets {
        if secret.is_empty() {
            continue;
        }
        if let Some(slot) = resolve_path_mut(dbx, path) {
            *slot = Value::String(secret.clone());
        }
    }
}

/// Fill only blank secret fields (reconnect / test-connection backup).
pub(crate) fn fill_blank_secrets_in_dbx(dbx: &mut Value, secrets: &BTreeMap<String, String>) {
    for (path, secret) in secrets {
        if secret.is_empty() {
            continue;
        }
        if let Some(slot) = resolve_path_mut(dbx, path) {
            let blank = slot.as_str().map(str::is_empty).unwrap_or(true);
            if blank {
                *slot = Value::String(secret.clone());
            }
        }
    }
}

/// Blank secret fields on the disk-shaped clone (empty string, same shape the
/// API sanitizer uses for sensitive keys / credential-bearing URL fields).
pub(crate) fn blank_secrets_in_dbx(dbx: &mut Value) {
    for path in extract_secrets_from_dbx(dbx).keys() {
        if let Some(slot) = resolve_path_mut(dbx, path) {
            *slot = Value::String(String::new());
        }
    }
}

/// dbx defaults `save_password` to true; missing key means "keep secrets".
pub(crate) fn save_password_enabled(connection: &AeroricDbConnectionConfig) -> bool {
    connection
        .dbx
        .get("save_password")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Prepare one connection's disk copy: keyring first, blank on success.
///
/// - `save_password=false`: delete keyring, blank disk fields.
/// - Non-empty secrets + keyring write ok: blank disk.
/// - Keyring write fails: keep plaintext on disk (legacy fallback).
pub(crate) fn prepare_connection_for_disk(
    store: &dyn DbxConnectionSecretStore,
    connection: &AeroricDbConnectionConfig,
) -> Result<AeroricDbConnectionConfig, String> {
    let mut next = connection.clone();
    if !save_password_enabled(&next) {
        let _ = store.keyring_delete(&next.id);
        blank_secrets_in_dbx(&mut next.dbx);
        return Ok(next);
    }

    let secrets = extract_secrets_from_dbx(&next.dbx);
    if !secrets.values().any(|value| !value.is_empty()) {
        // Nothing live in memory; still blank any leftover plaintext shape so
        // disk cannot resurrect stale secrets after a sanitize-only save.
        blank_secrets_in_dbx(&mut next.dbx);
        return Ok(next);
    }

    match store.keyring_write(&next.id, &secrets_blob(&secrets)?) {
        Ok(()) => {
            blank_secrets_in_dbx(&mut next.dbx);
        }
        Err(_) => {
            // Keyring unavailable: keep plaintext on disk so restart reconnect
            // still works. migrate-on-read will retry the keyring next load.
        }
    }
    Ok(next)
}

pub(crate) fn prepare_connections_for_disk(
    store: &dyn DbxConnectionSecretStore,
    connections: &[AeroricDbConnectionConfig],
) -> Result<Vec<AeroricDbConnectionConfig>, String> {
    connections
        .iter()
        .map(|connection| prepare_connection_for_disk(store, connection))
        .collect()
}

/// Load-time hydrate + migrate-on-read.
///
/// Keyring blob wins over disk plaintext. Disk-only secrets are written into
/// the keyring immediately so a crash mid-session still migrates; disk is
/// blanked on the next successful save.
pub(crate) fn hydrate_connection_on_load(
    store: &dyn DbxConnectionSecretStore,
    connection: &mut AeroricDbConnectionConfig,
) {
    let disk_secrets = extract_secrets_from_dbx(&connection.dbx);
    let disk_has = disk_secrets.values().any(|value| !value.is_empty());
    let keyring_secrets = store
        .keyring_read(&connection.id)
        .ok()
        .flatten()
        .and_then(|blob| parse_secrets_blob(&blob).ok())
        .filter(|map| map.values().any(|value| !value.is_empty()));

    if let Some(keyring) = keyring_secrets {
        apply_secrets_to_dbx(&mut connection.dbx, &keyring);
        if disk_has {
            let mut combined = disk_secrets;
            for (path, secret) in keyring {
                combined.insert(path, secret);
            }
            if let Ok(blob) = secrets_blob(&combined) {
                let _ = store.keyring_write(&connection.id, &blob);
            }
        }
        return;
    }

    if disk_has {
        if let Ok(blob) = secrets_blob(&disk_secrets) {
            let _ = store.keyring_write(&connection.id, &blob);
        }
        // In-memory copy already carries the disk secrets.
    }
}

pub(crate) fn hydrate_connections_on_load(
    store: &dyn DbxConnectionSecretStore,
    connections: &mut [AeroricDbConnectionConfig],
) {
    for connection in connections.iter_mut() {
        hydrate_connection_on_load(store, connection);
    }
}

/// Fill blank secret fields from keyring (does not overwrite live values).
pub(crate) fn hydrate_blank_secrets(
    store: &dyn DbxConnectionSecretStore,
    connection: &mut AeroricDbConnectionConfig,
) {
    let Ok(Some(blob)) = store.keyring_read(&connection.id) else {
        return;
    };
    let Ok(secrets) = parse_secrets_blob(&blob) else {
        return;
    };
    fill_blank_secrets_in_dbx(&mut connection.dbx, &secrets);
}

pub(crate) fn delete_connection_secrets(store: &dyn DbxConnectionSecretStore, connection_id: &str) {
    let _ = store.keyring_delete(connection_id);
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[derive(Default, Clone)]
    pub(crate) struct MemoryDbxSecretStore {
        entries: Arc<Mutex<BTreeMap<String, String>>>,
        fail_writes: Arc<Mutex<bool>>,
    }

    impl MemoryDbxSecretStore {
        pub(crate) fn with_blob(connection_id: &str, blob: &str) -> Self {
            let store = Self::default();
            store
                .entries
                .lock()
                .insert(connection_id.to_string(), blob.to_string());
            store
        }

        pub(crate) fn fail_writes(&self, fail: bool) {
            *self.fail_writes.lock() = fail;
        }

        pub(crate) fn blob(&self, connection_id: &str) -> Option<String> {
            self.entries.lock().get(connection_id).cloned()
        }
    }

    impl DbxConnectionSecretStore for MemoryDbxSecretStore {
        fn keyring_read(&self, connection_id: &str) -> Result<Option<String>, String> {
            Ok(self
                .entries
                .lock()
                .get(connection_id)
                .cloned()
                .filter(|value| !value.is_empty()))
        }

        fn keyring_write(&self, connection_id: &str, secrets_json: &str) -> Result<(), String> {
            if *self.fail_writes.lock() {
                return Err("keyring unavailable".to_string());
            }
            if secrets_json.is_empty() {
                self.entries.lock().remove(connection_id);
            } else {
                self.entries
                    .lock()
                    .insert(connection_id.to_string(), secrets_json.to_string());
            }
            Ok(())
        }

        fn keyring_delete(&self, connection_id: &str) -> Result<(), String> {
            self.entries.lock().remove(connection_id);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MemoryDbxSecretStore;
    use super::*;
    use crate::database::types::DbxDatabaseType;
    use serde_json::json;

    fn mysql_connection(id: &str, dbx: Value) -> AeroricDbConnectionConfig {
        AeroricDbConnectionConfig {
            id: id.to_string(),
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

    fn rich_dbx() -> Value {
        json!({
            "id": "mysql-1",
            "name": "mysql",
            "db_type": "mysql",
            "host": "127.0.0.1",
            "port": 3306,
            "username": "root",
            "password": "root-secret",
            "redis_sentinel_password": "sentinel-secret",
            "transport_layers": [
                { "id": "transport-a", "type": "ssh", "password": "secret-a", "key_passphrase": "pass-a" },
                { "type": "ssh", "password": "secret-1" }
            ]
        })
    }

    #[test]
    fn extract_uses_stable_paths_and_prefers_layer_ids() {
        let secrets = extract_secrets_from_dbx(&rich_dbx());
        assert_eq!(
            secrets.get("password").map(String::as_str),
            Some("root-secret")
        );
        assert_eq!(
            secrets.get("redis_sentinel_password").map(String::as_str),
            Some("sentinel-secret")
        );
        assert_eq!(
            secrets
                .get("transport_layers:transport-a:password")
                .map(String::as_str),
            Some("secret-a")
        );
        assert_eq!(
            secrets
                .get("transport_layers:transport-a:key_passphrase")
                .map(String::as_str),
            Some("pass-a")
        );
        // Layer without id falls back to index.
        assert_eq!(
            secrets
                .get("transport_layers:1:password")
                .map(String::as_str),
            Some("secret-1")
        );
    }

    #[test]
    fn save_blanks_disk_and_writes_keyring_when_store_succeeds() {
        let store = MemoryDbxSecretStore::default();
        let connection = mysql_connection("mysql-1", rich_dbx());

        let prepared = prepare_connection_for_disk(&store, &connection).expect("prepare");
        let disk = serde_json::to_string(&prepared.dbx).expect("serialize");

        assert!(
            !disk.contains("root-secret"),
            "disk must not keep plaintext: {disk}"
        );
        assert!(!disk.contains("sentinel-secret"));
        assert!(!disk.contains("secret-a"));
        assert_eq!(prepared.dbx["password"], "");
        assert_eq!(prepared.dbx["transport_layers"][0]["password"], "");

        let blob = store.blob("mysql-1").expect("keyring blob written");
        assert!(blob.contains("root-secret"));
        assert!(blob.contains("secret-a"));

        // In-memory source connection is untouched — only the disk clone blanks.
        assert_eq!(connection.dbx["password"], "root-secret");
    }

    #[test]
    fn save_keeps_disk_plaintext_when_keyring_write_fails() {
        let store = MemoryDbxSecretStore::default();
        store.fail_writes(true);
        let connection = mysql_connection("mysql-1", rich_dbx());

        let prepared = prepare_connection_for_disk(&store, &connection).expect("prepare");
        let disk = serde_json::to_string(&prepared.dbx).expect("serialize");
        assert!(
            disk.contains("root-secret"),
            "keyring failure must fall back to plaintext on disk"
        );
        assert!(store.blob("mysql-1").is_none());
    }

    #[test]
    fn load_prefers_keyring_over_legacy_disk_plaintext() {
        let store = MemoryDbxSecretStore::with_blob("mysql-1", r#"{"password":"keyring-secret"}"#);
        // Disk still has older plaintext (migration not yet saved).
        let mut connection = mysql_connection(
            "mysql-1",
            json!({ "id": "mysql-1", "password": "disk-secret" }),
        );

        hydrate_connection_on_load(&store, &mut connection);

        assert_eq!(connection.dbx["password"], "keyring-secret");
    }

    #[test]
    fn load_migrates_disk_plaintext_into_keyring() {
        let store = MemoryDbxSecretStore::default();
        let mut connection = mysql_connection(
            "mysql-1",
            json!({
                "id": "mysql-1",
                "password": "disk-secret",
                "redis_sentinel_password": "sentinel"
            }),
        );

        hydrate_connection_on_load(&store, &mut connection);

        // Memory keeps the secrets so reconnect works this session.
        assert_eq!(connection.dbx["password"], "disk-secret");
        let blob = store
            .blob("mysql-1")
            .expect("migrate-on-read must write keyring");
        assert!(blob.contains("disk-secret"));
        assert!(blob.contains("sentinel"));
    }

    #[test]
    fn save_password_false_clears_keyring_and_blanks_disk() {
        let secrets = extract_secrets_from_dbx(&rich_dbx());
        let store =
            MemoryDbxSecretStore::with_blob("mysql-1", &secrets_blob(&secrets).expect("blob"));
        let mut connection = mysql_connection("mysql-1", rich_dbx());
        connection.dbx["save_password"] = Value::Bool(false);

        let prepared = prepare_connection_for_disk(&store, &connection).expect("prepare");

        assert_eq!(prepared.dbx["password"], "");
        assert_eq!(prepared.dbx["redis_sentinel_password"], "");
        assert_eq!(prepared.dbx["transport_layers"][0]["password"], "");
        assert!(store.blob("mysql-1").is_none());
    }

    #[test]
    fn reconnect_path_fills_blank_password_from_keyring() {
        let store =
            MemoryDbxSecretStore::with_blob("mysql-1", r#"{"password":"reconnect-secret"}"#);
        // Disk-shaped connection: secrets blanked after a successful keyring save.
        let mut connection = mysql_connection(
            "mysql-1",
            json!({
                "id": "mysql-1",
                "name": "mysql",
                "db_type": "mysql",
                "host": "127.0.0.1",
                "port": 3306,
                "username": "root",
                "password": "",
            }),
        );

        hydrate_blank_secrets(&store, &mut connection);
        let config = crate::database::connections::parse_core_config(&connection).expect("parse");
        assert_eq!(config.password, "reconnect-secret");
    }

    #[test]
    fn hydrate_blank_does_not_overwrite_live_values() {
        let store = MemoryDbxSecretStore::with_blob("mysql-1", r#"{"password":"stored"}"#);
        let mut connection = mysql_connection(
            "mysql-1",
            json!({ "id": "mysql-1", "password": "just-typed" }),
        );

        hydrate_blank_secrets(&store, &mut connection);
        assert_eq!(connection.dbx["password"], "just-typed");
    }

    #[test]
    fn secret_blob_helpers_round_trip() {
        let mut secrets = BTreeMap::new();
        secrets.insert("password".to_string(), "pw".to_string());
        let blob = secrets_blob(&secrets).expect("blob");
        let parsed = parse_secrets_blob(&blob).expect("parse");
        assert_eq!(parsed.get("password").map(String::as_str), Some("pw"));
    }

    #[test]
    fn connection_secrets_module_stays_off_raw_keyring_and_secrets_get() {
        // Concat so this file itself never contains the forbidden literals that
        // `secrets::tests::no_tauri_command_calls_secrets_get` scans for.
        let forbidden_keyring_path = format!("{}{}", "keyring", "::");
        let forbidden_plaintext_reader = format!("secrets::{}", "get");
        let source = include_str!("connection_secrets.rs");
        assert!(
            !source.contains(&forbidden_keyring_path),
            "connection_secrets.rs must go through crate::secrets wrappers, not the raw keyring crate"
        );
        assert!(
            !source.contains(&forbidden_plaintext_reader),
            "connection_secrets.rs must not mention the plaintext secrets reader"
        );
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production.contains(&forbidden_plaintext_reader),
            "connection_secrets.rs production path must not call the plaintext secrets reader"
        );
        // Wrappers only.
        assert!(production.contains("read_dbx_connection_secrets"));
        assert!(production.contains("store_dbx_connection_secrets"));
    }
}
