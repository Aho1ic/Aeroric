//! Definition-only legacy compatibility with synthetic projects and writer leases.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use aeroric_core::clock::FixedClock;
use aeroric_core::context::CoreContext;
use aeroric_core::files::path_policy::{validate_legacy_project_root, validate_project_root};
use aeroric_core::ide::run_config::{
    read_run_configs, read_run_configs_from_root, validate_document, validate_legacy_run_config,
    write_run_configs, write_run_configs_legacy, RunConfigDocument, RunConfigError,
};
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use serde_json::{json, Value};

#[path = "common/scratch.rs"]
mod scratch;

struct Local;
impl StorageLocationProbe for Local {
    fn probe(&self, directory: &Path) -> StorageLocation {
        StorageLocation::new(directory, VolumeKind::Local)
    }
}

struct Writer {
    context: CoreContext,
    root: scratch::Scratch,
}
impl Writer {
    fn new(label: &str) -> Self {
        let root = scratch::Scratch::new(label);
        let context = CoreContext::acquire(
            AppPaths::with_data_dir(root.path(), Some(&root.path().join("data"))),
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &LeaseOwner {
                application: "run-config-test".into(),
                version: "test".into(),
            },
            &Local,
        )
        .unwrap();
        Self { context, root }
    }
}

fn document(version: u32) -> RunConfigDocument {
    serde_json::from_value(json!({"version": version, "configs": [
        {"type":"shell", "id":"same", "name":" 中文 shell ", "command":"never executed", "cwd":".", "env":{"FOO":" retained "}},
        {"type":"debug", "id":"same", "name":"debug", "debugType":"python",
         "program":"missing.py", "cwd":"missing-dir/..", "args":["--fake"], "env":{},
         "breakpoints":[{"file":"missing.py", "line":3, "column":1}]}
    ]})).unwrap()
}

fn file(project: &Path) -> PathBuf {
    project.join(".aeroric/run-configs.json")
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    u32::from(fs::metadata(path).unwrap().permissions().readonly())
}

fn seed(project: &Path, bytes: &[u8]) {
    let file = file(project);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file.parent().unwrap(), fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o640)).unwrap();
    }
}

fn snapshot(project: &Path) -> (Vec<u8>, u32, u32, Vec<String>) {
    let file = file(project);
    let directory = file.parent().unwrap();
    (
        fs::read(&file).unwrap(),
        mode(&file),
        mode(directory),
        entries(directory),
    )
}

#[test]
fn legacy_root_guard_preserves_original_path_canonicalization_and_error_order() {
    let scratch = scratch::Scratch::new("run-config-roots");
    for value in ["", "relative/path"] {
        assert_eq!(
            validate_legacy_project_root(value).unwrap_err(),
            "Project path must be absolute"
        );
        assert_eq!(
            read_run_configs(value).unwrap_err().to_string(),
            "Project path must be absolute"
        );
        assert_eq!(
            write_run_configs_legacy(value, document(99))
                .unwrap_err()
                .to_string(),
            "Project path must be absolute"
        );
    }
    let absent = scratch.path().join("absent");
    let error = validate_legacy_project_root(absent.to_str().unwrap()).unwrap_err();
    assert!(error.starts_with("Cannot resolve project path:"), "{error}");
    assert_eq!(
        validate_project_root(absent.to_str().unwrap()).unwrap_err(),
        "Project path does not exist"
    );
    assert!(!absent.exists());
    let plain_file = scratch.path().join("not-a-directory");
    fs::write(&plain_file, "kept").unwrap();
    assert_eq!(
        validate_legacy_project_root(plain_file.to_str().unwrap()).unwrap_err(),
        "Project path is not a directory"
    );
    let project = scratch.path().join("project 中文 ");
    fs::create_dir_all(project.join("nested")).unwrap();
    let source = project.join("nested/..");
    assert_eq!(
        validate_legacy_project_root(source.to_str().unwrap()).unwrap(),
        project.canonicalize().unwrap()
    );
    write_run_configs_legacy(source.to_str().unwrap(), document(99)).unwrap();
    assert!(file(&project).exists());
    assert!(!scratch.path().join("project 中文").exists());
}

#[test]
fn reads_preserve_versions_duplicates_defaults_bytes_modes_and_directory_entries() {
    let scratch = scratch::Scratch::new("run-config-read");
    let project = scratch.path().join("project");
    fs::create_dir(&project).unwrap();
    assert_eq!(
        read_run_configs(project.to_str().unwrap()).unwrap(),
        RunConfigDocument::default()
    );
    assert!(!project.join(".aeroric").exists());
    for version in [0, 1, 99] {
        seed(&project, &serde_json::to_vec(&document(version)).unwrap());
        let before = snapshot(&project);
        let expected = document(if version == 0 { 1 } else { version });
        assert_eq!(
            read_run_configs(project.to_str().unwrap()).unwrap(),
            expected
        );
        assert_eq!(read_run_configs_from_root(&project).unwrap(), expected);
        assert_eq!(snapshot(&project), before);
    }
    // Retain serde's struct defaults/sequence representation; the document is
    // not restricted to a new object-only format during this migration.
    for raw in [b"{}".as_slice(), b"[]", b"[0,[]]"] {
        seed(&project, raw);
        let before = snapshot(&project);
        assert_eq!(
            read_run_configs(project.to_str().unwrap()).unwrap(),
            RunConfigDocument::default()
        );
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn checked_and_legacy_writes_replace_whole_documents_with_exact_pretty_json_and_inherited_modes() {
    let writer = Writer::new("run-config-write");
    for checked in [true, false] {
        let project = writer.root.path().join(format!("project-{checked} 中文 "));
        seed(&project, b"corrupt old document, explicitly replaced");
        let result = if checked {
            write_run_configs(&writer.context, project.to_str().unwrap(), document(99))
        } else {
            write_run_configs_legacy(project.to_str().unwrap(), document(99))
        }
        .unwrap();
        assert_eq!(result, document(1));
        assert_eq!(
            fs::read_to_string(file(&project)).unwrap(),
            serde_json::to_string_pretty(&result).unwrap()
        );
        assert_eq!(entries(&project.join(".aeroric")), ["run-configs.json"]);
        assert!(!project.join("missing.py").exists());
        assert!(!project.join("missing-dir").exists());
        let inherited = writer.root.path().join(format!("mode-{checked}"));
        fs::write(&inherited, "control").unwrap();
        assert_eq!(mode(&file(&project)), mode(&inherited));
        #[cfg(unix)]
        assert_eq!(mode(&project.join(".aeroric")), 0o750);
    }
}

#[test]
fn native_policy_stays_distinct_from_path_aware_legacy_definition_validation() {
    let root = Path::new("/synthetic-project");
    let duplicate = document(1);
    assert!(validate_document(&duplicate)
        .unwrap_err()
        .contains("duplicate config id"));
    for config in &duplicate.configs {
        validate_legacy_run_config(root, config).unwrap();
    }
    assert!(validate_document(&document(99))
        .unwrap_err()
        .contains("version"));
    let mut value = serde_json::to_value(document(1)).unwrap();
    value["configs"].as_array_mut().unwrap().truncate(1);
    value["configs"][0]["command"] = json!(" ");
    let invalid: RunConfigDocument = serde_json::from_value(value).unwrap();
    validate_document(&invalid).unwrap();
    assert_eq!(
        validate_legacy_run_config(root, &invalid.configs[0]).unwrap_err(),
        "Run config command cannot be empty"
    );
}

#[test]
fn all_legacy_config_validation_precedes_directory_creation_and_keeps_existing_file() {
    let scratch = scratch::Scratch::new("run-config-validation");
    let project = scratch.path().join("project");
    fs::create_dir(&project).unwrap();
    let cases = [
        ("/configs/0/id", json!(" "), "Run config id cannot be empty"),
        (
            "/configs/0/name",
            json!(" "),
            "Run config name cannot be empty",
        ),
        (
            "/configs/0/command",
            json!(" "),
            "Run config command cannot be empty",
        ),
        (
            "/configs/0/cwd",
            json!(" "),
            "Run config cwd cannot be empty",
        ),
        (
            "/configs/0/cwd",
            json!("../outside"),
            "Run config cwd is outside project root",
        ),
        (
            "/configs/1/program",
            json!(" "),
            "Run debug program cannot be empty",
        ),
        (
            "/configs/1/program",
            json!("../outside"),
            "Run debug program is outside project root",
        ),
        (
            "/configs/1/cwd",
            json!("../outside"),
            "Run config cwd is outside project root",
        ),
        (
            "/configs/1/breakpoints/0/line",
            json!(0),
            "Run debug breakpoint line must be at least 1",
        ),
        (
            "/configs/1/breakpoints/0/column",
            json!(0),
            "Run debug breakpoint column must be at least 1",
        ),
        (
            "/configs/1/breakpoints/0/file",
            json!(" "),
            "Run debug breakpoint file cannot be empty",
        ),
        (
            "/configs/1/breakpoints/0/file",
            json!("../outside"),
            "Run debug breakpoint is outside project root",
        ),
    ];
    for existing in [false, true] {
        if existing {
            seed(&project, b"prior unchanged bytes");
        }
        for (pointer, bad, expected) in &cases {
            let mut value = serde_json::to_value(document(99)).unwrap();
            *value.pointer_mut(pointer).unwrap() = bad.clone();
            let invalid = serde_json::from_value(value).unwrap();
            let error = write_run_configs_legacy(project.to_str().unwrap(), invalid).unwrap_err();
            assert!(matches!(error, RunConfigError::InvalidArgument(_)));
            assert_eq!(error.to_string(), *expected);
            if existing {
                assert_eq!(fs::read(file(&project)).unwrap(), b"prior unchanged bytes");
            } else {
                assert!(!project.join(".aeroric").exists());
            }
        }
    }
}

#[test]
fn parse_and_io_errors_do_not_repair_and_failed_atomic_rename_cleans_temps() {
    let scratch = scratch::Scratch::new("run-config-io");
    let project = scratch.path().join("project");
    for bytes in [b"{invalid".as_slice(), &[0xff, 0xfe], b"null"] {
        seed(&project, bytes);
        let before = snapshot(&project);
        assert!(matches!(
            read_run_configs(project.to_str().unwrap()),
            Err(RunConfigError::Io(_))
        ));
        assert_eq!(snapshot(&project), before);
    }
    let config = file(&project);
    fs::remove_file(&config).unwrap();
    fs::create_dir(&config).unwrap();
    fs::write(config.join("keep"), "unchanged").unwrap();
    assert!(matches!(
        read_run_configs(project.to_str().unwrap()),
        Err(RunConfigError::Io(_))
    ));
    assert!(matches!(
        write_run_configs_legacy(project.to_str().unwrap(), document(1)),
        Err(RunConfigError::Io(_))
    ));
    assert_eq!(entries(&project.join(".aeroric")), ["run-configs.json"]);
    assert_eq!(
        fs::read_to_string(config.join("keep")).unwrap(),
        "unchanged"
    );

    let not_dir = scratch.path().join("config-directory-file");
    fs::create_dir(&not_dir).unwrap();
    fs::write(not_dir.join(".aeroric"), "file, not directory").unwrap();
    assert_eq!(
        read_run_configs(not_dir.to_str().unwrap()).unwrap(),
        RunConfigDocument::default()
    );
    assert!(matches!(
        write_run_configs_legacy(not_dir.to_str().unwrap(), document(1)),
        Err(RunConfigError::Io(_))
    ));
    assert_eq!(
        fs::read_to_string(not_dir.join(".aeroric")).unwrap(),
        "file, not directory"
    );
}

#[test]
fn missing_or_lost_writer_lease_refuses_before_path_io_but_explicit_legacy_keeps_authority() {
    let scratch = scratch::Scratch::new("run-config-authority");
    let context = CoreContext::read_only(AppPaths::with_data_dir(
        scratch.path(),
        Some(&scratch.path().join("data")),
    ));
    let project = scratch.path().join("project");
    for path in [project.to_str().unwrap(), "relative"] {
        assert!(matches!(
            write_run_configs(&context, path, document(1)),
            Err(RunConfigError::WritePermission(_))
        ));
    }
    assert!(!project.exists());
    assert!(!context.paths.data_dir().exists());
    fs::create_dir(&project).unwrap();
    write_run_configs_legacy(project.to_str().unwrap(), document(99)).unwrap();
    assert_eq!(
        read_run_configs(project.to_str().unwrap()).unwrap(),
        document(1)
    );
    assert!(!context.paths.data_dir().exists());

    let writer = Writer::new("run-config-lost-lease");
    let lease = writer.context.lease().unwrap().lock_path().to_owned();
    fs::rename(&lease, lease.with_extension("old")).unwrap();
    fs::write(&lease, "replacement inode").unwrap();
    let error = write_run_configs(&writer.context, "relative", document(1)).unwrap_err();
    assert!(matches!(error, RunConfigError::WritePermission(_)));
    assert!(error.to_string().contains("lease was lost"), "{error}");
}

#[cfg(unix)]
#[test]
fn canonical_root_and_config_directory_links_are_followed_but_leaf_replacement_keeps_target() {
    use std::os::unix::fs::symlink;
    let scratch = scratch::Scratch::new("run-config-links");
    let actual = scratch.path().join("actual");
    let project = scratch.path().join("linked root ");
    let directory = scratch.path().join("config-directory");
    fs::create_dir(&actual).unwrap();
    fs::create_dir(&directory).unwrap();
    symlink(&actual, &project).unwrap();
    symlink(&directory, actual.join(".aeroric")).unwrap();
    let target = scratch.path().join("target.json");
    let bytes = serde_json::to_vec(&document(99)).unwrap();
    fs::write(&target, &bytes).unwrap();
    let config = directory.join("run-configs.json");
    symlink(&target, &config).unwrap();
    assert_eq!(
        read_run_configs(project.to_str().unwrap()).unwrap(),
        document(99)
    );
    assert!(fs::symlink_metadata(&config)
        .unwrap()
        .file_type()
        .is_symlink());
    write_run_configs_legacy(project.to_str().unwrap(), document(99)).unwrap();
    assert!(!fs::symlink_metadata(&config)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&target).unwrap(), bytes);
    assert!(fs::symlink_metadata(actual.join(".aeroric"))
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(&config).unwrap();
    let absent = scratch.path().join("absent");
    symlink(&absent, &config).unwrap();
    assert_eq!(
        read_run_configs(project.to_str().unwrap()).unwrap(),
        RunConfigDocument::default()
    );
    assert!(fs::symlink_metadata(&config)
        .unwrap()
        .file_type()
        .is_symlink());
    write_run_configs_legacy(project.to_str().unwrap(), document(1)).unwrap();
    assert!(!fs::symlink_metadata(&config)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!absent.exists());
}

#[test]
fn mixed_hosted_and_legacy_concurrent_writes_keep_whole_documents_for_lock_free_readers() {
    let writer = Writer::new("run-config-concurrency");
    let project = writer.root.path().join("project");
    fs::create_dir(&project).unwrap();
    let documents: Vec<_> = (0..6)
        .map(|i| {
            let mut value = serde_json::to_value(document(1)).unwrap();
            value["configs"][0]["name"] = json!(format!("writer-{i}"));
            value["configs"][0]["command"] = Value::String("never executed ".repeat(100));
            serde_json::from_value::<RunConfigDocument>(value).unwrap()
        })
        .collect();
    write_run_configs(
        &writer.context,
        project.to_str().unwrap(),
        documents[0].clone(),
    )
    .unwrap();
    let start = Barrier::new(7);
    std::thread::scope(|scope| {
        for (i, document) in documents.iter().enumerate() {
            let project = &project;
            let context = &writer.context;
            let start = &start;
            scope.spawn(move || {
                start.wait();
                for _ in 0..8 {
                    let result = if i % 2 == 0 {
                        write_run_configs(context, project.to_str().unwrap(), document.clone())
                    } else {
                        write_run_configs_legacy(project.to_str().unwrap(), document.clone())
                    }
                    .unwrap();
                    assert_eq!(&result, document);
                }
            });
        }
        start.wait();
        for _ in 0..96 {
            let actual = read_run_configs(project.to_str().unwrap()).unwrap();
            assert!(documents.contains(&actual));
        }
    });
    assert!(documents.contains(&read_run_configs(project.to_str().unwrap()).unwrap()));
    assert_eq!(entries(&project.join(".aeroric")), ["run-configs.json"]);
}
