//! The injected environment every service runs against (Task P2-02 Action 2).
//!
//! Nothing in here reads an environment variable or a home directory by itself: the
//! caller supplies paths, the clock, and the external providers, so a test never touches
//! a real `~/.aeroric` and no service can quietly depend on process state. The
//! providers are the seams where the shipping client's keychain, hook installer and
//! settings cache plug in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::clock::{Clock, SystemClock};
use crate::location::StorageLocationProbe;
use crate::paths::AppPaths;
use crate::storage_lock::{DataDirectoryLease, LeaseError, LeaseOwner};

/// Where a secret lives, by the naming the shipping client already uses
/// (`ssh-password:{id}`, `dbx-connection-secrets:{id}`, `notebook:embedding:key`).
/// The core keeps that vocabulary: two writers on one directory must not disagree about
/// which entry holds which credential.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecretKey(pub String);

/// The platform credential store. Implemented by the keychain in the adapters; the
/// in-memory fake here is what tests use, and it is *not* a fallback the core will ever
/// silently choose — a missing implementation is an error, not an empty store.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, String>;
    /// Empty string deletes, which is the legacy semantics the UI already relies on.
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), String>;
    fn delete(&self, key: &SecretKey) -> Result<(), String>;
}

/// Installs the hook fragment an agent needs to report status. Returns the file it
/// wrote so the caller can keep or restore it; `None` means this agent family has no
/// hook mechanism.
pub trait HookProvider: Send + Sync {
    fn install(&self, agent_home: &Path) -> Result<Option<PathBuf>, String>;
    fn restore(&self, previous: &Option<PathBuf>) -> Result<(), String>;
}

/// Where agent-generated settings come from when a service needs them without the
/// caller passing them. The Tauri adapter answers from its cache; the native host
/// answers from disk.
pub trait SettingsSource: Send + Sync {
    fn read(&self) -> Result<Option<serde_json::Value>, String>;
}

/// The host-side primitive behind `Persist::ViaHost` (§12 蓝图 of
/// `e3836e62:docs/native/reviews/tauri-adapter.md`): on a platform that has NOT been handed the
/// writer role (ADR-021), core still decides the task table — somebody else stores the
/// bytes. That somebody is this trait, implemented by the adapter over
/// `write_task_records_raw`, i.e. byte-faithful JSON with no typed struct in between
/// (计划补记六十's mutation proved why). It is a mechanism, not a platform policy: the
/// host chooses when to route a write through it, and an absent saver is an error, never
/// a silent fallback.
///
/// Deliberately narrow: it saves and nothing else. No cover bookkeeping is offered —
/// `runtime.json` belongs to the writer lease, which this path does not hold, and that is
/// why a `ViaHost` write reports `revision: None`: "the host stored my decision" and
/// "I published a revision" are different facts.
pub trait HostTaskSaver: Send + Sync {
    /// Store the whole task list for one project, verbatim. An empty list means the
    /// project's file must go away (ADR-037 后果 1) — the implementer owns that mirroring.
    fn save_task_records(
        &self,
        project_id: &str,
        records: &[serde_json::Value],
    ) -> Result<(), String>;
}

#[derive(Clone, Debug, Default)]
pub struct MemorySecrets {
    entries: Arc<Mutex<BTreeMap<String, String>>>,
}

impl MemorySecrets {
    pub fn with(pairs: &[(&str, &str)]) -> Self {
        let store = Self::default();
        for (key, value) in pairs {
            store
                .set(&SecretKey((*key).to_string()), value)
                .expect("seed value");
        }
        store
    }

    pub fn peek(&self, key: &str) -> Option<String> {
        self.entries
            .lock()
            .expect("secrets mutex")
            .get(key)
            .cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        self.entries
            .lock()
            .expect("secrets mutex")
            .keys()
            .cloned()
            .collect()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, String> {
        Ok(self
            .entries
            .lock()
            .expect("secrets mutex")
            .get(&key.0)
            .cloned())
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), String> {
        let mut guard = self.entries.lock().expect("secrets mutex");
        if value.is_empty() {
            guard.remove(&key.0);
        } else {
            guard.insert(key.0.clone(), value.to_string());
        }
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), String> {
        self.entries.lock().expect("secrets mutex").remove(&key.0);
        Ok(())
    }
}

/// An agent family with no hook mechanism.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;

impl HookProvider for NoHooks {
    fn install(&self, _agent_home: &Path) -> Result<Option<PathBuf>, String> {
        Ok(None)
    }
    fn restore(&self, _previous: &Option<PathBuf>) -> Result<(), String> {
        Ok(())
    }
}

/// Everything a service is allowed to reach outside its own arguments.
pub struct CoreContext {
    pub paths: AppPaths,
    pub clock: Arc<dyn Clock>,
    pub secrets: Arc<dyn SecretStore>,
    pub hooks: Arc<dyn HookProvider>,
    /// OS 服务端口（剪贴板/文件管理器 reveal）。默认 [`NoPlatformServices`]——
    /// 剪贴板按端口契约返回空列表，调用方拿到的是中性的"没有能力"而不是错误；
    /// 真正的宿主（Tauri/CLI/headless host）用 [`CoreContext::with_platform`] 注入。
    pub platform: Arc<dyn crate::runtime::PlatformServices>,
    /// `Some` while this process holds the single-writer lease. A service that intends
    /// to write must be built from a context that has one; readers may proceed without
    /// it, because a shared read of a directory mid-write is recoverable, whereas a
    /// blind second writer is not.
    lease: Option<Arc<DataDirectoryLease>>,
    /// The byte-saving partner for `Persist::ViaHost` writes; see [`HostTaskSaver`].
    task_saver: Option<Arc<dyn HostTaskSaver>>,
}

impl CoreContext {
    /// Read-only context: no lease, so any attempt to write returns
    /// [`LeaseError::AlreadyInUse`]-style refusal from [`CoreContext::require_write`].
    pub fn read_only(paths: AppPaths) -> Self {
        Self {
            paths,
            clock: Arc::new(SystemClock),
            secrets: Arc::new(MemorySecrets::default()),
            hooks: Arc::new(NoHooks),
            platform: Arc::new(NoPlatformServices),
            lease: None,
            task_saver: None,
        }
    }

    /// Installs the host's byte saver for `ViaHost` writes. Only the adapter that has
    /// decided - per platform policy, never per core's guess - that it is a deferring
    /// host calls this, so the presence of a saver is itself the host's statement
    /// "core decides, I store".
    pub fn with_task_saver(mut self, saver: Arc<dyn HostTaskSaver>) -> Self {
        self.task_saver = Some(saver);
        self
    }

    pub fn task_saver(&self) -> Option<&Arc<dyn HostTaskSaver>> {
        self.task_saver.as_ref()
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    pub fn with_hooks(mut self, hooks: Arc<dyn HookProvider>) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_platform(mut self, platform: Arc<dyn crate::runtime::PlatformServices>) -> Self {
        self.platform = platform;
        self
    }

    /// Takes the writer role. Fails if another live process (or this one) holds it, or
    /// if the probe says this location should not host a writer (ADR-010).
    ///
    /// The probe is a required argument rather than a default: an adapter that has not
    /// answered "what filesystem is this" gets `VolumeKind::Unknown`, which is a refusal.
    /// Fail-open here would make the check optional in exactly the places it matters.
    pub fn acquire(
        paths: AppPaths,
        clock: Arc<dyn Clock>,
        owner: &LeaseOwner,
        probe: &dyn StorageLocationProbe,
    ) -> Result<Self, LeaseError> {
        let clock = clock.clone();
        let location = probe.probe(paths.data_dir());
        let lease = Arc::new(DataDirectoryLease::acquire(
            &paths,
            clock.as_ref(),
            owner,
            &location,
        )?);
        Ok(Self {
            paths,
            clock,
            secrets: Arc::new(MemorySecrets::default()),
            hooks: Arc::new(NoHooks),
            platform: Arc::new(NoPlatformServices),
            lease: Some(lease),
            task_saver: None,
        })
    }

    /// One start-up notice the caller owes the user when the directory happens to sit on a
    /// sync client's root (ADR-010 decision 1). `None` means nothing to report.
    pub fn location_warning(&self) -> Option<String> {
        let lease = self.lease.as_deref()?;
        lease.must_warn().then(|| {
            format!(
                "{} is inside a cloud-synced directory: Aeroric's own lock holds on this machine, but a second machine editing the same data is not something a lock can settle",
                lease.directory().display()
            )
        })
    }

    pub fn lease(&self) -> Option<&DataDirectoryLease> {
        self.lease.as_deref()
    }

    /// The gate every mutating storage call goes through first.
    ///
    /// It also re-checks that the lock file is still the one we locked: holding a lease
    /// granted minutes ago is not evidence about the current moment.
    pub fn require_write(&self) -> Result<&DataDirectoryLease, StorageWritePermission> {
        let lease = self.lease.as_deref().ok_or_else(|| {
            StorageWritePermission::missing_lease(self.paths.data_dir().to_path_buf())
        })?;
        lease.verify_still_held().map_err(|e| {
            StorageWritePermission::missing_lease(self.paths.data_dir().to_path_buf())
                .with_reason(e.to_string())
        })?;
        Ok(lease)
    }
}

/// A write was attempted without the right to write — either no lease was ever taken, or
/// the lease that was taken stopped being ours (see `DataDirectoryLease::verify_still_held`).
#[derive(thiserror::Error, Debug)]
#[error("no write permission for {directory:?}: {why}")]
pub struct StorageWritePermission {
    pub directory: PathBuf,
    pub why: String,
}

impl StorageWritePermission {
    fn missing_lease(directory: PathBuf) -> Self {
        Self {
            directory,
            why: "acquire the data-directory lease first (INV-03)".to_string(),
        }
    }

    /// Keep the same refusal, add the reason it changed: "I never had it" and "I had it and
    /// lost it" need different answers from a caller that retries.
    fn with_reason(self, reason: String) -> Self {
        Self {
            why: format!("the lease was lost: {reason}"),
            ..self
        }
    }
}

/// 无 OS 服务的默认实现（测试/未注入宿主）。剪贴板按端口契约返回空列表
/// （"平台没有对应能力"的中性答案），reveal 返回 `Unsupported`。
pub struct NoPlatformServices;

impl crate::runtime::PlatformServices for NoPlatformServices {
    fn reveal_in_file_manager(
        &self,
        _path: &std::path::Path,
    ) -> Result<(), crate::runtime::PlatformError> {
        Err(crate::runtime::PlatformError::Unsupported(
            "no platform services installed",
        ))
    }

    fn read_clipboard_file_paths(&self) -> Result<Vec<String>, crate::runtime::PlatformError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;

    #[test]
    fn memory_secrets_follow_the_empty_means_delete_rule() {
        let store = MemorySecrets::default();
        let key = SecretKey("ssh-password:conn-1".into());
        store.set(&key, "hunter2").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("hunter2"));
        store.set(&key, "").unwrap();
        assert_eq!(store.get(&key).unwrap(), None, "an empty write deletes");
        store.set(&key, "x").unwrap();
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
        // Deleting what is already absent is not an error, matching the keychain's
        // `NoEntry == Ok` handling in the shipping client.
        store.delete(&key).unwrap();
    }

    #[test]
    fn a_context_without_the_lease_cannot_write() {
        let root = std::env::temp_dir().join(format!("aeroric-ctx-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let context = CoreContext::read_only(paths.clone());
        assert!(context.lease().is_none());
        let error = context
            .require_write()
            .err()
            .expect("a read-only context must not be able to write");
        assert!(error.to_string().contains("lease"), "{error}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A local volume whose private permissions the core enforces itself.
    struct LocalEligible;
    impl crate::location::StorageLocationProbe for LocalEligible {
        fn probe(&self, data_dir: &std::path::Path) -> crate::location::StorageLocation {
            crate::location::StorageLocation::new(data_dir, crate::location::VolumeKind::Local)
        }
    }

    #[test]
    fn acquiring_gives_the_write_permission_and_releasing_takes_it_away() {
        let root = std::env::temp_dir().join(format!("aeroric-ctx-held-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let owner = LeaseOwner {
            application: "test".into(),
            version: "0".into(),
        };
        let probe = LocalEligible;
        {
            let held =
                CoreContext::acquire(paths.clone(), Arc::new(FixedClock::at(9)), &owner, &probe)
                    .expect("acquire");
            assert!(held.require_write().is_ok());
            assert_eq!(held.clock.now_ms(), 9, "the injected clock must survive");
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
