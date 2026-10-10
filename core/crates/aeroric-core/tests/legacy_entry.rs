//! Legacy entry/create behavior is not the native relative-path contract.

use aeroric_core::files::{legacy_entry, path_policy, read_write};
use std::fs;

#[path = "common/scratch.rs"]
mod scratch;

const COLLISION: &str = "A file or folder with that name already exists";

#[test]
fn name_policy_keeps_byte_limits_and_platform_specific_rules() {
    assert_eq!(legacy_entry::validate_entry_name(&"中".repeat(85)), Ok(()));
    let too_long = "é".repeat(128);
    assert_eq!(
        legacy_entry::validate_entry_name(&too_long)
            .err()
            .as_deref(),
        Some("File name is too long (max 255 bytes)")
    );
    assert_eq!(path_policy::validate_relative_path(&too_long), Ok(()));
    assert_eq!(
        legacy_entry::validate_entry_name("a\\b").err().as_deref(),
        Some("File name contains forbidden characters")
    );
    assert_eq!(path_policy::validate_relative_path("a\\b"), Ok(()));

    #[cfg(unix)]
    for name in [
        "CON", "nul.txt", "COM0", "lpt9.log", " space ", "dot.", "x:y", "a\u{1}b",
    ] {
        assert_eq!(legacy_entry::validate_entry_name(name), Ok(()));
    }
    // These assertions compile/run only on Windows; a macOS pass is not their evidence.
    #[cfg(target_os = "windows")]
    {
        for (name, stem) in [
            ("CON", "CON"),
            ("nul.txt", "nul"),
            ("COM0", "COM0"),
            ("lpt9.log", "lpt9"),
        ] {
            assert_eq!(
                legacy_entry::validate_entry_name(name),
                Err(format!("File name '{stem}' is reserved on Windows"))
            );
        }
        assert_eq!(
            legacy_entry::validate_entry_name("tail.").err().as_deref(),
            Some("File name cannot end with a space or a dot")
        );
        assert_eq!(
            legacy_entry::validate_entry_name("a\u{1}b")
                .err()
                .as_deref(),
            Some("File name contains control characters")
        );
    }
}

#[test]
fn new_path_keeps_raw_leaf_and_file_root_without_initializing() {
    let scratch = scratch::Scratch::new("legacy-entry-path");
    let root = scratch.path();
    let marker = root.join("file-root");
    fs::write(&marker, b"unchanged file root").unwrap();
    let before = fs::metadata(&marker).unwrap();
    assert_eq!(
        legacy_entry::validate_new_path_within(
            root.join(" raw leaf ").to_str().unwrap(),
            root.to_str().unwrap()
        )
        .unwrap(),
        (root.canonicalize().unwrap(), " raw leaf ".into())
    );
    assert_eq!(
        legacy_entry::validate_new_path_within(
            marker.join("child").to_str().unwrap(),
            marker.to_str().unwrap()
        )
        .unwrap(),
        (marker.canonicalize().unwrap(), "child".into())
    );
    assert!(path_policy::validate_project_root(marker.to_str().unwrap()).is_err());
    let absent = root.join("absent");
    assert_eq!(
        legacy_entry::validate_new_path_within(
            absent.join("new").to_str().unwrap(),
            root.to_str().unwrap()
        )
        .err(),
        Some(format!(
            "Cannot resolve parent directory: {}",
            absent.canonicalize().unwrap_err()
        ))
    );
    assert_eq!(fs::read(&marker).unwrap(), b"unchanged file root");
    let after = fs::metadata(&marker).unwrap();
    assert_eq!(before.permissions(), after.permissions());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn exclusive_creators_keep_existing_bytes_and_distinct_native_errors() {
    let scratch = scratch::Scratch::new("legacy-entry-create");
    let root = scratch.path();
    let existing = root.join("existing");
    let directory = root.join("directory");
    fs::write(&existing, b"do not truncate").unwrap();
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("child"), b"keep child").unwrap();
    let before = fs::metadata(&existing).unwrap();
    for path in [&existing, &directory] {
        assert_eq!(
            legacy_entry::create_file(path).err().as_deref(),
            Some(COLLISION)
        );
        assert_eq!(
            legacy_entry::create_directory(path).err().as_deref(),
            Some(COLLISION)
        );
    }
    assert_eq!(
        read_write::create_file(root, "existing").err().as_deref(),
        Some("file already exists")
    );
    assert_eq!(
        read_write::create_directory(root, "directory")
            .err()
            .as_deref(),
        Some("directory already exists")
    );
    let new_file = root.join("new file");
    let new_directory = root.join("new directory");
    assert_eq!(legacy_entry::create_file(&new_file), Ok(()));
    assert_eq!(legacy_entry::create_directory(&new_directory), Ok(()));
    assert_eq!(
        legacy_entry::create_file(&new_directory).err().as_deref(),
        Some(COLLISION)
    );
    assert_eq!(
        legacy_entry::create_directory(&new_file).err().as_deref(),
        Some(COLLISION)
    );
    assert!(fs::read(&new_file).unwrap().is_empty());
    assert_eq!(fs::read_dir(&new_directory).unwrap().count(), 0);
    assert_eq!(fs::read(&existing).unwrap(), b"do not truncate");
    assert_eq!(fs::read(directory.join("child")).unwrap(), b"keep child");
    let after = fs::metadata(&existing).unwrap();
    assert_eq!(before.permissions(), after.permissions());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(fs::read_dir(root).unwrap().count(), 4);
}

#[test]
fn creators_keep_raw_io_failures_and_do_not_repair_missing_parents() {
    let scratch = scratch::Scratch::new("legacy-entry-failure");
    let root = scratch.path();
    let target = root.join("missing/new");
    let before = fs::metadata(root).unwrap();
    let file_error = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .unwrap_err()
        .to_string();
    let directory_error = fs::create_dir(&target).unwrap_err().to_string();
    assert_eq!(legacy_entry::create_file(&target), Err(file_error));
    assert_eq!(
        legacy_entry::create_directory(&target),
        Err(directory_error)
    );
    let after = fs::metadata(root).unwrap();
    assert_eq!(before.permissions(), after.permissions());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}
