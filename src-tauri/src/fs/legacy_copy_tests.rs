//! Before/after compatibility for legacy recursive-copy helpers and actual clipboard copy.
//! Exact scratch child, no shell/agent/Trash/UI, no inherited home or PATH.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_COPY_CASE";
const ROOT: &str = "AERORIC_LEGACY_COPY_ROOT";
const COLLISION: &str = "A file or folder with that name already exists";
const OUTSIDE: &str = "Path is outside the allowed directory";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only this scratch tree");
    }
}

fn isolated(name: &str, run: impl FnOnce(&Path)) {
    if std::env::var(CASE).as_deref() == Ok(name) {
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        assert_eq!(PathBuf::from(std::env::var_os("HOME").unwrap()), root);
        assert_eq!(
            PathBuf::from(std::env::var_os("PATH").unwrap()),
            root.join("bin")
        );
        assert_eq!(
            std::env::current_dir().unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
        run(&root);
        assert!(
            !root.join("data").exists(),
            "no settings/history initialization"
        );
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 0);
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        Scratch(PathBuf::from("/tmp").join(format!("legacy-copy-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_copy_tests::{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(CASE, name)
        .env(ROOT, &root.0)
        .env("HOME", &root.0)
        .env("USERPROFILE", &root.0)
        .env("AERORIC_HOME", root.0.join("data"))
        .env("PATH", root.0.join("bin"))
        .env("SHELL", "/bin/sh")
        .env("TMPDIR", &root.0)
        .current_dir(&root.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !timed_out
            && output.status.success()
            && stdout.contains("running 1 test")
            && stdout.contains("1 passed; 0 failed; 0 ignored"),
        "{name}: {}, timeout={timed_out}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

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

fn command(sources: &[&Path], target: &Path, project: &Path) -> Result<(), String> {
    runtime().block_on(super::copy_paths_to_directory(
        sources
            .iter()
            .map(|path| path.to_str().unwrap().to_string())
            .collect(),
        target.to_str().unwrap().to_string(),
        project.to_str().unwrap().to_string(),
    ))
}

fn directory_mode(root: &Path) -> u32 {
    let oracle = root.join("directory-mode-oracle");
    fs::create_dir(&oracle).unwrap();
    fs::metadata(oracle).unwrap().permissions().mode()
}

fn assert_copied_only(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    source: &Path,
    destination: &Path,
    directory_mode: u32,
    replace_file: bool,
) {
    let source = source.strip_prefix(root).unwrap();
    let destination = destination.strip_prefix(root).unwrap();
    if replace_file {
        assert!(expected.remove(destination).unwrap().bytes.is_some());
    } else {
        assert!(!expected.contains_key(destination));
    }
    let copied: Vec<_> = expected
        .iter()
        .filter(|(path, _)| path.starts_with(source))
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect();
    assert!(!copied.is_empty());
    let after = snapshot(root);
    let mut parents = BTreeSet::new();
    for (key, mut entry) in copied {
        let new = destination.join(key.strip_prefix(source).unwrap());
        if entry.bytes.is_none() && entry.link.is_none() {
            entry.mode = directory_mode;
        }
        // New/copied entry mtimes are not promised to equal source mtimes.
        entry.modified = after.get(&new).unwrap().modified;
        assert!(expected.insert(new.clone(), entry).is_none());
        parents.insert(new.parent().unwrap().to_path_buf());
    }
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_eq!(after, expected);
}

#[test]
fn self_copy_guard_keeps_non_directory_fast_return_and_canonical_order() {
    isolated(
        "self_copy_guard_keeps_non_directory_fast_return_and_canonical_order",
        |root| {
            let source = root.join("source");
            let child = source.join("child");
            let other = root.join("other");
            let missing = root.join("missing");
            fs::create_dir_all(&child).unwrap();
            fs::create_dir(&other).unwrap();
            fs::write(root.join("file"), b"file").unwrap();
            symlink(&source, root.join("source-alias")).unwrap();
            symlink(&child, root.join("target-alias")).unwrap();
            symlink(&missing, root.join("dangling")).unwrap();
            let before = snapshot(root);
            for input in [root.join("file"), missing.clone(), root.join("dangling")] {
                assert_eq!(
                    super::ensure_copy_target_not_inside_source(&input, &missing),
                    Ok(())
                );
            }
            assert_eq!(
                super::ensure_copy_target_not_inside_source(Path::new("relative"), Path::new("\0")),
                Ok(())
            );
            for target in [&source, &child, &root.join("target-alias")] {
                assert_eq!(
                    super::ensure_copy_target_not_inside_source(&source, target),
                    Err("Cannot copy a folder into itself".into())
                );
            }
            assert_eq!(
                super::ensure_copy_target_not_inside_source(&root.join("source-alias"), &child),
                Err("Cannot copy a folder into itself".into())
            );
            assert_eq!(
                super::ensure_copy_target_not_inside_source(&source, &missing),
                Err(format!(
                    "Cannot resolve paste target: {}",
                    missing.canonicalize().unwrap_err()
                ))
            );
            assert_eq!(
                super::ensure_copy_target_not_inside_source(&source, &other),
                Ok(())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn recursive_copy_keeps_tree_bytes_link_text_file_modes_and_default_directory_modes() {
    isolated(
        "recursive_copy_keeps_tree_bytes_link_text_file_modes_and_default_directory_modes",
        |root| {
            let mode = directory_mode(root);
            let source = root.join("source");
            let destination = root.join("destination");
            fs::create_dir_all(source.join("nested/empty")).unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
            fs::set_permissions(source.join("nested"), fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(
                source.join("nested/ 日本語.txt"),
                b"\0\xef\xbb\xbfexact\r\nbytes",
            )
            .unwrap();
            fs::set_permissions(
                source.join("nested/ 日本語.txt"),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
            fs::write(
                root.join("outside"),
                b"outside must not be copied or changed",
            )
            .unwrap();
            symlink("nested/ 日本語.txt", source.join("file-link")).unwrap();
            symlink("nested", source.join("directory-link")).unwrap();
            symlink(root.join("outside"), source.join("outside-link")).unwrap();
            symlink("never-created", source.join("dangling")).unwrap();
            symlink("loop", source.join("loop")).unwrap();
            let before = snapshot(root);
            assert_eq!(super::copy_path_recursive(&source, &destination), Ok(()));
            assert_copied_only(root, before, &source, &destination, mode, false);
        },
    );
}

#[test]
fn recursive_io_keeps_raw_errors_collision_split_and_direct_file_overwrite() {
    isolated(
        "recursive_io_keeps_raw_errors_collision_split_and_direct_file_overwrite",
        |root| {
            let mode = directory_mode(root);
            let file = root.join("file");
            let directory = root.join("directory");
            let occupied = root.join("occupied");
            let link = root.join("link");
            let missing = root.join("missing");
            fs::write(&file, b"new bytes\r\n").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
            fs::write(&occupied, b"old bytes").unwrap();
            fs::create_dir(&directory).unwrap();
            symlink("file", &link).unwrap();
            let before = snapshot(root);
            assert_eq!(
                super::copy_path_recursive(&missing, &root.join("unused")),
                Err(missing.symlink_metadata().unwrap_err().to_string())
            );
            assert_eq!(
                super::copy_path_recursive(&directory, &occupied),
                Err(COLLISION.into())
            );
            let symlink_error = symlink("oracle", &occupied).unwrap_err().to_string();
            assert_eq!(
                super::copy_path_recursive(&link, &occupied),
                Err(symlink_error)
            );
            let directory_error = fs::copy(&file, &directory).unwrap_err().to_string();
            assert_eq!(
                super::copy_path_recursive(&file, &directory),
                Err(directory_error)
            );
            let absent_parent_target = missing.join("file");
            let parent_error = fs::copy(&file, &absent_parent_target)
                .unwrap_err()
                .to_string();
            assert_eq!(
                super::copy_path_recursive(&file, &absent_parent_target),
                Err(parent_error)
            );
            assert_eq!(snapshot(root), before);
            // The low-level helper is not the caller's exclusive destination check.
            assert_eq!(super::copy_path_recursive(&file, &occupied), Ok(()));
            assert_copied_only(root, before, &file, &occupied, mode, true);
        },
    );
}

#[test]
fn actual_copy_keeps_empty_target_source_name_collision_and_self_copy_order() {
    isolated(
        "actual_copy_keeps_empty_target_source_name_collision_and_self_copy_order",
        |root| {
            let project = root.join("project");
            let target = project.join("target");
            let source_dir = project.join("source-dir");
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(source_dir.join("child")).unwrap();
            fs::create_dir(source_dir.join("free-child")).unwrap();
            fs::write(
                source_dir.join("child/source-dir"),
                b"collision beats self-copy",
            )
            .unwrap();
            fs::write(root.join("file"), b"source").unwrap();
            fs::write(project.join("not-directory"), b"not a directory").unwrap();
            fs::write(root.join("bad\\name"), b"invalid new-entry name").unwrap();
            fs::write(target.join("file"), b"occupied").unwrap();
            fs::create_dir(root.join("outside-dir")).unwrap();
            symlink(root.join("outside-dir"), project.join("outside-target")).unwrap();
            symlink(root.join("missing"), root.join("dangling")).unwrap();
            symlink("loop", root.join("loop")).unwrap();
            symlink(&source_dir, project.join("source-link")).unwrap();
            let before = snapshot(root);
            assert_eq!(
                command(&[], Path::new("relative"), &root.join("missing")),
                Ok(())
            );
            assert_eq!(
                command(&[Path::new("relative")], Path::new("relative"), &project),
                Err("Path must be absolute".into())
            );
            assert_eq!(
                command(
                    &[Path::new("relative")],
                    &project.join("not-directory"),
                    &project
                ),
                Err("Paste target must be a directory".into())
            );
            assert_eq!(
                command(
                    &[Path::new("relative")],
                    &project.join("outside-target"),
                    &project
                ),
                Err(OUTSIDE.into())
            );
            assert_eq!(
                command(&[Path::new("relative")], &target, Path::new("project")),
                Err("Source path must be absolute".into())
            );
            for source in [
                root.join("missing"),
                root.join("dangling"),
                root.join("loop"),
            ] {
                assert_eq!(
                    command(&[&source], &target, &project),
                    Err(format!("Source path does not exist: {}", source.display()))
                );
            }
            assert_eq!(
                command(&[&project.join("..")], &target, &project),
                Err("Invalid source file name".into())
            );
            assert_eq!(
                command(&[&root.join("bad\\name")], &target, &project),
                Err("File name contains forbidden characters".into())
            );
            assert_eq!(
                command(&[&root.join("file")], &target, &project),
                Err(COLLISION.into())
            );
            assert_eq!(
                command(&[&source_dir], &source_dir.join("child"), &project),
                Err(COLLISION.into())
            );
            for source in [&source_dir, &project.join("source-link")] {
                assert_eq!(
                    command(&[source], &source_dir.join("free-child"), &project),
                    Err("Cannot copy a folder into itself".into())
                );
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_copy_keeps_external_raw_names_alias_targets_and_sequential_partial_progress() {
    isolated(
        "actual_copy_keeps_external_raw_names_alias_targets_and_sequential_partial_progress",
        |root| {
            let mode = directory_mode(root);
            let project = root.join("project");
            let target = project.join("target");
            let external = root.join("external-sources");
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(external.join("tree/nested")).unwrap();
            fs::write(external.join(" 原名 "), b"external raw-name bytes").unwrap();
            fs::write(external.join("tree/nested/payload"), b"payload").unwrap();
            symlink("absent", external.join("tree/dangling")).unwrap();
            symlink("tree", external.join("directory-link")).unwrap();
            symlink(&target, project.join("target-alias")).unwrap();
            for name in [" 原名 ", "tree", "directory-link"] {
                let source = external.join(name);
                let destination = target.join(name);
                let before = snapshot(root);
                assert_eq!(
                    command(&[&source], &project.join("target-alias"), &project),
                    Ok(())
                );
                assert_copied_only(root, before, &source, &destination, mode, false);
            }
            let first = external.join("first");
            let last = external.join("last");
            fs::write(&first, b"first survives later refusal").unwrap();
            fs::write(&last, b"last is never copied").unwrap();
            let before = snapshot(root);
            assert_eq!(
                command(&[&first, Path::new("relative"), &last], &target, &project),
                Err("Source path must be absolute".into())
            );
            assert_copied_only(root, before, &first, &target.join("first"), mode, false);
            let duplicate = external.join("duplicate");
            fs::write(&duplicate, b"one copy before duplicate collision").unwrap();
            let before = snapshot(root);
            assert_eq!(
                command(&[&duplicate, &duplicate], &target, &project),
                Err(COLLISION.into())
            );
            assert_copied_only(
                root,
                before,
                &duplicate,
                &target.join("duplicate"),
                mode,
                false,
            );
        },
    );
}

#[test]
fn actual_copy_preserves_dangling_destination_kind_specific_behavior() {
    isolated(
        "actual_copy_preserves_dangling_destination_kind_specific_behavior",
        |root| {
            let mode = directory_mode(root);
            let project = root.join("project");
            let target = project.join("target");
            let source = root.join("source");
            let outside = root.join("outside-project-but-still-scratch");
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(source.join("directory")).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(
                source.join("file"),
                b"legacy follows a dangling destination",
            )
            .unwrap();
            fs::set_permissions(source.join("file"), fs::Permissions::from_mode(0o640)).unwrap();
            fs::write(source.join("loop-file"), b"loop must fail without writing").unwrap();
            symlink("file", source.join("link")).unwrap();
            for name in ["file", "directory", "link"] {
                symlink(outside.join(name), target.join(name)).unwrap();
            }
            symlink("loop-file", target.join("loop-file")).unwrap();
            let before = snapshot(root);
            assert!(!target.join("file").exists());
            assert_eq!(command(&[&source.join("file")], &target, &project), Ok(()));
            // Observe the old side effect only on an explicitly owned scratch referent.
            assert_copied_only(
                root,
                before,
                &source.join("file"),
                &outside.join("file"),
                mode,
                false,
            );
            let before = snapshot(root);
            assert_eq!(
                command(&[&source.join("directory")], &target, &project),
                Err(COLLISION.into())
            );
            let link_error = symlink("oracle", target.join("link"))
                .unwrap_err()
                .to_string();
            assert_eq!(
                command(&[&source.join("link")], &target, &project),
                Err(link_error)
            );
            let loop_error = fs::copy(source.join("loop-file"), target.join("loop-file"))
                .unwrap_err()
                .to_string();
            assert_eq!(
                command(&[&source.join("loop-file")], &target, &project),
                Err(loop_error)
            );
            assert_eq!(snapshot(root), before);
        },
    );
}
