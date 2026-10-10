//! Actual `fs::write_file_content` behavior: strict guard → hex snapshot →
//! `fs::write`. Exact env-cleared scratch children; no formatter, Trash,
//! clipboard, real HOME or PATH is touched.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_WRITE_CASE";
const ROOT: &str = "AERORIC_LEGACY_WRITE_ROOT";

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
            "no unrelated settings/home initialization"
        );
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 0);
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        Scratch(PathBuf::from("/tmp").join(format!("legacy-write-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_write_tests::{name}"),
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

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn write(project: &Path, file: &Path, content: &str) -> Result<(), String> {
    runtime().block_on(super::write_file_content(
        text(file),
        content.to_string(),
        text(project),
    ))
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

fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let project = root.join("project with spaces");
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
        assert_eq!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("txt"),
            "hex-layout entries are .txt"
        );
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
fn actual_write_refusals_leave_no_trace() {
    isolated("actual_write_refusals_leave_no_trace", |root| {
        let (project, file) = fixture(root);
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
        // The strict guard accepts an existing in-root directory; the history
        // validator then refuses the non-file before any write.
        let err = write(&project, &directory, "new content\n").unwrap_err();
        assert_eq!(err, "File path is not a file");

        assert!(
            !project.join(".aeroric").exists(),
            "no history side effects"
        );
        assert_eq!(fs::read(&file).unwrap(), b"current\n");
        assert_eq!(fs::read(&outside).unwrap(), b"outside retained");
        assert_changes(root, before, &[], &[]);
    });
}

#[test]
fn actual_write_snapshots_prior_content_then_overwrites() {
    isolated(
        "actual_write_snapshots_prior_content_then_overwrites",
        |root| {
            let (project, file) = fixture(root);
            let before = snapshot(root);

            // A path with `..` still records and writes through the canonical
            // resolved text; the resolved target keeps its mode and link shape.
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

            // A symlink that resolves inside the project writes its canonical
            // target; the link itself is not replaced.
            let link = project.join("src/alias.ts");
            symlink(&file, &link).unwrap();
            let before = snapshot(root);

            write(&project, &link, "third\n").unwrap();

            assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
            assert_eq!(fs::read_link(&link).unwrap(), file);
            let dir = history(&project, "src/app-日本語.ts");
            let added = new_snapshot_paths(root, &before, &dir, b"next\n");
            assert_changes(root, before, &added, &[(file.clone(), b"third\n")]);
        },
    );
}

#[test]
fn actual_write_content_vocabulary_truncate_and_grow() {
    isolated(
        "actual_write_content_vocabulary_truncate_and_grow",
        |root| {
            let (project, file) = fixture(root);
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
            // Grow then shrink: truncation is real, not an append.
            write(&project, &file, "shrunk\n").unwrap();
            let added = new_snapshot_paths(root, &before, &dir, &previous);
            assert_changes(root, before.clone(), &added, &[(file.clone(), b"shrunk\n")]);
            assert_eq!(fs::read(&file).unwrap(), b"shrunk\n");
        },
    );
}

#[test]
fn snapshot_failure_refuses_write_and_keeps_original_bytes() {
    isolated(
        "snapshot_failure_refuses_write_and_keeps_original_bytes",
        |root| {
            let (project, file) = fixture(root);
            // A regular file where the history directory must go makes snapshot
            // creation fail after the guard has accepted the target.
            fs::write(project.join(".aeroric"), b"blocker").unwrap();
            let before = snapshot(root);

            let err = write(&project, &file, "new content\n").unwrap_err();
            assert!(
                err.contains("Not a directory") || err.contains("File exists"),
                "unexpected snapshot error: {err}"
            );
            assert_eq!(fs::read(&file).unwrap(), b"current\n");
            assert_changes(root, before, &[], &[]);
        },
    );
}

#[test]
fn write_failure_keeps_snapshot_and_original_bytes() {
    isolated("write_failure_keeps_snapshot_and_original_bytes", |root| {
        let (project, file) = fixture(root);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o440)).unwrap();
        let before = snapshot(root);

        let err = write(&project, &file, "new content\n").unwrap_err();
        assert!(err.contains("Permission denied"), "unexpected error: {err}");
        assert_eq!(fs::read(&file).unwrap(), b"current\n");
        let dir = history(&project, "src/app-日本語.ts");
        let added = new_snapshot_paths(root, &before, &dir, b"current\n");
        assert_changes(root, before, &added, &[]);
    });
}
