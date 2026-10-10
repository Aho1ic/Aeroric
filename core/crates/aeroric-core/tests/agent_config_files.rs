//! Shared explicit config IO; scratch paths and real scratch leases only.
use std::fs;
use std::path::Path;
use std::sync::Arc;

use aeroric_core::clock::FixedClock;
use aeroric_core::context::CoreContext;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::settings::agent_config_files::{read, write, write_legacy};
use aeroric_core::storage::atomic::write_legacy_atomic;
use aeroric_core::storage_lock::LeaseOwner;

#[path = "common/scratch.rs"]
mod scratch;

struct Local;
impl StorageLocationProbe for Local {
    fn probe(&self, path: &Path) -> StorageLocation {
        StorageLocation::new(path, VolumeKind::Local)
    }
}

struct Writer {
    context: CoreContext,
    root: scratch::Scratch,
}
impl Writer {
    fn new(label: &str) -> Self {
        let root = scratch::Scratch::new(label);
        let context = CoreContext::acquire(
            AppPaths::with_data_dir(root.path(), Some(&root.path().join("data"))),
            Arc::new(FixedClock::at(1)),
            &LeaseOwner {
                application: "config-files-test".into(),
                version: "test".into(),
            },
            &Local,
        )
        .unwrap();
        Self { context, root }
    }
}

fn entries(path: &Path) -> Vec<String> {
    let mut result: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    result.sort();
    result
}

#[test]
fn read_preserves_raw_content_absence_and_errors_without_creation_or_repair() {
    let root = scratch::Scratch::new("acf-read-core");
    let path = root.path().join("absent/config.json");
    assert_eq!(read(&path).unwrap(), None);
    assert!(entries(root.path()).is_empty());
    let path = root.path().join("config.json");
    fs::write(&path, " raw\0\n中文\r\n").unwrap();
    assert_eq!(read(&path).unwrap().as_deref(), Some(" raw\0\n中文\r\n"));
    fs::write(&path, b"\xff").unwrap();
    assert!(read(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"\xff");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(read(&path).is_err());
    assert!(entries(&path).is_empty());
    assert_eq!(entries(root.path()), ["config.json"]);
}

#[test]
fn both_authority_seams_share_raw_atomic_writes_and_inherited_modes() {
    let fixture = Writer::new("acf-write-core");
    let parent = fixture.root.path().join("nested/with spaces");
    let checked = parent.join("checked.json");
    let legacy = parent.join("legacy.json");
    for content in [" raw\nnot parsed\0配置\r\n", "", "[invalid JSON"] {
        write(&fixture.context, &checked, content).unwrap();
        write_legacy(&legacy, content).unwrap();
        assert_eq!(read(&checked).unwrap().as_deref(), Some(content));
        assert_eq!(read(&legacy).unwrap().as_deref(), Some(content));
        assert_eq!(entries(&parent), ["checked.json", "legacy.json"]);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&checked, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o750)).unwrap();
        let control = parent.join("mode-control");
        write_legacy_atomic(&control, "mode").unwrap();
        let mode = fs::metadata(&control).unwrap().permissions().mode() & 0o777;
        write(&fixture.context, &checked, "updated").unwrap();
        write_legacy(&legacy, "updated").unwrap();
        for path in [&checked, &legacy] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
        assert_eq!(
            fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }
}

#[test]
fn checked_non_writer_refuses_before_any_parent_io_but_legacy_seam_is_explicit() {
    let root = scratch::Scratch::new("acf-ro-core");
    let context = CoreContext::read_only(AppPaths::with_data_dir(
        root.path(),
        Some(&root.path().join("absent-data")),
    ));
    let path = root.path().join("absent-config/file");
    assert!(write(&context, &path, "refused")
        .unwrap_err()
        .contains("no write permission"));
    assert!(entries(root.path()).is_empty());
    write_legacy(&path, "legacy writer").unwrap();
    assert_eq!(read(&path).unwrap().as_deref(), Some("legacy writer"));
    assert!(!context.paths.data_dir().exists());
}

#[test]
fn a_lost_lease_is_rechecked_at_the_shared_write_boundary() {
    let fixture = Writer::new("acf-inode-core");
    let path = fixture.root.path().join("config.txt");
    write(&fixture.context, &path, "before").unwrap();
    let lock = fixture.context.paths.lease_file();
    fs::rename(&lock, lock.with_extension("retired")).unwrap();
    fs::write(&lock, "replacement inode").unwrap();
    assert!(write(&fixture.context, &path, "refused")
        .unwrap_err()
        .contains("no write permission"));
    assert_eq!(read(&path).unwrap().as_deref(), Some("before"));
    let new = fixture.root.path().join("absent/config.txt");
    assert!(write(&fixture.context, &new, "").is_err());
    assert!(!new.parent().unwrap().exists());
}

#[cfg(unix)]
#[test]
fn writes_replace_file_directory_and_dangling_symlink_leaves_without_following_them() {
    let fixture = Writer::new("acf-links-core");
    for kind in ["file", "directory", "dangling"] {
        let root = fixture.root.path().join(kind);
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        let leaf = root.join("config");
        match kind {
            "file" => fs::write(&target, "target before").unwrap(),
            "directory" => {
                fs::create_dir(&target).unwrap();
                fs::write(target.join("sentinel"), "sentinel").unwrap();
            }
            _ => {}
        }
        std::os::unix::fs::symlink(&target, &leaf).unwrap();
        write(&fixture.context, &leaf, "new config").unwrap();
        assert!(!fs::symlink_metadata(&leaf).unwrap().is_symlink());
        assert_eq!(read(&leaf).unwrap().as_deref(), Some("new config"));
        match kind {
            "file" => assert_eq!(fs::read_to_string(&target).unwrap(), "target before"),
            "directory" => assert_eq!(
                fs::read_to_string(target.join("sentinel")).unwrap(),
                "sentinel"
            ),
            _ => assert!(!target.exists()),
        }
    }
}

#[test]
fn replacement_breaks_hardlinks_instead_of_mutating_the_other_name() {
    let root = scratch::Scratch::new("acf-hardlink-core");
    let target = root.path().join("target");
    let leaf = root.path().join("config");
    fs::write(&target, "keep alias").unwrap();
    fs::hard_link(&target, &leaf).unwrap();
    write_legacy(&leaf, "new config").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep alias");
    assert_eq!(read(&leaf).unwrap().as_deref(), Some("new config"));
}

#[test]
fn real_directory_and_parent_file_errors_leave_no_temporary_config_copy() {
    let root = scratch::Scratch::new("acf-error-core");
    let leaf = root.path().join("directory");
    fs::create_dir(&leaf).unwrap();
    fs::write(leaf.join("sentinel"), "keep").unwrap();
    assert!(write_legacy(&leaf, "must not leak").is_err());
    assert_eq!(entries(root.path()), ["directory"]);
    assert_eq!(fs::read_to_string(leaf.join("sentinel")).unwrap(), "keep");
    let parent = root.path().join("file-parent");
    fs::write(&parent, "not a directory").unwrap();
    assert!(write_legacy(&parent.join("config"), "must not leak").is_err());
    assert_eq!(fs::read_to_string(&parent).unwrap(), "not a directory");
    assert_eq!(entries(root.path()), ["directory", "file-parent"]);
}
