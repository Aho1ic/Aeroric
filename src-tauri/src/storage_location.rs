//! The platform half of ADR-010: what kind of volume this data directory really sits on,
//! and whether private permissions are enforced by someone other than the core.
//!
//! `aeroric-core` deliberately owns only the *rule* (`writer_eligible`, the volume kinds,
//! the fail-closed default). It cannot answer "is `/Users/me/repo` on an SMB mount" because
//! that is a filesystem question, and a core that guessed from the path string would be
//! guessing at exactly the case that matters — a network mount spelled like a local one.
//! So the rule lives there and the measurement lives here.
//!
//! Two answers per probe:
//!
//! - **volume kind** — `statvfs`/`statfs` on unix, `GetDriveTypeW` on Windows. Anything the
//!   platform cannot answer stays `Unknown`, and `Unknown` does not get a writer role.
//!   The path heuristic in core then upgrades a *measured local* volume to
//!   `CloudSyncedRoot` when it sits under a sync root: `statfs` cannot see a sync client,
//!   so those two halves are complementary rather than redundant.
//! - **permission receipt** — on unix the core creates its own files `0600`/dirs `0700`,
//!   so the answer holds by construction. On Windows the answer comes from the ACE the
//!   shipping client already installs on this very directory (`storage::ensure_private_dir`
//!   → `windows_acl::restrict_to_current_user`); this module only stops throwing that
//!   verdict away. It is `true` when the OS accepted a protected DACL carrying exactly one
//!   inherited ALLOW for the current user, `false` otherwise.
//!
//! A directory that does not exist yet is the normal first-run case, so the unix probe
//! walks to the nearest existing ancestor instead of reporting `Unknown`.

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use aeroric_core::location::{
    classify_by_path, with_cloud_sync_heuristic, StorageLocation, StorageLocationProbe, VolumeKind,
};

/// Filesystem names that mean "this is a remote mount" on macOS/BSD, where `statvfs`
/// carries the type name. Matched as a prefix because these appear with version suffixes
/// (`nfs4`, `smb2`, `afpfs`).
const NETWORK_FSTYPENAMES: &[&str] = &[
    "nfs", "smb", "cifs", "afp", "webdav", "davfs", "curlftpfs", "sshfs", "9p", "ncp", "ceph",
    "gluster", "lustre", "fuse", "afs",
];

/// Local filesystem names on macOS/BSD. Only a *prefix* match here, so a name nobody
/// recognises falls through to `None` — "no opinion", which is not the same as "local".
const LOCAL_FSTYPENAMES: &[&str] = &["apfs", "hfs", "ext", "xfs", "btrfs", "zfs", "ntfs", "exfat"];

/// Superblock magics Linux reports in `statfs::f_type` for volumes whose record-lock
/// semantics a lease cannot trust. Every value below is quoted from the kernel's
/// `include/uapi/linux/magic.h`; the naming of an entry is a claim about the *lock*, not
/// about marketing — `FUSE` is included because `sshfs`/`rclonefs` report it, and a
/// local-backed fuse mount therefore also gets refused. Refusing is the safe direction.
const NETWORK_FTYPE_MAGICS: &[u64] = &[
    0x6969,      // NFS_SUPER_MAGIC
    0x517B,      // SMB_SUPER_MAGIC
    0xFE53_4D42, // SMB2_SUPER_MAGIC
    0xFF53_4D42, // CIFS_SUPER_MAGIC
    0x564C,      // NCP_SUPER_MAGIC
    0x7375_7245, // CODA_SUPER_MAGIC
    0x6B41_4653, // AFS_FS_MAGIC
    0x00C3_6400, // CEPH_SUPER_MAGIC
    0x7461_636F, // OCFS2_SUPER_MAGIC (shared-disk cluster)
    0x6573_5546, // FUSE_SUPER_MAGIC
    0x0102_1997, // V9FS_MAGIC (9p)
];

/// Magics for filesystems that are local and *must not* appear above. The test that
/// asserts these two tables are disjoint is the reason nobody can paste
/// `BTRFS_SUPER_MAGIC` into the veto list by eyeballing a hex prefix. Public, and the same
/// kind of reviewable data as the veto table rather than a test fixture.
pub const LOCAL_FTYPE_MAGICS: &[(&str, u64)] = &[
    ("TMPFS_MAGIC", 0x0102_1994),
    ("OVERLAYFS_SUPER_MAGIC", 0x794C_7630),
    ("EXT2/EXT4_SUPER_MAGIC", 0xEF53),
    ("XFS_SUPER_MAGIC", 0x5846_5342),
    ("BTRFS_SUPER_MAGIC", 0x9123_683E),
];

/// Vetoes only: a recognised remote superblock, or `None` for "no opinion". Calling
/// anything else local would be a guess, and the caller already has a real answer for the
/// rest (`f_fstypename` on the platforms that have one).
pub fn classify_f_type(magic: u64) -> Option<VolumeKind> {
    NETWORK_FTYPE_MAGICS
        .contains(&magic)
        .then_some(VolumeKind::NonLocal)
}

/// macOS/BSD `statvfs::f_fstypename`.
pub fn classify_fstypename(name: &str) -> Option<VolumeKind> {
    let lowered = name.to_ascii_lowercase();
    if NETWORK_FSTYPENAMES
        .iter()
        .any(|network| lowered.starts_with(network))
    {
        return Some(VolumeKind::NonLocal);
    }
    LOCAL_FSTYPENAMES
        .iter()
        .any(|local| lowered.starts_with(local))
        .then_some(VolumeKind::Local)
}

/// Windows `GetDriveTypeW`. Only the values that answer the lock question are mapped;
/// `DRIVE_NO_ROOT_DIR` and `DRIVE_UNKNOWN` are refusals, not "local". A UNC path that is
/// not a mapped drive letter lands in that refusal bucket, which is the safe way to
/// discover it: `GetDriveTypeW` only resolves drive roots.
pub fn classify_drive_type(kind: u32) -> Option<VolumeKind> {
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_RAMDISK: u32 = 6;
    match kind {
        DRIVE_REMOTE => Some(VolumeKind::NonLocal),
        DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK => Some(VolumeKind::Local),
        _ => None,
    }
}

/// What the platform could say about one directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeVolume {
    Measured(VolumeKind),
    NotVerifiable,
}

/// The raw platform measurement, without core's path heuristic.
struct RawPlatformProbe;

impl StorageLocationProbe for RawPlatformProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        let volume = match probe_volume(data_dir) {
            ProbeVolume::Measured(kind) => kind,
            ProbeVolume::NotVerifiable => VolumeKind::Unknown,
        };
        StorageLocation::with_permissions(data_dir, volume, private_permissions_enforced(data_dir))
    }
}

/// The adapter's probe. Delegates the measurement to [`RawPlatformProbe`] and then lets
/// core relabel a measured `Local` as `CloudSyncedRoot` where the path says so — the
/// heuristic can never turn `Unknown` into something eligible.
pub struct PlatformLocationProbe;

impl StorageLocationProbe for PlatformLocationProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        with_cloud_sync_heuristic(&RawPlatformProbe, data_dir)
    }
}

fn probe_volume(data_dir: &Path) -> ProbeVolume {
    #[cfg(unix)]
    {
        probe_unix_with_fallback(data_dir)
    }
    #[cfg(windows)]
    {
        windows_drive_type(data_dir)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = data_dir;
        ProbeVolume::NotVerifiable
    }
}

/// Whether somebody has actually made this directory private, as far as *this* platform
/// can prove. On unix the core's own file modes answer it; elsewhere only an ACE receipt
/// counts, and a failed receipt means the core stays a reader while the shipping client
/// keeps writing — no data is lost by a component that declined the writer role.
fn private_permissions_enforced(data_dir: &Path) -> bool {
    #[cfg(unix)]
    {
        let _ = data_dir;
        true
    }
    #[cfg(windows)]
    {
        crate::storage::windows_private_acl_receipt(data_dir)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = data_dir;
        false
    }
}

/// macOS/iOS/BSD: `statfs::f_fstypename` carries the filesystem *name*, the strongest
/// answer available, and needs no mount table, helper process, or root. (`statvfs` is the
/// POSIX-portable call, and precisely because it is portable it omits the type — which is
/// the one field this probe exists to read.)
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn unix_stat(data_dir: &Path) -> ProbeVolume {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let mut c_path = Vec::with_capacity(data_dir.as_os_str().as_bytes().len() + 1);
    c_path.extend_from_slice(data_dir.as_os_str().as_bytes());
    c_path.push(0);

    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    let rc = unsafe { libc::statfs(c_path.as_ptr() as *const libc::c_char, stat.as_mut_ptr()) };
    if rc != 0 {
        return ProbeVolume::NotVerifiable;
    }
    let stat = unsafe { stat.assume_init() };
    let name = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    match classify_fstypename(&name.to_string_lossy()) {
        Some(kind) => ProbeVolume::Measured(kind),
        None => ProbeVolume::NotVerifiable,
    }
}

/// Linux: `statvfs` has no type field, so the superblock magic comes from `statfs`.
#[cfg(target_os = "linux")]
fn unix_stat(data_dir: &Path) -> ProbeVolume {
    use std::os::unix::ffi::OsStrExt;

    let mut c_path = Vec::with_capacity(data_dir.as_os_str().as_bytes().len() + 1);
    c_path.extend_from_slice(data_dir.as_os_str().as_bytes());
    c_path.push(0);

    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    let rc = unsafe { libc::statfs(c_path.as_ptr() as *const libc::c_char, stat.as_mut_ptr()) };
    if rc != 0 {
        return ProbeVolume::NotVerifiable;
    }
    let stat = unsafe { stat.assume_init() };
    match classify_f_type(stat.f_type as u64) {
        // `None` is "no opinion", never "local": an unrecognised superblock must not
        // become a licence to write.
        Some(kind) => ProbeVolume::Measured(kind),
        None => ProbeVolume::NotVerifiable,
    }
}

#[cfg(not(any(unix, windows)))]
fn unix_stat(data_dir: &Path) -> ProbeVolume {
    let _ = data_dir;
    ProbeVolume::NotVerifiable
}

#[cfg(windows)]
fn windows_drive_type(data_dir: &Path) -> ProbeVolume {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    let root = drive_root(data_dir);
    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
    match classify_drive_type(kind) {
        Some(volume) => ProbeVolume::Measured(volume),
        None => ProbeVolume::NotVerifiable,
    }
}

/// `GetDriveTypeW` wants a drive root like `C:\`, not a subdirectory.
#[cfg(windows)]
pub fn drive_root(path: &Path) -> PathBuf {
    let bytes: Vec<u8> = path
        .to_string_lossy()
        .as_bytes()
        .iter()
        .take(3)
        .copied()
        .collect();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return PathBuf::from(format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()));
    }
    PathBuf::from(path)
}

/// A path that does not exist cannot be classified — the parent chain is walked so a
/// not-yet-created data directory still gets a real answer. The core creates the directory
/// itself, so this is the normal first-run case, not an edge case.
#[cfg(unix)]
pub fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut cursor = path;
    loop {
        if std::fs::symlink_metadata(cursor).is_ok() {
            return Some(cursor);
        }
        match cursor.parent() {
            Some(up) => cursor = up,
            None => return None,
        }
    }
}

#[cfg(unix)]
fn probe_unix_with_fallback(data_dir: &Path) -> ProbeVolume {
    match nearest_existing_ancestor(data_dir) {
        Some(existing) => unix_stat(existing),
        None => ProbeVolume::NotVerifiable,
    }
}

#[cfg(not(unix))]
fn probe_unix_with_fallback(data_dir: &Path) -> ProbeVolume {
    let _ = data_dir;
    ProbeVolume::NotVerifiable
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn remote_filesystem_names_are_named_as_the_reason_for_refusal() {
        for name in [
            "nfs", "nfs4", "smbfs", "smb2", "cifs", "afpfs", "afs", "webdav", "sshfs", "9p",
            "ceph", "fuse",
        ] {
            assert_eq!(
                classify_fstypename(name),
                Some(VolumeKind::NonLocal),
                "{name}"
            );
        }
        for name in ["apfs", "hfsplus", "ext4", "xfs", "zfs", "btrfs", "ntfs", "exfat"] {
            assert_eq!(
                classify_fstypename(name),
                Some(VolumeKind::Local),
                "{name}"
            );
        }
        // No opinion, not "local": an unrecognised name must not become a licence to write.
        assert_eq!(classify_fstypename("whateverfs"), None);
        assert_eq!(classify_fstypename("devfs"), None);
        assert_eq!(classify_fstypename(""), None);
    }

    #[test]
    fn a_local_superblock_is_never_in_the_remote_table() {
        // Membership decides, not eyeballing a hex prefix: `0x9123683e` (BTRFS) and
        // `0x01021994` (TMPFS) sit close enough to remote entries that a hand-typed table
        // can put a local filesystem on the veto list, which would silently cost the writer
        // role on the machine the product actually runs on.
        let vetoed: HashSet<u64> = NETWORK_FTYPE_MAGICS.iter().copied().collect();
        for (name, magic) in LOCAL_FTYPE_MAGICS {
            assert!(!vetoed.contains(magic), "{name} is local but was vetoed");
        }
        for magic in NETWORK_FTYPE_MAGICS {
            assert_eq!(classify_f_type(*magic), Some(VolumeKind::NonLocal));
        }
    }

    #[test]
    fn the_local_superblocks_fall_through_to_no_opinion() {
        // They are not "measured local" here because on Linux the only answer this function
        // gives is a veto; the caller's `statfs` already knows the rest.
        for (_, magic) in LOCAL_FTYPE_MAGICS {
            assert_eq!(classify_f_type(*magic), None, "{magic:#x}");
        }
    }

    #[test]
    fn windows_drive_kinds_map_only_the_values_that_answer() {
        assert_eq!(
            classify_drive_type(4),
            Some(VolumeKind::NonLocal),
            "DRIVE_REMOTE"
        );
        assert_eq!(classify_drive_type(3), Some(VolumeKind::Local), "DRIVE_FIXED");
        assert_eq!(
            classify_drive_type(2),
            Some(VolumeKind::Local),
            "DRIVE_REMOVABLE: a USB stick is local, its own risk profile notwithstanding"
        );
        assert_eq!(classify_drive_type(0), None, "DRIVE_UNKNOWN is a refusal");
        assert_eq!(classify_drive_type(1), None, "DRIVE_NO_ROOT_DIR");
        assert_eq!(classify_drive_type(5), None, "DRIVE_CDROM");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_becomes_the_root_getdrivetype_needs() {
        assert_eq!(
            drive_root(Path::new(r"C:\Users\me\AppData\Roaming")),
            PathBuf::from(r"C:\")
        );
        assert_eq!(drive_root(Path::new(r"d:\data")), PathBuf::from(r"D:\"));
        // A UNC path is not a drive root: it is passed through and `GetDriveTypeW` answers
        // `DRIVE_NO_ROOT_DIR`, which is a refusal rather than a guess.
        assert_eq!(
            drive_root(Path::new(r"\\nas\share\x")),
            PathBuf::from(r"\\nas\share\x")
        );
    }

    #[test]
    fn a_real_probe_of_a_real_temp_directory_is_measured_not_assumed() {
        // The adapter's whole purpose: this must answer on the machine the product runs on.
        // If it ever reports `NotVerifiable` for a plain temp dir, the core silently loses
        // its writer role, which is a load-bearing assertion rather than a smoke test.
        let dir = std::env::temp_dir();
        match probe_volume(&dir) {
            ProbeVolume::Measured(VolumeKind::Local) => {}
            ProbeVolume::Measured(other) => panic!("{dir:?} reported as {other:?}"),
            ProbeVolume::NotVerifiable => {
                panic!("{dir:?} could not be classified; the core would refuse to write")
            }
        }
    }

    #[test]
    fn the_wired_probe_grants_the_writer_role_on_this_machine() {
        // "接不上就是红的": the point of the exercise is that `CoreContext::acquire` gets a
        // probe whose answer is eligible, so an unwired adapter cannot pass for a wired one.
        let location = PlatformLocationProbe.probe(&std::env::temp_dir());
        assert_eq!(
            location.writer_eligible(),
            Ok(()),
            "probe answered {:?}",
            location
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_directory_is_measured_through_its_nearest_existing_ancestor() {
        let base = std::env::temp_dir();
        let missing = base.join(format!("aeroric-probe-missing-{}", std::process::id()));
        assert!(!missing.exists(), "the fixture must not exist");
        let ancestor = nearest_existing_ancestor(&missing).expect("some ancestor exists");
        assert!(
            matches!(probe_volume(&missing), ProbeVolume::Measured(VolumeKind::Local)),
            "walking up from {missing:?} reached {ancestor:?} and still could not measure it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_does_not_exist_yet_still_gets_a_real_answer() {
        // The ancestor walk is not a nicety: on first run the data directory is not there
        // yet, and if that produced `Unknown` the core would refuse to create it. Walking up
        // has to land on a volume it can measure.
        let deep = Path::new("/aeroric-definitely-not-here/nope/deeper");
        assert!(
            matches!(probe_unix_with_fallback(deep), ProbeVolume::Measured(_)),
            "a missing path must be measured through an ancestor, not reported as unverifiable"
        );
    }

    #[test]
    fn an_unanswered_volume_refuses_even_where_the_modes_are_private() {
        // The other half of the rule: `Unknown` is never "probably fine". `StorageLocation::new`
        // reports private modes on unix, so this asserts the volume answer alone can't carry it.
        let location = StorageLocation::new(Path::new("/anywhere"), VolumeKind::Unknown);
        assert_eq!(
            location.writer_eligible(),
            Err(aeroric_core::location::Ineligible::VolumeUnverified)
        );
    }

    #[test]
    fn a_sync_root_under_a_measured_local_volume_warns_and_stays_eligible() {
        // The path heuristic must not invent eligibility: it only relabels a real answer.
        let base = std::env::temp_dir().join("Library/Mobile Documents/.aeroric");
        let location = PlatformLocationProbe.probe(&base);
        assert_eq!(location.volume, VolumeKind::CloudSyncedRoot);
        assert!(location.must_warn());
        assert!(location.writer_eligible().is_ok());
    }

    #[test]
    fn the_probe_answer_and_the_cores_rule_disagree_about_nothing() {
        // Guards against the adapter quietly "fixing" the verdict downstream.
        for volume in [
            VolumeKind::Local,
            VolumeKind::CloudSyncedRoot,
            VolumeKind::NonLocal,
            VolumeKind::Unknown,
        ] {
            let location = StorageLocation::with_permissions(
                Path::new("/anywhere"),
                volume,
                private_permissions_enforced(Path::new("/anywhere")),
            );
            assert_eq!(
                location.writer_eligible().is_ok(),
                matches!(volume, VolumeKind::Local | VolumeKind::CloudSyncedRoot)
                    && private_permissions_enforced(Path::new("/anywhere")),
                "{volume:?}"
            );
        }
    }

    #[test]
    fn classify_by_path_catches_a_sync_root_the_platform_cannot_see() {
        // Proves the two halves really are complementary: `statfs` calls iCloud Drive `apfs`.
        assert_eq!(
            classify_by_path(
                Path::new("/Users/me/Library/Mobile Documents/.aeroric"),
                aeroric_core::location::CLOUD_SYNCED_ROOT_MARKERS
            ),
            Some(VolumeKind::CloudSyncedRoot)
        );
    }
}
