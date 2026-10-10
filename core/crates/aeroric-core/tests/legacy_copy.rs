//! Legacy helper IO is not the native project-copy admission contract.

use aeroric_core::files::{legacy_copy, read_write};
use std::fs;

#[path = "common/scratch.rs"]
mod scratch;

const COLLISION: &str = "A file or folder with that name already exists";

#[test]
fn self_copy_guard_preserves_non_directory_short_circuit_and_containment() {
    let scratch = scratch::Scratch::new("legacy-copy-self");
    let source = scratch.path().join("source");
    let child = source.join("child");
    let other = scratch.path().join("other");
    let missing = scratch.path().join("missing");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&other).unwrap();
    let file = scratch.path().join("file");
    fs::write(&file, b"unchanged").unwrap();
    let before = fs::metadata(scratch.path()).unwrap().modified().unwrap();
    for input in [&file, &missing] {
        assert_eq!(
            legacy_copy::ensure_copy_target_not_inside_source(input, &missing),
            Ok(())
        );
    }
    for target in [&source, &child] {
        assert_eq!(
            legacy_copy::ensure_copy_target_not_inside_source(&source, target),
            Err("Cannot copy a folder into itself".into())
        );
    }
    assert_eq!(
        legacy_copy::ensure_copy_target_not_inside_source(&source, &other),
        Ok(())
    );
    assert_eq!(
        legacy_copy::ensure_copy_target_not_inside_source(&source, &missing),
        Err(format!(
            "Cannot resolve paste target: {}",
            missing.canonicalize().unwrap_err()
        ))
    );
    assert_eq!(fs::read(&file).unwrap(), b"unchanged");
    assert_eq!(
        fs::metadata(scratch.path()).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn recursive_copy_keeps_bytes_raw_errors_and_directory_permission_policy() {
    let scratch = scratch::Scratch::new("legacy-copy-io");
    let source = scratch.path().join("source");
    let destination = scratch.path().join("destination");
    let oracle = scratch.path().join("directory-oracle");
    fs::create_dir_all(source.join("child/empty")).unwrap();
    fs::create_dir(&oracle).unwrap();
    let source_file = source.join("child/payload");
    fs::write(&source_file, b"\0\xef\xbb\xbfexact\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&source_file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let source_time = fs::metadata(&source_file).unwrap().modified().unwrap();
    assert_eq!(
        legacy_copy::copy_path_recursive(&source, &destination),
        Ok(())
    );
    assert_eq!(
        fs::read(destination.join("child/payload")).unwrap(),
        b"\0\xef\xbb\xbfexact\r\n"
    );
    assert!(destination.join("child/empty").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode(),
            fs::metadata(&oracle).unwrap().permissions().mode()
        );
        assert_eq!(
            fs::metadata(destination.join("child/payload"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    assert_eq!(
        legacy_copy::copy_path_recursive(&source, &destination),
        Err(COLLISION.into())
    );
    let missing = scratch.path().join("missing");
    assert_eq!(
        legacy_copy::copy_path_recursive(&missing, &scratch.path().join("unused")),
        Err(missing.symlink_metadata().unwrap_err().to_string())
    );
    assert_eq!(fs::read(&source_file).unwrap(), b"\0\xef\xbb\xbfexact\r\n");
    assert_eq!(
        fs::metadata(&source_file).unwrap().modified().unwrap(),
        source_time
    );
    assert!(!scratch.path().join("unused").exists());
}

#[test]
fn recursive_links_keep_text_and_never_recurse_into_the_referent() {
    let scratch = scratch::Scratch::new("legacy-copy-links");
    let source = scratch.path().join("source");
    let destination = scratch.path().join("destination");
    fs::create_dir_all(source.join("directory")).unwrap();
    fs::write(source.join("plain"), b"plain").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(scratch.path().join("outside"), b"outside untouched").unwrap();
        symlink("plain", source.join("file-link")).unwrap();
        symlink("directory", source.join("dir-link")).unwrap();
        symlink(scratch.path().join("outside"), source.join("outside-link")).unwrap();
        symlink("not-created", source.join("dangling")).unwrap();
        symlink("loop", source.join("loop")).unwrap();
    }
    assert_eq!(
        legacy_copy::copy_path_recursive(&source, &destination),
        Ok(())
    );
    assert_eq!(fs::read(destination.join("plain")).unwrap(), b"plain");
    #[cfg(unix)]
    {
        for name in ["file-link", "dir-link", "outside-link", "dangling", "loop"] {
            assert_eq!(
                fs::read_link(destination.join(name)).unwrap(),
                fs::read_link(source.join(name)).unwrap()
            );
            assert!(destination
                .join(name)
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink());
        }
        assert_eq!(
            fs::read(scratch.path().join("outside")).unwrap(),
            b"outside untouched"
        );
        assert!(!source.join("not-created").exists());
        assert!(!destination.join("not-created").exists());
    }
    assert_eq!(fs::read(source.join("plain")).unwrap(), b"plain");
}

#[test]
fn legacy_low_level_file_copy_is_not_native_exclusive_destination_admission() {
    let scratch = scratch::Scratch::new("legacy-copy-native");
    let project = scratch.path().join("project");
    let target = project.join("target");
    fs::create_dir_all(&target).unwrap();
    let source = project.join("source.txt");
    let destination = target.join("source.txt");
    fs::write(&source, b"new bytes").unwrap();
    fs::write(&destination, b"old bytes").unwrap();
    let before = fs::metadata(&source).unwrap().modified().unwrap();
    assert!(read_write::copy_project_paths_to_directory(
        &project,
        "target",
        &["source.txt".into()]
    )
    .unwrap_err()
    .contains("the target already exists"));
    assert_eq!(fs::read(&destination).unwrap(), b"old bytes");
    assert_eq!(
        legacy_copy::copy_path_recursive(&source, &destination),
        Ok(())
    );
    assert_eq!(fs::read(&destination).unwrap(), b"new bytes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(&destination).unwrap();
        let referent = scratch.path().join("outside-project-but-scratch");
        symlink(&referent, &destination).unwrap();
        assert!(read_write::copy_project_paths_to_directory(
            &project,
            "target",
            &["source.txt".into()]
        )
        .unwrap_err()
        .contains("the target already exists"));
        assert!(!referent.exists());
        assert_eq!(fs::read_link(&destination).unwrap(), referent);
        assert_eq!(
            legacy_copy::copy_path_recursive(&source, &destination),
            Ok(())
        );
        assert_eq!(fs::read(&referent).unwrap(), b"new bytes");
        assert_eq!(fs::read_link(&destination).unwrap(), referent);
    }
    assert_eq!(fs::read(&source).unwrap(), b"new bytes");
    assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), before);
}

// Slice50: the ordered loop receives a target; it does not admit a project root.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LoopEntry {
    directory: bool,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn loop_snapshot(root: &Path) -> BTreeMap<PathBuf, LoopEntry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, LoopEntry>) {
        let meta = path.symlink_metadata().unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            LoopEntry {
                directory: meta.is_dir(),
                readonly: meta.permissions().readonly(),
                #[cfg(unix)]
                mode: {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode()
                },
                modified: meta.modified().unwrap(),
                bytes: meta.is_file().then(|| fs::read(path).unwrap()),
                link: meta
                    .file_type()
                    .is_symlink()
                    .then(|| fs::read_link(path).unwrap()),
            },
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn copy_loop(sources: &[&Path], target: &Path) -> Result<(), String> {
    legacy_copy::copy_paths_to_directory(
        sources
            .iter()
            .map(|path| path.to_str().unwrap().to_string())
            .collect(),
        target.to_path_buf(),
    )
}

fn assert_loop_copied_only(
    root: &Path,
    mut expected: BTreeMap<PathBuf, LoopEntry>,
    source: &Path,
    destination: &Path,
    directory_oracle: &Path,
) {
    let source = source.strip_prefix(root).unwrap();
    let destination = destination.strip_prefix(root).unwrap();
    assert!(!expected.contains_key(destination));
    let oracle = expected[directory_oracle.strip_prefix(root).unwrap()].clone();
    assert!(oracle.directory);
    let copied: Vec<_> = expected
        .iter()
        .filter(|(path, _)| path.starts_with(source))
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect();
    assert!(!copied.is_empty());
    let after = loop_snapshot(root);
    let mut parents = BTreeSet::new();
    for (key, mut entry) in copied {
        let new = destination.join(key.strip_prefix(source).unwrap());
        if entry.directory {
            entry.readonly = oracle.readonly;
            #[cfg(unix)]
            {
                entry.mode = oracle.mode;
            }
        }
        entry.modified = after[&new].modified;
        assert!(expected.insert(new.clone(), entry).is_none());
        parents.insert(new.parent().unwrap().to_path_buf());
    }
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_eq!(after, expected);
}

#[test]
fn copy_loop_preserves_target_source_name_collision_and_self_copy_order() {
    let scratch = scratch::Scratch::new("legacy-copy-loop-order");
    let root = scratch.path();
    let target = root.join("target");
    let source = root.join("source");
    let missing = root.join("missing");
    let file = root.join("file");
    fs::create_dir(&target).unwrap();
    fs::create_dir_all(source.join("child")).unwrap();
    fs::create_dir(source.join("free-child")).unwrap();
    fs::write(&file, b"source").unwrap();
    fs::write(target.join("file"), b"occupied").unwrap();
    fs::write(source.join("child/source"), b"collision before self-copy").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(root.join("bad\\name"), b"invalid basename").unwrap();
        symlink(&missing, root.join("dangling")).unwrap();
        symlink("loop", root.join("loop")).unwrap();
        symlink(&source, root.join("source-link")).unwrap();
    }
    let before = loop_snapshot(root);
    // Only the actual Tauri command succeeds before validating an empty request.
    for invalid_target in [&file, &missing] {
        for sources in [vec![], vec![Path::new("relative")]] {
            assert_eq!(
                copy_loop(&sources, invalid_target),
                Err("Paste target must be a directory".into())
            );
        }
    }
    assert_eq!(copy_loop(&[], &target), Ok(()));
    assert_eq!(
        copy_loop(&[Path::new("relative")], &target),
        Err("Source path must be absolute".into())
    );
    for missing_source in [&missing, &root.join("bad\0name")] {
        assert_eq!(
            copy_loop(&[missing_source], &target),
            Err(format!(
                "Source path does not exist: {}",
                missing_source.display()
            ))
        );
    }
    assert_eq!(
        copy_loop(&[&source.join("..")], &target),
        Err("Invalid source file name".into())
    );
    assert_eq!(copy_loop(&[&file], &target), Err(COLLISION.into()));
    assert_eq!(
        copy_loop(&[&source], &source.join("child")),
        Err(COLLISION.into())
    );
    assert_eq!(
        copy_loop(&[&source], &source.join("free-child")),
        Err("Cannot copy a folder into itself".into())
    );
    #[cfg(unix)]
    {
        for name in ["dangling", "loop"] {
            let path = root.join(name);
            assert_eq!(
                copy_loop(&[&path], &target),
                Err(format!("Source path does not exist: {}", path.display()))
            );
        }
        assert_eq!(
            copy_loop(&[&root.join("bad\\name")], &target),
            Err("File name contains forbidden characters".into())
        );
        assert_eq!(
            copy_loop(&[&root.join("source-link")], &source.join("free-child")),
            Err("Cannot copy a folder into itself".into())
        );
    }
    assert_eq!(loop_snapshot(root), before);
}

#[test]
fn copy_loop_keeps_raw_external_sources_alias_targets_and_recursive_link_entries() {
    let scratch = scratch::Scratch::new("legacy-copy-loop-raw");
    let root = scratch.path();
    let project = root.join("project");
    let target = project.join("target");
    let external = root.join("external-sources");
    let oracle = root.join("directory-oracle");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(external.join("tree/nested/empty")).unwrap();
    fs::create_dir(&oracle).unwrap();
    fs::write(external.join(" leading-日本語.txt"), b"\0raw\r\nbytes").unwrap();
    fs::write(external.join("tree/nested/payload"), b"tree payload").unwrap();
    #[cfg(unix)]
    let (target_path, names) = {
        use std::os::unix::fs::{symlink, PermissionsExt};
        fs::set_permissions(external.join("tree"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(
            external.join("tree/nested/payload"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        fs::write(external.join(" 原名 "), b"no trim").unwrap();
        fs::write(root.join("outside-referent"), b"untouched referent").unwrap();
        symlink("absent", external.join("tree/dangling")).unwrap();
        symlink("loop", external.join("tree/loop")).unwrap();
        symlink(
            root.join("outside-referent"),
            external.join("tree/outside-link"),
        )
        .unwrap();
        symlink("tree", external.join("directory-link")).unwrap();
        symlink(&target, project.join("target-alias")).unwrap();
        (
            project.join("target-alias"),
            vec![" leading-日本語.txt", "tree", " 原名 ", "directory-link"],
        )
    };
    #[cfg(not(unix))]
    let (target_path, names) = (target.clone(), vec![" leading-日本語.txt", "tree"]);
    for name in names {
        let source = external.join(name);
        let before = loop_snapshot(root);
        assert_eq!(copy_loop(&[&source], &target_path), Ok(()));
        assert_loop_copied_only(root, before, &source, &target.join(name), &oracle);
    }
}

#[test]
fn copy_loop_preserves_source_order_and_partial_success_without_prevalidation_or_rollback() {
    let scratch = scratch::Scratch::new("legacy-copy-loop-partial");
    let root = scratch.path();
    let target = root.join("target");
    let oracle = root.join("directory-oracle");
    let first = root.join("first");
    let last = root.join("last");
    let duplicate = root.join("duplicate");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&oracle).unwrap();
    fs::write(&first, b"first survives later refusal").unwrap();
    fs::write(&last, b"last must not be copied").unwrap();
    fs::write(&duplicate, b"first duplicate wins").unwrap();
    let before = loop_snapshot(root);
    assert_eq!(
        copy_loop(&[&first, Path::new("relative"), &last], &target),
        Err("Source path must be absolute".into())
    );
    assert_loop_copied_only(root, before, &first, &target.join("first"), &oracle);
    let before = loop_snapshot(root);
    assert_eq!(
        copy_loop(&[&duplicate, &duplicate, &last], &target),
        Err(COLLISION.into())
    );
    assert_loop_copied_only(root, before, &duplicate, &target.join("duplicate"), &oracle);
    let before = loop_snapshot(root);
    assert_eq!(copy_loop(&[&first, &last], &target), Err(COLLISION.into()));
    assert_eq!(loop_snapshot(root), before);
}

#[test]
fn copy_loop_retains_kind_specific_dangling_destination_effects_not_native_authority() {
    let scratch = scratch::Scratch::new("legacy-copy-loop-leaf");
    let root = scratch.path();
    let project = root.join("project");
    let target = project.join("target");
    let oracle = root.join("directory-oracle");
    let source = project.join("file");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir(&oracle).unwrap();
    fs::write(&source, b"legacy source bytes").unwrap();
    fs::write(target.join("file"), b"occupied unchanged").unwrap();
    let before = loop_snapshot(root);
    assert_eq!(copy_loop(&[&source], &target), Err(COLLISION.into()));
    assert!(
        read_write::copy_project_paths_to_directory(&project, "target", &["file".into()])
            .unwrap_err()
            .contains("the target already exists")
    );
    assert_eq!(loop_snapshot(root), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let outside = root.join("outside-project-but-still-scratch");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(project.join("directory")).unwrap();
        fs::write(project.join("loop-file"), b"loop fails").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        symlink("file", project.join("link")).unwrap();
        fs::remove_file(target.join("file")).unwrap();
        for name in ["file", "directory", "link"] {
            symlink(outside.join(name), target.join(name)).unwrap();
        }
        symlink("loop-file", target.join("loop-file")).unwrap();
        let before = loop_snapshot(root);
        assert!(
            read_write::copy_project_paths_to_directory(&project, "target", &["file".into()])
                .unwrap_err()
                .contains("the target already exists")
        );
        assert_eq!(loop_snapshot(root), before);
        assert_eq!(copy_loop(&[&source], &target), Ok(()));
        assert_loop_copied_only(root, before, &source, &outside.join("file"), &oracle);
        let before = loop_snapshot(root);
        assert_eq!(
            copy_loop(&[&project.join("directory")], &target),
            Err(COLLISION.into())
        );
        let link_error = symlink("oracle", target.join("link"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            copy_loop(&[&project.join("link")], &target),
            Err(link_error)
        );
        let loop_error = fs::copy(project.join("loop-file"), target.join("loop-file"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            copy_loop(&[&project.join("loop-file")], &target),
            Err(loop_error)
        );
        assert_eq!(loop_snapshot(root), before);
    }
}
