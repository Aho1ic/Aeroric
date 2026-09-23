//! `runtime.json` — which writer session owns this directory, and how far it got.
//!
//! The record files themselves cannot answer "is this snapshot newer than the one I am
//! about to write?" because the shipping client has no revision anywhere in them. This
//! file is where `schema`, the writer `epoch` and the monotonic `revision` live, kept
//! separate from user data so a half-written metadata record can never corrupt a project
//! list. The legacy client never opens this file, so adding it changes nothing for the
//! product that is shipping today.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::CoreContext;
use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};
use crate::storage::compat;
use crate::storage::fingerprint::Fingerprint;

/// The layout this build understands. A *higher* value in the file means a newer core
/// has been here, and this one must stop rather than downgrade the directory.
///
/// Version 2 added `contentFingerprints` (ADR-012 decision 2, landed in P2-04). It is a
/// schema change rather than an optional extra field because a v1 writer would keep
/// bumping the revision while never recording what it covered, and a v2 reader would then
/// treat every legacy save as a divergence. The counter is shared, so the column has to
/// be published together with it.
pub const RUNTIME_SCHEMA_VERSION: u32 = 2;

/// Decimal string, not a number: the same rule the wire uses for `seq` and `revision`
/// (plan §2.3), because a counter that silently wraps past 2^53-1 is silent data loss.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeMetadata {
    pub schema_version: u32,
    pub epoch: String,
    pub revision: String,
    pub updated_at_ms: u64,
    /// ADR-010: the volume this writer was granted on, so a reader can tell "local" from
    /// "inside a sync client's root" without re-probing. A file written before this field
    /// existed reads back as `unknown` rather than failing.
    #[serde(default = "unrecorded_volume")]
    pub volume_kind: String,
    /// ADR-012: the exact bytes this core last published, per file. `None` means "last
    /// write left no file at all", which is a state the record files can be in.
    /// A v1 file reads back empty, and the first v2 write fills in what it covers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub content_fingerprints: BTreeMap<String, Option<Fingerprint>>,
}

fn unrecorded_volume() -> String {
    crate::location::VolumeKind::Unknown.as_str().to_string()
}

/// What a successful bump found before it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bumped {
    pub metadata: RuntimeMetadata,
    /// The epoch of the *previous* writer, when this directory was already in use by a
    /// session we did not start — in practice, the legacy client or an earlier host run.
    pub previous_epoch: Option<String>,
}

impl RuntimeMetadata {
    pub fn revision_value(&self) -> Option<u64> {
        canonical_decimal(&self.revision)
    }
}

/// `u64` in, decimal string out — the only spelling this file accepts.
fn canonical_decimal(text: &str) -> Option<u64> {
    let value = text.parse::<u64>().ok()?;
    (value.to_string() == text).then_some(value)
}

fn parse(path: &std::path::Path, meta: RuntimeMetadata) -> Result<RuntimeMetadata, StorageError> {
    if meta.schema_version > RUNTIME_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            path: path.to_path_buf(),
            found: meta.schema_version,
            supported: RUNTIME_SCHEMA_VERSION,
        });
    }
    if canonical_decimal(&meta.revision).is_none() {
        return Err(StorageError::Corrupt {
            path: path.to_path_buf(),
            reason: format!(
                "revision {:?} is not a canonical decimal string like \"42\"",
                meta.revision
            ),
        });
    }
    if meta.schema_version < RUNTIME_SCHEMA_VERSION {
        // Reading an older layout is fine; only writing it back would be a downgrade,
        // and a bump carries it forward because the fields are a superset.
        return Ok(RuntimeMetadata {
            schema_version: RUNTIME_SCHEMA_VERSION,
            ..meta
        });
    }
    Ok(meta)
}

pub fn load(context: &CoreContext) -> Result<ReadOutcome<RuntimeMetadata>, StorageError> {
    let path = context.paths.runtime_metadata_file();
    let found = match atomic::read_json(&path)? {
        ReadOutcome::Absent => return Ok(ReadOutcome::Absent),
        ReadOutcome::Found(value) => value,
    };
    let meta: RuntimeMetadata =
        serde_json::from_value(found).map_err(|e| StorageError::Corrupt {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    parse(&path, meta).map(ReadOutcome::Found)
}

/// Take the revision one step forward, creating the file on first use.
///
/// Only a context holding the lease reaches the write, so two processes cannot both
/// believe they were revision 7. A foreign epoch is inherited rather than erased: the
/// counter keeps climbing across writer generations, which is the whole question a
/// revision has to answer.
pub fn bump_revision(context: &CoreContext) -> Result<Bumped, StorageError> {
    bump_covering(context, &[])
}

/// Bump the revision and publish what this change covered, in one write.
///
/// The two belong together: a revision that advanced without recording the bytes it
/// replaced would leave [`recorded_fingerprint`] pointing at the change *before* it, and
/// the next writer would then be told its own output is a foreign edit.
pub fn bump_covering(
    context: &CoreContext,
    updates: &[(String, Option<Fingerprint>)],
) -> Result<Bumped, StorageError> {
    let lease = context.require_write()?;
    let path = context.paths.runtime_metadata_file();
    let (mut metadata, previous_epoch) = match load(context)? {
        ReadOutcome::Found(meta) => {
            let foreign = meta.epoch != lease.holder().epoch;
            let previous_epoch = foreign.then(|| meta.epoch.clone());
            (meta, previous_epoch)
        }
        ReadOutcome::Absent => (
            RuntimeMetadata {
                schema_version: RUNTIME_SCHEMA_VERSION,
                epoch: lease.holder().epoch.clone(),
                revision: "0".to_string(),
                updated_at_ms: context.clock.now_ms(),
                volume_kind: lease.location().volume.as_str().to_string(),
                content_fingerprints: BTreeMap::new(),
            },
            None,
        ),
    };
    let next = metadata
        .revision_value()
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| StorageError::Corrupt {
            path: path.clone(),
            reason: format!("revision {:?} cannot be advanced", metadata.revision),
        })?;
    metadata.revision = next.to_string();
    metadata.epoch = lease.holder().epoch.clone();
    metadata.updated_at_ms = context.clock.now_ms();
    metadata.volume_kind = lease.location().volume.as_str().to_string();
    for (key, value) in updates {
        metadata
            .content_fingerprints
            .insert(key.clone(), value.clone());
    }
    // No backup generation here: the counter is monotonic, and a stale copy of it would
    // suggest a rollback is safe when it is not.
    let bytes = compat::pretty(&serde_json::to_value(&metadata).expect("metadata is serializable"));
    atomic::write_atomic(&path, &bytes, FileMode::Private)?;
    Ok(Bumped {
        metadata,
        previous_epoch,
    })
}

/// The table entry itself, so "never written" and "written, and it deleted the file"
/// stay distinguishable. `None` inside `Some` is the latter.
pub fn recorded_cover(
    context: &CoreContext,
    key: &str,
) -> Result<Option<Option<Fingerprint>>, StorageError> {
    Ok(match load(context)? {
        ReadOutcome::Absent => None,
        ReadOutcome::Found(meta) => meta.content_fingerprints.get(key).cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::location::VolumeKind;
    use crate::paths::AppPaths;
    use crate::storage::test_support::{writer, FixedLocation};
    use crate::storage_lock::LeaseOwner;
    use std::sync::Arc;

    #[test]
    fn a_fresh_directory_starts_at_the_current_schema_and_revision_zero() {
        let (root, context) = writer("fresh");
        let first = bump_revision(&context).expect("bump");
        assert_eq!(first.metadata.revision, "1");
        assert_eq!(first.metadata.schema_version, RUNTIME_SCHEMA_VERSION);
        assert_eq!(first.previous_epoch, None, "nothing was here before");
        let text = std::fs::read_to_string(context.paths.runtime_metadata_file()).unwrap();
        assert!(
            text.contains(&format!("\"schemaVersion\": {}", RUNTIME_SCHEMA_VERSION)),
            "{text}"
        );
        assert!(
            text.contains("\"revision\": \"1\""),
            "the revision must be a string on disk: {text}"
        );
        assert!(
            text.contains("\"volumeKind\": \"local\""),
            "the volume this lease was granted on must be recorded: {text}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(context.paths.runtime_metadata_file())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "metadata is private like the data");
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn revisions_climb_and_a_foreign_writer_is_recorded() {
        let (root, context) = writer("climb");
        std::fs::create_dir_all(context.paths.data_dir()).unwrap();
        std::fs::write(
            context.paths.runtime_metadata_file(),
            br#"{"schemaVersion":1,"epoch":"pid-1-at-2","revision":"7","updatedAtMs":3}"#,
        )
        .unwrap();
        let second = bump_revision(&context).expect("take over at 8");
        assert_eq!(second.metadata.revision, "8");
        assert_eq!(
            second.metadata.epoch,
            context.lease().unwrap().holder().epoch
        );
        assert_eq!(second.previous_epoch.as_deref(), Some("pid-1-at-2"));
        let after = load(&context).unwrap().found().expect("present");
        assert_eq!(after.revision_value(), Some(8));
        assert_eq!(
            after.volume_kind, "local",
            "a pre-existing file is re-stamped with the volume actually in use"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_newer_schema_is_refused_and_left_untouched() {
        let (root, context) = writer("newer");
        std::fs::create_dir_all(context.paths.data_dir()).unwrap();
        let path = context.paths.runtime_metadata_file();
        let bytes = br#"{"schemaVersion":99,"epoch":"newer-core","revision":"3","updatedAtMs":4}"#;
        std::fs::write(&path, bytes).unwrap();
        let error = bump_revision(&context).expect_err("a newer core must not be downgraded");
        assert!(
            matches!(error, StorageError::UnsupportedSchema { found: 99, .. }),
            "{error:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "and it stays on disk");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_non_canonical_revision_is_a_refusal_not_a_reset() {
        for revision in ["\"007\"", "\"\"", "\"+1\"", "7.5"] {
            let (root, context) = writer("revision");
            std::fs::create_dir_all(context.paths.data_dir()).unwrap();
            std::fs::write(
                context.paths.runtime_metadata_file(),
                format!(
                    r#"{{"schemaVersion":1,"epoch":"e","revision":{revision},"updatedAtMs":4}}"#
                )
                .into_bytes(),
            )
            .unwrap();
            let error = bump_revision(&context)
                .expect_err("{revision} must not be read as a number that then resets");
            assert!(
                matches!(error, StorageError::Corrupt { .. }),
                "{revision} -> {error:?}"
            );
            std::fs::remove_dir_all(root).ok();
        }
    }

    #[test]
    fn a_synced_volume_is_recorded_as_what_it_is() {
        let (root, _) = writer("volume");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed2")));
        let context = CoreContext::acquire(
            paths,
            Arc::new(FixedClock::at(11)),
            &LeaseOwner {
                application: "test".into(),
                version: "0".into(),
            },
            &FixedLocation(VolumeKind::CloudSyncedRoot),
        )
        .expect("a synced root is eligible");
        let bumped = bump_revision(&context).expect("bump");
        assert_eq!(bumped.metadata.volume_kind, "cloud-synced-root");
        assert!(
            context.lease().unwrap().must_warn(),
            "the same answer the lease carries is what the caller must be warned with"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_reader_without_the_lease_cannot_establish_metadata() {
        let (root, _) = writer("readonly");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let context = CoreContext::read_only(paths);
        assert_eq!(load(&context).unwrap(), ReadOutcome::Absent);
        let error = bump_revision(&context).expect_err("absence of a file is not permission");
        assert!(error.to_string().contains("no write permission"), "{error}");
        assert!(
            !context.paths.runtime_metadata_file().exists(),
            "a refused bump creates nothing"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
