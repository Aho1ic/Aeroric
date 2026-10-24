//! Persistence for the managed data directory.
//!
//! Two layers, deliberately separate: [`atomic`] is the mechanism (temp file, flush,
//! rename, backup, permissions), [`compat`] is the file contract the shipping client
//! already has on disk (which record shape a file holds, and what survives a round
//! trip). [`projects`], [`tasks`] and [`settings`] are that contract spelled out per
//! file, and [`runtime`] is the new file that records which writer session is ahead.
//! A third layer — the business services that decide *when* to write — lands with
//! Task P2-03 and P2-04.

pub mod atomic;
pub mod backup;
pub mod compat;
pub mod fingerprint;
pub mod projects;
pub mod runtime;
pub mod settings;
pub mod tasks;

pub use atomic::{FileMode, ReadOutcome, StorageError};
pub use backup::{BackupManifest, RestoreOutcome};
pub use compat::{update_record, Record};
pub use fingerprint::{Cover, Fingerprint};
pub use projects::{load_projects, save_projects};
pub use runtime::{bump_revision, load as load_runtime, RuntimeMetadata, RUNTIME_SCHEMA_VERSION};
pub use settings::{load_settings, save_settings};
pub use tasks::{load_tasks, save_tasks};

#[cfg(test)]
pub(crate) mod test_support {
    use crate::clock::{Clock, FixedClock};
    use crate::context::CoreContext;
    use crate::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use crate::paths::AppPaths;
    use crate::storage::Record;
    use crate::storage_lock::LeaseOwner;
    use serde_json::json;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A writer over its own throwaway directory: nothing in a test reaches `~/.aeroric`.
    pub(crate) fn writer(tag: &str) -> (PathBuf, CoreContext) {
        let root = std::env::temp_dir().join(format!(
            "aeroric-core-{tag}-{}-{}",
            std::process::id(),
            crate::clock::SystemClock.now_ms()
        ));
        std::fs::remove_dir_all(&root).ok();
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let owner = LeaseOwner {
            application: "test".into(),
            version: "0".into(),
        };
        let context = CoreContext::acquire(
            paths,
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &owner,
            &LocalEligible,
        )
        .expect("lease");
        (root, context)
    }

    /// A directory on a local volume whose permissions the core enforces itself. Tests
    /// that are not about eligibility use this so the fixture is never the thing under test.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct LocalEligible;

    impl StorageLocationProbe for LocalEligible {
        fn probe(&self, data_dir: &Path) -> StorageLocation {
            StorageLocation::new(data_dir, VolumeKind::Local)
        }
    }

    /// A probe with a chosen answer, for the refusal cases.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FixedLocation(pub VolumeKind);

    impl StorageLocationProbe for FixedLocation {
        fn probe(&self, data_dir: &Path) -> StorageLocation {
            StorageLocation::new(data_dir, self.0)
        }
    }

    /// The fields named in P2-02's acceptance, plus one the core has never heard of.
    pub(crate) fn legacy_record() -> Record {
        Record(
            json!({
                "id": "t1", "projectId": "p1", "name": "keep me", "prompt": "hi",
                "agent": "codex", "permissionMode": "ask", "status": "interrupted",
                "createdAt": 1_700_000_000_000_i64,
                "claudeSessionId": "c-1", "claudeSessionPath": "/s/c.jsonl",
                "codexSessionId": "x-1", "sessionCodexLike": true,
                "worktreePath": "/w", "worktreeBranch": "b", "baseBranch": "main",
                "archivedAt": 1_700_000_001_000_i64, "completedAt": 1_700_000_002_000_i64,
                "selectedModel": "gpt-5", "reasoningEffort": "high", "speed": "fast",
                "attentionRequestedAt": null,
                "aFieldTheCoreHasNeverSeen": { "nested": [1, 2, 3] }
            })
            .as_object()
            .unwrap()
            .clone(),
        )
    }
}
