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

pub(crate) use aeroric_core::database::dbx_secrets::DbxConnectionSecretStore;

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

#[cfg(test)]
mod tests {
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
