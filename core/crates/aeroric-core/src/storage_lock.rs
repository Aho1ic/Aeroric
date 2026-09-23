//! One data directory, one writer (plan INV-03).
//!
//! The shipping client has no cross-process lock at all — two instances of it
//! last-writer-wins over `projects.json` and every `tasks.json` today. The native host,
//! the CLI and the Tauri adapter *will* run against the same directory during migration,
//! so the lease is the thing that makes that situation fail loudly instead of quietly.
//!
//! Two rules the implementation must not trade away:
//! * Liveness comes from an OS file lock held by a live process, never from "the lock
//!   file exists" and never from "the pid inside it is alive". A pid that happens to be
//!   reused by another process is a false positive, and a crashed holder's file outlives
//!   its lock.
//! * The lease reports who holds it. Refusing without a reason is how a user ends up
//!   killing the wrong process.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::location::StorageLocation;
use crate::paths::AppPaths;

/// How old a `.name.<pid>-<nanos>.tmp` must be before the writer sweeping it may delete
/// it. Generous on purpose: a slow but live writer's temp is not garbage.
pub const STALE_TEMP_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

/// Who is asking. Supplied by the caller rather than read from the environment, so a
/// test can name itself and nothing here depends on how the process was launched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseOwner {
    pub application: String,
    pub version: String,
}

/// Written *after* the lock is held: a report, never the lock itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderInfo {
    pub pid: u32,
    pub application: String,
    pub version: String,
    pub epoch: String,
    pub acquired_at_ms: u64,
}

#[derive(thiserror::Error, Debug)]
pub enum LeaseError {
    /// Another live process holds the directory. `holder` is best effort — an empty
    /// value means the lock was taken but the hint file could not be read, which is
    /// still a refusal, not a licence to proceed.
    #[error("data directory {directory} is already held by another process ({holder})")]
    AlreadyInUse { directory: PathBuf, holder: String },
    /// The directory is not a place where a writer may be trusted (ADR-010). Refused
    /// *before* anything is created there, because "we could not take the lock" and
    /// "this volume should not be written at all" need opposite answers from the user.
    #[error("cannot take the writer role for {directory}: {reason}")]
    UnsupportedLocation { directory: PathBuf, reason: String },
    /// The lock we were handed is no longer the lock we hold: the file disappeared or
    /// was replaced, so some other process may have locked the new file and written.
    /// Continuing would be a blind overwrite, so the caller must stop and be told.
    #[error(
        "lost the writer lease on {directory}: the lock file was removed or replaced underneath us"
    )]
    IdentityLost { directory: PathBuf },
    #[error("cannot create the private data directory {0}")]
    UnusableDirectory(PathBuf),
    #[error("cannot open {0} for writing: check permissions before retrying")]
    NotWritable(PathBuf),
    #[error("{context}: {reason}")]
    Io { context: String, reason: String },
}

/// Held for the lifetime of the value: dropping it closes the file, which is what
/// releases the OS lock.
pub struct DataDirectoryLease {
    file: File,
    directory: PathBuf,
    lock_path: PathBuf,
    holder: HolderInfo,
    identity: LockIdentity,
    location: StorageLocation,
}

/// Which file our lock sits on. A path that still exists is not proof we still hold it:
/// a cleanup script that deletes and recreates `.write.lock` leaves a *new* file for
/// another process to lock, and both would believe they are alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockIdentity {
    /// `(dev, ino)` where the platform exposes them. `None` on Windows, where std has no
    /// stable equivalent — the check then degrades to existence, and that difference is
    /// asserted by a test rather than glossed over (ADR-010 decision 7).
    inode: Option<(u64, u64)>,
}

impl LockIdentity {
    #[cfg(unix)]
    fn of(path: &Path) -> Self {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(meta) => Self {
                inode: Some((meta.dev(), meta.ino())),
            },
            Err(_) => Self { inode: None },
        }
    }

    #[cfg(not(unix))]
    fn of(_path: &Path) -> Self {
        // Windows: no stable std API for volume serial + file id, so existence is all we
        // can check. `checks_inode() == false` keeps that difference from being hidden.
        Self { inode: None }
    }

    /// False exactly where the inode check is unavailable, so callers can report the
    /// weaker guarantee instead of implying a stronger one.
    pub fn checks_inode(&self) -> bool {
        self.inode.is_some()
    }
}

impl DataDirectoryLease {
    /// Takes the writer role for a directory the caller has *proven* is eligible.
    ///
    /// The eligibility answer comes from outside because only the platform can give it
    /// (`crate::location`), and refusing before `ensure_private` means a volume we should
    /// not write is not even created.
    pub fn acquire(
        paths: &AppPaths,
        clock: &dyn Clock,
        owner: &LeaseOwner,
        location: &StorageLocation,
    ) -> Result<Self, LeaseError> {
        let directory = paths.data_dir();
        if let Err(reason) = location.writer_eligible() {
            return Err(LeaseError::UnsupportedLocation {
                directory: directory.to_path_buf(),
                reason: reason.to_string(),
            });
        }
        paths
            .ensure_private()
            .map_err(|_| LeaseError::UnusableDirectory(directory.to_path_buf()))?;
        let canonical = std::fs::canonicalize(directory)
            .map_err(|_| LeaseError::UnusableDirectory(directory.to_path_buf()))?;
        let lock_path = canonical.join(
            paths
                .lease_file()
                .file_name()
                .expect("the lease file name is a constant"),
        );

        claim(&canonical)?;

        let file = match OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(_) => {
                relinquish(&canonical);
                return Err(LeaseError::NotWritable(lock_path));
            }
        };
        let lock_result = file.try_lock();
        if lock_result.is_err() {
            relinquish(&canonical);
            return Err(LeaseError::AlreadyInUse {
                directory: canonical.clone(),
                holder: describe_holder(&lock_path),
            });
        }
        // The hint file is written next; the mode is applied after it exists.

        let holder = HolderInfo {
            pid: std::process::id(),
            application: owner.application.clone(),
            version: owner.version.clone(),
            epoch: crate::clock::writer_epoch(clock),
            acquired_at_ms: clock.now_ms(),
        };
        // Best effort and deliberately last: a process that cannot write the hint still
        // holds the lock, and refusing to start over that would be worse.
        let _ = write_holder(&lock_path, &holder);
        let identity = LockIdentity::of(&lock_path);
        Ok(Self {
            file,
            directory: canonical.clone(),
            lock_path,
            holder,
            identity,
            location: location.clone(),
        })
    }

    pub fn holder(&self) -> &HolderInfo {
        &self.holder
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// The answer this lease was granted under, so a caller can report "eligible, but on
    /// a synced volume" alongside the fact that writes are happening.
    pub fn location(&self) -> &StorageLocation {
        &self.location
    }

    /// ADR-010 decision 1: a cloud-synced root may be written, but the user is owed one
    /// notice because the risk a lock cannot cover is another machine's copy.
    pub fn must_warn(&self) -> bool {
        self.location.must_warn()
    }

    pub fn identity(&self) -> LockIdentity {
        self.identity
    }

    /// Re-check the *file* our lock lives in, not just that some file is there.
    ///
    /// The OS lock is ours until we close it, but a cleanup script that removes
    /// `.write.lock` lets a second process lock a brand new file — after which both
    /// write. Stopping here is the difference between "one writer" and "one writer who
    /// noticed".
    pub fn verify_still_held(&self) -> Result<(), LeaseError> {
        let current = LockIdentity::of(&self.lock_path);
        let lost = match (self.identity.inode, current.inode) {
            (Some(expected), Some(found)) => expected != found,
            (Some(_), None) => true,
            // Existence is the whole check where no inode is available (Windows).
            _ => !self.lock_path.exists(),
        };
        if lost {
            return Err(LeaseError::IdentityLost {
                directory: self.directory.clone(),
            });
        }
        Ok(())
    }

    /// The lease is still the OS's, so this only re-checks that *we* did not lose it.
    pub fn still_held(&self) -> bool {
        self.verify_still_held().is_ok()
    }

    /// Sweep the temp files a crashed writer left behind — allowed only through a held
    /// lease, because deleting a user's file needs the one guarantee a lease provides:
    /// nobody else is writing right now (ADR-010 decision 8).
    ///
    /// `now_ms` comes from the caller's clock so a test can name the age it is checking.
    /// Anything newer than [`STALE_TEMP_MAX_AGE_MS`] survives: it may belong to a writer
    /// that is still running.
    pub fn sweep_stale_temps(&self, now_ms: u64) -> Vec<PathBuf> {
        crate::storage::atomic::remove_stale_temps(&self.directory, now_ms, STALE_TEMP_MAX_AGE_MS)
    }
}

impl Drop for DataDirectoryLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        relinquish(&self.directory);
    }
}

/// The lock file's contents, if a readable hint exists.
pub fn holder_hint(lock_path: &Path) -> Option<HolderInfo> {
    let text = std::fs::read_to_string(lock_path).ok()?;
    serde_json::from_str(&text).ok()
}

fn describe_holder(lock_path: &Path) -> String {
    match holder_hint(lock_path) {
        Some(info) => format!(
            "{} {} pid={} epoch={} since={}ms; reported, not verified — never kill a process on this information alone",
            info.application, info.version, info.pid, info.epoch, info.acquired_at_ms
        ),
        None => "unknown process; the hint file was unreadable".to_string(),
    }
}

/// The process's own set of held directories. A POSIX record lock does not conflict
/// between two handles of the same process, so without this a second `acquire()` in the
/// same binary would "succeed" and two writers would share one lease.
fn claim(directory: &Path) -> Result<(), LeaseError> {
    let mut guard = holders().lock().expect("lease registry poisoned");
    if guard.insert(directory.to_path_buf()) {
        Ok(())
    } else {
        Err(LeaseError::AlreadyInUse {
            directory: directory.to_path_buf(),
            holder: "this same process already holds it; a second writer must be a                      separate lease, not a second handle"
                .to_string(),
        })
    }
}

fn relinquish(directory: &Path) {
    let mut guard = holders().lock().expect("lease registry poisoned");
    guard.remove(directory);
}

fn holders() -> &'static Mutex<BTreeSet<PathBuf>> {
    static HOLDERS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    HOLDERS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Best-effort tightening of the hint file. It contains a pid and a version, no
/// secrets, so a failure here must not abort a held lease. On Windows the inherited
/// ACE applied to the data directory covers new files, and asserting that is the
/// platform adapter's job (plan P2-02 Action 5), not this function's.
#[cfg(not(windows))]
fn private_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn private_file(_path: &Path) {}

fn write_holder(lock_path: &Path, holder: &HolderInfo) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(holder)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(lock_path, format!("{text}\n"))?;
    private_file(lock_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, SystemClock};
    use crate::location::{StorageLocation, StorageLocationProbe, UnprobedLocation, VolumeKind};
    use crate::paths::AppPaths;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-lease-{tag}-{}-{}",
            std::process::id(),
            SystemClock.now_ms()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// These tests are about the lock, not the volume, so they always answer "local".
    fn eligible(paths: &AppPaths) -> StorageLocation {
        StorageLocation::new(paths.data_dir(), VolumeKind::Local)
    }

    fn owner() -> LeaseOwner {
        LeaseOwner {
            application: "aeroric-test".into(),
            version: "0.0.0-test".into(),
        }
    }

    #[test]
    fn a_lease_is_taken_and_released_with_the_value() {
        let root = temp_dir("basic");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let first =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(100), &owner(), &eligible(&paths))
                .expect("first acquire");
        assert_eq!(first.holder().acquired_at_ms, 100);
        assert_eq!(first.holder().pid, std::process::id());
        assert_eq!(
            holder_hint(first.lock_path()).map(|h| h.epoch),
            Some(first.holder().epoch.clone()),
            "the hint file must describe the live holder"
        );
        assert!(first.still_held());
        drop(first);
        let second =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(200), &owner(), &eligible(&paths))
                .expect("after the drop the directory is free again");
        assert_eq!(second.holder().acquired_at_ms, 200);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_second_writer_in_the_same_process_is_refused_with_a_reason() {
        let root = temp_dir("same-process");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let held =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &eligible(&paths))
                .unwrap();
        let error =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(2), &owner(), &eligible(&paths))
                .err()
                .expect("a second lease in one process must be refused");
        assert!(
            matches!(error, LeaseError::AlreadyInUse { .. }),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("this same process"),
            "the refusal must say who to look at: {error}"
        );
        drop(held);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn different_directories_can_be_held_at_once() {
        let root = temp_dir("two-dirs");
        let left = AppPaths::with_data_dir(&root, Some(&root.join("a")));
        let right = AppPaths::with_data_dir(&root, Some(&root.join("b")));
        let one =
            DataDirectoryLease::acquire(&left, &FixedClock::at(1), &owner(), &eligible(&left))
                .unwrap();
        let two =
            DataDirectoryLease::acquire(&right, &FixedClock::at(2), &owner(), &eligible(&right))
                .unwrap();
        assert_ne!(one.directory(), two.directory());
        drop((one, two));
        fs::remove_dir_all(&root).ok();
    }

    /// The rule the plan names explicitly: a stale hint is not a lock, and a pid is
    /// not proof of life. Here the hint claims *this* process holds it, which is the
    /// most misleading value a naive check could be fooled by.
    #[test]
    fn a_hint_file_claims_nothing() {
        let root = temp_dir("stale-hint");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        paths.ensure_private().expect("managed dir");
        let lock_path = paths.lease_file();
        fs::write(
            &lock_path,
            serde_json::json!({
                "pid": std::process::id(),
                "application": "ghost",
                "version": "9.9.9",
                "epoch": "pid-stale",
                "acquiredAtMs": 1,
            })
            .to_string(),
        )
        .expect("seed a stale hint");
        let lease =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(7), &owner(), &eligible(&paths))
                .expect("a stale hint must not block a live writer");
        assert_eq!(lease.holder().application, "aeroric-test");
        drop(lease);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unusable_directory_is_a_distinct_refusal() {
        let root = temp_dir("unusable");
        let blocker = root.join("managed");
        fs::write(&blocker, b"not a directory").expect("file where a dir is wanted");
        let paths = AppPaths::with_data_dir(&root, Some(&blocker));
        let error =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &eligible(&paths))
                .err()
                .expect("must refuse");
        assert!(
            matches!(
                error,
                LeaseError::UnusableDirectory(_) | LeaseError::NotWritable(_)
            ),
            "{error:?}"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_network_volume_is_refused_before_anything_is_created() {
        let root = temp_dir("nfs");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let location = StorageLocation::new(paths.data_dir(), VolumeKind::NonLocal);
        let error = DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &location)
            .err()
            .expect("a network mount must not be granted the writer role");
        assert!(
            matches!(error, LeaseError::UnsupportedLocation { .. }),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("network volume"),
            "the message must name the fix: {error}"
        );
        assert!(
            !paths.data_dir().exists(),
            "refusing means not creating the directory either: {:?}",
            paths.data_dir()
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unanswered_probe_is_a_refusal_rather_than_a_pass() {
        let root = temp_dir("unprobed");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let error = DataDirectoryLease::acquire(
            &paths,
            &FixedClock::at(1),
            &owner(),
            &UnprobedLocation.probe(paths.data_dir()),
        )
        .err()
        .expect("nobody answered, so nobody writes");
        assert!(
            matches!(error, LeaseError::UnsupportedLocation { .. }),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("probe") || error.to_string().contains("permissions"),
            "the refusal must say what is missing: {error}"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_local_volume_still_needs_its_permission_verdict() {
        let root = temp_dir("perms");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let unverified =
            StorageLocation::with_permissions(paths.data_dir(), VolumeKind::Local, false);
        let error = DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &unverified)
            .err()
            .expect("a world-readable data directory is not a writer location");
        assert!(
            error.to_string().contains("private file permissions"),
            "{error}"
        );
        assert!(
            !paths.lease_file().exists(),
            "and nothing is locked inside it"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_cloud_synced_root_may_write_but_owes_the_user_a_warning() {
        let root = temp_dir("cloud");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let location = StorageLocation::new(paths.data_dir(), VolumeKind::CloudSyncedRoot);
        let lease = DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &location)
            .expect("eligible, with a notice");
        assert!(lease.must_warn(), "the sync risk must be visible");
        assert_eq!(lease.location().volume, VolumeKind::CloudSyncedRoot);
        drop(lease);
        fs::remove_dir_all(root).ok();
    }

    /// Deleting and recreating the lock file is the case a plain existence check cannot
    /// see — and the case in which two processes would both believe they are alone.
    #[test]
    #[cfg(unix)]
    fn a_replaced_lock_file_stops_the_writer() {
        let root = temp_dir("identity");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let lease =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &eligible(&paths))
                .expect("first acquire");
        assert!(lease.identity().checks_inode(), "unix can check the inode");
        assert!(lease.still_held());

        let lock = lease.lock_path().to_path_buf();
        fs::remove_file(&lock).expect("simulate a cleanup script");
        fs::write(&lock, b"").expect("and a replacement appears");

        assert!(
            !lease.still_held(),
            "the same path is not the same file: the lock we hold is on an inode nobody else can lock"
        );
        let error = lease
            .verify_still_held()
            .expect_err("the writer must be told, not left guessing");
        assert!(
            matches!(error, LeaseError::IdentityLost { .. }),
            "{error:?}"
        );
        drop(lease);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn stale_temps_are_swept_only_through_a_held_lease() {
        let root = temp_dir("sweep");
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let lease =
            DataDirectoryLease::acquire(&paths, &FixedClock::at(1), &owner(), &eligible(&paths))
                .expect("lease");
        let now_ns = 2_000_000_000_000u64 * 1_000;
        let old = lease.directory().join(format!(
            ".projects.json.111-{}.tmp",
            now_ns - STALE_TEMP_MAX_AGE_MS * 2 * 1_000
        ));
        let fresh = lease
            .directory()
            .join(format!(".projects.json.222-{}.tmp", now_ns - 1_000));
        fs::write(&old, b"half a file").unwrap();
        fs::write(&fresh, b"keep me").unwrap();

        let removed = lease.sweep_stale_temps(now_ns / 1_000);
        assert_eq!(removed, vec![old.clone()], "only the ten-day-old temp goes");
        assert!(!old.exists());
        assert!(fresh.exists(), "a live writer's temp must survive");
        drop(lease);
        fs::remove_dir_all(root).ok();
    }
}
