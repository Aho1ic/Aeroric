//! Credentials in `settings.json` live in the OS keychain, not in the file.
//!
//! Both settings writers (`settings::load_save` with the typed `AppSettings`, and
//! `storage::settings` with the raw map the host `settings.*` methods use) pass the
//! JSON object through here at the disk boundary, so the in-memory shape and every
//! caller stay unchanged.
//!
//! On disk a stored secret is replaced by [`KEYCHAIN_MARKER`]. That keeps three
//! states apart:
//! - empty string: the user cleared it, so the keychain entry is deleted;
//! - marker: the value is in the keychain;
//! - marker still present after loading: the keychain read failed. The marker is
//!   left in memory and the next save leaves the keychain alone, so a transient
//!   keychain error can never erase a credential.
//!
//! No store configured (unit tests, embedding) means the file keeps plaintext as
//! before; only the production host binary installs the OS keychain.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde_json::{Map, Value};

use crate::context::{SecretKey, SecretStore};

pub const KEYCHAIN_MARKER: &str = "aeroric-keychain-ref";

static STORE: RwLock<Option<Arc<dyn SecretStore>>> = RwLock::new(None);

/// Installed once by the host binary before settings are first read.
pub fn configure_secret_store(store: Arc<dyn SecretStore>) {
    *STORE.write().expect("secret store lock") = Some(store);
}

pub(crate) fn secret_store() -> Option<Arc<dyn SecretStore>> {
    STORE.read().expect("secret store lock").clone()
}

/// Visit every credential field that exists in the map, with its keychain account.
fn for_each_secret(
    fields: &mut Map<String, Value>,
    f: &mut dyn FnMut(&str, &mut Value) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(Value::Array(agents)) = fields.get_mut("custom_agents") {
        for agent in agents.iter_mut() {
            let Some(object) = agent.as_object_mut() else {
                continue;
            };
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if id.is_empty() {
                continue;
            }
            for name in ["api_key", "password"] {
                if let Some(value) = object.get_mut(name) {
                    f(&format!("agent-config:custom:{id}:{name}"), value)?;
                }
            }
        }
    }
    if let Some(Value::Object(builtins)) = fields.get_mut("builtin_agent_credentials") {
        for (agent, credentials) in builtins.iter_mut() {
            if let Some(value) = credentials
                .as_object_mut()
                .and_then(|o| o.get_mut("api_key"))
            {
                f(&format!("agent-config:builtin:{agent}:api_key"), value)?;
            }
        }
    }
    if let Some(value) = fields
        .get_mut("proxy_settings")
        .and_then(Value::as_object_mut)
        .and_then(|o| o.get_mut("password"))
    {
        f("proxy:password", value)?;
    }
    if let Some(value) = fields
        .get_mut("local_router_settings")
        .and_then(Value::as_object_mut)
        .and_then(|o| o.get_mut("access_token"))
    {
        f("local-router:access_token", value)?;
    }
    Ok(())
}

/// Accounts whose value the file says is in the keychain.
fn marked_accounts(fields: &mut Map<String, Value>) -> BTreeSet<String> {
    let mut accounts = BTreeSet::new();
    let _ = for_each_secret(fields, &mut |account, value| {
        if value.as_str() == Some(KEYCHAIN_MARKER) {
            accounts.insert(account.to_string());
        }
        Ok(())
    });
    accounts
}

/// Marked accounts in the file currently on disk; unreadable or absent means none.
pub fn marked_accounts_on_disk(path: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
            Value::Object(mut fields) => Some(marked_accounts(&mut fields)),
            _ => None,
        })
        .unwrap_or_default()
}

/// True when the map still holds a credential in plaintext (pre-migration file).
pub fn has_plaintext_secrets(fields: &mut Map<String, Value>) -> bool {
    let mut found = false;
    let _ = for_each_secret(fields, &mut |_, value| {
        if matches!(value.as_str(), Some(s) if !s.is_empty() && s != KEYCHAIN_MARKER) {
            found = true;
        }
        Ok(())
    });
    found
}

/// Move credentials out of the map before it is written. `previous` is what the
/// file on disk referenced, so entries for removed agents are deleted too.
/// A keychain write failure aborts the save: writing plaintext instead would
/// silently undo the migration.
pub fn split_for_disk(
    fields: &mut Map<String, Value>,
    previous: &BTreeSet<String>,
    store: &dyn SecretStore,
) -> Result<(), String> {
    let mut kept = BTreeSet::new();
    for_each_secret(fields, &mut |account, value| {
        let key = SecretKey(account.to_string());
        match value.as_str() {
            // Unchanged, or a failed read carried over: leave the keychain as is.
            Some(KEYCHAIN_MARKER) => {
                kept.insert(account.to_string());
            }
            Some("") | None => store.delete(&key)?,
            Some(secret) => {
                store
                    .set(&key, secret)
                    .map_err(|e| format!("keychain write {account}: {e}"))?;
                *value = Value::String(KEYCHAIN_MARKER.to_string());
                kept.insert(account.to_string());
            }
        }
        Ok(())
    })?;
    for orphan in previous.difference(&kept) {
        store.delete(&SecretKey(orphan.clone()))?;
    }
    Ok(())
}

/// Custom agents' credentials stay as markers after reading and are fetched
/// only when that agent needs them ([`resolve_custom_agent_secret`]). Settings
/// are read all the time; the native App keeps launching through Agent 配置, so
/// reading every legacy profile's key on each read only made macOS ask for
/// dozens of keychain items (2026-10-27).
const LAZY_PREFIX: &str = "agent-config:custom:";

/// Put credentials back after reading. A missing or unreadable entry keeps the
/// marker (see module docs) and is reported, never treated as "cleared".
pub fn hydrate_from_store(fields: &mut Map<String, Value>, store: &dyn SecretStore) -> Vec<String> {
    let mut failures = Vec::new();
    let _ = for_each_secret(fields, &mut |account, value| {
        if value.as_str() != Some(KEYCHAIN_MARKER) || account.starts_with(LAZY_PREFIX) {
            return Ok(());
        }
        match store.get(&SecretKey(account.to_string())) {
            Ok(Some(secret)) => *value = Value::String(secret),
            Ok(None) => failures.push(format!("{account}: not in keychain")),
            Err(error) => failures.push(format!("{account}: {error}")),
        }
        Ok(())
    });
    failures
}

/// A custom agent's `api_key` / `password` as stored in settings: the value
/// itself, or — still a keychain marker — the credential read now. `None` when
/// empty or unreadable.
pub fn resolve_custom_agent_secret(agent_id: &str, name: &str, value: &str) -> Option<String> {
    let resolved = if value == KEYCHAIN_MARKER {
        secret_store()?
            .get(&SecretKey(format!("{LAZY_PREFIX}{agent_id}:{name}")))
            .ok()
            .flatten()?
    } else {
        value.to_string()
    };
    Some(resolved).filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MemorySecrets;
    use serde_json::json;

    fn sample() -> Map<String, Value> {
        json!({
            "custom_agents": [
                {"id": "ds", "api_key": "sk-ds", "password": "pw"},
                {"id": "", "api_key": "no-id-is-left-alone"}
            ],
            "builtin_agent_credentials": {"claude": {"api_key": "sk-claude", "base_url": "u"}},
            "proxy_settings": {"url": "http://p", "password": "proxy-pw"},
            "local_router_settings": {"access_token": ""},
            "send_shortcut": "mod_enter"
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn split_moves_every_credential_and_round_trips() {
        let store = MemorySecrets::default();
        let original = sample();
        let mut disk = original.clone();
        split_for_disk(&mut disk, &BTreeSet::new(), &store).unwrap();

        let text = serde_json::to_string(&disk).unwrap();
        for secret in ["sk-ds", "\"pw\"", "sk-claude", "proxy-pw"] {
            assert!(!text.contains(secret), "{secret} leaked to disk: {text}");
        }
        assert!(
            text.contains("no-id-is-left-alone"),
            "unkeyed entry untouched"
        );
        assert!(!has_plaintext_secrets(&mut disk.clone()) || text.contains("no-id"));

        let mut loaded = disk.clone();
        assert!(hydrate_from_store(&mut loaded, &store).is_empty());
        // Custom agents' credentials stay markers until that agent needs them.
        let agent = &loaded["custom_agents"][0];
        assert_eq!(
            (agent["api_key"].as_str(), agent["password"].as_str()),
            (Some(KEYCHAIN_MARKER), Some(KEYCHAIN_MARKER))
        );
        let mut expected = original.clone();
        expected["custom_agents"][0]["api_key"] = json!(KEYCHAIN_MARKER);
        expected["custom_agents"][0]["password"] = json!(KEYCHAIN_MARKER);
        assert_eq!(loaded, expected);
        assert_eq!(
            store
                .get(&SecretKey("agent-config:custom:ds:api_key".into()))
                .unwrap()
                .as_deref(),
            Some("sk-ds"),
            "the key itself is in the store, read on demand"
        );
        // Saving again leaves markers alone: nothing is rewritten or deleted.
        let mut again = loaded.clone();
        split_for_disk(&mut again, &marked_accounts(&mut disk.clone()), &store).unwrap();
        assert_eq!(
            store
                .get(&SecretKey("agent-config:custom:ds:password".into()))
                .unwrap()
                .as_deref(),
            Some("pw")
        );
    }

    #[test]
    fn clearing_a_field_deletes_the_entry() {
        let store = MemorySecrets::default();
        let mut disk = sample();
        split_for_disk(&mut disk, &BTreeSet::new(), &store).unwrap();
        let previous = marked_accounts(&mut disk.clone());

        disk["proxy_settings"]["password"] = json!("");
        split_for_disk(&mut disk, &previous, &store).unwrap();
        assert!(!store.keys().contains(&"proxy:password".to_string()));
    }

    #[test]
    fn removing_an_agent_prunes_its_entries() {
        let store = MemorySecrets::default();
        let mut disk = sample();
        split_for_disk(&mut disk, &BTreeSet::new(), &store).unwrap();
        let previous = marked_accounts(&mut disk.clone());

        disk["custom_agents"] = json!([]);
        split_for_disk(&mut disk, &previous, &store).unwrap();
        let keys = store.keys();
        assert!(
            !keys
                .iter()
                .any(|k| k.starts_with("agent-config:custom:ds:")),
            "{keys:?}"
        );
        assert!(keys.contains(&"agent-config:builtin:claude:api_key".to_string()));
    }

    #[test]
    fn a_failed_read_keeps_the_marker_and_the_next_save_keeps_the_entry() {
        let store = MemorySecrets::default();
        let mut disk = sample();
        split_for_disk(&mut disk, &BTreeSet::new(), &store).unwrap();
        let previous = marked_accounts(&mut disk.clone());

        // Simulate a read that cannot reach the entry.
        let empty = MemorySecrets::default();
        let mut loaded = disk.clone();
        let failures = hydrate_from_store(&mut loaded, &empty);
        assert!(!failures.is_empty());
        assert_eq!(loaded["proxy_settings"]["password"], json!(KEYCHAIN_MARKER));

        // Saving that state must not delete what the real store holds.
        split_for_disk(&mut loaded, &previous, &store).unwrap();
        assert_eq!(
            store
                .get(&SecretKey("proxy:password".into()))
                .unwrap()
                .as_deref(),
            Some("proxy-pw")
        );
    }

    #[test]
    fn plaintext_detection_drives_migration() {
        let store = MemorySecrets::default();
        let mut fields = sample();
        assert!(has_plaintext_secrets(&mut fields));
        fields["custom_agents"] = json!([{"id": "ds", "api_key": "k"}]);
        split_for_disk(&mut fields, &BTreeSet::new(), &store).unwrap();
        assert!(!has_plaintext_secrets(&mut fields));
    }
}
