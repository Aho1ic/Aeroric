//! Direct core model/empty/error boundaries. Full listing and fake-git behavior
//! are exercised by the unchanged actual Tauri command compatibility fixtures.

use std::fs;

use aeroric_core::files::legacy_directory::{read_dir_entries, FsEntry};
use aeroric_core::files::read_write::FileEntry;
use serde_json::json;

#[path = "common/scratch.rs"]
mod scratch;

#[test]
fn legacy_and_native_models_keep_distinct_frozen_wire_shapes() {
    let legacy = FsEntry {
        name: "File.MD".into(),
        path: "/fixture/File.MD".into(),
        is_dir: false,
        extension: Some("md".into()),
        modified_at_ms: None,
        is_gitignored: true,
        is_symlink: false,
    };
    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        json!({
            "name": "File.MD", "path": "/fixture/File.MD",
            "is_dir": false, "extension": "md", "modified_at_ms": null,
            "is_gitignored": true, "is_symlink": false
        })
    );
    let native = FileEntry {
        name: "File.MD".into(),
        path: "File.MD".into(),
        is_dir: false,
        modified_at_ms: None,
        is_gitignored: false,
        is_symlink: false,
    };
    assert_eq!(
        serde_json::to_value(native).unwrap(),
        json!({
            "name": "File.MD", "path": "File.MD", "isDir": false,
            "modifiedAtMs": null, "isGitignored": false, "isSymlink": false
        })
    );
}

#[test]
fn empty_directory_does_not_initialize_or_resolve_a_project() {
    let scratch = scratch::Scratch::new("legacy-directory-empty");
    let directory = scratch.path().join("empty");
    let missing_project = scratch.path().join("not-initialized");
    fs::create_dir(&directory).unwrap();
    let modified = fs::metadata(&directory).unwrap().modified().unwrap();
    assert!(
        read_dir_entries(&directory, missing_project.to_str().unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(!missing_project.exists());
    assert_eq!(
        fs::metadata(&directory).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}

#[test]
fn initial_directory_io_errors_remain_unwrapped_without_side_effects() {
    let scratch = scratch::Scratch::new("legacy-directory-errors");
    let file = scratch.path().join("file");
    fs::write(&file, b"unchanged").unwrap();
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    for path in [file.clone(), scratch.path().join("missing")] {
        let expected = fs::read_dir(&path).err().unwrap().to_string();
        assert!(matches!(
            read_dir_entries(&path, scratch.path().to_str().unwrap()),
            Err(error) if error == expected
        ));
    }
    assert_eq!(fs::read(&file).unwrap(), b"unchanged");
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}
