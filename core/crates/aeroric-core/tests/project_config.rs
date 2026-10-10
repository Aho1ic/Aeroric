//! Project-local configuration compatibility, on scratch files only.

use std::fs;

use aeroric_core::settings::project_config::{read_project_config, ProjectConfig, DEFAULT_CONFIG};
use serde_json::{json, Value};

#[path = "common/scratch.rs"]
mod scratch;

fn value(config: ProjectConfig) -> Value {
    serde_json::to_value(config).unwrap()
}

#[test]
fn frozen_template_and_default_share_the_legacy_wire() {
    let default = value(ProjectConfig::default());
    assert_eq!(value(toml::from_str(DEFAULT_CONFIG).unwrap()), default);
    let mut keys: Vec<_> = default
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(keys, ["agent", "editor", "git"]);
    assert_eq!(
        default["agent"],
        json!({
            "default": "claude", "default_permission_mode": "full_access", "prompt_prefix": ""
        })
    );
    assert_eq!(default["git"]["commit_message_timeout_secs"], 15);
    assert_eq!(
        default["editor"],
        json!({
            "format_on_save": false,
            "file_browser_sort": {"field": "modified", "direction": "desc"},
            "sftp_sort": {"field": "modified", "direction": "desc"}
        })
    );
}

#[test]
fn legacy_optional_defaults_and_unvalidated_user_strings_are_retained() {
    let root = scratch::Scratch::new("project-config-legacy");
    let directory = root.path().join(".aeroric");
    fs::create_dir(&directory).unwrap();
    let file = directory.join("config.toml");
    let raw = b"[agent]\ndefault = ' CUSTOM '\n[git]\ncommit_prompt = ' fixture '\n[editor]\nfile_browser_sort = { field = ' custom ', direction = 'anything' }\n";
    fs::write(&file, raw).unwrap();
    let config = value(read_project_config(root.path().to_str().unwrap()).unwrap());
    assert_eq!(config["agent"]["default"], " CUSTOM ");
    assert_eq!(config["agent"]["default_permission_mode"], "full_access");
    assert_eq!(config["agent"]["prompt_prefix"], "");
    assert_eq!(config["git"]["commit_prompt"], " fixture ");
    assert_eq!(config["git"]["commit_message_timeout_secs"], 15);
    assert_eq!(config["editor"]["file_browser_sort"]["field"], " custom ");
    assert_eq!(
        config["editor"]["file_browser_sort"]["direction"],
        "anything"
    );
    assert_eq!(fs::read(file).unwrap(), raw);
    assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
}

#[test]
fn missing_or_malformed_toml_defaults_but_utf8_and_io_failures_propagate() {
    let root = scratch::Scratch::new("project-config-errors");
    let project = root.path().join("not-created");
    assert_eq!(
        value(read_project_config(project.to_str().unwrap()).unwrap()),
        value(ProjectConfig::default())
    );
    assert!(!project.exists());

    let directory = root.path().join(".aeroric");
    fs::create_dir(&directory).unwrap();
    let file = directory.join("config.toml");
    for raw in ["", "[invalid", "[agent]\ndefault = 'missing-git'"] {
        fs::write(&file, raw).unwrap();
        assert_eq!(
            value(read_project_config(root.path().to_str().unwrap()).unwrap()),
            value(ProjectConfig::default())
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), raw);
    }
    fs::write(&file, [0xff, 0xfe]).unwrap();
    assert!(read_project_config(root.path().to_str().unwrap()).is_err());
    assert_eq!(fs::read(&file).unwrap(), [0xff, 0xfe]);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert!(read_project_config(root.path().to_str().unwrap()).is_err());
    assert!(file.is_dir());
    assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
}
