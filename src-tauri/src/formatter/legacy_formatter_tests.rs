//! Frozen before/after policy and actual command refusals, never a successful
//! formatter run. Every filesystem referent and child environment is scratch.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_FORMATTER_CASE";
const ROOT: &str = "AERORIC_LEGACY_FORMATTER_ROOT";
const UNSUPPORTED: &str = "No formatter is configured for this file type.";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only the owned scratch tree");
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
            "no settings/home initialization"
        );
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 0);
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Scratch(
        PathBuf::from("/tmp").join(format!("legacy-formatter-{}-{nonce}", std::process::id())),
    );
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("formatter::legacy_formatter_tests::{name}"),
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
        .env("SHELL", root.0.join("bin/no-shell"))
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

#[derive(Clone, PartialEq, Eq)]
struct Entry {
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        entries.insert(
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
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn assert_snapshot(root: &Path, expected: BTreeMap<PathBuf, Entry>) {
    let after = snapshot(root);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (path, entry) in expected {
        // Do not dump large/binary content into a child process's bounded pipe.
        assert!(
            after[&path] == entry,
            "unexpected state: {}",
            path.display()
        );
    }
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let project = root.join("project with spaces");
    let file = project.join("src/original-日本語.unknownfmt");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"\xef\xbb\xbforiginal\r\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    (project, file)
}

fn format(project: &Path, file: &Path) -> Result<super::FormatFileResult, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(super::format_file(text(project).into(), text(file).into()))
}

fn resolve_error(path: &Path) -> String {
    format!(
        "Cannot resolve file path: {}",
        path.canonicalize().unwrap_err()
    )
}

// Independent legacy layout oracle; never call a production history helper.
fn history(project: &Path, relative: &str) -> PathBuf {
    let hex: String = relative
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    project.join(".aeroric/local-history").join(hex)
}

fn assert_only_snapshot_added(
    root: &Path,
    mut expected: BTreeMap<PathBuf, Entry>,
    directory: &Path,
    content: &[u8],
) {
    let files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let file = &files[0];
    assert_eq!(file.extension().unwrap(), "txt");
    let id = file.file_stem().unwrap().to_str().unwrap();
    assert!(id.chars().all(|ch| ch.is_ascii_digit() || ch == '-'));
    assert!(id.split('-').next().unwrap().parse::<u64>().unwrap() > 0);
    assert!(file.symlink_metadata().unwrap().is_file());
    assert_eq!(fs::read(file).unwrap(), content);
    let after = snapshot(root);
    let mut additions = vec![file.clone()];
    let mut next = directory;
    while !expected.contains_key(next.strip_prefix(root).unwrap()) {
        assert!(next.symlink_metadata().unwrap().is_dir());
        additions.push(next.to_path_buf());
        next = next.parent().unwrap();
    }
    let mut parents = BTreeSet::new();
    for path in additions {
        let path = path.strip_prefix(root).unwrap();
        assert!(!expected.contains_key(path));
        expected.insert(path.to_path_buf(), after[path].clone());
        parents.insert(path.parent().unwrap().to_path_buf());
    }
    for parent in parents {
        expected.get_mut(&parent).unwrap().modified = after[&parent].modified;
    }
    assert_snapshot(root, expected);
}

#[test]
fn selects_complete_extension_vocabulary_and_literal_argv() {
    isolated(
        "selects_complete_extension_vocabulary_and_literal_argv",
        |root| {
            let before = snapshot(root);
            let path = "/not-opened/空 格/--name;'$(literal)'.x\0tail";
            for ext in [
                "js", "jsx", "ts", "tsx", "css", "scss", "json", "jsonc", "md", "mdx", "html",
                "yaml", "yml", "rs", "go", "py",
            ] {
                let (program, prefix): (&str, &[&str]) = match ext {
                    "rs" => ("rustfmt", &[]),
                    "go" => ("gofmt", &["-w"]),
                    "py" => ("ruff", &["format"]),
                    _ => ("pnpm", &["exec", "prettier", "--write"]),
                };
                for spelling in [
                    ext.to_string(),
                    ext.to_ascii_uppercase(),
                    format!(".{ext}"),
                    format!("...{}", ext.to_ascii_uppercase()),
                ] {
                    let actual = super::formatter_for_extension(&spelling, path).unwrap();
                    assert_eq!(actual.program, program);
                    let mut args: Vec<String> = prefix.iter().map(|arg| (*arg).into()).collect();
                    args.push(path.into());
                    assert_eq!(actual.args, args, "{spelling}");
                    assert_eq!(actual, actual.clone());
                }
            }
            for unsupported in [
                "", ".", "...", "png", "c", "cpp", "sh", "txt", "vue", "svelte", " js", "js ",
                ". js", "file.ts", "/x.rs", "ＪＳ", "py\0", "js\n",
            ] {
                assert!(super::formatter_for_extension(unsupported, path).is_none());
            }
            assert_snapshot(root, before);
        },
    );
}

#[test]
fn format_result_keeps_exact_camelcase_wire_and_type_traits() {
    isolated(
        "format_result_keeps_exact_camelcase_wire_and_type_traits",
        |root| {
            let before = snapshot(root);
            let result = super::FormatFileResult {
                file_path: " 空/文档.rs\r\n".into(),
                command: "literal \"command\" \0 not a shell script".into(),
            };
            let encoded = serde_json::to_value(result.clone()).unwrap();
            assert_eq!(
                encoded,
                serde_json::json!({
                    "filePath": " 空/文档.rs\r\n",
                    "command": "literal \"command\" \0 not a shell script"
                })
            );
            assert!(format!("{result:?}").starts_with("FormatFileResult"));
            assert_snapshot(root, before);
        },
    );
}

#[test]
fn validator_preserves_error_order_and_canonical_path() {
    isolated(
        "validator_preserves_error_order_and_canonical_path",
        |root| {
            let (project, file) = fixture(root);
            let canonical_root = project.canonicalize().unwrap();
            let canonical_file = file.canonicalize().unwrap();
            let outside = root.join("outside directory");
            fs::create_dir(&outside).unwrap();
            let before = snapshot(root);
            for path in ["", "relative\0file", "./file.rs", "../file.rs"] {
                assert_eq!(
                    super::validate_file_path(&canonical_root, path).unwrap_err(),
                    "File path must be absolute"
                );
            }
            for path in [project.join("missing.rs"), project.join("nul\0.rs")] {
                assert_eq!(
                    super::validate_file_path(&canonical_root, text(&path)).unwrap_err(),
                    resolve_error(&path)
                );
            }
            assert_eq!(
                super::validate_file_path(&canonical_root, text(&outside)).unwrap_err(),
                "File path is outside the project"
            );
            assert_eq!(
                super::validate_file_path(&canonical_root, text(&project)).unwrap_err(),
                "File path is not a file"
            );
            for path in [
                file.clone(),
                project.join("src/../src/original-日本語.unknownfmt"),
            ] {
                assert_eq!(
                    super::validate_file_path(&canonical_root, text(&path)).unwrap(),
                    canonical_file
                );
            }
            // This helper assumes root admission already happened; it is not that guard.
            assert_eq!(
                super::validate_file_path(&canonical_file, text(&file)).unwrap(),
                canonical_file
            );
            assert_snapshot(root, before);
        },
    );
}

#[test]
fn validator_keeps_alias_and_containment_policy() {
    isolated("validator_keeps_alias_and_containment_policy", |root| {
        let (project, file) = fixture(root);
        let outside = root.join("outside.rs");
        fs::write(&outside, b"outside sentinel").unwrap();
        let inward = project.join("inward.rs");
        let return_link = root.join("return.rs");
        let escape = project.join("escape.rs");
        let dangling = project.join("dangling.rs");
        let looping = project.join("looping.rs");
        let root_alias = root.join("project alias");
        symlink(&file, &inward).unwrap();
        symlink(&file, &return_link).unwrap();
        symlink(&outside, &escape).unwrap();
        symlink(project.join("absent.rs"), &dangling).unwrap();
        symlink(&looping, &looping).unwrap();
        symlink(&project, &root_alias).unwrap();
        let canonical_root = project.canonicalize().unwrap();
        let before = snapshot(root);
        for path in [&inward, &return_link] {
            assert_eq!(
                super::validate_file_path(&canonical_root, text(path)).unwrap(),
                file.canonicalize().unwrap()
            );
        }
        assert_eq!(
            super::validate_file_path(&canonical_root, text(&escape)).unwrap_err(),
            "File path is outside the project"
        );
        for path in [&dangling, &looping] {
            assert_eq!(
                super::validate_file_path(&canonical_root, text(path)).unwrap_err(),
                resolve_error(path)
            );
        }
        assert_eq!(
            super::validate_file_path(&root_alias, text(&file)).unwrap_err(),
            "File path is outside the project"
        );
        assert_snapshot(root, before);
    });
}

#[test]
fn command_rejects_invalid_roots_and_files_before_history() {
    isolated(
        "command_rejects_invalid_roots_and_files_before_history",
        |root| {
            let (project, file) = fixture(root);
            let outside = root.join("outside.rs");
            fs::write(&outside, b"outside sentinel").unwrap();
            let escape = project.join("escape.rs");
            symlink(&outside, &escape).unwrap();
            let missing_root = root.join("missing project");
            let missing_file = project.join("absent.rs");
            let before = snapshot(root);
            assert_eq!(
                format(Path::new("relative\0project"), &file).unwrap_err(),
                "Project path must be absolute"
            );
            assert_eq!(
                format(&missing_root, Path::new("relative\0file")).unwrap_err(),
                format!(
                    "Cannot resolve project path: {}",
                    missing_root.canonicalize().unwrap_err()
                )
            );
            assert_eq!(
                format(&file, Path::new("relative\0file")).unwrap_err(),
                "Project path is not a directory"
            );
            for (path, error) in [
                (
                    PathBuf::from("relative\0file"),
                    "File path must be absolute".into(),
                ),
                (missing_file.clone(), resolve_error(&missing_file)),
                (project.clone(), "File path is not a file".into()),
                (outside, "File path is outside the project".into()),
                (escape, "File path is outside the project".into()),
            ] {
                assert_eq!(format(&project, &path).unwrap_err(), error);
            }
            assert_snapshot(root, before);
        },
    );
}

#[test]
fn command_records_legacy_snapshot_before_extension_refusal() {
    isolated(
        "command_records_legacy_snapshot_before_extension_refusal",
        |root| {
            let (project, file) = fixture(root);
            let no_extension = project.join("src/no-extension");
            fs::write(&no_extension, b"no extension\n").unwrap();
            let project_alias = root.join("project alias");
            let file_alias = root.join("apparently-supported.rs");
            symlink(&project, &project_alias).unwrap();
            symlink(&file, &file_alias).unwrap();
            let before = snapshot(root);
            // Canonical target extension wins, not the alias's apparently supported one.
            assert_eq!(
                format(&project_alias, &file_alias).unwrap_err(),
                UNSUPPORTED
            );
            assert_only_snapshot_added(
                root,
                before,
                &history(&project, "src/original-日本語.unknownfmt"),
                b"\xef\xbb\xbforiginal\r\n",
            );
            let before = snapshot(root);
            assert_eq!(format(&project, &no_extension).unwrap_err(), UNSUPPORTED);
            assert_only_snapshot_added(
                root,
                before,
                &history(&project, "src/no-extension"),
                b"no extension\n",
            );
        },
    );
}

#[test]
fn command_preserves_history_skips_before_extension_refusal() {
    isolated(
        "command_preserves_history_skips_before_extension_refusal",
        |root| {
            let (project, _) = fixture(root);
            let empty = project.join("empty.unknownfmt");
            let binary = project.join("binary.unknownfmt");
            let oversized = project.join("oversized.unknownfmt");
            let history_file = project.join(".aeroric/local-history/self.unknownfmt");
            fs::write(&empty, b"").unwrap();
            fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
            fs::write(&oversized, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
            fs::create_dir_all(history_file.parent().unwrap()).unwrap();
            fs::write(&history_file, b"history sentinel").unwrap();
            let before = snapshot(root);
            for path in [&empty, &binary, &oversized, &history_file] {
                assert_eq!(format(&project, path).unwrap_err(), UNSUPPORTED);
            }
            assert_snapshot(root, before);
        },
    );
}

#[test]
fn command_history_failure_precedes_extension_and_process_work() {
    isolated(
        "command_history_failure_precedes_extension_and_process_work",
        |root| {
            let (project, file) = fixture(root);
            fs::write(project.join(".aeroric"), b"history obstruction").unwrap();
            let before = snapshot(root);
            let expected = fs::create_dir_all(history(&project, "src/original-日本語.unknownfmt"))
                .unwrap_err()
                .to_string();
            let actual = format(&project, &file).unwrap_err();
            assert_eq!(actual, expected);
            assert_ne!(actual, UNSUPPORTED);
            assert_snapshot(root, before);
        },
    );
}
