//! Local project-config mutation compatibility; only isolated filesystem fixtures.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};

use aeroric_core::clock::FixedClock;
use aeroric_core::context::CoreContext;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::settings::project_config::{
    init_project_config, init_project_config_legacy, read_project_config, write_project_config,
    write_project_config_legacy, ProjectConfig, ProjectConfigWriteError, DEFAULT_CONFIG,
};
use aeroric_core::storage::atomic::write_legacy_atomic;
use aeroric_core::storage_lock::LeaseOwner;

#[path = "common/scratch.rs"]
mod scratch;

struct Local;
impl StorageLocationProbe for Local {
    fn probe(&self, directory: &Path) -> StorageLocation {
        StorageLocation::new(directory, VolumeKind::Local)
    }
}

struct Writer {
    // Release the lease before deleting the fixture root.
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
                application: "config-test".into(),
                version: "test".into(),
            },
            &Local,
        )
        .unwrap();
        Self { context, root }
    }
}

fn replacement() -> ProjectConfig {
    let mut result = ProjectConfig::default();
    result.agent.default = " fixture-agent ".into();
    result.agent.prompt_prefix = "中文\nsecond line".into();
    result.git.commit_prompt = " fixture prompt ".into();
    result.git.commit_message_timeout_secs = 43;
    result.editor.format_on_save = true;
    result.editor.file_browser_sort.field = "custom-field".into();
    result
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn init_keeps_the_template_and_preserves_existing_valid_or_malformed_files() {
    let fixture = Writer::new("config-init-core");
    let project = fixture.root.path().join("project 中文 ");
    let directory = project.join(".aeroric");
    let file = directory.join("config.toml");
    let config = init_project_config(&fixture.context, project.to_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap(),
        serde_json::to_value(ProjectConfig::default()).unwrap()
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), DEFAULT_CONFIG);
    assert_eq!(entries(&directory), ["attachments", "config.toml"]);

    for bytes in [
        "[agent]\ndefault = 'kept'\n[git]\ncommit_prompt = 'custom'\n",
        "[broken",
        "",
    ] {
        fs::write(&file, bytes).unwrap();
        let expected = read_project_config(project.to_str().unwrap()).unwrap();
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        let actual = init_project_config(&fixture.context, project.to_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), bytes);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    }
}

#[test]
fn write_preserves_exact_pretty_toml_original_path_and_inherited_modes_without_attachments() {
    let fixture = Writer::new("config-write-core");
    let project = fixture.root.path().join("project 中文  ");
    let directory = project.join(".aeroric");
    let file = directory.join("config.toml");
    let decoy = fixture.root.path().join("project 中文");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&file, "# old comment\nunknown = 'removed'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let config = replacement();
    write_project_config(&fixture.context, project.to_str().unwrap(), &config).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        toml::to_string_pretty(&config).unwrap()
    );
    assert_eq!(entries(&directory), ["config.toml"]);
    assert!(!decoy.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let control = fixture.root.path().join("inherited");
        fs::write(&control, "control").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            fs::metadata(control).unwrap().permissions().mode() & 0o777
        );
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }
}

#[test]
fn read_only_context_refuses_before_io_but_explicit_legacy_entrypoints_keep_old_authority() {
    let root = scratch::Scratch::new("config-read-only-core");
    let paths = AppPaths::with_data_dir(root.path(), Some(&root.path().join("not-created-data")));
    let context = CoreContext::read_only(paths);
    let project = root.path().join("not-created-project");
    assert!(matches!(
        init_project_config(&context, project.to_str().unwrap()),
        Err(ProjectConfigWriteError::WritePermission(_))
    ));
    assert!(matches!(
        write_project_config(&context, project.to_str().unwrap(), &replacement()),
        Err(ProjectConfigWriteError::WritePermission(_))
    ));
    assert!(!project.exists());
    assert!(!context.paths.data_dir().exists());

    init_project_config_legacy(project.to_str().unwrap()).unwrap();
    let config = replacement();
    write_project_config_legacy(project.to_str().unwrap(), &config).unwrap();
    assert_eq!(
        fs::read_to_string(project.join(".aeroric/config.toml")).unwrap(),
        toml::to_string_pretty(&config).unwrap()
    );
    assert!(
        !context.paths.data_dir().exists(),
        "legacy path never grants a core lease"
    );
}

#[test]
fn io_failures_keep_legacy_directory_order_and_remove_failed_write_temps() {
    let fixture = Writer::new("config-errors-core");
    let project = fixture.root.path().join("project");
    let directory = project.join(".aeroric");
    let file = directory.join("config.toml");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&file, [0xff, 0xfe]).unwrap();
    assert!(init_project_config(&fixture.context, project.to_str().unwrap()).is_err());
    assert!(directory.join("attachments").is_dir());
    assert_eq!(fs::read(&file).unwrap(), [0xff, 0xfe]);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    fs::write(file.join("sentinel"), "keep").unwrap();
    assert!(
        write_project_config(&fixture.context, project.to_str().unwrap(), &replacement()).is_err()
    );
    assert_eq!(entries(&directory), ["attachments", "config.toml"]);
    assert_eq!(fs::read_to_string(file.join("sentinel")).unwrap(), "keep");
}

#[test]
fn checked_and_legacy_entrypoints_serialize_init_against_explicit_save() {
    let fixture = Writer::new("config-race-core");
    let config = replacement();
    for iteration in 0..24 {
        let project = fixture.root.path().join(format!("project-{iteration}"));
        let path = project.to_str().unwrap();
        let barrier = Barrier::new(3);
        std::thread::scope(|scope| {
            let init = scope.spawn(|| {
                barrier.wait();
                init_project_config(&fixture.context, path).unwrap();
            });
            let write = scope.spawn(|| {
                barrier.wait();
                write_project_config_legacy(path, &config).unwrap();
            });
            barrier.wait();
            init.join().unwrap();
            write.join().unwrap();
        });
        assert_eq!(
            fs::read_to_string(project.join(".aeroric/config.toml")).unwrap(),
            toml::to_string_pretty(&config).unwrap()
        );
        assert_eq!(
            entries(&project.join(".aeroric")),
            ["attachments", "config.toml"]
        );
    }
}

#[test]
fn legacy_atomic_mechanism_keeps_io_errors_and_no_parent_creation_or_failed_rename_temps() {
    let root = scratch::Scratch::new("legacy-atomic-core");
    let absent = root.path().join("missing-parent/file");
    let expected = fs::File::create(&absent).unwrap_err().to_string();
    assert_eq!(
        write_legacy_atomic(&absent, "content").unwrap_err(),
        expected
    );
    assert!(!absent.parent().unwrap().exists());
    let file = root.path().join("plain.txt");
    write_legacy_atomic(&file, "first").unwrap();
    write_legacy_atomic(&file, "second 中文").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "second 中文");
    let directory = root.path().join("not-a-file");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep"), "unchanged").unwrap();
    assert!(write_legacy_atomic(&directory, "do not replace the directory").is_err());
    assert_eq!(
        fs::read_to_string(directory.join("keep")).unwrap(),
        "unchanged"
    );
    assert_eq!(entries(root.path()), ["not-a-file", "plain.txt"]);
}

#[cfg(unix)]
#[test]
fn init_follows_existing_leaf_symlink_but_atomic_write_replaces_only_the_link() {
    let fixture = Writer::new("config-links-core");
    let project = fixture.root.path().join("linked-project");
    let directory = fixture.root.path().join("configuration");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&directory).unwrap();
    std::os::unix::fs::symlink(&directory, project.join(".aeroric")).unwrap();
    let target = fixture.root.path().join("target.toml");
    fs::write(&target, DEFAULT_CONFIG).unwrap();
    let link = directory.join("config.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    init_project_config(&fixture.context, project.to_str().unwrap()).unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    write_project_config(&fixture.context, project.to_str().unwrap(), &replacement()).unwrap();
    assert!(!fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(project.join(".aeroric"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), DEFAULT_CONFIG);
}
