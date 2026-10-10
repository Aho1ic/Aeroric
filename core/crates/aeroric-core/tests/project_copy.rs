#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

use aeroric_core::files::path_policy::validate_directory_destination;
use aeroric_core::files::read_write::copy_project_paths_to_directory as copy;

#[path = "common/scratch.rs"]
mod scratch;
use scratch::Scratch;

fn fixture(label: &str) -> (Scratch, PathBuf, PathBuf) {
    let scratch = Scratch::new(label);
    let root = scratch.path().join("project");
    let outside = scratch.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(root.join("source.bin"), [0, 1, 255, 2]).unwrap();
    fs::set_permissions(root.join("source.bin"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::write(outside.join("sentinel"), "outside").unwrap();
    (scratch, root, outside)
}

#[test]
fn directory_validation_is_read_only_and_rejects_outside_and_dangling_ancestors() {
    let (_scratch, root, outside) = fixture("copy-ancestors");
    symlink(&outside, root.join("linked")).unwrap();
    symlink(outside.join("absent"), root.join("dangling")).unwrap();
    for relative in ["linked", "linked/new/nested", "dangling", "dangling/new"] {
        assert!(
            validate_directory_destination(&root, &root.join(relative)).is_err(),
            "{relative}"
        );
        assert!(copy(&root, relative, &["source.bin".into()]).is_err());
    }
    validate_directory_destination(&root, &root.join("missing/nested")).unwrap();
    assert!(!root.join("missing").exists(), "validation does not mkdir");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
}

#[test]
fn nested_in_root_links_preserve_path_spelling_binary_bytes_and_modes() {
    let (_scratch, root, _outside) = fixture("copy-inroot");
    fs::create_dir(root.join("real")).unwrap();
    symlink(root.join("real"), root.join("alias")).unwrap();
    let copied = copy(&root, "alias/new/nested", &["source.bin".into()]).unwrap();
    let expected = root.join("alias/new/nested/source.bin");
    assert_eq!(copied, [expected.to_string_lossy()]);
    assert_eq!(fs::read(&expected).unwrap(), [0, 1, 255, 2]);
    assert_eq!(
        fs::metadata(&expected).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read(root.join("real/new/nested/source.bin")).unwrap(),
        [0, 1, 255, 2]
    );
    fs::write(&expected, b"keep").unwrap();
    assert!(copy(&root, "alias/new/nested", &["source.bin".into()])
        .unwrap_err()
        .contains("Refusing to overwrite"));
    assert_eq!(fs::read(&expected).unwrap(), b"keep");
}

#[test]
fn dangling_leaf_is_preserved_and_source_symlinks_cannot_escape() {
    let (_scratch, root, outside) = fixture("copy-leaf");
    fs::create_dir(root.join("out")).unwrap();
    let leaf = root.join("out/source.bin");
    let missing = outside.join("must-not-exist");
    symlink(&missing, &leaf).unwrap();
    assert!(copy(&root, "out", &["source.bin".into()])
        .unwrap_err()
        .contains("Refusing to overwrite"));
    assert_eq!(fs::read_link(&leaf).unwrap(), missing);
    assert!(!missing.exists());
    symlink(outside.join("sentinel"), root.join("source-link")).unwrap();
    assert!(copy(&root, "source-refusal", &["source-link".into()]).is_err());
    assert!(
        root.join("source-refusal").is_dir(),
        "existing create-before-source order"
    );
    assert_eq!(
        fs::read_dir(root.join("source-refusal")).unwrap().count(),
        0
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[test]
fn lexical_targets_and_later_source_failures_keep_the_original_partial_policy() {
    let (_scratch, root, outside) = fixture("copy-partial");
    for target in ["../outside", "/outside", "", "./out"] {
        assert!(copy(&root, target, &["source.bin".into()]).is_err());
    }
    assert!(copy(
        &root,
        "partial/nested",
        &["source.bin".into(), "../bad".into()]
    )
    .is_err());
    assert_eq!(
        fs::read(root.join("partial/nested/source.bin")).unwrap(),
        [0, 1, 255, 2]
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}
