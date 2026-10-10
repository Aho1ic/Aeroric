//! Legacy rename uses scratch entries only; no OS Trash or clipboard calls.

use aeroric_core::files::legacy_modify;
#[cfg(unix)]
use aeroric_core::files::read_write;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "common/scratch.rs"]
mod scratch;

const COLLISION: &str = "A file or folder with that name already exists";
const OUTSIDE: &str = "Path is outside the allowed directory";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    directory: bool,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let meta = path.symlink_metadata().unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
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

fn rename(source: &Path, name: &str, project: &Path) -> Result<(), String> {
    legacy_modify::rename_path(
        source.to_str().unwrap().to_string(),
        name.to_string(),
        project.to_str().unwrap().to_string(),
    )
}

fn assert_renamed_only(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    source: &Path,
    destination: &Path,
    replace_entry: bool,
) {
    let source = source.strip_prefix(root).unwrap();
    let destination = destination.strip_prefix(root).unwrap();
    if replace_entry {
        let entry = expected.remove(destination).unwrap();
        assert!(entry.bytes.is_some() || entry.link.is_some());
    } else {
        assert!(!expected.contains_key(destination));
    }
    let moved: Vec<_> = expected
        .keys()
        .filter(|path| path.starts_with(source))
        .cloned()
        .collect();
    assert!(!moved.is_empty());
    for old in moved {
        let entry = expected.remove(&old).unwrap();
        let new = destination.join(old.strip_prefix(source).unwrap());
        assert!(expected.insert(new, entry).is_none());
    }
    let after = snapshot(root);
    for parent in [source.parent().unwrap(), destination.parent().unwrap()] {
        expected.get_mut(parent).unwrap().modified = after[parent].modified;
    }
    assert_eq!(after, expected);
}

#[test]
fn rename_preserves_name_before_source_errors_protection_and_no_write_collisions() {
    let scratch = scratch::Scratch::new("legacy-rename-order");
    let root = scratch.path();
    let project = root.join("project");
    let source = project.join("source");
    let missing = root.join("missing-root");
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::write(&source, b"source retained").unwrap();
    fs::write(project.join(".git/child"), b"protected retained").unwrap();
    fs::write(project.join("occupied-file"), b"destination retained").unwrap();
    fs::create_dir(project.join("occupied-directory")).unwrap();
    fs::write(root.join("outside"), b"outside retained").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("source", project.join("occupied-link")).unwrap();
        symlink(root.join("outside"), project.join("occupied-outside-link")).unwrap();
    }
    let before = snapshot(root);
    // The relative spelling includes NUL, so it cannot name a real cwd entry
    // even if a future regression bypasses admission.
    let relative = Path::new("relative\0source");
    for (name, error) in [
        ("  ", "File name cannot be empty"),
        (".", "Invalid file name"),
        ("..", "Invalid file name"),
        (" a/b ", "File name contains forbidden characters"),
        ("a\\b", "File name contains forbidden characters"),
        ("a\0b", "File name contains forbidden characters"),
    ] {
        assert_eq!(rename(relative, name, &missing), Err(error.into()));
    }
    assert_eq!(
        rename(relative, &"é".repeat(128), &missing),
        Err("File name is too long (max 255 bytes)".into())
    );
    assert_eq!(
        rename(relative, "valid", &missing),
        Err("Path must be absolute".into())
    );
    assert_eq!(
        rename(&source, "valid", &missing),
        Err(format!(
            "Cannot resolve root directory: {}",
            missing.canonicalize().unwrap_err()
        ))
    );
    for name in ["source", "occupied-file", "occupied-directory"] {
        assert_eq!(rename(&source, name, &project), Err(COLLISION.into()));
    }
    for (path, error) in [
        (
            project.join(".git"),
            "Cannot rename protected directory: .git",
        ),
        (
            project.join(".git/child"),
            "Cannot rename protected directory: .git",
        ),
        (project.join(".git/absent"), "Path does not exist"),
        (project.join("absent"), "Path does not exist"),
        (root.join("outside"), OUTSIDE),
        (project.clone(), OUTSIDE),
    ] {
        assert_eq!(rename(&path, "unused", &project), Err(error.into()));
    }
    #[cfg(unix)]
    for name in ["occupied-link", "occupied-outside-link"] {
        assert_eq!(rename(&source, name, &project), Err(COLLISION.into()));
    }
    assert_eq!(snapshot(root), before);
}

#[test]
fn rename_keeps_trim_raw_source_names_modes_directory_children_and_aliases() {
    let scratch = scratch::Scratch::new("legacy-rename-success");
    let root = scratch.path();
    let project = root.join("project with spaces");
    fs::create_dir_all(project.join("sub")).unwrap();
    let file = project.join(" leading-日本語.txt");
    fs::write(&file, b"\0\xef\xbb\xbfexact\r\npayload").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let before = snapshot(root);
    assert_eq!(rename(&file, "  renamed-日本語.txt  ", &project), Ok(()));
    assert_renamed_only(
        root,
        before,
        &file,
        &project.join("renamed-日本語.txt"),
        false,
    );
    let directory = project.join("directory");
    fs::create_dir_all(directory.join("child/empty")).unwrap();
    fs::write(directory.join("child/payload"), b"child remains").unwrap();
    let before = snapshot(root);
    let input = PathBuf::from(format!("{}/", directory.display()));
    assert_eq!(rename(&input, " moved-directory ", &project), Ok(()));
    assert_renamed_only(
        root,
        before,
        &directory,
        &project.join("moved-directory"),
        false,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let source = project.join("sub/raw\\source");
        fs::write(&source, b"old name is not a new name").unwrap();
        symlink(project.join("sub"), root.join("outside-alias-in")).unwrap();
        symlink(&project, root.join("root-alias")).unwrap();
        let before = snapshot(root);
        assert_eq!(
            rename(
                &root.join("outside-alias-in/raw\\source"),
                " from-alias ",
                &root.join("root-alias")
            ),
            Ok(())
        );
        assert_renamed_only(
            root,
            before,
            &source,
            &project.join("sub/from-alias"),
            false,
        );
    }
}

#[test]
fn rename_preserves_unfollowed_source_leaf_identity_including_outside_and_broken_links() {
    let scratch = scratch::Scratch::new("legacy-rename-leaves");
    let root = scratch.path();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let plain = project.join("plain");
    fs::write(&plain, b"plain entry").unwrap();
    let before = snapshot(root);
    assert_eq!(rename(&plain, "moved-plain", &project), Ok(()));
    assert_renamed_only(root, before, &plain, &project.join("moved-plain"), false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(project.join("referent"), b"inside stays").unwrap();
        fs::write(root.join("outside-referent"), b"outside stays").unwrap();
        symlink("referent", project.join("inside-link")).unwrap();
        symlink(root.join("outside-referent"), project.join("outside-link")).unwrap();
        symlink(root.join("absent-referent"), project.join("dangling")).unwrap();
        symlink("loop", project.join("loop")).unwrap();
        let before = snapshot(root);
        assert!(read_write::rename_path(&project, "outside-link", "native-unused").is_err());
        assert_eq!(snapshot(root), before);
        for name in ["inside-link", "outside-link", "dangling", "loop"] {
            let source = project.join(name);
            let new_name = format!("renamed-{name}");
            let before = snapshot(root);
            assert_eq!(rename(&source, &new_name, &project), Ok(()));
            assert_renamed_only(root, before, &source, &project.join(new_name), false);
        }
    }
}

#[test]
fn rename_keeps_dangling_destination_entry_replacement_distinct_from_native_overwrite() {
    let scratch = scratch::Scratch::new("legacy-rename-destination");
    let root = scratch.path();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let source = project.join("source");
    let occupied = project.join("occupied");
    fs::write(&source, b"legacy source retained").unwrap();
    fs::write(&occupied, b"legacy destination retained").unwrap();
    let before = snapshot(root);
    assert_eq!(rename(&source, "occupied", &project), Err(COLLISION.into()));
    assert_eq!(snapshot(root), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let native = project.join("native-source");
        fs::write(&native, b"native overwrite is a different policy").unwrap();
        let before = snapshot(root);
        assert_eq!(
            read_write::rename_path(&project, "native-source", "occupied"),
            Ok(())
        );
        assert_renamed_only(root, before, &native, &occupied, true);
        for (index, looping) in [false, true].into_iter().enumerate() {
            let source = project.join(format!("raw\\source-{index}"));
            let name = format!("destination-{index}");
            let destination = project.join(&name);
            let referent = if looping {
                destination.clone()
            } else {
                root.join("outside-project-but-still-scratch")
            };
            fs::write(&source, b"replace the link, never its referent").unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
            symlink(&referent, &destination).unwrap();
            assert!(!destination.exists());
            assert!(destination
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink());
            let before = snapshot(root);
            assert_eq!(rename(&source, &name, &project), Ok(()));
            assert_renamed_only(root, before, &source, &destination, true);
        }
    }
}
