//! Before/after actual legacy history commands and the unchanged file-save consumer.
//! Exact scratch child, no shell/agent/Trash/UI, no inherited home or PATH.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_HISTORY_CASE";
const ROOT: &str = "AERORIC_LEGACY_HISTORY_ROOT";

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
    let root = Scratch(
        PathBuf::from("/tmp").join(format!("legacy-history-{}-{nonce}", std::process::id())),
    );
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("local_history::legacy_history_tests::{name}"),
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

fn list(project: &Path, file: &Path) -> Result<Vec<super::LocalHistoryEntry>, String> {
    runtime().block_on(super::list_local_history(text(project), text(file)))
}

fn read(project: &Path, file: &Path, id: &str) -> Result<super::LocalHistorySnapshot, String> {
    runtime().block_on(super::read_local_history_entry(
        text(project),
        text(file),
        id.to_string(),
    ))
}

fn restore(project: &Path, file: &Path, id: &str) -> Result<super::LocalHistorySnapshot, String> {
    runtime().block_on(super::restore_local_history_entry(
        text(project),
        text(file),
        id.to_string(),
    ))
}

fn write(project: &Path, file: &Path, content: &str) -> Result<(), String> {
    runtime().block_on(crate::fs::write_file_content(
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
    id: &str,
    content: &[u8],
) -> Vec<PathBuf> {
    let file = directory.join(format!("{id}.txt"));
    assert_eq!(fs::read(&file).unwrap(), content);
    assert!(file.symlink_metadata().unwrap().is_file());
    let mut added = vec![file];
    let mut next = directory;
    while !before.contains_key(next.strip_prefix(root).unwrap()) {
        assert!(next.symlink_metadata().unwrap().is_dir());
        added.push(next.to_path_buf());
        next = next.parent().unwrap();
    }
    added
}

#[test]
fn actual_queries_preserve_root_file_admission_and_no_write_errors() {
    isolated(
        "actual_queries_preserve_root_file_admission_and_no_write_errors",
        |root| {
            let (project, file) = fixture(root);
            let outside = root.join("outside.txt");
            fs::write(&outside, b"outside retained").unwrap();
            let missing = project.join("missing");
            let missing_root = root.join("missing-project");
            let before = snapshot(root);
            let cases = [
                (
                    PathBuf::from("relative\0project"),
                    file.clone(),
                    "Project path must be absolute".to_string(),
                ),
                (
                    missing_root.clone(),
                    file.clone(),
                    format!(
                        "Cannot resolve project path: {}",
                        missing_root.canonicalize().unwrap_err()
                    ),
                ),
                (
                    file.clone(),
                    file.clone(),
                    "Project path is not a directory".into(),
                ),
                (
                    project.clone(),
                    PathBuf::from("relative\0file"),
                    "File path must be absolute".into(),
                ),
                (
                    project.clone(),
                    missing.clone(),
                    format!(
                        "Cannot resolve file path: {}",
                        missing.canonicalize().unwrap_err()
                    ),
                ),
                (
                    project.clone(),
                    outside,
                    "File path is outside the project".into(),
                ),
                (
                    project.clone(),
                    project.join("src"),
                    "File path is not a file".into(),
                ),
            ];
            for (project, file, error) in cases {
                assert_eq!(list(&project, &file).map(|_| ()), Err(error.clone()));
                assert_eq!(
                    read(&project, &file, "../invalid").map(|_| ()),
                    Err(error.clone())
                );
                assert_eq!(
                    restore(&project, &file, "../invalid").map(|_| ()),
                    Err(error)
                );
            }
            assert_eq!(list(&project, &file), Ok(Vec::new()));
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_list_keeps_hex_layout_metadata_wire_sort_and_limit() {
    isolated(
        "actual_list_keeps_hex_layout_metadata_wire_sort_and_limit",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            fs::create_dir_all(&directory).unwrap();
            for id in 1000..1101 {
                fs::write(directory.join(format!("{id}.txt")), b"old").unwrap();
            }
            for id in ["3000-2", "3000-10"] {
                fs::write(directory.join(format!("{id}.txt")), "日本語").unwrap();
            }
            fs::write(directory.join("4000-junk.txt"), [0xff, 0xfe]).unwrap();
            fs::write(directory.join("9999.TXT"), b"wrong extension").unwrap();
            fs::write(directory.join("nonnumeric.txt"), b"wrong timestamp").unwrap();
            fs::create_dir(directory.join("9000.txt")).unwrap();
            let native_decoy = project.join(".aeroric/local-history/src/app-日本語.ts");
            fs::create_dir_all(&native_decoy).unwrap();
            fs::write(native_decoy.join("999999.txt"), b"not legacy history").unwrap();
            let before = snapshot(root);
            let entries = list(&project, &file).unwrap();
            assert_eq!(entries.len(), 100);
            assert_eq!(
                (&entries[0].id, entries[0].size),
                (&"4000-junk".to_string(), 2)
            );
            assert_eq!(
                (&entries[1].id, &entries[2].id),
                (&"3000-10".into(), &"3000-2".into())
            );
            assert_eq!(entries.last().unwrap().id, "1004");
            assert_eq!(
                serde_json::to_value(&entries[0]).unwrap(),
                serde_json::json!({
                    "id": "4000-junk", "filePath": text(&file.canonicalize().unwrap()),
                    "relativePath": "src/app-日本語.ts", "createdAtMs": 4000, "size": 2
                })
            );
            assert_eq!(snapshot(root), before);

            // Existing non-directory history is an error, not native's empty list.
            let other = project.join("other.txt");
            fs::write(&other, b"other").unwrap();
            let bad_directory = history(&project, "other.txt");
            fs::write(&bad_directory, b"not a directory").unwrap();
            let before = snapshot(root);
            assert_eq!(
                list(&project, &other),
                Err(fs::read_dir(&bad_directory).unwrap_err().to_string())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_read_keeps_id_metadata_timestamp_and_utf8_error_order() {
    isolated(
        "actual_read_keeps_id_metadata_timestamp_and_utf8_error_order",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("1000--.txt"), "日本語\r\n").unwrap();
            fs::write(directory.join("-.txt"), b"invalid timestamp").unwrap();
            fs::write(directory.join("3000.txt"), [0xff]).unwrap();
            fs::create_dir(directory.join("2000.txt")).unwrap();
            let before = snapshot(root);
            for id in ["", "../bad", "1000/x", "1000x", " 1000", "1000\\x"] {
                assert_eq!(
                    read(&project, &file, id),
                    Err("Invalid local history entry id".into())
                );
            }
            for id in ["--", "9000"] {
                assert_eq!(
                    read(&project, &file, id),
                    Err(fs::metadata(directory.join(format!("{id}.txt")))
                        .unwrap_err()
                        .to_string())
                );
            }
            assert_eq!(
                read(&project, &file, "-"),
                Err("Invalid local history entry id".into())
            );
            assert_eq!(
                read(&project, &file, "2000"),
                Err("Local history entry is not a file".into())
            );
            assert_eq!(
                read(&project, &file, "3000"),
                Err(fs::read_to_string(directory.join("3000.txt"))
                    .unwrap_err()
                    .to_string())
            );
            let value = read(&project, &file, "1000--").unwrap();
            assert_eq!(value.content, "日本語\r\n");
            assert_eq!(value.entry.created_at_ms, 1000);
            assert_eq!(value.entry.size, 11);
            assert_eq!(
                serde_json::to_value(&value).unwrap(),
                serde_json::json!({
                    "entry": {"id": "1000--", "filePath": text(&file.canonicalize().unwrap()),
                        "relativePath": "src/app-日本語.ts", "createdAtMs": 1000, "size": 11},
                    "content": "日本語\r\n"
                })
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_save_snapshots_old_bytes_and_preserves_modes_and_matching_skip() {
    isolated(
        "actual_save_snapshots_old_bytes_and_preserves_modes_and_matching_skip",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            let before = snapshot(root);
            write(&project, &file, "after 日本語\r\n").unwrap();
            let entries = list(&project, &file).unwrap();
            assert_eq!(entries.len(), 1);
            assert!(entries[0].created_at_ms > 1_577_836_800_000);
            assert_eq!(
                read(&project, &file, &entries[0].id).unwrap().content,
                "current\n"
            );
            let added = new_snapshot_paths(root, &before, &directory, &entries[0].id, b"current\n");
            assert_changes(
                root,
                before,
                &added,
                &[(file.clone(), "after 日本語\r\n".as_bytes())],
            );
            let before = snapshot(root);
            write(&project, &file, "after 日本語\r\n").unwrap();
            assert_eq!(list(&project, &file).unwrap(), entries);
            assert_changes(
                root,
                before,
                &[],
                &[(file.clone(), "after 日本語\r\n".as_bytes())],
            );

            let before = snapshot(root);
            write(&project, &file, "third\n").unwrap();
            let next = list(&project, &file).unwrap();
            assert_eq!(next.len(), 2);
            let new = next.iter().find(|entry| entry.id != entries[0].id).unwrap();
            let added = new_snapshot_paths(
                root,
                &before,
                &directory,
                &new.id,
                "after 日本語\r\n".as_bytes(),
            );
            assert_changes(root, before, &added, &[(file, b"third\n")]);
        },
    );
}

#[test]
fn actual_save_preserves_snapshot_limits_binary_self_history_and_missing_refusal() {
    isolated(
        "actual_save_preserves_snapshot_limits_binary_self_history_and_missing_refusal",
        |root| {
            let (project, _) = fixture(root);
            let large = project.join("large.txt");
            let binary = project.join("binary.txt");
            let own_history = project.join(".aeroric/local-history/manual.txt");
            fs::create_dir_all(own_history.parent().unwrap()).unwrap();
            fs::write(&large, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
            fs::write(&binary, [0xff]).unwrap();
            fs::write(&own_history, b"manual").unwrap();
            for file in [&large, &binary, &own_history] {
                let before = snapshot(root);
                write(&project, file, "replacement").unwrap();
                assert_changes(root, before, &[], &[(file.to_path_buf(), b"replacement")]);
            }
            let exact = project.join("exact.txt");
            let payload = vec![b'x'; 2 * 1024 * 1024];
            fs::write(&exact, &payload).unwrap();
            let before = snapshot(root);
            write(&project, &exact, "small").unwrap();
            let entries = list(&project, &exact).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].size, payload.len() as u64);
            let added = new_snapshot_paths(
                root,
                &before,
                &history(&project, "exact.txt"),
                &entries[0].id,
                &payload,
            );
            assert_changes(root, before, &added, &[(exact, b"small")]);
            let missing = project.join("missing.txt");
            let before = snapshot(root);
            assert_eq!(
                write(&project, &missing, "must not create"),
                Err("Path is outside the allowed directory".into())
            );
            // The actual strict writer refuses before reaching history.
            // Calling the history helper itself has a different error.
            assert_eq!(
                super::record_snapshot_before_write(&text(&project), &text(&missing), "next"),
                Err(format!(
                    "Cannot resolve file path: {}",
                    missing.canonicalize().unwrap_err()
                ))
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_restore_preserves_backup_skips_and_read_failure_before_write() {
    isolated(
        "actual_restore_preserves_backup_skips_and_read_failure_before_write",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("1000.txt"), b"snapshot\n").unwrap();
            let before = snapshot(root);
            let restored = restore(&project, &file, "1000").unwrap();
            assert_eq!(restored.content, "snapshot\n");
            assert_eq!(restored.entry.id, "1000");
            let entries = list(&project, &file).unwrap();
            assert_eq!(entries.len(), 2);
            let backup = entries.iter().find(|entry| entry.id != "1000").unwrap();
            let added = new_snapshot_paths(root, &before, &directory, &backup.id, b"current\n");
            assert_changes(root, before, &added, &[(file.clone(), b"snapshot\n")]);
            for current in [
                b"snapshot\n".to_vec(),
                vec![0xff],
                vec![b'x'; 2 * 1024 * 1024 + 1],
            ] {
                fs::write(&file, current).unwrap();
                let before = snapshot(root);
                restore(&project, &file, "1000").unwrap();
                assert_eq!(list(&project, &file).unwrap(), entries);
                assert_changes(root, before, &[], &[(file.clone(), b"snapshot\n")]);
            }
            fs::write(directory.join("2000.txt"), [0xff]).unwrap();
            let before = snapshot(root);
            assert_eq!(
                restore(&project, &file, "2000"),
                Err(fs::read_to_string(directory.join("2000.txt"))
                    .unwrap_err()
                    .to_string())
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn actual_history_preserves_source_aliases_and_distinct_history_link_policy() {
    isolated(
        "actual_history_preserves_source_aliases_and_distinct_history_link_policy",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            let root_alias = root.join("root-alias");
            let source_alias = project.join("source-alias");
            symlink(&project, &root_alias).unwrap();
            symlink(&file, &source_alias).unwrap();
            let outside = root.join("outside.txt");
            fs::write(&outside, b"outside entry payload").unwrap();
            symlink(&outside, project.join("outside-source")).unwrap();
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("1000.txt"), b"snapshot").unwrap();
            symlink(&outside, directory.join("5000.txt")).unwrap();
            let before = snapshot(root);
            let entries = list(&root_alias, &source_alias).unwrap();
            assert_eq!(
                entries.len(),
                1,
                "DirEntry metadata does not follow the entry link"
            );
            assert_eq!(entries[0].file_path, text(&file.canonicalize().unwrap()));
            assert_eq!(entries[0].relative_path, "src/app-日本語.ts");
            assert_eq!(
                read(&root_alias, &source_alias, "5000").unwrap().content,
                "outside entry payload"
            );
            assert_eq!(
                list(&project, &project.join("outside-source")),
                Err("File path is outside the project".into())
            );
            assert_eq!(
                write(&project, &project.join("outside-source"), "forbidden"),
                Err("Path is outside the allowed directory".into())
            );
            assert_eq!(snapshot(root), before);

            let raw_file = project.join("src/raw\\name.txt");
            fs::write(&raw_file, b"raw source").unwrap();
            let other_directory = root.join("other-history");
            fs::create_dir(&other_directory).unwrap();
            fs::write(other_directory.join("7000.txt"), b"history directory link").unwrap();
            symlink(&other_directory, history(&project, "src/raw\\name.txt")).unwrap();
            let before = snapshot(root);
            assert_eq!(
                list(&project, &raw_file).unwrap()[0].relative_path,
                "src/raw\\name.txt"
            );
            assert_eq!(
                read(&project, &raw_file, "7000").unwrap().content,
                "history directory link"
            );
            assert_eq!(snapshot(root), before, "all referents remain scratch-only");
        },
    );
}

#[test]
fn actual_prune_failure_leaves_new_snapshot_but_does_not_write_target() {
    isolated(
        "actual_prune_failure_leaves_new_snapshot_but_does_not_write_target",
        |root| {
            let (project, file) = fixture(root);
            let directory = history(&project, "src/app-日本語.ts");
            fs::create_dir_all(directory.join("0.txt")).unwrap();
            for id in 1..100 {
                fs::write(directory.join(format!("{id}.txt")), b"old retained").unwrap();
            }
            let before = snapshot(root);
            assert_eq!(
                write(&project, &file, "must not reach final write"),
                Err(fs::remove_file(directory.join("0.txt"))
                    .unwrap_err()
                    .to_string())
            );
            let entries = list(&project, &file).unwrap();
            assert_eq!(entries.len(), 100);
            let new = entries
                .iter()
                .find(|entry| entry.created_at_ms > 1_577_836_800_000)
                .unwrap();
            let added = new_snapshot_paths(root, &before, &directory, &new.id, b"current\n");
            assert_changes(root, before, &added, &[]);
            assert_eq!(fs::read(&file).unwrap(), b"current\n");
        },
    );
}
