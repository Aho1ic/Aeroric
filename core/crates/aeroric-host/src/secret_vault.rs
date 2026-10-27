//! The host's credentials in one keychain item (2026-10-27).
//!
//! macOS asks again for every keychain item whenever the reading program's
//! signature identity changes, and a build signed without a team id is keyed
//! by its content hash — every rebuild is a "new" program. With one item per
//! credential (dozens: connection keys, SSH passwords, database secrets) and
//! settings re-read on every launch, the App kept asking. Now:
//!
//! - one vault item (`com.aeroric.native` / `secrets`, a JSON map) holds them
//!   all: at most one prompt per new build;
//! - everything is read once per host process and kept in memory, failures
//!   included, so a denied prompt is not shown again and again;
//! - a credential still under the old per-item scheme (`com.aeroric.desktop`)
//!   is read the first time it is asked for and copied into the vault; the old
//!   item is left as it is. A deletion is remembered in the vault so the old
//!   item never brings the value back.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const LEGACY_SERVICE: &str = "com.aeroric.desktop";
pub const VAULT_SERVICE: &str = "com.aeroric.native";
pub const VAULT_ACCOUNT: &str = "secrets";

/// The OS keychain, one generic password per (service, account).
pub trait Keychain: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String>;
}

#[derive(Default, Serialize, Deserialize)]
struct VaultFile {
    #[serde(default)]
    values: BTreeMap<String, String>,
    /// Deleted here: the old per-item entry must not resurrect them.
    #[serde(default)]
    deleted: BTreeSet<String>,
}

#[derive(Default)]
struct State {
    loaded: bool,
    /// The vault could not be read (e.g. access denied): never overwrite it.
    unreadable: Option<String>,
    file: VaultFile,
    /// Old items already looked at this process (`Err` = the read failed).
    legacy: HashMap<String, Result<Option<String>, String>>,
}

pub struct SecretVault<K: Keychain> {
    keychain: K,
    state: Mutex<State>,
}

impl<K: Keychain> SecretVault<K> {
    pub fn new(keychain: K) -> Self {
        Self {
            keychain,
            state: Mutex::new(State::default()),
        }
    }

    fn load(&self, state: &mut State) {
        if state.loaded {
            return;
        }
        state.loaded = true;
        eprintln!("[aeroric-host] keychain: reading the credential vault");
        match self.keychain.get(VAULT_SERVICE, VAULT_ACCOUNT) {
            Ok(Some(text)) => match serde_json::from_str(&text) {
                Ok(file) => state.file = file,
                Err(error) => {
                    state.unreadable = Some(format!("钥匙串里的 Aeroric 凭据库无法解析：{error}"))
                }
            },
            Ok(None) => {}
            Err(error) => {
                eprintln!("[aeroric-host] keychain: the credential vault is unreadable: {error}");
                state.unreadable = Some(error)
            }
        }
    }

    fn save(&self, state: &State) -> Result<(), String> {
        if let Some(error) = &state.unreadable {
            return Err(format!(
                "无法读取钥匙串里的 Aeroric 凭据库，未写入：{error}"
            ));
        }
        let text = serde_json::to_string(&state.file).map_err(|error| error.to_string())?;
        let saved = self.keychain.set(VAULT_SERVICE, VAULT_ACCOUNT, &text);
        if let Err(error) = &saved {
            eprintln!("[aeroric-host] keychain: saving the credential vault failed: {error}");
        }
        saved
    }

    pub fn get(&self, account: &str) -> Result<Option<String>, String> {
        let mut state = self.state.lock();
        self.load(&mut state);
        if let Some(value) = state.file.values.get(account) {
            return Ok(Some(value.clone()));
        }
        if state.file.deleted.contains(account) {
            return Ok(None);
        }
        if let Some(known) = state.legacy.get(account) {
            return known.clone();
        }
        // Names only, never values: which old items still cost a prompt.
        eprintln!("[aeroric-host] keychain: reading old item {account}");
        let found = self
            .keychain
            .get(LEGACY_SERVICE, account)
            .map(|value| value.filter(|value| !value.is_empty()));
        eprintln!(
            "[aeroric-host] keychain: old item {account}: {}",
            match &found {
                Ok(Some(_)) => "found",
                Ok(None) => "absent",
                Err(_) => "refused",
            }
        );
        state.legacy.insert(account.to_string(), found.clone());
        if let Ok(Some(value)) = &found {
            state.file.values.insert(account.to_string(), value.clone());
            // Best effort: the value is already in memory for this process.
            let _ = self.save(&state);
        }
        found
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), String> {
        if value.is_empty() {
            return self.delete(account);
        }
        let mut state = self.state.lock();
        self.load(&mut state);
        state
            .file
            .values
            .insert(account.to_string(), value.to_string());
        state.file.deleted.remove(account);
        self.save(&state)
    }

    pub fn delete(&self, account: &str) -> Result<(), String> {
        let mut state = self.state.lock();
        self.load(&mut state);
        let had = state.file.values.remove(account).is_some();
        let tombstone = state.file.deleted.insert(account.to_string());
        state.legacy.insert(account.to_string(), Ok(None));
        if had || tombstone {
            self.save(&state)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct FakeKeychain {
        items: Arc<Mutex<HashMap<(String, String), String>>>,
        reads: Arc<Mutex<Vec<(String, String)>>>,
        deny: Arc<Mutex<BTreeSet<String>>>,
    }

    impl Keychain for FakeKeychain {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
            self.reads.lock().push((service.into(), account.into()));
            if self.deny.lock().contains(service) {
                return Err("User canceled the operation.".into());
            }
            Ok(self
                .items
                .lock()
                .get(&(service.into(), account.into()))
                .cloned())
        }
        fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
            self.items
                .lock()
                .insert((service.into(), account.into()), value.into());
            Ok(())
        }
    }

    fn legacy(keychain: &FakeKeychain, account: &str, value: &str) {
        keychain
            .items
            .lock()
            .insert((LEGACY_SERVICE.into(), account.into()), value.into());
    }

    #[test]
    fn reads_once_migrates_old_items_and_keeps_them() {
        let keychain = FakeKeychain::default();
        legacy(&keychain, "agent-config:custom:a:api_key", "sk-a");
        let vault = SecretVault::new(keychain.clone());
        for _ in 0..5 {
            assert_eq!(
                vault
                    .get("agent-config:custom:a:api_key")
                    .unwrap()
                    .as_deref(),
                Some("sk-a")
            );
            assert_eq!(vault.get("ssh-password:1").unwrap(), None);
        }
        // One vault read + one read per old item, however often settings load.
        assert_eq!(keychain.reads.lock().len(), 3);
        assert!(keychain.items.lock().contains_key(&(
            LEGACY_SERVICE.into(),
            "agent-config:custom:a:api_key".into()
        )));

        // A new process finds it in the vault without touching the old item.
        let next = SecretVault::new(keychain.clone());
        keychain.reads.lock().clear();
        assert_eq!(
            next.get("agent-config:custom:a:api_key")
                .unwrap()
                .as_deref(),
            Some("sk-a")
        );
        assert_eq!(
            *keychain.reads.lock(),
            [(VAULT_SERVICE.to_string(), VAULT_ACCOUNT.to_string())]
        );
    }

    #[test]
    fn deletion_is_remembered_so_the_old_item_does_not_come_back() {
        let keychain = FakeKeychain::default();
        legacy(&keychain, "ssh-password:9", "pw");
        let vault = SecretVault::new(keychain.clone());
        vault.set("ssh-password:9", "").unwrap();
        assert_eq!(vault.get("ssh-password:9").unwrap(), None);
        let next = SecretVault::new(keychain.clone());
        assert_eq!(next.get("ssh-password:9").unwrap(), None);
        next.set("ssh-password:9", "new").unwrap();
        assert_eq!(
            SecretVault::new(keychain)
                .get("ssh-password:9")
                .unwrap()
                .as_deref(),
            Some("new")
        );
    }

    #[test]
    fn a_denied_prompt_is_not_repeated_and_an_unreadable_vault_is_never_overwritten() {
        let keychain = FakeKeychain::default();
        keychain.deny.lock().insert(VAULT_SERVICE.into());
        keychain.deny.lock().insert(LEGACY_SERVICE.into());
        let vault = SecretVault::new(keychain.clone());
        for _ in 0..3 {
            assert!(vault.get("k").is_err());
        }
        assert_eq!(
            keychain.reads.lock().len(),
            2,
            "one vault try, one old-item try"
        );
        assert!(vault.set("k", "v").is_err());
        assert!(keychain.items.lock().is_empty());
    }
}
