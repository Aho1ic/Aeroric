//! Headless Aeroric core. INV-02: never depends on tauri, window handles,
//! React or Swift. Services land here in P2/P3.
//!
//! `paths`/`clock`/`context` are the injected environment (Task P2-02), `storage_lock`
//! is the cross-process single writer, and `storage` reads and writes the file layout
//! the shipping client already uses — including the fields this crate does not model.

pub mod agents;
pub mod clock;
pub mod connections;
pub mod context;
pub mod database;
pub mod events;
pub mod extensions;
pub mod files;
pub mod git;
pub mod ide;
pub mod infrastructure;
pub mod local_router;
pub mod location;
pub mod location_probe;
pub mod notebook;
pub mod paths;
pub mod platform;
pub mod projects;
pub mod runtime;
pub mod sessions;
pub mod sse;
pub mod storage;
pub mod storage_backends;
pub mod storage_lock;
pub mod tasks;
pub mod terminal;

pub use clock::{Clock, FixedClock, SystemClock};
pub use context::{
    CoreContext, HookProvider, SecretKey, SecretStore, SettingsSource, StorageWritePermission,
};
pub use location::{
    classify_by_path, with_cloud_sync_heuristic, Ineligible, StorageLocation, StorageLocationProbe,
    UnprobedLocation, VolumeKind, CLOUD_SYNCED_ROOT_MARKERS,
};
pub use paths::{AppPaths, PathError};
pub use projects::{service::ProjectService, ProjectError as ProjectsError};
pub use storage::{atomic, compat, ReadOutcome, Record, StorageError};
pub use storage_lock::{
    holder_hint, DataDirectoryLease, HolderInfo, LeaseError, LeaseOwner, LockIdentity,
    STALE_TEMP_MAX_AGE_MS,
};

pub fn crate_name() -> &'static str {
    "aeroric-core"
}
