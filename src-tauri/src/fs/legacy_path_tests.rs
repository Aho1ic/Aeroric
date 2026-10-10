//! Before/after compatibility for the actual legacy guard and its callers.
//! Every child runs only its own case in scratch cwd/HOME with an empty PATH.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_PATH_CASE";
const ROOT: &str = "AERORIC_LEGACY_PATH_ROOT";
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
        Scratch(PathBuf::from("/tmp").join(format!("legacy-path-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_path_tests::{name}"),
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

// Keep the actual fs symbol before and after extraction; never substitute a model.
fn guard(target: &Path, root: &str, loose: bool) -> Result<PathBuf, String> {
    super::validate_path_within(target.to_str().unwrap(), root, loose)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
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

#[test]
fn target_absolute_check_precedes_original_root_errors() {
    isolated(
        "target_absolute_check_precedes_original_root_errors",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let missing_root = root.join("missing-root");
            let target = project.join("missing-leaf");
            let before = snapshot(root);
            for loose in [false, true] {
                for target in ["", "relative", "../relative", "."] {
                    assert_eq!(
                        guard(Path::new(target), missing_root.to_str().unwrap(), loose),
                        Err("Path must be absolute".into())
                    );
                }
                for allowed in [missing_root.clone(), root.join("nul\0root")] {
                    let expected = format!(
                        "Cannot resolve root directory: {}",
                        allowed.canonicalize().unwrap_err()
                    );
                    assert_eq!(
                        guard(&target, allowed.to_str().unwrap(), loose),
                        Err(expected)
                    );
                }
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn canonical_fast_path_keeps_dot_alias_relative_and_file_roots() {
    isolated(
        "canonical_fast_path_keeps_dot_alias_relative_and_file_roots",
        |root| {
            let project = root.join(" project 中 ");
            fs::create_dir_all(project.join("nested")).unwrap();
            let file = project.join("kept.txt");
            fs::write(&file, b"exact\r\nbytes").unwrap();
            // An outside spelling may resolve back inside; do not add lexical-prefix checks.
            let outside_alias = root.join("outside-alias");
            symlink(&project, &outside_alias).unwrap();
            let before = snapshot(root);
            for loose in [false, true] {
                for spelling in [
                    file.clone(),
                    project.join("nested/../kept.txt"),
                    project.join("./kept.txt"),
                    outside_alias.join("kept.txt"),
                ] {
                    assert_eq!(
                        guard(&spelling, " project 中 ", loose),
                        Ok(file.canonicalize().unwrap())
                    );
                }
                assert_eq!(
                    guard(&project, outside_alias.to_str().unwrap(), loose),
                    Ok(project.canonicalize().unwrap())
                );
                assert_eq!(
                    guard(&file, file.to_str().unwrap(), loose),
                    Ok(file.canonicalize().unwrap()),
                    "the old guard does not require the root to be a directory"
                );
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn strict_mode_keeps_one_error_for_missing_dangling_and_escape() {
    isolated(
        "strict_mode_keeps_one_error_for_missing_dangling_and_escape",
        |root| {
            let project = root.join("project");
            let outside = root.join("project-sibling");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("file"), b"outside fixture").unwrap();
            symlink(outside.join("file"), project.join("out-file")).unwrap();
            symlink(&outside, project.join("out-dir")).unwrap();
            symlink(project.join("missing"), project.join("dangling")).unwrap();
            symlink(project.join("loop"), project.join("loop")).unwrap();
            let before = snapshot(root);
            for target in [
                project.join("missing"),
                project.join("missing-parent/leaf"),
                project.join("dangling"),
                project.join("loop"),
                project.join("out-file"),
                project.join("out-dir/file"),
                project.join("../project-sibling/file"),
                project.join("nul\0leaf"),
                outside.join("file"),
            ] {
                assert_eq!(
                    guard(&target, project.to_str().unwrap(), false),
                    Err(OUTSIDE.into()),
                    "{}",
                    target.display()
                );
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn loose_mode_keeps_unresolved_leaf_and_canonical_parent() {
    isolated(
        "loose_mode_keeps_unresolved_leaf_and_canonical_parent",
        |root| {
            let project = root.join("project");
            let outside = root.join("outside");
            fs::create_dir_all(project.join("real-parent")).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("file"), b"outside fixture").unwrap();
            symlink(outside.join("file"), project.join("out-file")).unwrap();
            symlink(project.join("missing"), project.join("dangling")).unwrap();
            symlink(project.join("loop"), project.join("loop")).unwrap();
            symlink(project.join("real-parent"), project.join("parent-alias")).unwrap();
            let before = snapshot(root);
            let canonical = project.canonicalize().unwrap();
            for name in ["out-file", "missing", "dangling", "loop", "nul\0leaf"] {
                assert_eq!(
                    guard(&project.join(name), project.to_str().unwrap(), true),
                    Ok(canonical.join(name)),
                    "fallback does not require an existing symlink: {name:?}"
                );
            }
            assert_eq!(
                guard(
                    &project.join("parent-alias/absent"),
                    project.to_str().unwrap(),
                    true
                ),
                Ok(canonical.join("real-parent/absent"))
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn loose_mode_preserves_parent_containment_and_error_precedence() {
    isolated(
        "loose_mode_preserves_parent_containment_and_error_precedence",
        |root| {
            let project = root.join("project");
            let outside = root.join("project-sibling");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("file"), b"outside fixture").unwrap();
            symlink(&outside, project.join("out-dir")).unwrap();
            let before = snapshot(root);
            for target in [
                outside.join("file"),
                outside.join("absent"),
                project.join("out-dir/file"),
                project.join("out-dir/absent"),
                project.join("../project-sibling/file"),
            ] {
                assert_eq!(
                    guard(&target, project.to_str().unwrap(), true),
                    Err(OUTSIDE.into())
                );
            }
            let missing_parent = project.join("missing-parent");
            assert_eq!(
                guard(
                    &missing_parent.join("leaf"),
                    project.to_str().unwrap(),
                    true
                ),
                Err(format!(
                    "Cannot resolve path: {}",
                    missing_parent.canonicalize().unwrap_err()
                ))
            );
            assert_eq!(
                guard(Path::new("/"), project.to_str().unwrap(), true),
                Err("Invalid file name".into())
            );
            assert_eq!(
                guard(Path::new("/"), project.to_str().unwrap(), false),
                Err(OUTSIDE.into())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_local_text_and_image_reads_keep_strict_read_difference() {
    isolated(
        "actual_local_text_and_image_reads_keep_strict_read_difference",
        |root| {
            let project = root.join("project");
            let outside = root.join("outside");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("content"), "scratch 文本\r\n").unwrap();
            fs::write(outside.join("image"), b"fixture").unwrap();
            symlink(outside.join("content"), project.join("instructions.md")).unwrap();
            symlink(outside.join("image"), project.join("preview.png")).unwrap();
            let before = snapshot(root);
            let rt = runtime();
            let text_path = project
                .join("instructions.md")
                .to_string_lossy()
                .into_owned();
            let project_path = project.to_string_lossy().into_owned();
            assert_eq!(
                rt.block_on(super::read_file_content(
                    text_path.clone(),
                    project_path.clone()
                )),
                Ok("scratch 文本\r\n".into())
            );
            assert_eq!(
                rt.block_on(super::read_project_file_content(
                    text_path,
                    project_path.clone()
                )),
                Err(OUTSIDE.into())
            );
            let image = rt
                .block_on(super::read_image_preview(
                    project.join("preview.png").to_string_lossy().into_owned(),
                    project_path,
                ))
                .unwrap();
            assert_eq!(
                serde_json::to_value(image).unwrap(),
                serde_json::json!({
                    "dataUrl": "data:image/png;base64,Zml4dHVyZQ==",
                    "mimeType": "image/png",
                    "byteLength": 7
                })
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_directory_write_and_copy_refuse_before_side_effects() {
    isolated(
        "actual_directory_write_and_copy_refuse_before_side_effects",
        |root| {
            let project = root.join("project");
            let outside = root.join("outside");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("kept.txt"), b"preserved outside fixture").unwrap();
            fs::write(project.join("source.txt"), b"copy source").unwrap();
            symlink(&outside, project.join("out-dir")).unwrap();
            symlink(outside.join("kept.txt"), project.join("out-file")).unwrap();
            let before = snapshot(root);
            let rt = runtime();
            let project_path = project.to_string_lossy().into_owned();
            let directory = project.join("out-dir").to_string_lossy().into_owned();
            assert_eq!(
                rt.block_on(super::read_dir_entries(
                    directory.clone(),
                    project_path.clone()
                ))
                .err(),
                Some(OUTSIDE.into())
            );
            assert_eq!(
                rt.block_on(super::write_file_content(
                    project.join("out-file").to_string_lossy().into_owned(),
                    "must not write".into(),
                    project_path.clone(),
                )),
                Err(OUTSIDE.into())
            );
            assert_eq!(
                rt.block_on(super::copy_paths_to_directory(
                    vec![project.join("source.txt").to_string_lossy().into_owned()],
                    directory,
                    project_path,
                )),
                Err(OUTSIDE.into())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}
