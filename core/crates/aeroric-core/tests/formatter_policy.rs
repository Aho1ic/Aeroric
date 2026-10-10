//! Shared formatter policy and native pre-spawn refusal, not a formatter-run
//! or rollback guarantee. Every filesystem target/referent stays in scratch.

use aeroric_core::files::format;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "common/scratch.rs"]
mod scratch;

#[derive(PartialEq, Eq)]
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

fn assert_unchanged(root: &Path, expected: BTreeMap<PathBuf, Entry>) {
    let after = snapshot(root);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (path, entry) in expected {
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
    let file = project.join("src/文档.unknownfmt");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"\xef\xbb\xbfsentinel\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    (project, file)
}

fn resolve_error(path: &Path) -> String {
    format!(
        "Cannot resolve file path: {}",
        path.canonicalize().unwrap_err()
    )
}

#[test]
fn complete_selector_table_keeps_normalization_and_literal_arguments() {
    let path = "literal 空 格/'$(not-run)';--file\0tail";
    let prettier = [
        "js", "jsx", "ts", "tsx", "css", "scss", "json", "jsonc", "md", "mdx", "html", "yaml",
        "yml",
    ];
    let mut expected: Vec<(&str, &str, Vec<&str>)> = prettier
        .iter()
        .map(|extension| {
            (
                *extension,
                "pnpm",
                vec!["exec", "prettier", "--write", path],
            )
        })
        .collect();
    expected.extend([
        ("rs", "rustfmt", vec![path]),
        ("go", "gofmt", vec!["-w", path]),
        ("py", "ruff", vec!["format", path]),
    ]);
    assert_eq!(expected.len(), 16);
    for (extension, program, args) in expected {
        for spelling in [
            extension.to_string(),
            extension.to_ascii_uppercase(),
            format!(".{extension}"),
            format!("...{}", extension.to_ascii_uppercase()),
        ] {
            let command = format::formatter_for_extension(&spelling, path).unwrap();
            assert_eq!(command.program, program);
            assert_eq!(command.args, args, "{spelling}");
        }
    }
    for extension in [
        "", ".", "...", "js ", " js", ". js", "file.ts", "/x.rs", "ＪＳ", "py\0", "js\n", "png",
        "c", "cpp", "sh", "txt", "vue", "svelte",
    ] {
        assert!(format::formatter_for_extension(extension, path).is_none());
    }
}

#[test]
fn shared_types_keep_traits_and_exact_camelcase_result_wire() {
    let command = format::FormatterCommand {
        program: "literal program".into(),
        args: vec!["argument with spaces".into()],
    };
    assert_eq!(command, command.clone());
    let result = format::FormatFileResult {
        file_path: " 空/文档.rs\r\n".into(),
        command: "literal \"command\" \0 not a shell script".into(),
    };
    assert_eq!(
        serde_json::to_value(result.clone()).unwrap(),
        serde_json::json!({
            "filePath": " 空/文档.rs\r\n",
            "command": "literal \"command\" \0 not a shell script"
        })
    );
    assert!(format!("{result:?}").starts_with("FormatFileResult"));
}

#[test]
fn readonly_validator_preserves_canonical_paths_and_error_precedence() {
    let scratch = scratch::Scratch::new("formatter-path-policy");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let canonical_root = project.canonicalize().unwrap();
    let canonical_file = file.canonicalize().unwrap();
    let outside_directory = root.join("outside directory");
    fs::create_dir(&outside_directory).unwrap();
    let before = snapshot(root);
    for relative in ["", "relative\0file", "../file.rs", "./file.rs"] {
        assert_eq!(
            format::validate_file_path(&canonical_root, relative).unwrap_err(),
            "File path must be absolute"
        );
    }
    for path in [project.join("absent.rs"), project.join("nul\0.rs")] {
        assert_eq!(
            format::validate_file_path(&canonical_root, text(&path)).unwrap_err(),
            resolve_error(&path)
        );
    }
    assert_eq!(
        format::validate_file_path(&canonical_root, text(&outside_directory)).unwrap_err(),
        "File path is outside the project"
    );
    assert_eq!(
        format::validate_file_path(&canonical_root, text(&project)).unwrap_err(),
        "File path is not a file"
    );
    assert_eq!(
        format::validate_file_path(&canonical_root, text(&file)).unwrap(),
        canonical_file
    );
    // Root admission is the caller's responsibility, not duplicated by this helper.
    assert_eq!(
        format::validate_file_path(&canonical_file, text(&file)).unwrap(),
        canonical_file
    );
    assert_unchanged(root, before);
}

#[test]
fn canonical_aliases_keep_the_supplied_root_policy_without_writes() {
    let scratch = scratch::Scratch::new("formatter-path-aliases");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let canonical_root = project.canonicalize().unwrap();
    let canonical_file = file.canonicalize().unwrap();
    let alias = project.join("src/../src/文档.unknownfmt");
    let before = snapshot(root);
    assert_eq!(
        format::validate_file_path(&canonical_root, text(&alias)).unwrap(),
        canonical_file
    );
    assert_unchanged(root, before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
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
        let before = snapshot(root);
        for path in [&inward, &return_link] {
            assert_eq!(
                format::validate_file_path(&canonical_root, text(path)).unwrap(),
                canonical_file
            );
        }
        assert_eq!(
            format::validate_file_path(&canonical_root, text(&escape)).unwrap_err(),
            "File path is outside the project"
        );
        for path in [&dangling, &looping] {
            assert_eq!(
                format::validate_file_path(&canonical_root, text(path)).unwrap_err(),
                resolve_error(path)
            );
        }
        assert_eq!(
            format::validate_file_path(&root_alias, text(&file)).unwrap_err(),
            "File path is outside the project"
        );
        assert_unchanged(root, before);
    }
}

#[test]
fn native_pre_spawn_refusals_keep_snapshot_callback_order() {
    use std::cell::{Cell, RefCell};
    let scratch = scratch::Scratch::new("formatter-native-refusals");
    let root = scratch.path();
    let (project, file) = fixture(root);
    let canonical_root = project.canonicalize().unwrap();
    let canonical_file = file.canonicalize().unwrap();
    let no_extension = project.join("no-extension");
    fs::write(&no_extension, b"sentinel").unwrap();
    let before = snapshot(root);
    let calls = Cell::new(0);
    let reject = |path: &str| {
        calls.set(calls.get() + 1);
        assert_eq!(path, text(&canonical_file));
        Err("snapshot refusal sentinel".into())
    };
    assert_eq!(
        format::format_file(&canonical_root, "relative\0file", Some(&reject)).unwrap_err(),
        "File path must be absolute"
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(
        format::format_file(&canonical_root, text(&file), Some(&reject)).unwrap_err(),
        "snapshot refusal sentinel"
    );
    assert_eq!(calls.get(), 1);
    let observed = RefCell::new(Vec::new());
    let record = |path: &str| {
        observed.borrow_mut().push(path.to_string());
        Ok(())
    };
    for path in [&file, &no_extension] {
        assert_eq!(
            format::format_file(&canonical_root, text(path), Some(&record)).unwrap_err(),
            "No formatter is configured for this file type."
        );
    }
    assert_eq!(
        *observed.borrow(),
        vec![
            text(&canonical_file).to_string(),
            text(&no_extension.canonicalize().unwrap()).to_string()
        ]
    );
    assert_eq!(
        format::format_file(&canonical_root, text(&file), None).unwrap_err(),
        "No formatter is configured for this file type."
    );
    assert_unchanged(root, before);
}
