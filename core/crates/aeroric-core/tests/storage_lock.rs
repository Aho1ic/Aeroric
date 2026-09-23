//! Cross-process proof for the single-writer lease (P2-02 acceptance: "两个独立进程
//! 竞争相同目录只允许一个写入；不同目录可并存").
//!
//! Threads are not enough: a POSIX record lock does not conflict between two handles of
//! one process, so an in-process test could pass with a lock that two real processes
//! would both grant. Everything here therefore runs the test binary a second time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aeroric_core::storage_lock::{holder_hint, DataDirectoryLease, LeaseError, LeaseOwner};
use aeroric_core::{AppPaths, Clock, FixedClock, StorageLocation, SystemClock, VolumeKind};

const DIR_ENV: &str = "AERORIC_LEASE_CHILD_DIR";
const READY_FILE: &str = "AERORIC_LEASE_CHILD_READY";

/// The volume question is not under test here, so every case answers "local".
fn eligible(paths: &AppPaths) -> StorageLocation {
    StorageLocation::new(paths.data_dir(), VolumeKind::Local)
}

fn owner(name: &str) -> LeaseOwner {
    LeaseOwner {
        application: name.to_string(),
        version: "0.0.0-test".into(),
    }
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-lease-{tag}-{}-{}",
        std::process::id(),
        SystemClock.now_ms()
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("temp root");
    dir
}

/// The child half, `#[ignore]`d so a plain run never takes it. It writes the ready
/// marker only after it genuinely holds the lock, so the parent never races it.
#[test]
#[ignore]
fn lease_holder_child() {
    let dir = std::env::var(DIR_ENV).expect("the parent passes a directory");
    let ready = std::env::var(READY_FILE).expect("the parent passes a ready marker");
    let paths = AppPaths::with_data_dir(Path::new(&dir), Some(Path::new(&dir)));
    let lease = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(1),
        &owner("aeroric-child"),
        &eligible(&paths),
    )
    .expect("the child must be able to take an untouched directory");
    fs::write(&ready, lease.holder().epoch.as_bytes()).expect("write the marker");
    std::thread::sleep(Duration::from_secs(120));
}

fn spawn_holder(dir: &Path) -> Child {
    fs::remove_dir_all(dir).ok();
    fs::create_dir_all(dir).expect("child root");
    let marker = dir.join(".aeroric-child-holds-it");
    let child = Command::new(std::env::current_exe().expect("test binary"))
        .arg("--exact")
        .arg("--ignored")
        .arg("lease_holder_child")
        .arg("--nocapture")
        .env(DIR_ENV, dir)
        .env(READY_FILE, &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the holder process");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "the holder process never took the lock"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    // The marker is only written by a process that holds the lease, so its contents
    // double as proof the harness works rather than a file that happens to exist.
    let epoch = fs::read_to_string(&marker).expect("read the marker");
    assert!(epoch.contains("pid-"), "unexpected epoch {epoch:?}");
    child
}

#[test]
fn a_second_process_cannot_write_the_same_directory() {
    let root = temp_root("contend");
    let managed = root.join("managed");
    let paths = AppPaths::with_data_dir(&root, Some(&managed));
    let mut holder = spawn_holder(&managed);

    let error = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(2),
        &owner("aeroric-parent"),
        &eligible(&paths),
    )
    .err()
    .expect("another live process holds this directory");
    let LeaseError::AlreadyInUse {
        holder: described,
        directory,
    } = &error
    else {
        panic!("expected AlreadyInUse, got {error:?}");
    };
    assert!(
        described.contains("aeroric-child"),
        "the refusal must name the holder: {described}"
    );
    assert!(
        described.contains("never kill"),
        "and must warn against acting on it: {described}"
    );
    // The refusal names the *canonical* directory: two paths to one directory must
    // yield one writer, which is also why a symlinked data dir cannot smuggle in a
    // second lease.
    assert_eq!(
        directory,
        &fs::canonicalize(&managed).expect("canonical managed dir"),
        "the refusal must name the real directory"
    );

    let other = AppPaths::with_data_dir(&root, Some(&root.join("other")));
    let second = DataDirectoryLease::acquire(
        &other,
        &FixedClock::at(3),
        &owner("aeroric-parent"),
        &eligible(&other),
    )
    .expect("an unrelated data directory is not blocked");
    drop(second);

    holder.kill().expect("stop the child");
    let _ = holder.wait();
    // The lock file still exists and still names the dead child's pid, and yet the
    // directory is free: liveness came from the OS lock, never from the file.
    assert!(
        paths.lease_file().exists(),
        "the stale hint file is left behind"
    );
    let hint = holder_hint(&paths.lease_file()).expect("the hint stays readable");
    assert_eq!(hint.application, "aeroric-child");
    let reclaimed = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(4),
        &owner("aeroric-parent"),
        &eligible(&paths),
    )
    .expect("a dead holder must not keep a directory locked");
    assert_eq!(reclaimed.holder().application, "aeroric-parent");
    drop(reclaimed);
    fs::remove_dir_all(root).ok();
}

#[test]
fn dropping_a_lease_releases_without_deleting_the_hint() {
    let root = temp_root("no-cleanup");
    let managed = root.join("managed");
    let paths = AppPaths::with_data_dir(&root, Some(&managed));
    let first = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(1),
        &owner("first"),
        &eligible(&paths),
    )
    .unwrap();
    let first_epoch = first.holder().epoch.clone();
    let lock_path = first.lock_path().to_path_buf();
    drop(first);
    assert!(
        lock_path.exists(),
        "releasing must not delete the hint a reader depends on"
    );
    let second = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(2),
        &owner("second"),
        &eligible(&paths),
    )
    .unwrap();
    assert_ne!(second.holder().epoch, first_epoch);
    assert_eq!(
        holder_hint(&lock_path).map(|hint| hint.application),
        Some("second".to_string()),
        "the hint must describe the current holder, not the last one"
    );
    drop(second);
    fs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn a_symlink_to_the_same_directory_is_the_same_lease() {
    let root = temp_root("symlink");
    let real = root.join("real");
    let link = root.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("make the symlink");
    let via_real = AppPaths::with_data_dir(&root, Some(&real));
    let via_link = AppPaths::with_data_dir(&root, Some(&link));
    let held = DataDirectoryLease::acquire(
        &via_real,
        &FixedClock::at(1),
        &owner("first"),
        &eligible(&via_real),
    )
    .unwrap();
    let error = DataDirectoryLease::acquire(
        &via_link,
        &FixedClock::at(2),
        &owner("second"),
        &eligible(&via_link),
    )
    .err()
    .expect("following a symlink must not grant a second writer");
    assert!(
        matches!(error, LeaseError::AlreadyInUse { .. }),
        "{error:?}"
    );
    drop(held);
    let after = DataDirectoryLease::acquire(
        &via_link,
        &FixedClock::at(3),
        &owner("second"),
        &eligible(&via_link),
    )
    .expect("once released, either path may take it");
    drop(after);
    fs::remove_dir_all(root).ok();
}

#[test]
fn an_unusable_directory_is_a_distinct_refusal() {
    let root = temp_root("unusable");
    let blocked = root.join("managed");
    fs::write(&blocked, b"not a directory").expect("file where a directory is wanted");
    let paths = AppPaths::with_data_dir(&root, Some(&blocked));
    let error = DataDirectoryLease::acquire(
        &paths,
        &FixedClock::at(1),
        &owner("test"),
        &eligible(&paths),
    )
    .err()
    .expect("must refuse");
    assert!(
        matches!(
            error,
            LeaseError::UnusableDirectory(_) | LeaseError::NotWritable(_)
        ),
        "{error:?}"
    );
    fs::remove_dir_all(root).ok();
}
