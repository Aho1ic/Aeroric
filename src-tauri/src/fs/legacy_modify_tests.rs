//! Before/after compatibility for legacy existing-entry guards and actual rename.
//! Exact scratch child, no shell/agent/Trash/UI, no inherited home or PATH.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_MODIFY_CASE";
const ROOT: &str = "AERORIC_LEGACY_MODIFY_ROOT";
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
    let root = Scratch(
        PathBuf::from("/tmp").join(format!("legacy-modify-{}-{nonce}", std::process::id())),
    );
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_modify_tests::{name}"),
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

fn expect_error(target: &Path, project: &Path, error: &str) {
    let target = target.to_str().unwrap();
    let project = project.to_str().unwrap();
    assert_eq!(
        super::validate_existing_path_for_delete(target, project),
        Err(error.to_string())
    );
    assert_eq!(
        super::validate_existing_path_for_modify(target, project, "rename"),
        Err(error.to_string())
    );
}

fn expect_allowed(target: &Path, project: &Path, expected: &Path) {
    let target = target.to_str().unwrap();
    let project = project.to_str().unwrap();
    assert_eq!(
        super::validate_existing_path_for_delete(target, project),
        Ok(expected.to_path_buf())
    );
    assert_eq!(
        super::validate_existing_path_for_modify(target, project, "rename"),
        Ok((
            expected.to_path_buf(),
            Path::new(project).canonicalize().unwrap()
        ))
    );
}

fn rename(target: &Path, name: &str, project: &Path) -> Result<(), String> {
    runtime().block_on(super::rename_path(
        target.to_str().unwrap().to_string(),
        name.to_string(),
        project.to_str().unwrap().to_string(),
    ))
}

fn assert_renamed_only(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    source: &Path,
    destination: &Path,
    replace_link: bool,
) {
    let source = source.strip_prefix(root).unwrap();
    let destination = destination.strip_prefix(root).unwrap();
    if replace_link {
        assert!(expected.remove(destination).unwrap().link.is_some());
    } else {
        assert!(!expected.contains_key(destination));
    }
    let moved: Vec<_> = expected
        .keys()
        .filter(|path| path.starts_with(source))
        .cloned()
        .collect();
    assert!(!moved.is_empty());
    for key in moved {
        let value = expected.remove(&key).unwrap();
        let new = destination.join(key.strip_prefix(source).unwrap());
        assert!(expected.insert(new, value).is_none());
    }
    let after = snapshot(root);
    let parents = BTreeSet::from([
        source.parent().unwrap().to_path_buf(),
        destination.parent().unwrap().to_path_buf(),
    ]);
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_eq!(after, expected);
}

#[test]
fn guards_keep_absolute_name_parent_root_and_containment_error_order() {
    isolated(
        "guards_keep_absolute_name_parent_root_and_containment_error_order",
        |root| {
            let project = root.join("project");
            let missing_root = root.join("missing-root");
            let missing_parent = root.join("missing-parent");
            fs::create_dir(&project).unwrap();
            let before = snapshot(root);
            expect_error(
                Path::new("relative"),
                &missing_root,
                "Path must be absolute",
            );
            expect_error(Path::new("/"), &missing_root, "Invalid file name");
            expect_error(&project.join(".."), &missing_root, "Invalid file name");
            expect_error(
                &missing_parent.join("leaf"),
                &missing_root,
                &format!(
                    "Cannot resolve parent directory: {}",
                    missing_parent.canonicalize().unwrap_err()
                ),
            );
            expect_error(
                &project.join("leaf"),
                &missing_root,
                &format!(
                    "Cannot resolve root directory: {}",
                    missing_root.canonicalize().unwrap_err()
                ),
            );
            expect_error(&root.join("outside-missing"), &project, OUTSIDE);
            // The actual root-target guard fails containment before protection.
            expect_error(&project, &project, OUTSIDE);
            for leaf in ["absent".to_string(), "a\0b".to_string(), "x".repeat(256)] {
                expect_error(&project.join(leaf), &project, "Path does not exist");
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn guards_keep_unresolved_existing_leaves_including_outside_and_broken_links() {
    isolated(
        "guards_keep_unresolved_existing_leaves_including_outside_and_broken_links",
        |root| {
            let project = root.join("project");
            let outside = root.join("outside");
            fs::create_dir(&project).unwrap();
            fs::write(&outside, b"outside remains unchanged").unwrap();
            fs::write(project.join("file"), b"inside remains unchanged").unwrap();
            fs::create_dir(project.join("directory")).unwrap();
            symlink("file", project.join("inside-link")).unwrap();
            symlink(&outside, project.join("outside-link")).unwrap();
            symlink(root.join("not-created"), project.join("dangling-link")).unwrap();
            symlink("loop-link", project.join("loop-link")).unwrap();
            let canonical = project.canonicalize().unwrap();
            let before = snapshot(root);
            for name in [
                "file",
                "directory",
                "inside-link",
                "outside-link",
                "dangling-link",
                "loop-link",
            ] {
                expect_allowed(&project.join(name), &project, &canonical.join(name));
            }
            expect_allowed(
                Path::new(&format!("{}/directory/", project.display())),
                &project,
                &canonical.join("directory"),
            );
            // The strict read guard is deliberately not an equivalent policy.
            assert_eq!(
                super::validate_path_within(
                    project.join("outside-link").to_str().unwrap(),
                    project.to_str().unwrap(),
                    false
                ),
                Err(OUTSIDE.to_string())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn guards_keep_raw_names_relative_roots_file_roots_and_parent_aliases() {
    isolated(
        "guards_keep_raw_names_relative_roots_file_roots_and_parent_aliases",
        |root| {
            let project = root.join(" raw project ");
            let outside = root.join("outside-dir");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::create_dir(project.join("sub")).unwrap();
            for name in [" 原名 ", "source\\name"] {
                fs::write(project.join(name), name.as_bytes()).unwrap();
            }
            fs::write(project.join("sub/file"), b"sub").unwrap();
            fs::write(outside.join("file"), b"outside").unwrap();
            symlink(project.join("sub"), root.join("outside-alias-in")).unwrap();
            symlink(&outside, project.join("inside-alias-out")).unwrap();
            symlink(&project, root.join("root-alias")).unwrap();
            let file_root = root.join("file-root");
            fs::write(&file_root, b"not a directory").unwrap();
            let canonical = project.canonicalize().unwrap();
            let before = snapshot(root);
            for name in [" 原名 ", "source\\name"] {
                expect_allowed(
                    &project.join(name),
                    Path::new(" raw project "),
                    &canonical.join(name),
                );
                expect_allowed(
                    &root.join("root-alias").join(name),
                    &root.join("root-alias"),
                    &canonical.join(name),
                );
            }
            expect_allowed(
                &root.join("outside-alias-in/file"),
                &project,
                &canonical.join("sub/file"),
            );
            expect_allowed(
                &project.join("sub/../ 原名 "),
                &project,
                &canonical.join(" 原名 "),
            );
            expect_error(&project.join("inside-alias-out/file"), &project, OUTSIDE);
            expect_error(&file_root.join("child"), &file_root, "Path does not exist");
            assert_eq!(
                super::validate_entry_name("source\\name"),
                Err("File name contains forbidden characters".into())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn guards_protect_only_first_canonical_segment_after_leaf_metadata() {
    isolated(
        "guards_protect_only_first_canonical_segment_after_leaf_metadata",
        |root| {
            // Separate projects avoid case-insensitive APFS fixture collisions.
            for (index, protected) in [".git", ".GiT", ".aeroric", ".AeRoRiC"]
                .into_iter()
                .enumerate()
            {
                let project = root.join(format!("project-{index}"));
                fs::create_dir(&project).unwrap();
                expect_error(&project.join(protected), &project, "Path does not exist");
                fs::create_dir(project.join(protected)).unwrap();
                fs::write(project.join(protected).join("child"), b"protected").unwrap();
                fs::create_dir_all(project.join("sub").join(protected)).unwrap();
                fs::write(project.join(".git-extra"), b"ordinary").unwrap();
                fs::write(project.join(".aeroric-extra"), b"ordinary").unwrap();
                symlink(protected, project.join("ordinary-link")).unwrap();
                symlink(protected, project.join("parent-alias")).unwrap();
                let before = snapshot(root);
                for target in [
                    project.join(protected),
                    project.join(protected).join("child"),
                    project.join("parent-alias/child"),
                ] {
                    let target = target.to_str().unwrap();
                    let project_text = project.to_str().unwrap();
                    assert_eq!(
                        super::validate_existing_path_for_delete(target, project_text),
                        Err(format!("Cannot delete protected directory: {protected}"))
                    );
                    for action in ["rename", " move 原样 "] {
                        assert_eq!(
                            super::validate_existing_path_for_modify(target, project_text, action),
                            Err(format!("Cannot {action} protected directory: {protected}"))
                        );
                    }
                }
                expect_error(
                    &project.join(protected).join("absent"),
                    &project,
                    "Path does not exist",
                );
                for target in [
                    project.join("sub").join(protected),
                    project.join(".git-extra"),
                    project.join(".aeroric-extra"),
                    project.join("ordinary-link"),
                ] {
                    let expected = target
                        .parent()
                        .unwrap()
                        .canonicalize()
                        .unwrap()
                        .join(target.file_name().unwrap());
                    expect_allowed(&target, &project, &expected);
                }
                assert_eq!(snapshot(root), before);
            }
        },
    );
}

#[test]
fn actual_rename_keeps_trim_modes_directory_contents_and_link_identity() {
    isolated(
        "actual_rename_keeps_trim_modes_directory_contents_and_link_identity",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            fs::write(project.join("referent"), b"inside referent").unwrap();
            fs::write(root.join("outside"), b"outside referent").unwrap();
            for name in [
                " 原名 ",
                "directory",
                "inside",
                "outside",
                "dangling",
                "loop",
            ] {
                let source = project.join(name);
                match name {
                    " 原名 " => {
                        fs::write(&source, b"BOM? no, exact\r\npayload").unwrap();
                        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
                    }
                    "directory" => {
                        fs::create_dir(&source).unwrap();
                        fs::write(source.join("child"), b"retained child").unwrap();
                    }
                    "inside" => symlink("referent", &source).unwrap(),
                    "outside" => symlink(root.join("outside"), &source).unwrap(),
                    "dangling" => symlink(root.join("absent"), &source).unwrap(),
                    "loop" => symlink("loop", &source).unwrap(),
                    _ => unreachable!(),
                }
                let new_name = format!("renamed-{}", name.trim());
                let destination = project.join(&new_name);
                let before = snapshot(root);
                let input = if name == "directory" {
                    PathBuf::from(format!("{}/", source.display()))
                } else {
                    source.clone()
                };
                assert_eq!(rename(&input, &format!("  {new_name}  "), &project), Ok(()));
                assert_renamed_only(root, before, &source, &destination, false);
            }
        },
    );
}

#[test]
fn actual_rename_refusals_are_no_write_with_name_validation_first() {
    isolated(
        "actual_rename_refusals_are_no_write_with_name_validation_first",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let source = project.join("source");
            fs::write(&source, b"source retained").unwrap();
            fs::write(root.join("outside"), b"outside retained").unwrap();
            fs::write(project.join("collision-file"), b"destination retained").unwrap();
            fs::create_dir(project.join("collision-dir")).unwrap();
            fs::create_dir(project.join(".git")).unwrap();
            symlink("source", project.join("collision-link")).unwrap();
            symlink(root.join("outside"), project.join("collision-external")).unwrap();
            let before = snapshot(root);
            for (name, error) in [
                ("  ", "File name cannot be empty"),
                (".", "Invalid file name"),
                ("..", "Invalid file name"),
                ("a/b", "File name contains forbidden characters"),
                ("a\\b", "File name contains forbidden characters"),
                ("a\0b", "File name contains forbidden characters"),
            ] {
                assert_eq!(
                    rename(Path::new("relative"), name, &root.join("missing")),
                    Err(error.to_string())
                );
            }
            assert_eq!(
                rename(
                    Path::new("relative"),
                    &"é".repeat(128),
                    &root.join("missing")
                ),
                Err("File name is too long (max 255 bytes)".into())
            );
            for destination in [
                "source",
                "collision-file",
                "collision-dir",
                "collision-link",
                "collision-external",
            ] {
                assert_eq!(
                    rename(&source, destination, &project),
                    Err(COLLISION.into())
                );
            }
            for (target, error) in [
                (
                    project.join(".git"),
                    "Cannot rename protected directory: .git",
                ),
                (project.join("absent"), "Path does not exist"),
                (root.join("outside"), OUTSIDE),
                (project.clone(), OUTSIDE),
            ] {
                assert_eq!(rename(&target, "unused", &project), Err(error.to_string()));
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_rename_preserves_dangling_destination_and_raw_source_name_quirks() {
    isolated(
        "actual_rename_preserves_dangling_destination_and_raw_source_name_quirks",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            for (index, looping) in [false, true].into_iter().enumerate() {
                let source = project.join(format!("raw\\source-{index}"));
                let destination = project.join(format!("destination-{index}"));
                fs::write(&source, b"replace the link, never its referent").unwrap();
                let referent = if looping {
                    destination.clone()
                } else {
                    root.join("missing-outside-referent")
                };
                symlink(referent, &destination).unwrap();
                assert!(!destination.exists());
                assert!(destination
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink());
                let before = snapshot(root);
                assert_eq!(
                    rename(
                        &source,
                        destination.file_name().unwrap().to_str().unwrap(),
                        &project
                    ),
                    Ok(())
                );
                assert_renamed_only(root, before, &source, &destination, true);
            }
        },
    );
}
