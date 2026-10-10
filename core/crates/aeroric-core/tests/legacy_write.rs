//! Direct legacy write composition cases on scratch fixtures only; no
//! formatter, Trash, clipboard, HOME or settings are touched.

use aeroric_core::files::legacy_write::write_file_content;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "common/scratch.rs"]
mod scratch;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, all: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        all.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
                mode: meta.permissions().mode(),
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
                visit(root, &entry.unwrap().path(), all);
            }
        }
    }
    let mut all = BTreeMap::new();
    visit(root, root, &mut all);
    all
}

fn write(project: &Path, file: &Path, content: &str) -> Result<(), String> {
    write_file_content(project.to_str().unwrap(), file.to_str().unwrap(), content)
}

// Independent fixture spelling, not a call to either production layout helper.
fn history(project: &Path, relative: &str) -> PathBuf {
    let hex: String = relative
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    project.join(".aeroric/local-history").join(hex)
}

fn fixture(scratch: &scratch::Scratch) -> (PathBuf, PathBuf) {
    let project = scratch.path().join("project with spaces");
    let file = project.join("src/app-日本語.ts");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"current\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    (project, file)
}

fn assert_changes(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    added: &[PathBuf],
    writes: &[(PathBuf, &[u8])],
) {
    let after = snapshot(root);
    let mut parents = BTreeSet::new();
    for path in added {
        let path = path.strip_prefix(root).unwrap();
        assert!(!expected.contains_key(path), "only explicitly new entries");
        expected.insert(path.to_path_buf(), after[path].clone());
        parents.insert(path.parent().unwrap().to_path_buf());
    }
    for (path, bytes) in writes {
        let path = path.strip_prefix(root).unwrap();
        let old = expected.get_mut(path).unwrap();
        assert!(old.bytes.is_some() && old.link.is_none());
        old.bytes = Some(bytes.to_vec());
        old.modified = after[path].modified;
    }
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_eq!(
        after, expected,
        "all other bytes/modes/links/entries/mtimes stay"
    );
}

fn new_snapshot_paths(
    root: &Path,
    before: &BTreeMap<PathBuf, Entry>,
    directory: &Path,
    content: &[u8],
) -> Vec<PathBuf> {
    let mut added = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if before.contains_key(path.strip_prefix(root).unwrap()) {
            continue;
        }
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("txt"));
        assert_eq!(fs::read(&path).unwrap(), content);
        assert!(path.symlink_metadata().unwrap().is_file());
        files.push(path);
    }
    assert_eq!(files.len(), 1, "exactly one new snapshot entry");
    added.push(files.pop().unwrap());
    let mut next = directory;
    while !before.contains_key(next.strip_prefix(root).unwrap()) {
        assert!(next.symlink_metadata().unwrap().is_dir());
        added.push(next.to_path_buf());
        next = next.parent().unwrap();
    }
    added
}

#[test]
fn refusals_leave_no_trace() {
    let scratch = scratch::Scratch::new("legacy-write-refusals");
    let root = scratch.path();
    let (project, file) = fixture(&scratch);
    let outside = root.join("outside.txt");
    fs::write(&outside, b"outside retained").unwrap();
    let escaping = project.join("escaping.ts");
    symlink(&outside, &escaping).unwrap();
    let missing = project.join("missing.ts");
    let missing_project = root.join("missing-project");
    let directory = project.join("src");
    let before = snapshot(root);

    let relative_target = PathBuf::from("relative\0file");
    let cases: [(&PathBuf, &PathBuf, &str); 5] = [
        (&relative_target, &project, "Path must be absolute"),
        (
            &file,
            &missing_project,
            "Cannot resolve root directory: No such file or directory (os error 2)",
        ),
        (&missing, &project, "Path is outside the allowed directory"),
        (&outside, &project, "Path is outside the allowed directory"),
        (&escaping, &project, "Path is outside the allowed directory"),
    ];
    for (target, project_path, expected) in cases {
        let err = write(project_path, target, "new content\n").unwrap_err();
        assert_eq!(err, expected, "{target:?}");
    }
    let err = write(&project, &directory, "new content\n").unwrap_err();
    assert_eq!(err, "File path is not a file");

    assert!(
        !project.join(".aeroric").exists(),
        "no history side effects"
    );
    assert_eq!(fs::read(&file).unwrap(), b"current\n");
    assert_eq!(fs::read(&outside).unwrap(), b"outside retained");
    assert_changes(root, before, &[], &[]);
}

#[test]
fn success_snapshots_prior_content_and_writes_canonical_target() {
    let scratch = scratch::Scratch::new("legacy-write-success");
    let root = scratch.path();
    let (project, file) = fixture(&scratch);
    let before = snapshot(root);

    let indirect = project.join("src").join("..").join("src/app-日本語.ts");
    write(&project, &indirect, "next\n").unwrap();

    let dir = history(&project, "src/app-日本語.ts");
    let added = new_snapshot_paths(root, &before, &dir, b"current\n");
    assert_changes(root, before, &added, &[(file.clone(), b"next\n")]);
    assert_eq!(
        file.symlink_metadata().unwrap().permissions().mode() & 0o777,
        0o640,
        "mode stays"
    );

    let link = project.join("src/alias.ts");
    symlink(&file, &link).unwrap();
    let before = snapshot(root);
    write(&project, &link, "third\n").unwrap();

    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_link(&link).unwrap(), file);
    let added = new_snapshot_paths(root, &before, &dir, b"next\n");
    assert_changes(root, before, &added, &[(file.clone(), b"third\n")]);
}

#[test]
fn content_vocabulary_truncate_and_grow() {
    let scratch = scratch::Scratch::new("legacy-write-content");
    let root = scratch.path();
    let (project, file) = fixture(&scratch);
    let dir = history(&project, "src/app-日本語.ts");
    let mut before = snapshot(root);
    let mut previous: Vec<u8> = b"current\n".to_vec();

    let contents: Vec<String> = vec![
        String::new(),
        "\r\n".to_string(),
        "\u{feff}bom\n".to_string(),
        "多行\n中文内容\n".to_string(),
        "x".repeat(4096),
    ];
    for content in contents {
        write(&project, &file, &content).unwrap();
        let added = new_snapshot_paths(root, &before, &dir, &previous);
        assert_changes(
            root,
            before.clone(),
            &added,
            &[(file.clone(), content.as_bytes())],
        );
        previous = content.into_bytes();
        before = snapshot(root);
    }
    write(&project, &file, "shrunk\n").unwrap();
    let added = new_snapshot_paths(root, &before, &dir, &previous);
    assert_changes(root, before.clone(), &added, &[(file.clone(), b"shrunk\n")]);
    assert_eq!(fs::read(&file).unwrap(), b"shrunk\n");
}

#[test]
fn snapshot_failure_refuses_write() {
    let scratch = scratch::Scratch::new("legacy-write-snapshot-failure");
    let root = scratch.path();
    let (project, file) = fixture(&scratch);
    fs::write(project.join(".aeroric"), b"blocker").unwrap();
    let before = snapshot(root);

    let err = write(&project, &file, "new content\n").unwrap_err();
    assert!(
        err.contains("Not a directory") || err.contains("File exists"),
        "unexpected snapshot error: {err}"
    );
    assert_eq!(fs::read(&file).unwrap(), b"current\n");
    assert_changes(root, before, &[], &[]);
}

#[test]
fn write_failure_keeps_snapshot_and_original_bytes() {
    let scratch = scratch::Scratch::new("legacy-write-write-failure");
    let root = scratch.path();
    let (project, file) = fixture(&scratch);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o440)).unwrap();
    let before = snapshot(root);

    let err = write(&project, &file, "new content\n").unwrap_err();
    assert!(err.contains("Permission denied"), "unexpected error: {err}");
    assert_eq!(fs::read(&file).unwrap(), b"current\n");
    let dir = history(&project, "src/app-日本語.ts");
    let added = new_snapshot_paths(root, &before, &dir, b"current\n");
    assert_changes(root, before, &added, &[]);
}
