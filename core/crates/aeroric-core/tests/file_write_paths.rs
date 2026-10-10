#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use aeroric_core::files::path_policy::MAX_FILE_BYTES;
use aeroric_core::files::read_write::{create_file, write_file_content as write};

#[path = "common/scratch.rs"]
mod scratch;
use scratch::Scratch;

fn fixture(label: &str) -> (Scratch, PathBuf, PathBuf) {
    let scratch = Scratch::new(label);
    let root = scratch.path().join("project");
    let outside = scratch.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    for path in [root.join("target.txt"), outside.join("sentinel.txt")] {
        fs::write(&path, b"original\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    (scratch, root, outside)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn outside_unchanged(outside: &Path) {
    assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
    assert_eq!(
        fs::read(outside.join("sentinel.txt")).unwrap(),
        b"original\n"
    );
    assert_eq!(mode(&outside.join("sentinel.txt")), 0o640);
}

#[test]
fn existing_outside_write_leaf_cannot_change_its_referent_or_link() {
    let (_scratch, root, outside) = fixture("leaf-existing");
    let target = outside.join("sentinel.txt");
    symlink(&target, root.join("alias.txt")).unwrap();
    assert!(write(&root, "alias.txt", "changed")
        .unwrap_err()
        .contains("escapes"));
    assert_eq!(fs::read_link(root.join("alias.txt")).unwrap(), target);
    outside_unchanged(&outside);
}

#[test]
fn dangling_write_leaves_inside_and_outside_are_not_missing_directory_entries() {
    let (_scratch, root, outside) = fixture("leaf-dangling");
    for (name, target) in [
        ("outside-link", outside.join("not-created")),
        ("inside-link", root.join("not-created")),
    ] {
        symlink(&target, root.join(name)).unwrap();
        assert!(write(&root, name, "must not create").is_err(), "{name}");
        assert_eq!(fs::read_link(root.join(name)).unwrap(), target);
        assert!(!target.exists());
    }
    outside_unchanged(&outside);
}

#[test]
fn resolved_leaf_and_parent_aliases_keep_links_modes_and_exact_content() {
    let (_scratch, root, outside) = fixture("leaf-aliases");
    symlink("target.txt", root.join("leaf-alias")).unwrap();
    let content = "\u{feff}你好\r\n";
    write(&root, "leaf-alias", content).unwrap();
    assert_eq!(
        fs::read(root.join("target.txt")).unwrap(),
        content.as_bytes()
    );
    assert_eq!(mode(&root.join("target.txt")), 0o640);
    assert_eq!(
        fs::read_link(root.join("leaf-alias")).unwrap(),
        Path::new("target.txt")
    );
    fs::create_dir(root.join("real")).unwrap();
    symlink("real", root.join("parent-alias")).unwrap();
    // Preserve the writer's existing backslash normalization on Unix, too.
    write(&root, "parent-alias\\文件.txt", content).unwrap();
    assert_eq!(
        fs::read(root.join("real/文件.txt")).unwrap(),
        content.as_bytes()
    );
    assert_eq!(
        fs::read_link(root.join("parent-alias")).unwrap(),
        Path::new("real")
    );
    outside_unchanged(&outside);
}

#[test]
fn create_refuses_every_existing_entry_without_truncating_or_following_links() {
    let (_scratch, root, outside) = fixture("leaf-exclusive");
    fs::create_dir(root.join("directory")).unwrap();
    for (name, target) in [
        ("inside-link", root.join("target.txt")),
        ("outside-link", outside.join("sentinel.txt")),
        ("inside-dangling", root.join("not-created")),
        ("outside-dangling", outside.join("not-created")),
    ] {
        symlink(&target, root.join(name)).unwrap();
        assert_eq!(create_file(&root, name).unwrap_err(), "file already exists");
        assert_eq!(fs::read_link(root.join(name)).unwrap(), target);
    }
    for name in ["target.txt", "directory"] {
        assert_eq!(create_file(&root, name).unwrap_err(), "file already exists");
    }
    assert_eq!(fs::read(root.join("target.txt")).unwrap(), b"original\n");
    assert_eq!(mode(&root.join("target.txt")), 0o640);
    assert!(!root.join("not-created").exists());
    outside_unchanged(&outside);
}

#[test]
fn direct_new_files_empty_overwrites_and_size_before_path_order_are_preserved() {
    let (_scratch, root, outside) = fixture("leaf-direct");
    fs::write(root.join("mode-control"), b"").unwrap();
    create_file(&root, "new.txt").unwrap();
    assert_eq!(fs::read(root.join("new.txt")).unwrap(), b"");
    assert_eq!(
        mode(&root.join("new.txt")),
        mode(&root.join("mode-control"))
    );
    write(&root, "written.txt", "\u{feff}line\r\n").unwrap();
    assert_eq!(
        fs::read(root.join("written.txt")).unwrap(),
        "\u{feff}line\r\n".as_bytes()
    );
    assert_eq!(
        mode(&root.join("written.txt")),
        mode(&root.join("mode-control"))
    );
    write(&root, "target.txt", "").unwrap();
    assert_eq!(fs::read(root.join("target.txt")).unwrap(), b"");
    assert_eq!(mode(&root.join("target.txt")), 0o640);
    // The RPC frame limit is smaller; test the file-size guard directly in core.
    let big = "x".repeat((MAX_FILE_BYTES + 1) as usize);
    let error = write(&root.join("absent-root"), "../invalid", &big).unwrap_err();
    assert!(error.contains("byte limit"), "{error}");
    assert!(!root.join("absent-root").exists());
    outside_unchanged(&outside);
}

#[test]
fn lexical_and_parent_escape_refusals_do_not_create_or_modify_files() {
    let (_scratch, root, outside) = fixture("leaf-parent");
    symlink(&outside, root.join("outside-dir")).unwrap();
    for relative in [
        "../escape",
        "./target.txt",
        "",
        "missing/leaf",
        "outside-dir/new",
    ] {
        assert!(
            write(&root, relative, "must not write").is_err(),
            "{relative}"
        );
        assert!(create_file(&root, relative).is_err(), "{relative}");
    }
    assert!(!root.join("missing").exists());
    assert_eq!(fs::read(root.join("target.txt")).unwrap(), b"original\n");
    outside_unchanged(&outside);
}
