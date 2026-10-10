//! Direct legacy-history service tests; all IO stays in explicitly supplied scratch.
//! Native history is a different policy, not an alias or a production cutover.

use aeroric_core::files::{legacy_history as history, local_history as native};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "common/scratch.rs"]
mod scratch;

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

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let project = root.join("project with spaces");
    let file = project.join("src/app-日本語.ts");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"current\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    (project, file)
}

fn directory(project: &Path, relative: &str) -> PathBuf {
    let hex: String = relative
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    project.join(".aeroric/local-history").join(hex)
}

fn record(
    project: &Path,
    file: &Path,
    next: &str,
) -> Result<Option<native::LocalHistoryEntry>, String> {
    history::record_snapshot_before_write(text(project), text(file), next)
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
fn shared_types_keep_legacy_hex_layout_wire_and_native_raw_layout_separate() {
    let scratch = scratch::Scratch::new("legacy-history-wire");
    let root = scratch.path();
    let (project, file) = fixture(root);
    assert_eq!(native::CODE_LAYOUT.dir, ".aeroric/local-history");
    assert_eq!(native::CODE_LAYOUT.max_entries, 100);
    assert_eq!(native::CODE_LAYOUT.min_interval_ms, 0);
    assert_eq!(native::MAX_SNAPSHOT_BYTES, 2 * 1024 * 1024);
    assert_eq!(native::SNAPSHOT_EXTENSION, "txt");
    let before = snapshot(root);
    // Assigning to the native DTO proves type identity, not just a JSON conversion.
    let entry: native::LocalHistoryEntry = record(&project, &file, "next").unwrap().unwrap();
    assert!(entry.created_at_ms > 1_577_836_800_000);
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        serde_json::json!({
            "id": entry.id, "filePath": text(&file.canonicalize().unwrap()),
            "relativePath": "src/app-日本語.ts", "createdAtMs": entry.created_at_ms, "size": 8
        })
    );
    let value: native::LocalHistorySnapshot =
        history::read_entry(text(&project), text(&file), &entry.id).unwrap();
    assert_eq!(value.content, "current\n");
    let history_dir = directory(&project, "src/app-日本語.ts");
    let added = new_snapshot_paths(root, &before, &history_dir, &entry.id, b"current\n");
    assert_changes(root, before, &added, &[]);
    let before = snapshot(root);
    assert!(record(&project, &file, "current\n").unwrap().is_none());
    assert!(native::list_entries(
        native::CODE_LAYOUT,
        &project.canonicalize().unwrap(),
        &file.canonicalize().unwrap(),
    )
    .unwrap()
    .is_empty());
    assert_eq!(snapshot(root), before);
}

#[test]
fn legacy_services_keep_root_and_file_errors_without_creating_paths() {
    let scratch = scratch::Scratch::new("legacy-history-admission");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let missing_project = root.join("missing-project");
    let missing = project.join("missing.txt");
    let outside = root.join("outside.txt");
    fs::write(&outside, b"outside preserved").unwrap();
    let before = snapshot(root);
    for (project, file, error) in [
        (
            PathBuf::from("relative\0root"),
            file.clone(),
            "Project path must be absolute".to_string(),
        ),
        (
            missing_project.clone(),
            file.clone(),
            format!(
                "Cannot resolve project path: {}",
                missing_project.canonicalize().unwrap_err()
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
    ] {
        assert_eq!(
            record(&project, &file, "next").map(|_| ()),
            Err(error.clone())
        );
        assert_eq!(
            history::list_entries(text(&project), text(&file)).map(|_| ()),
            Err(error.clone())
        );
        assert_eq!(
            history::read_entry(text(&project), text(&file), "../bad").map(|_| ()),
            Err(error.clone())
        );
        assert_eq!(
            history::restore_entry(text(&project), text(&file), "../bad").map(|_| ()),
            Err(error)
        );
    }
    assert_eq!(snapshot(root), before);
}

#[test]
fn legacy_list_and_read_keep_metadata_id_errors_suffix_order_and_custom_limit() {
    let scratch = scratch::Scratch::new("legacy-history-query");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let layout = native::HistoryLayout {
        dir: "custom/history",
        max_entries: 2,
        min_interval_ms: 60_000,
    };
    let key = directory(&project, "src/app-日本語.ts")
        .file_name()
        .unwrap()
        .to_owned();
    let history_dir = project.join(layout.dir).join(key);
    fs::create_dir_all(&history_dir).unwrap();
    for id in ["1000", "2000-2", "2000-10"] {
        fs::write(history_dir.join(format!("{id}.txt")), b"old").unwrap();
    }
    fs::write(history_dir.join("3000--.txt"), "日本語").unwrap();
    fs::write(history_dir.join("4000-x.txt"), [0xff]).unwrap();
    fs::write(history_dir.join("-.txt"), b"bad timestamp").unwrap();
    fs::create_dir(history_dir.join("9000.txt")).unwrap();
    let before = snapshot(root);
    let entries = history::list_entries_in(layout, &project, &file).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["4000-x", "3000--"]
    );
    assert_eq!(entries[0].size, 1);
    let all = history::list_entries_in(
        native::HistoryLayout {
            max_entries: 100,
            ..layout
        },
        &project,
        &file,
    )
    .unwrap();
    assert_eq!(
        all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["4000-x", "3000--", "2000-10", "2000-2", "1000"]
    );
    for id in ["", "../x", "4000-x", " 1000"] {
        assert_eq!(
            history::read_entry_in(layout, &project, &file, id),
            Err("Invalid local history entry id".into())
        );
    }
    assert_eq!(
        history::read_entry_in(layout, &project, &file, "-"),
        Err("Invalid local history entry id".into())
    );
    assert_eq!(
        history::read_entry_in(layout, &project, &file, "--"),
        Err(fs::metadata(history_dir.join("--.txt"))
            .unwrap_err()
            .to_string())
    );
    assert_eq!(
        history::read_entry_in(layout, &project, &file, "9000"),
        Err("Local history entry is not a file".into())
    );
    assert_eq!(
        history::read_entry_in(layout, &project, &file, "3000--")
            .unwrap()
            .entry
            .size,
        9
    );
    assert_eq!(snapshot(root), before);

    let other = project.join("other.txt");
    fs::write(&other, b"other").unwrap();
    let bad_dir = directory(&project, "other.txt");
    fs::create_dir_all(bad_dir.parent().unwrap()).unwrap();
    fs::write(&bad_dir, b"not a directory").unwrap();
    let before = snapshot(root);
    assert_eq!(
        history::list_entries(text(&project), text(&other)),
        Err(fs::read_dir(&bad_dir).unwrap_err().to_string())
    );
    assert_eq!(snapshot(root), before);
}

#[test]
fn legacy_record_preserves_matching_binary_size_and_self_history_skips() {
    let scratch = scratch::Scratch::new("legacy-history-skips");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let binary = project.join("binary");
    let large = project.join("large");
    let own = project.join(".aeroric/local-history/manual.txt");
    fs::create_dir_all(own.parent().unwrap()).unwrap();
    fs::write(&binary, [0xff]).unwrap();
    fs::write(&large, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    fs::write(&own, b"history itself").unwrap();
    let before = snapshot(root);
    assert!(record(&project, &file, "current\n").unwrap().is_none());
    for path in [&binary, &large, &own] {
        assert!(record(&project, path, "replacement").unwrap().is_none());
    }
    assert_eq!(snapshot(root), before);
    let exact = project.join("exact");
    let payload = vec![b'x'; 2 * 1024 * 1024];
    fs::write(&exact, &payload).unwrap();
    let before = snapshot(root);
    let entry = record(&project, &exact, "next").unwrap().unwrap();
    assert_eq!(entry.size, payload.len() as u64);
    let added = new_snapshot_paths(
        root,
        &before,
        &directory(&project, "exact"),
        &entry.id,
        &payload,
    );
    assert_changes(root, before, &added, &[]);
}

#[test]
fn legacy_restore_skips_matching_and_invalid_current_text_unlike_native_force() {
    let scratch = scratch::Scratch::new("legacy-history-restore");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let history_dir = directory(&project, "src/app-日本語.ts");
    fs::create_dir_all(&history_dir).unwrap();
    fs::write(history_dir.join("1000.txt"), b"snapshot\n").unwrap();
    let before = snapshot(root);
    history::restore_entry(text(&project), text(&file), "1000").unwrap();
    let entries = history::list_entries(text(&project), text(&file)).unwrap();
    assert_eq!(entries.len(), 2);
    let backup = entries.iter().find(|e| e.id != "1000").unwrap();
    let added = new_snapshot_paths(root, &before, &history_dir, &backup.id, b"current\n");
    assert_changes(root, before, &added, &[(file.clone(), b"snapshot\n")]);
    for current in [
        b"snapshot\n".to_vec(),
        vec![0xff],
        vec![b'x'; 2 * 1024 * 1024 + 1],
    ] {
        fs::write(&file, current).unwrap();
        let before = snapshot(root);
        history::restore_entry(text(&project), text(&file), "1000").unwrap();
        assert_eq!(
            history::list_entries(text(&project), text(&file)).unwrap(),
            entries
        );
        assert_changes(root, before, &[], &[(file.clone(), b"snapshot\n")]);
    }
    let native_dir = project.join(".aeroric/local-history/src/app-日本語.ts");
    fs::create_dir_all(&native_dir).unwrap();
    fs::write(native_dir.join("1000.txt"), b"snapshot\n").unwrap();
    let before = snapshot(root);
    let canonical_project = project.canonicalize().unwrap();
    let canonical_file = file.canonicalize().unwrap();
    native::restore_entry(
        native::CODE_LAYOUT,
        &canonical_project,
        &canonical_file,
        "1000",
        9000,
    )
    .unwrap();
    let added = new_snapshot_paths(root, &before, &native_dir, "9000", b"snapshot\n");
    assert_changes(root, before, &added, &[(file.clone(), b"snapshot\n")]);
    fs::write(&file, [0xff]).unwrap();
    let before = snapshot(root);
    assert_eq!(
        native::restore_entry(
            native::CODE_LAYOUT,
            &canonical_project,
            &canonical_file,
            "1000",
            10000
        ),
        Err(fs::read_to_string(&file).unwrap_err().to_string())
    );
    assert_eq!(snapshot(root), before);
}

#[test]
fn legacy_prune_keeps_sequence_order_and_post_create_error_side_effect() {
    let scratch = scratch::Scratch::new("legacy-history-prune");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let prune_dir = project.join("prune");
    fs::create_dir(&prune_dir).unwrap();
    for id in ["1000", "1001", "1001-2", "1001-10"] {
        fs::write(
            prune_dir.join(format!("{id}.txt")),
            b"retained or explicitly pruned",
        )
        .unwrap();
    }
    fs::write(prune_dir.join("unrelated"), b"retained").unwrap();
    let mut expected = snapshot(root);
    history::prune_history_dir(&prune_dir, 2).unwrap();
    for id in ["1000", "1001"] {
        assert!(expected
            .remove(
                prune_dir
                    .join(format!("{id}.txt"))
                    .strip_prefix(root)
                    .unwrap()
            )
            .is_some());
    }
    let after = snapshot(root);
    let relative_dir = prune_dir.strip_prefix(root).unwrap();
    expected.get_mut(relative_dir).unwrap().modified = after[relative_dir].modified;
    assert_eq!(after, expected);

    let history_dir = directory(&project, "src/app-日本語.ts");
    fs::create_dir_all(history_dir.join("0.txt")).unwrap();
    for id in 1..100 {
        fs::write(history_dir.join(format!("{id}.txt")), b"old").unwrap();
    }
    let before = snapshot(root);
    assert_eq!(
        record(&project, &file, "next"),
        Err(fs::remove_file(history_dir.join("0.txt"))
            .unwrap_err()
            .to_string())
    );
    let entries = history::list_entries(text(&project), text(&file)).unwrap();
    assert_eq!(entries.len(), 100);
    let new = entries
        .iter()
        .find(|e| e.created_at_ms > 1_577_836_800_000)
        .unwrap();
    let added = new_snapshot_paths(root, &before, &history_dir, &new.id, b"current\n");
    assert_changes(root, before, &added, &[]);
}

#[test]
fn legacy_source_admission_and_history_links_are_distinct_boundaries() {
    let scratch = scratch::Scratch::new("legacy-history-links");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let outside = root.join("outside");
    fs::write(&outside, b"safe scratch referent").unwrap();
    let before = snapshot(root);
    assert_eq!(
        history::list_entries(text(&project), text(&outside)),
        Err("File path is outside the project".into())
    );
    assert!(history::list_entries(text(&project), text(&file))
        .unwrap()
        .is_empty());
    assert_eq!(snapshot(root), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let history_dir = directory(&project, "src/app-日本語.ts");
        fs::create_dir_all(&history_dir).unwrap();
        fs::write(history_dir.join("1000.txt"), b"ordinary snapshot").unwrap();
        symlink(&outside, history_dir.join("2000.txt")).unwrap();
        symlink(&file, project.join("source-link")).unwrap();
        symlink(&outside, project.join("outside-link")).unwrap();
        symlink(&project, root.join("root-link")).unwrap();
        let before = snapshot(root);
        let entries = history::list_entries(
            text(&root.join("root-link")),
            text(&project.join("source-link")),
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_path, text(&file.canonicalize().unwrap()));
        assert_eq!(
            history::read_entry(text(&project), text(&file), "2000")
                .unwrap()
                .content,
            "safe scratch referent"
        );
        assert_eq!(
            history::list_entries(text(&project), text(&project.join("outside-link"))),
            Err("File path is outside the project".into())
        );
        assert_eq!(snapshot(root), before);
        let raw = project.join("src/raw\\name.txt");
        fs::write(&raw, b"source").unwrap();
        let alternate = root.join("alternate-history");
        fs::create_dir(&alternate).unwrap();
        fs::write(alternate.join("5000.txt"), b"alternate").unwrap();
        symlink(&alternate, directory(&project, "src/raw\\name.txt")).unwrap();
        let before = snapshot(root);
        assert_eq!(
            history::read_entry(text(&project), text(&raw), "5000")
                .unwrap()
                .content,
            "alternate"
        );
        assert_eq!(snapshot(root), before);
    }
}

#[test]
fn raw_layout_helpers_keep_caller_validation_responsibility_and_error_precedence() {
    let scratch = scratch::Scratch::new("legacy-history-raw-helpers");
    let root = scratch.path();
    let (project, _) = fixture(root);
    let missing = project.join("missing.txt");
    let history_dir = directory(&project, "missing.txt");
    fs::create_dir_all(&history_dir).unwrap();
    fs::write(
        history_dir.join("1000.txt"),
        b"source file not required by raw helper",
    )
    .unwrap();
    let before = snapshot(root);
    let entries = history::list_entries_in(native::CODE_LAYOUT, &project, &missing).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_path, text(&missing));
    assert_eq!(
        history::read_entry_in(native::CODE_LAYOUT, &project, &missing, "1000")
            .unwrap()
            .content,
        "source file not required by raw helper"
    );
    assert_eq!(
        history::list_entries(text(&project), text(&missing)),
        Err(format!(
            "Cannot resolve file path: {}",
            missing.canonicalize().unwrap_err()
        ))
    );
    let outside = root.join("not-a-project-source");
    assert_eq!(
        history::read_entry_in(native::CODE_LAYOUT, &project, &outside, "../bad"),
        Err("Invalid local history entry id".into())
    );
    assert_eq!(
        history::read_entry_in(native::CODE_LAYOUT, &project, &outside, "1000"),
        Err("File path is outside the project".into())
    );
    assert_eq!(snapshot(root), before);
}
