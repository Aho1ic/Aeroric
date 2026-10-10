//! Before/after compatibility for legacy name/new-path policy and actual creation.
//! Exact scratch child, no shell/agent/Trash/UI, no inherited home or PATH.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_ENTRY_CASE";
const ROOT: &str = "AERORIC_LEGACY_ENTRY_ROOT";
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
        Scratch(PathBuf::from("/tmp").join(format!("legacy-entry-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_entry_tests::{name}"),
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

fn create(path: &Path, project: &Path, directory: bool) -> Result<(), String> {
    let path = path.to_str().unwrap().to_string();
    let project = project.to_str().unwrap().to_string();
    let rt = runtime();
    if directory {
        rt.block_on(super::create_directory(path, project))
    } else {
        rt.block_on(super::create_file(path, project))
    }
}

fn new_path(path: &Path, project: &Path) -> Result<(PathBuf, String), String> {
    super::validate_new_path_within(path.to_str().unwrap(), project.to_str().unwrap())
}

fn assert_created_only(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    created: &[(&Path, bool, u32)],
) {
    let after = snapshot(root);
    let mut parents = BTreeSet::new();
    for (path, directory, mode) in created {
        let key = path.strip_prefix(root).unwrap().to_path_buf();
        assert!(!expected.contains_key(&key));
        let entry = after.get(&key).unwrap();
        assert_eq!(entry.mode & 0o777, *mode);
        assert!(entry.link.is_none());
        assert_eq!(fs::symlink_metadata(path).unwrap().is_dir(), *directory);
        assert_eq!(
            entry.bytes,
            if *directory { None } else { Some(Vec::new()) }
        );
        expected.insert(key.clone(), entry.clone());
        parents.insert(key.parent().unwrap().to_path_buf());
    }
    // Creation may change only the immediate containing directories' mtime.
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_eq!(after, expected);
}

fn oracle_modes(root: &Path) -> (u32, u32) {
    let file = root.join("mode-oracle-file");
    let directory = root.join("mode-oracle-dir");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .unwrap();
    fs::create_dir(&directory).unwrap();
    (
        fs::metadata(file).unwrap().permissions().mode() & 0o777,
        fs::metadata(directory).unwrap().permissions().mode() & 0o777,
    )
}

#[test]
fn names_keep_byte_limits_error_order_and_unix_rules() {
    isolated(
        "names_keep_byte_limits_error_order_and_unix_rules",
        |root| {
            let before = snapshot(root);
            for (name, error) in [
                ("", "File name cannot be empty"),
                (".", "Invalid file name"),
                ("..", "Invalid file name"),
                ("a/b", "File name contains forbidden characters"),
                ("a\\b", "File name contains forbidden characters"),
                ("a\0b", "File name contains forbidden characters"),
            ] {
                assert_eq!(
                    super::validate_entry_name(name).err().as_deref(),
                    Some(error)
                );
            }
            for name in [
                "a".repeat(256),
                "é".repeat(128),
                format!("/{}", "a".repeat(255)),
            ] {
                assert_eq!(
                    super::validate_entry_name(&name).err().as_deref(),
                    Some("File name is too long (max 255 bytes)")
                );
            }
            for name in ["a".repeat(255), "中".repeat(85)] {
                assert_eq!(name.len(), 255);
                assert_eq!(super::validate_entry_name(&name), Ok(()));
            }
            for name in [
                "CON",
                "nul.txt",
                "COM0",
                "lpt9.log",
                " trailing ",
                "trailing.",
                "a:b",
                "a?b",
                "a*b",
                "a<b",
                "a>b",
                "a|b",
                "a\"b",
                "a\u{1}b",
                ".git",
                ".aeroric",
                " ",
            ] {
                assert_eq!(super::validate_entry_name(name), Ok(()), "{name:?}");
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn new_path_keeps_name_parent_root_and_containment_error_order() {
    isolated(
        "new_path_keeps_name_parent_root_and_containment_error_order",
        |root| {
            let project = root.join("project");
            let missing = root.join("missing-root");
            let bad_parent = root.join("missing-parent");
            fs::create_dir(&project).unwrap();
            let before = snapshot(root);
            assert_eq!(
                new_path(Path::new("relative"), &missing).err().as_deref(),
                Some("Path must be absolute")
            );
            assert_eq!(
                new_path(Path::new("/"), &missing).err().as_deref(),
                Some("Invalid file name")
            );
            for leaf in ["bad\\name", "bad\0name"] {
                assert_eq!(
                    new_path(&bad_parent.join(leaf), &missing).err().as_deref(),
                    Some("File name contains forbidden characters")
                );
            }
            assert_eq!(
                new_path(&bad_parent.join("new"), &missing).err(),
                Some(format!(
                    "Cannot resolve parent directory: {}",
                    bad_parent.canonicalize().unwrap_err()
                ))
            );
            assert_eq!(
                new_path(&project.join("new"), &missing).err(),
                Some(format!(
                    "Cannot resolve root directory: {}",
                    missing.canonicalize().unwrap_err()
                ))
            );
            assert_eq!(
                new_path(&root.join("outside"), &project).err().as_deref(),
                Some(OUTSIDE)
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn new_path_keeps_raw_leaf_relative_file_roots_and_canonical_parent() {
    isolated(
        "new_path_keeps_raw_leaf_relative_file_roots_and_canonical_parent",
        |root| {
            let project = root.join(" project 中 ");
            fs::create_dir(&project).unwrap();
            fs::create_dir(project.join("sub")).unwrap();
            let file_root = root.join("file-root");
            fs::write(&file_root, b"file root").unwrap();
            let outside = root.join("outside");
            fs::write(&outside, b"outside").unwrap();
            symlink(&outside, project.join("leaf")).unwrap();
            let before = snapshot(root);
            let canonical = project.canonicalize().unwrap();
            for supplied_root in [project.as_path(), Path::new(" project 中 ")] {
                assert_eq!(
                    new_path(&project.join("sub/../ raw name "), supplied_root).unwrap(),
                    (canonical.clone(), " raw name ".to_string())
                );
                assert_eq!(
                    new_path(&project.join("looks-like-dir/"), supplied_root).unwrap(),
                    (canonical.clone(), "looks-like-dir".to_string())
                );
            }
            assert_eq!(
                new_path(&project.join("leaf"), &project).unwrap(),
                (canonical, "leaf".into())
            );
            assert_eq!(
                new_path(&file_root.join("child"), &file_root).unwrap(),
                (file_root.canonicalize().unwrap(), "child".into())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_creation_keeps_exact_names_exclusivity_and_default_modes() {
    isolated(
        "actual_creation_keeps_exact_names_exclusivity_and_default_modes",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            fs::write(project.join("sentinel"), b"unchanged").unwrap();
            fs::set_permissions(project.join("sentinel"), fs::Permissions::from_mode(0o640))
                .unwrap();
            let (file_mode, dir_mode) = oracle_modes(root);
            let file = project.join(" spaced 中 .txt ");
            let folder = project.join(" folder 中 ");
            let slash_file = project.join("looks-like-dir");
            let git = project.join(".git");
            let before = snapshot(root);
            assert_eq!(create(&file, &project, false), Ok(()));
            assert_eq!(create(&folder, &project, true), Ok(()));
            assert_eq!(
                create(&project.join("looks-like-dir/"), &project, false),
                Ok(())
            );
            assert_eq!(create(&git, &project, true), Ok(()));
            assert_created_only(
                root,
                before,
                &[
                    (&file, false, file_mode),
                    (&folder, true, dir_mode),
                    (&slash_file, false, file_mode),
                    (&git, true, dir_mode),
                ],
            );
        },
    );
}

#[test]
fn actual_creation_refuses_every_existing_leaf_without_following_links() {
    isolated(
        "actual_creation_refuses_every_existing_leaf_without_following_links",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            fs::write(project.join("file"), b"keep bytes").unwrap();
            fs::set_permissions(project.join("file"), fs::Permissions::from_mode(0o640)).unwrap();
            fs::create_dir(project.join("directory")).unwrap();
            fs::write(project.join("directory/child"), b"keep child").unwrap();
            fs::write(root.join("outside"), b"outside bytes").unwrap();
            symlink("file", project.join("inside-link")).unwrap();
            symlink(root.join("outside"), project.join("outside-link")).unwrap();
            symlink(root.join("never-created"), project.join("dangling")).unwrap();
            symlink("loop", project.join("loop")).unwrap();
            let before = snapshot(root);
            for name in [
                "file",
                "directory",
                "inside-link",
                "outside-link",
                "dangling",
                "loop",
            ] {
                for directory in [false, true] {
                    assert_eq!(
                        create(&project.join(name), &project, directory)
                            .err()
                            .as_deref(),
                        Some(COLLISION),
                        "{name}/{directory}"
                    );
                    assert_eq!(snapshot(root), before);
                }
            }
        },
    );
}

#[test]
fn actual_creation_keeps_guard_and_raw_io_refusals_without_repair() {
    isolated(
        "actual_creation_keeps_guard_and_raw_io_refusals_without_repair",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let file_root = root.join("file-root");
            fs::write(&file_root, b"not a directory").unwrap();
            let absent_parent = project.join("absent");
            let before = snapshot(root);
            for directory in [false, true] {
                assert_eq!(
                    create(Path::new("relative"), &root.join("bad-root"), directory)
                        .err()
                        .as_deref(),
                    Some("Path must be absolute")
                );
                assert_eq!(
                    create(
                        &absent_parent.join("bad\\name"),
                        &root.join("bad-root"),
                        directory
                    )
                    .err()
                    .as_deref(),
                    Some("File name contains forbidden characters")
                );
                assert_eq!(
                    create(&root.join("outside"), &project, directory)
                        .err()
                        .as_deref(),
                    Some(OUTSIDE)
                );
                assert_eq!(
                    create(&absent_parent.join("new"), &project, directory).err(),
                    Some(format!(
                        "Cannot resolve parent directory: {}",
                        absent_parent.canonicalize().unwrap_err()
                    ))
                );
                let target = file_root.join("child");
                let raw_error = if directory {
                    fs::create_dir(&target).unwrap_err().to_string()
                } else {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .unwrap_err()
                        .to_string()
                };
                assert_eq!(create(&target, &file_root, directory), Err(raw_error));
                assert_eq!(snapshot(root), before);
            }
        },
    );
}

#[test]
fn actual_creation_keeps_parent_alias_policy_and_relative_root() {
    isolated(
        "actual_creation_keeps_parent_alias_policy_and_relative_root",
        |root| {
            let project = root.join("project");
            let inner = project.join("inner");
            let outside = root.join("outside");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&inner).unwrap();
            fs::create_dir(&outside).unwrap();
            symlink(&inner, project.join("in-alias")).unwrap();
            symlink(&outside, project.join("out-alias")).unwrap();
            let (file_mode, dir_mode) = oracle_modes(root);
            let before = snapshot(root);
            for directory in [false, true] {
                assert_eq!(
                    create(&project.join("out-alias/refused"), &project, directory)
                        .err()
                        .as_deref(),
                    Some(OUTSIDE)
                );
            }
            assert_eq!(snapshot(root), before);
            let file = inner.join(" created ");
            let folder = inner.join(" folder ");
            assert_eq!(
                create(
                    &project.join("in-alias/ created "),
                    Path::new("project"),
                    false
                ),
                Ok(())
            );
            assert_eq!(
                create(&project.join("in-alias/ folder "), &project, true),
                Ok(())
            );
            assert_created_only(
                root,
                before,
                &[(&file, false, file_mode), (&folder, true, dir_mode)],
            );
        },
    );
}

#[test]
fn actual_rename_trim_and_copy_raw_names_keep_shared_name_policy() {
    isolated(
        "actual_rename_trim_and_copy_raw_names_keep_shared_name_policy",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let old = project.join("old");
            fs::write(&old, b"rename payload").unwrap();
            fs::set_permissions(&old, fs::Permissions::from_mode(0o640)).unwrap();
            let source = root.join(" raw copy ");
            fs::write(&source, b"copy payload").unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
            let bad_source = root.join("bad\\name");
            fs::write(&bad_source, b"bad-name source").unwrap();
            let before = snapshot(root);
            let rt = runtime();
            assert_eq!(
                rt.block_on(super::rename_path(
                    old.to_str().unwrap().into(),
                    " bad/name ".into(),
                    root.join("bad-root").to_str().unwrap().into()
                ))
                .err()
                .as_deref(),
                Some("File name contains forbidden characters")
            );
            assert_eq!(
                rt.block_on(super::copy_paths_to_directory(
                    vec![bad_source.to_str().unwrap().into()],
                    project.to_str().unwrap().into(),
                    project.to_str().unwrap().into()
                ))
                .err()
                .as_deref(),
                Some("File name contains forbidden characters")
            );
            assert_eq!(snapshot(root), before);
            assert_eq!(
                rt.block_on(super::rename_path(
                    old.to_str().unwrap().into(),
                    " renamed ".into(),
                    project.to_str().unwrap().into()
                )),
                Ok(())
            );
            let renamed = project.join("renamed");
            let after_rename = snapshot(root);
            let mut expected = before;
            let old_entry = expected.remove(old.strip_prefix(root).unwrap()).unwrap();
            expected.insert(renamed.strip_prefix(root).unwrap().to_path_buf(), old_entry);
            expected.get_mut(Path::new("project")).unwrap().modified =
                after_rename[Path::new("project")].modified;
            assert_eq!(after_rename, expected);

            assert_eq!(
                rt.block_on(super::copy_paths_to_directory(
                    vec![source.to_str().unwrap().into()],
                    project.to_str().unwrap().into(),
                    project.to_str().unwrap().into()
                )),
                Ok(())
            );
            let after_copy = snapshot(root);
            let copied = project.join(" raw copy ");
            let key = copied.strip_prefix(root).unwrap();
            let entry = &after_copy[key];
            assert_eq!(entry.bytes.as_deref(), Some(b"copy payload".as_slice()));
            assert_eq!(entry.mode, expected[Path::new(" raw copy ")].mode);
            assert!(entry.link.is_none());
            expected.insert(key.to_path_buf(), entry.clone());
            expected.get_mut(Path::new("project")).unwrap().modified =
                after_copy[Path::new("project")].modified;
            assert_eq!(after_copy, expected);
        },
    );
}
