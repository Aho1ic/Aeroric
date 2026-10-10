//! Sequential terminal-history cleanup compatibility, with isolated injected roots.

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use aeroric_core::clock::FixedClock;
use aeroric_core::context::CoreContext;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::terminal::history::{
    delete_task_terminal_histories, delete_task_terminal_histories_legacy, HistoryDeleteError,
};

#[path = "common/scratch.rs"]
mod scratch;

struct Local;

impl StorageLocationProbe for Local {
    fn probe(&self, directory: &Path) -> StorageLocation {
        StorageLocation::new(directory, VolumeKind::Local)
    }
}

struct Writer {
    // Drop the lease before the directory guard.
    context: CoreContext,
    root: scratch::Scratch,
}

impl Writer {
    fn new(label: &str) -> Self {
        let root = scratch::Scratch::new(label);
        let context = CoreContext::acquire(
            AppPaths::with_data_dir(root.path(), Some(&root.path().join("data"))),
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &LeaseOwner {
                application: "history-test".into(),
                version: "test".into(),
            },
            &Local,
        )
        .unwrap();
        Self { context, root }
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn seed(directory: &Path) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("first.log"), "first bytes").unwrap();
    fs::write(directory.join("later.log"), "later bytes").unwrap();
    fs::write(directory.join(" first .log"), "padded decoy").unwrap();
}

#[test]
fn legacy_empty_and_missing_batches_do_not_create_or_eagerly_resolve_directories() {
    let root = scratch::Scratch::new("history-legacy-empty");
    let directory = root.path().join("not-created-data/terminal-history");
    let calls = Cell::new(0);
    let mut resolver = || {
        calls.set(calls.get() + 1);
        Ok(directory.clone())
    };
    delete_task_terminal_histories_legacy(&[], &mut resolver).unwrap();
    assert_eq!(calls.get(), 0);
    delete_task_terminal_histories_legacy(&ids(&["missing", "missing"]), &mut resolver).unwrap();
    assert_eq!(
        calls.get(),
        2,
        "directory resolution remains per reached ID"
    );
    assert!(!directory.parent().unwrap().exists());
}

#[test]
fn checked_delete_uses_trimmed_history_name_and_preserves_other_files_and_modes() {
    let fixture = Writer::new("history-checked");
    let directory = fixture.context.paths.terminal_history_dir();
    seed(&directory);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(
            directory.join("later.log"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    delete_task_terminal_histories(&fixture.context, &ids(&["\tfirst ", "first", "missing"]))
        .unwrap();
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join(" first .log")).unwrap(),
        "padded decoy"
    );
    assert!(!fixture.context.paths.projects_file().exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(directory.join("later.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

#[test]
fn later_invalid_id_resolves_its_directory_then_stops_without_undoing_earlier_deletes() {
    let root = scratch::Scratch::new("history-partial-id");
    let directory = root.path().join("terminal-history");
    for bad in ["", "  ", "../escape", "a/b", "a\\b", "a b", "a\nb"] {
        seed(&directory);
        let calls = Cell::new(0);
        let error = delete_task_terminal_histories_legacy(&ids(&["first", bad, "later"]), || {
            calls.set(calls.get() + 1);
            Ok(directory.clone())
        })
        .unwrap_err();
        assert!(matches!(error, HistoryDeleteError::InvalidId(_)), "{error}");
        assert_eq!(
            calls.get(),
            2,
            "directory before ID validation; no later resolution"
        );
        assert!(!directory.join("first.log").exists(), "{bad:?}");
        assert_eq!(
            fs::read_to_string(directory.join("later.log")).unwrap(),
            "later bytes"
        );
    }
}

#[test]
fn directory_error_is_not_recursive_and_keeps_the_partial_batch() {
    let fixture = Writer::new("history-partial-io");
    let directory = fixture.context.paths.terminal_history_dir();
    seed(&directory);
    fs::create_dir(directory.join("blocked.log")).unwrap();
    fs::write(
        directory.join("blocked.log/child"),
        "not recursively deleted",
    )
    .unwrap();
    let error =
        delete_task_terminal_histories(&fixture.context, &ids(&["first", "blocked", "later"]))
            .unwrap_err();
    assert!(matches!(error, HistoryDeleteError::Io(_)), "{error}");
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
    assert_eq!(
        fs::read_to_string(directory.join("blocked.log/child")).unwrap(),
        "not recursively deleted"
    );
}

#[test]
fn legacy_resolver_failure_preserves_text_order_and_prior_effects() {
    let root = scratch::Scratch::new("history-resolver-error");
    let directory = root.path().join("terminal-history");
    seed(&directory);
    let calls = Cell::new(0);
    let error =
        delete_task_terminal_histories_legacy(&ids(&["first", "../invalid", "later"]), || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("synthetic directory resolver failure".into())
            } else {
                Ok(directory.clone())
            }
        })
        .unwrap_err();
    assert!(matches!(error, HistoryDeleteError::ResolveDirectory(_)));
    assert_eq!(error.to_string(), "synthetic directory resolver failure");
    assert_eq!(calls.get(), 2);
    assert!(!directory.join("first.log").exists());
    assert_eq!(
        fs::read_to_string(directory.join("later.log")).unwrap(),
        "later bytes"
    );
}

#[test]
fn absent_or_lost_writer_lease_refuses_before_deletion_while_legacy_keeps_old_authority() {
    let root = scratch::Scratch::new("history-no-writer");
    let context = CoreContext::read_only(AppPaths::with_data_dir(
        root.path(),
        Some(&root.path().join("not-created")),
    ));
    for task_ids in [vec![], ids(&["first"]), ids(&["../bad"])] {
        assert!(matches!(
            delete_task_terminal_histories(&context, &task_ids),
            Err(HistoryDeleteError::WritePermission(_))
        ));
    }
    assert!(!context.paths.data_dir().exists());
    let directory = context.paths.terminal_history_dir();
    seed(&directory);
    assert!(delete_task_terminal_histories(&context, &ids(&["first"])).is_err());
    assert_eq!(
        fs::read_to_string(directory.join("first.log")).unwrap(),
        "first bytes"
    );
    delete_task_terminal_histories_legacy(&ids(&["first"]), || Ok(directory.clone())).unwrap();
    assert!(
        !directory.join("first.log").exists(),
        "legacy does not acquire a core lease"
    );
    assert!(!context.paths.lease_file().exists());

    let fixture = Writer::new("history-lost-writer");
    let directory = fixture.context.paths.terminal_history_dir();
    seed(&directory);
    let lease = fixture.context.paths.lease_file();
    fs::rename(&lease, fixture.root.path().join("original-lock")).unwrap();
    fs::write(&lease, "replacement inode, not the leased file").unwrap();
    let error = delete_task_terminal_histories(&fixture.context, &ids(&["first"])).unwrap_err();
    assert!(
        matches!(error, HistoryDeleteError::WritePermission(_)),
        "{error}"
    );
    assert!(error.to_string().contains("lease was lost"), "{error}");
    assert_eq!(
        fs::read_to_string(directory.join("first.log")).unwrap(),
        "first bytes"
    );
}

#[cfg(unix)]
#[test]
fn exists_then_remove_keeps_dangling_links_and_removes_only_live_leaf_links() {
    use std::os::unix::fs::symlink;

    let fixture = Writer::new("history-links-core");
    let directory = fixture.context.paths.terminal_history_dir();
    fs::create_dir_all(&directory).unwrap();
    fs::remove_dir(&directory).unwrap();
    let outside = fixture.root.path().join("linked-history");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, &directory).unwrap();
    let target = fixture.root.path().join("target");
    fs::write(&target, "unchanged target").unwrap();
    symlink(&target, directory.join("live.log")).unwrap();
    symlink(
        fixture.root.path().join("missing"),
        directory.join("dangling.log"),
    )
    .unwrap();
    let target_dir = fixture.root.path().join("target-directory");
    fs::create_dir(&target_dir).unwrap();
    fs::write(target_dir.join("sentinel"), "keep").unwrap();
    symlink(&target_dir, directory.join("dir-link.log")).unwrap();
    fs::write(
        directory.join("regular.log"),
        "remove through directory link",
    )
    .unwrap();

    delete_task_terminal_histories(
        &fixture.context,
        &ids(&["live", "dangling", "dir-link", "regular"]),
    )
    .unwrap();
    assert!(fs::symlink_metadata(directory.join("live.log")).is_err());
    assert!(fs::symlink_metadata(directory.join("dir-link.log")).is_err());
    assert!(fs::symlink_metadata(directory.join("dangling.log"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!outside.join("regular.log").exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged target");
    assert_eq!(
        fs::read_to_string(target_dir.join("sentinel")).unwrap(),
        "keep"
    );
    assert!(fs::symlink_metadata(directory)
        .unwrap()
        .file_type()
        .is_symlink());
}
