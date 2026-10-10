//! Pure root composition and legacy lazy-callback compatibility; no real home.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use aeroric_core::paths::AppPaths;
use aeroric_core::settings::agent_config::{
    dsh_config_path, AgentConfigPaths, CustomAgentConfigPath,
};
use aeroric_core::settings::agent_config_roots::{builtin_path, custom_home, resolve_path};

#[path = "common/scratch.rs"]
mod scratch;

#[test]
fn builtin_filenames_and_only_the_selected_home_callback_are_preserved() {
    for (agent, expected, wanted) in [
        ("claude", "/external/.claude/settings.json", vec![]),
        ("claude_gpt55", "/external/.claude/start-gpt55.sh", vec![]),
        ("codex", "/external/.codex/config.toml", vec![]),
        ("dsh", "/provided/dsh/settings.yaml", vec!["dsh"]),
        ("omp", "/provided/omp/config.yml", vec!["omp"]),
    ] {
        let calls = RefCell::new(Vec::new());
        let result = builtin_path(
            agent,
            Path::new("/external"),
            || {
                calls.borrow_mut().push("dsh");
                Ok(dsh_config_path(Path::new("/provided/dsh")))
            },
            || {
                calls.borrow_mut().push("omp");
                Ok(PathBuf::from("/provided/omp"))
            },
        );
        assert_eq!(result.unwrap(), PathBuf::from(expected));
        assert_eq!(calls.into_inner(), wanted);
    }
}

#[test]
fn builtin_errors_propagate_without_calling_an_alternate_home() {
    for agent in ["dsh", "omp", "OMP", "unknown"] {
        let calls = RefCell::new(Vec::new());
        let result = builtin_path(
            agent,
            Path::new("/external"),
            || {
                calls.borrow_mut().push("dsh");
                Err("dsh root unavailable".to_string())
            },
            || {
                calls.borrow_mut().push("omp");
                Err("omp root unavailable".to_string())
            },
        );
        if matches!(agent, "dsh" | "omp") {
            assert_eq!(result.unwrap_err(), format!("{agent} root unavailable"));
            assert_eq!(calls.into_inner(), [agent]);
        } else {
            assert_eq!(
                result.unwrap_err(),
                format!("Unknown built-in agent: {agent}")
            );
            assert!(calls.into_inner().is_empty());
        }
    }
}

#[test]
fn custom_home_keeps_existing_sanitization_reserved_names_and_containment() {
    for (id, normalized) in [
        (" Team/DSH.. ", "team_dsh"),
        ("dsh", "local_dsh"),
        ("OMP", "local_omp"),
        ("Claude_GPT55", "local_claude_gpt55"),
        ("../escape", "escape"),
        ("a::b", "a_b"),
        ("keep..dots", "keep..dots"),
        ("_.agent-_", "agent"),
    ] {
        let calls = Cell::new(0);
        let root = PathBuf::from("/injected/agent-homes");
        let path = custom_home(id, || {
            calls.set(calls.get() + 1);
            Ok(root.clone())
        })
        .unwrap();
        assert_eq!(path, root.join(normalized), "{id}");
        assert_eq!(path.parent(), Some(root.as_path()));
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn invalid_custom_ids_fail_before_acquiring_a_root_and_root_errors_stay_errors() {
    for id in ["", "...", "///", " \t ", "中文"] {
        assert_eq!(
            custom_home(id, || panic!("invalid ID must not resolve the root")).unwrap_err(),
            format!("Invalid custom Agent id: {id}")
        );
    }
    assert_eq!(
        custom_home("valid", || Err("root unavailable".into())).unwrap_err(),
        "root unavailable"
    );
}

#[test]
fn root_shaping_retains_original_path_spelling_and_does_not_create_directories() {
    let root = scratch::Scratch::new("acr-pure");
    let external = root.path().join("external home/../literal home ");
    let managed = root.path().join("managed data/../literal data ");
    let paths = AppPaths::with_data_dir(&external, Some(&managed));
    let fields = AgentConfigPaths::default();
    assert_eq!(
        resolve_path("claude", &fields, &paths)
            .unwrap()
            .unwrap()
            .as_os_str(),
        external.join(".claude/settings.json").as_os_str()
    );
    assert_eq!(
        resolve_path("omp", &fields, &paths)
            .unwrap()
            .unwrap()
            .as_os_str(),
        managed.join("agent-homes/omp/config.yml").as_os_str()
    );
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn injected_defaults_separate_official_home_from_relocated_managed_homes() {
    let paths = AppPaths::with_data_dir("/external", Some(Path::new("/relocated")));
    let fields = AgentConfigPaths::default();
    for (agent, expected) in [
        ("claude", "/external/.claude/settings.json"),
        ("codex", "/external/.codex/config.toml"),
        ("dsh", "/relocated/agent-homes/local_dsh/settings.yaml"),
        ("omp", "/relocated/agent-homes/omp/config.yml"),
    ] {
        assert_eq!(
            resolve_path(agent, &fields, &paths).unwrap(),
            Some(PathBuf::from(expected))
        );
    }
    assert_eq!(resolve_path("claude_gpt55", &fields, &paths).unwrap(), None);
}

#[test]
fn injected_custom_dsh_sanitizes_while_custom_omp_keeps_the_builtin_home() {
    let paths = AppPaths::with_data_dir("/external", Some(Path::new("/managed")));
    let fields = AgentConfigPaths {
        custom_agents: vec![
            CustomAgentConfigPath {
                id: "DSH",
                path: "/not-a-config",
                family: " dsh ",
                codex_like: false,
            },
            CustomAgentConfigPath {
                id: "Team/DSH",
                path: "/not-a-config",
                family: "dsh",
                codex_like: false,
            },
            CustomAgentConfigPath {
                id: "custom-omp",
                path: "/not-a-config",
                family: "omp",
                codex_like: false,
            },
        ],
        ..Default::default()
    };
    for (agent, expected) in [
        ("DSH", "/managed/agent-homes/local_dsh/settings.yaml"),
        ("Team/DSH", "/managed/agent-homes/team_dsh/settings.yaml"),
        ("custom-omp", "/managed/agent-homes/omp/config.yml"),
    ] {
        assert_eq!(
            resolve_path(agent, &fields, &paths).unwrap(),
            Some(PathBuf::from(expected))
        );
    }
}

#[test]
fn injected_mode_keeps_configured_paths_none_and_exact_unknown_id_errors() {
    let paths = AppPaths::with_data_dir("/external", Some(Path::new("/managed")));
    let fields = AgentConfigPaths {
        claude_config_path: " /configured/claude ",
        claude_gpt55_config_path: " \t ",
        codex_config_path: " ./relative/config.toml ",
        dsh_config_path: "/configured/dsh",
        custom_agents: vec![CustomAgentConfigPath {
            id: "...",
            path: "/must-not-use-executable",
            family: "dsh",
            codex_like: false,
        }],
    };
    for (agent, expected) in [
        ("claude", "/configured/claude"),
        ("codex", "./relative/config.toml"),
        ("dsh", "/configured/dsh"),
    ] {
        assert_eq!(
            resolve_path(agent, &fields, &paths).unwrap(),
            Some(PathBuf::from(expected))
        );
    }
    for agent in ["claude_gpt55", "..."] {
        assert_eq!(resolve_path(agent, &fields, &paths).unwrap(), None);
    }
    assert_eq!(
        resolve_path(" codex", &fields, &paths).unwrap_err(),
        "Unknown agent:  codex"
    );
}
