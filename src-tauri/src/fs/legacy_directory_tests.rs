//! Compatibility fixtures for the actual directory IPC. Each child uses only
//! scratch roots and a fake git, including when exercising the pre-extraction body.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_DIRECTORY_CASE";
const ROOT: &str = "AERORIC_DIRECTORY_ROOT";

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
        run(&root);
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        Scratch(PathBuf::from("/tmp").join(format!("legacy-dir-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    fs::write(
        root.0.join("bin/git"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$AERORIC_DIRECTORY_ROOT/git-args"
printf '%s\n' "$PWD" > "$AERORIC_DIRECTORY_ROOT/git-cwd"
while IFS= read -r line; do
    printf '%s\n' "$line" >> "$AERORIC_DIRECTORY_ROOT/git-stdin"
    case "$line" in *.ignored) printf '%s\n' "$line";; esac
done
if [ -f "$AERORIC_DIRECTORY_ROOT/nonzero" ]; then exit 37; fi
"#,
    )
    .unwrap();
    fs::set_permissions(root.0.join("bin/git"), fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_directory_tests::{name}"),
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

// The actual registered IPC, before and after extraction. No route substitute.
fn list(path: &Path, project: &str) -> Result<Vec<super::FsEntry>, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(super::read_dir_entries(
            path.to_string_lossy().into_owned(),
            project.to_string(),
        ))
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
            for child in fs::read_dir(path).unwrap() {
                visit(root, &child.unwrap().path(), all);
            }
        }
    }
    let mut all = BTreeMap::new();
    visit(root, root, &mut all);
    all
}

#[test]
fn wire_filter_sort_and_raw_metadata_remain_legacy() {
    isolated("wire_filter_sort_and_raw_metadata_remain_legacy", |root| {
        let project = root.join("project spaces");
        let directory = project.join("nested");
        fs::create_dir_all(&directory).unwrap();
        // Do not create both .git and .Git on a case-insensitive filesystem.
        for name in ["node_modules", ".Git", "Alpha", "z-folder"] {
            fs::create_dir(directory.join(name)).unwrap();
        }
        for name in [".hidden", "f.MD", "README", "target", "with space.ignored"] {
            fs::write(directory.join(name), b"raw\0bytes\r\n").unwrap();
        }
        // APFS rejects invalid UTF-8 names; keep this fixture valid on macOS.
        fs::write(directory.join("bad-中文.TXT"), b"unicode filename").unwrap();
        let before = snapshot(&project);
        // Relative project roots already work; only the target must be absolute.
        let entries = list(&directory, "project spaces").unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                ".Git",
                "Alpha",
                "z-folder",
                ".hidden",
                "bad-中文.TXT",
                "f.MD",
                "README",
                "target",
                "with space.ignored"
            ]
        );
        let canonical = directory.canonicalize().unwrap();
        for entry in &entries {
            assert_eq!(entry.path, canonical.join(&entry.name).to_string_lossy());
            assert!(entry.modified_at_ms.is_some());
            assert!(!entry.is_symlink);
            assert_eq!(entry.is_gitignored, entry.name == "with space.ignored");
            let value = serde_json::to_value(entry).unwrap();
            let mut keys = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(
                keys,
                [
                    "extension",
                    "is_dir",
                    "is_gitignored",
                    "is_symlink",
                    "modified_at_ms",
                    "name",
                    "path"
                ]
            );
        }
        let by_name = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
        assert!(by_name("Alpha").is_dir);
        assert!(
            !by_name("target").is_dir,
            "ignored names filter directories only"
        );
        assert_eq!(by_name("f.MD").extension.as_deref(), Some("md"));
        assert_eq!(by_name("bad-中文.TXT").extension.as_deref(), Some("txt"));
        assert_eq!(by_name("README").extension, None);
        assert_eq!(snapshot(&project), before);
        assert_eq!(
            fs::read_to_string(root.join("git-args")).unwrap(),
            "check-ignore --stdin\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("git-cwd")).unwrap(),
            format!("{}\n", project.canonicalize().unwrap().display())
        );
        assert_eq!(
            fs::read_to_string(root.join("git-stdin")).unwrap(),
            entries
                .iter()
                .map(|entry| format!("{}\n", entry.path))
                .collect::<String>()
        );
    });
}

#[test]
fn symlink_flags_and_canonical_root_are_preserved() {
    isolated("symlink_flags_and_canonical_root_are_preserved", |root| {
        let project = root.join("project");
        fs::create_dir_all(project.join("real-dir")).unwrap();
        fs::write(project.join("real.txt"), b"content").unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("never-read"), b"outside").unwrap();
        symlink(project.join("real-dir"), project.join("link-dir")).unwrap();
        symlink(project.join("real.txt"), project.join("link.txt")).unwrap();
        symlink(project.join("missing"), project.join("dangling")).unwrap();
        symlink(&outside, project.join("outside-dir")).unwrap();
        let alias = root.join("project-alias");
        symlink(&project, &alias).unwrap();
        let before = snapshot(&project);
        let outside_before = snapshot(&outside);
        let entries = list(&alias, &alias.to_string_lossy()).unwrap();
        let by_name = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
        assert!(by_name("link-dir").is_dir && by_name("link-dir").is_symlink);
        assert!(by_name("outside-dir").is_dir && by_name("outside-dir").is_symlink);
        assert!(!by_name("link.txt").is_dir && by_name("link.txt").is_symlink);
        assert!(!by_name("dangling").is_dir && by_name("dangling").is_symlink);
        assert!(
            by_name("dangling").modified_at_ms.is_some(),
            "DirEntry metadata reports the link even when its target is missing"
        );
        let canonical = project.canonicalize().unwrap();
        assert!(entries
            .iter()
            .all(|entry| Path::new(&entry.path).starts_with(&canonical)));
        assert_eq!(snapshot(&project), before);
        assert_eq!(snapshot(&outside), outside_before);
    });
}

#[test]
fn strict_path_refusals_do_not_spawn_git() {
    isolated("strict_path_refusals_do_not_spawn_git", |root| {
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret"), b"scratch only").unwrap();
        symlink(&outside, project.join("escape")).unwrap();
        let before = snapshot(&project);
        let outside_before = snapshot(&outside);
        let relative = list(Path::new("relative"), &project.to_string_lossy());
        assert!(matches!(relative, Err(error) if error == "Path must be absolute"));
        for target in [&outside, &project.join("escape")] {
            assert!(matches!(
                list(target, &project.to_string_lossy()),
                Err(error) if error == "Path is outside the allowed directory"
            ));
        }
        assert!(matches!(
            list(&project, &root.join("missing-root").to_string_lossy()),
            Err(error) if error.starts_with("Cannot resolve root directory:")
        ));
        assert!(!root.join("git-args").exists());
        assert_eq!(snapshot(&project), before);
        assert_eq!(snapshot(&outside), outside_before);
    });
}

#[test]
fn git_stdout_marks_ignored_even_on_nonzero_exit() {
    isolated("git_stdout_marks_ignored_even_on_nonzero_exit", |root| {
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("raw.ignored"), b"content").unwrap();
        fs::write(root.join("nonzero"), b"exit 37").unwrap();
        let before = snapshot(&project);
        let entries = list(&project, &project.to_string_lossy()).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].is_gitignored,
            "legacy uses stdout without testing status"
        );
        assert_eq!(snapshot(&project), before);
    });
}

#[test]
fn missing_git_is_best_effort() {
    isolated("missing_git_is_best_effort", |root| {
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("raw.ignored"), b"content").unwrap();
        fs::remove_file(root.join("bin/git")).unwrap();
        let before = snapshot(&project);
        let entries = list(&project, &project.to_string_lossy()).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].is_gitignored);
        assert!(!root.join("git-args").exists());
        assert_eq!(snapshot(&project), before);
    });
}

#[test]
fn empty_and_io_failures_skip_git_and_leave_entries_unchanged() {
    isolated(
        "empty_and_io_failures_skip_git_and_leave_entries_unchanged",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let before = snapshot(&project);
            assert!(list(&project, &project.to_string_lossy())
                .unwrap()
                .is_empty());
            assert_eq!(snapshot(&project), before);
            let file = project.join("not-a-directory");
            fs::write(&file, b"file").unwrap();
            let before = snapshot(&project);
            let expected = fs::read_dir(&file).err().unwrap().to_string();
            assert!(
                matches!(list(&file, &project.to_string_lossy()), Err(error) if error == expected)
            );
            assert!(!root.join("git-args").exists());
            assert_eq!(snapshot(&project), before);
        },
    );
}
