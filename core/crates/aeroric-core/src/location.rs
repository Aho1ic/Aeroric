//! Where a data directory is allowed to be written at all (ADR-010).
//!
//! A lease proves that *this* process is alone among the processes that ask. It says
//! nothing about a filesystem that does not honour record locks (NFS/SMB/FUSE), and
//! nothing about a machine-local directory that a sync client later merges with another
//! machine's copy. Those are different failures with different fixes, so the core splits
//! them: the answer is a [`StorageLocation`], and only some answers qualify for the
//! writer role.
//!
//! The probe is a *port*. Determining the volume type needs `statvfs`/`getmntinfo` on
//! unix and `GetDriveType`/ACE work on Windows, and the adapters already carry those
//! dependencies (`src-tauri/Cargo.toml:41,122`) while the core deliberately carries
//! none. Duplicating the *policy* in each adapter is what this module exists to prevent:
//! adapters answer the question, the core decides what the answer means.

use std::path::{Path, PathBuf};

/// What kind of storage the managed directory sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    /// A local filesystem: record locks are honoured by every process on this machine.
    Local,
    /// Local bytes *plus* a background sync client (iCloud Drive, OneDrive, Dropbox).
    /// The lock works; the sync engine's cross-machine merge is the thing a lock cannot
    /// see, so this is eligible but must be reported (ADR-010 decision 1).
    CloudSyncedRoot,
    /// A network filesystem (NFS/SMB/FUSE): record-lock semantics are client-dependent and
    /// may silently degrade to "always succeeds". Not eligible.
    NonLocal,
    /// Nobody answered — the probe is unwired, the OS call failed, or the volume is not
    /// classifiable. Treated as *not* eligible: an unknown that allowed writes would make
    /// the whole check a formality (ADR-010 decision 4).
    Unknown,
}

impl VolumeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeKind::Local => "local",
            VolumeKind::CloudSyncedRoot => "cloud-synced-root",
            VolumeKind::NonLocal => "non-local",
            VolumeKind::Unknown => "unknown",
        }
    }

    /// Spelled exactly as it is persisted in `runtime.json`'s `volumeKind`.
    pub fn from_wire(text: &str) -> Self {
        match text {
            "local" => VolumeKind::Local,
            "cloud-synced-root" => VolumeKind::CloudSyncedRoot,
            "non-local" => VolumeKind::NonLocal,
            _ => VolumeKind::Unknown,
        }
    }
}

/// Why a location may not host a writer. Names the *fix*, because "storage error" would
/// make three different remedies look like one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ineligible {
    /// Move the data directory back to a local disk (`--data-dir`).
    NonLocalVolume,
    /// Wire the platform probe; the core refuses to guess.
    VolumeUnverified,
    /// The platform has not confirmed that only this user can read the files it creates.
    PrivatePermissionsUnverified,
}

impl std::fmt::Display for Ineligible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ineligible::NonLocalVolume => write!(
                f,
                "the data directory is on a network volume where record locks are not guaranteed"
            ),
            Ineligible::VolumeUnverified => write!(
                f,
                "no storage-location probe answered, so the filesystem cannot be confirmed to honour locks"
            ),
            Ineligible::PrivatePermissionsUnverified => write!(
                f,
                "the platform has not confirmed private file permissions (0600/0700 or an inherited ACE)"
            ),
        }
    }
}

/// One probe answer about one directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageLocation {
    pub directory: PathBuf,
    pub volume: VolumeKind,
    /// Windows tightens with an inherited ACE (see `src-tauri/src/storage.rs:284-288`);
    /// unix gets `true` from the core's own 0600/0700 handling. `false` means *nobody
    /// checked*, not "checked and it is fine".
    pub private_permissions_enforced: bool,
}

impl StorageLocation {
    /// Only correct where the core enforces private modes itself (unix: new files are
    /// created 0600 and directories 0700). Anywhere else the permission verdict has to be
    /// stated by the platform, so this constructor reports `false` and the writer check
    /// refuses — an adapter on Windows uses [`StorageLocation::with_permissions`].
    pub fn new(directory: impl Into<PathBuf>, volume: VolumeKind) -> Self {
        Self {
            directory: directory.into(),
            volume,
            // On unix the core creates files 0600 and directories 0700 itself; elsewhere
            // only an explicit `with_permissions` from the platform counts as a verdict.
            private_permissions_enforced: cfg!(unix),
        }
    }

    /// The full answer: volume kind plus whoever is responsible says private permissions
    /// really are in force. `false` is never "unknown-ish okay", it is a refusal.
    pub fn with_permissions(
        directory: impl Into<PathBuf>,
        volume: VolumeKind,
        private_permissions_enforced: bool,
    ) -> Self {
        Self {
            directory: directory.into(),
            volume,
            private_permissions_enforced,
        }
    }

    /// A path that is merely readable is not a *writer* location. Cloud-synced roots
    /// qualify; they additionally owe the caller a warning.
    /// Volume first: a network mount is the more specific diagnosis, and reporting a
    /// permissions problem there would send the user to fix the wrong thing.
    pub fn writer_eligible(&self) -> Result<(), Ineligible> {
        match self.volume {
            VolumeKind::Local | VolumeKind::CloudSyncedRoot => {}
            VolumeKind::NonLocal => return Err(Ineligible::NonLocalVolume),
            VolumeKind::Unknown => return Err(Ineligible::VolumeUnverified),
        }
        if !self.private_permissions_enforced {
            return Err(Ineligible::PrivatePermissionsUnverified);
        }
        Ok(())
    }

    /// Eligible but worth telling the user about, once, at start-up (ADR-010 decision 1).
    pub fn must_warn(&self) -> bool {
        self.volume == VolumeKind::CloudSyncedRoot
    }
}

/// The port an adapter implements.
pub trait StorageLocationProbe: Send + Sync {
    fn probe(&self, data_dir: &Path) -> StorageLocation;
}

/// Known sync-client roots, as path *substrings* relative to a home directory. Deliberately
/// conservative: a match downgrades to "eligible + warn", it never blocks, so a false
/// positive costs one notice and a false negative costs only the missing notice.
pub const CLOUD_SYNCED_ROOT_MARKERS: &[&str] = &[
    "Library/Mobile Documents", // iCloud Drive
    "com~apple~CloudDocs",      // iCloud Drive container spelling
    "Microsoft OneDrive",       // OneDrive folder name
    "/OneDrive/",               // OneDrive under any user profile
    "Dropbox/PC",               // classic Dropbox sync root
    "Google Drive",             // Drive for Desktop
    "GoogleDrive",
];

/// Classify by path alone. This is the *fallback* half of the answer — it catches sync
/// roots, which look exactly like local volumes to `statfs`. Network mounts are caught by
/// the platform probe, never by guessing here (ADR-010 decision 3).
pub fn classify_by_path(path: &Path, markers: &[&str]) -> Option<VolumeKind> {
    let text = path.to_string_lossy().replace('\\', "/");
    markers
        .iter()
        .any(|marker| text.contains(marker))
        .then_some(VolumeKind::CloudSyncedRoot)
}

/// The fail-closed default: nothing probed, so nothing may write.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnprobedLocation;

impl StorageLocationProbe for UnprobedLocation {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        let volume =
            classify_by_path(data_dir, CLOUD_SYNCED_ROOT_MARKERS).unwrap_or(VolumeKind::Unknown);
        StorageLocation {
            directory: data_dir.to_path_buf(),
            volume,
            private_permissions_enforced: false,
        }
    }
}

/// Rewrites the volume kind from the path heuristics while keeping the caller's own
/// permission verdict. Adapters wrap their platform answer with this so a sync root is
/// reported even when `statfs` says "local".
pub fn with_cloud_sync_heuristic(
    inner: &dyn StorageLocationProbe,
    data_dir: &Path,
) -> StorageLocation {
    let mut answer = inner.probe(data_dir);
    if answer.volume == VolumeKind::Local {
        if let Some(kind) = classify_by_path(data_dir, CLOUD_SYNCED_ROOT_MARKERS) {
            answer.volume = kind;
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        answer: StorageLocation,
    }

    impl StorageLocationProbe for Fake {
        fn probe(&self, _data_dir: &Path) -> StorageLocation {
            self.answer.clone()
        }
    }

    fn location(volume: VolumeKind, perms: bool) -> StorageLocation {
        StorageLocation {
            directory: "/home/dev/.aeroric".into(),
            volume,
            private_permissions_enforced: perms,
        }
    }

    #[test]
    fn only_local_and_cloud_synced_volumes_may_write() {
        assert_eq!(
            location(VolumeKind::Local, true).writer_eligible(),
            Ok(()),
            "the ordinary case must not be gated away"
        );
        assert_eq!(
            location(VolumeKind::CloudSyncedRoot, true).writer_eligible(),
            Ok(())
        );
        assert_eq!(
            location(VolumeKind::NonLocal, true).writer_eligible(),
            Err(Ineligible::NonLocalVolume)
        );
        assert_eq!(
            location(VolumeKind::Unknown, true).writer_eligible(),
            Err(Ineligible::VolumeUnverified),
            "an unanswered probe is not a licence to write"
        );
    }

    #[test]
    fn unverified_permissions_refuse_even_on_a_local_volume() {
        assert_eq!(
            location(VolumeKind::Local, false).writer_eligible(),
            Err(Ineligible::PrivatePermissionsUnverified),
            "a writable volume with world-readable files is still a leak"
        );
    }

    #[test]
    fn the_cloud_synced_root_is_eligible_but_owes_a_warning() {
        assert!(location(VolumeKind::CloudSyncedRoot, true).must_warn());
        assert!(!location(VolumeKind::Local, true).must_warn());
        assert!(
            location(VolumeKind::NonLocal, true)
                .writer_eligible()
                .is_err(),
            "warning is not the same tier as refusal"
        );
    }

    #[test]
    fn volume_kind_spelling_survives_the_round_trip() {
        for kind in [
            VolumeKind::Local,
            VolumeKind::CloudSyncedRoot,
            VolumeKind::NonLocal,
            VolumeKind::Unknown,
        ] {
            assert_eq!(VolumeKind::from_wire(kind.as_str()), kind);
        }
        assert_eq!(
            VolumeKind::from_wire("whatever-a-newer-core-wrote"),
            VolumeKind::Unknown,
            "an unrecognised spelling must degrade to the refusal, not to Local"
        );
    }

    #[test]
    fn path_heuristics_catch_sync_roots_and_nobody_else() {
        for path in [
            "/home/dev/Library/Mobile Documents/com~apple~CloudDocs/.aeroric",
            "C:\\Users\\me\\OneDrive\\Documents\\.aeroric",
            "/home/me/Google Drive/.aeroric",
        ] {
            assert_eq!(
                classify_by_path(Path::new(path), CLOUD_SYNCED_ROOT_MARKERS),
                Some(VolumeKind::CloudSyncedRoot),
                "{path} is a sync root"
            );
        }
        assert_eq!(
            classify_by_path(Path::new("/home/dev/.aeroric"), CLOUD_SYNCED_ROOT_MARKERS),
            None,
            "an ordinary home directory must not be reported as synced"
        );
        assert_eq!(
            classify_by_path(Path::new("/mnt/nfs/share/.aeroric"), CLOUD_SYNCED_ROOT_MARKERS),
            None,
            "network mounts are the probe's job: guessing here would replace one error with another"
        );
    }

    #[test]
    fn an_unwired_probe_refuses_to_write() {
        let answer = UnprobedLocation.probe(Path::new("/home/dev/.aeroric"));
        assert_eq!(answer.volume, VolumeKind::Unknown);
        assert_eq!(
            answer.writer_eligible(),
            Err(Ineligible::VolumeUnverified),
            "the default probe must fail closed"
        );
        assert_eq!(
            StorageLocation::with_permissions(Path::new("/srv/aeroric"), VolumeKind::Local, false)
                .writer_eligible(),
            Err(Ineligible::PrivatePermissionsUnverified),
            "and so must an answer that never checked permissions"
        );
    }

    #[test]
    fn the_heuristic_upgrades_only_an_answered_local_volume() {
        let inner = Fake {
            answer: location(VolumeKind::Local, true),
        };
        let synced = with_cloud_sync_heuristic(
            &inner,
            Path::new("/home/dev/Library/Mobile Documents/.aeroric"),
        );
        assert_eq!(synced.volume, VolumeKind::CloudSyncedRoot);
        assert!(synced.writer_eligible().is_ok(), "and it stays eligible");
        assert!(synced.must_warn());

        let ordinary = with_cloud_sync_heuristic(&inner, Path::new("/home/dev/.aeroric"));
        assert_eq!(ordinary.volume, VolumeKind::Local);
        assert!(!ordinary.must_warn());
    }
}
