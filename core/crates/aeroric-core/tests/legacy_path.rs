//! Direct shared-policy boundaries, not native/legacy policy equivalence.

use aeroric_core::files::{legacy_path, path_policy};
use std::fs;

#[path = "common/scratch.rs"]
mod scratch;

#[test]
fn relative_target_refuses_before_resolving_the_root() {
    let scratch = scratch::Scratch::new("legacy-path-relative");
    let missing_root = scratch.path().join("never-created");
    for target in ["", "relative", "../relative", "."] {
        for loose in [false, true] {
            assert_eq!(
                legacy_path::validate_path_within(target, missing_root.to_str().unwrap(), loose),
                Err("Path must be absolute".into())
            );
        }
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn legacy_file_root_is_not_the_native_directory_root_policy() {
    let scratch = scratch::Scratch::new("legacy-path-file-root");
    let file = scratch.path().join("root-file");
    fs::write(&file, b"unchanged").unwrap();
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    for loose in [false, true] {
        assert_eq!(
            legacy_path::validate_path_within(
                file.to_str().unwrap(),
                file.to_str().unwrap(),
                loose
            ),
            Ok(file.canonicalize().unwrap())
        );
    }
    assert_eq!(
        path_policy::validate_project_root(file.to_str().unwrap()),
        Err("Project path is not a directory".into())
    );
    assert_eq!(
        path_policy::validate_legacy_project_root(file.to_str().unwrap()),
        Err("Project path is not a directory".into())
    );
    assert_eq!(fs::read(&file).unwrap(), b"unchanged");
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}

#[test]
fn missing_leaf_fallback_does_not_create_it_or_change_native_resolution() {
    let scratch = scratch::Scratch::new("legacy-path-absent");
    let root = scratch.path().to_str().unwrap();
    let target = scratch.path().join("absent");
    let before = fs::metadata(scratch.path()).unwrap().modified().unwrap();
    assert_eq!(
        legacy_path::validate_path_within(target.to_str().unwrap(), root, false),
        Err("Path is outside the allowed directory".into())
    );
    assert_eq!(
        legacy_path::validate_path_within(target.to_str().unwrap(), root, true),
        Ok(scratch.path().canonicalize().unwrap().join("absent"))
    );
    let error = target.canonicalize().unwrap_err();
    assert_eq!(
        path_policy::resolve_within(scratch.path(), "absent"),
        Err(format!("cannot resolve path: {error}"))
    );
    assert_eq!(
        fs::metadata(scratch.path()).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
