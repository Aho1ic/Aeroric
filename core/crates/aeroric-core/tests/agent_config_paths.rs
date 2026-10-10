//! Compatibility corpus for the shared selector; all ports return fake paths.
use std::cell::RefCell;
use std::path::PathBuf;

use aeroric_core::settings::agent_config::{resolve_path, AgentConfigPaths, CustomAgentConfigPath};
use aeroric_core::settings::schema::AppSettings;
use serde_json::json;

fn profile<'a>(
    id: &'a str,
    path: &'a str,
    family: &'a str,
    codex_like: bool,
) -> CustomAgentConfigPath<'a> {
    CustomAgentConfigPath {
        id,
        path,
        family,
        codex_like,
    }
}

fn selected(
    agent: &str,
    paths: &AgentConfigPaths<'_>,
) -> (Result<Option<PathBuf>, String>, Vec<String>) {
    let calls = RefCell::new(Vec::new());
    let result = resolve_path(
        agent,
        paths,
        |id| {
            calls.borrow_mut().push(format!("builtin:{id}"));
            Ok(PathBuf::from(format!("/fake/defaults/{id}.config")))
        },
        |id| {
            calls.borrow_mut().push(format!("custom:{id}"));
            Ok(PathBuf::from(format!("/fake/homes/{id}")))
        },
    );
    (result, calls.into_inner())
}

#[test]
fn configured_builtin_paths_trim_without_calling_either_home_port() {
    let paths = AgentConfigPaths {
        claude_config_path: " /configured/claude ",
        claude_gpt55_config_path: " \t/configured/gpt55\n",
        codex_config_path: "/configured/codex",
        dsh_config_path: " /configured/dsh ",
        custom_agents: vec![profile("claude", "/wrong/profile", "omp", true)],
    };
    for (agent, expected) in [
        ("claude", "/configured/claude"),
        ("claude_gpt55", "/configured/gpt55"),
        ("codex", "/configured/codex"),
        ("dsh", "/configured/dsh"),
    ] {
        let (result, calls) = selected(agent, &paths);
        assert_eq!(result.unwrap(), Some(PathBuf::from(expected)));
        assert!(calls.is_empty(), "{agent}: {calls:?}");
    }
}

#[test]
fn builtin_defaults_are_lazy_and_gpt55_does_not_have_one() {
    let paths = AgentConfigPaths {
        claude_config_path: "\t",
        claude_gpt55_config_path: " ",
        codex_config_path: "",
        dsh_config_path: "\n",
        custom_agents: vec![],
    };
    for agent in ["claude", "codex", "dsh", "omp"] {
        let (result, calls) = selected(agent, &paths);
        assert_eq!(
            result.unwrap(),
            Some(PathBuf::from(format!("/fake/defaults/{agent}.config")))
        );
        assert_eq!(calls, [format!("builtin:{agent}")]);
    }
    assert_eq!(selected("claude_gpt55", &paths), (Ok(None), vec![]));
}

#[test]
fn omp_always_uses_the_builtin_managed_config_not_a_profile_executable() {
    let paths = AgentConfigPaths {
        custom_agents: vec![
            profile("omp", "/must/not/read/builtin-binary", "dsh", false),
            profile("custom-omp", "/must/not/read/custom-binary", " omp ", false),
        ],
        ..Default::default()
    };
    for agent in ["omp", "custom-omp"] {
        let (result, calls) = selected(agent, &paths);
        assert_eq!(
            result.unwrap(),
            Some(PathBuf::from("/fake/defaults/omp.config"))
        );
        assert_eq!(calls, ["builtin:omp"]);
    }
}

#[test]
fn custom_dsh_uses_its_own_home_and_all_other_families_keep_configured_paths() {
    let paths = AgentConfigPaths {
        custom_agents: vec![
            profile("dsh-profile", "/not/the/config", " dsh ", true),
            profile("claude-profile", " /config/claude ", "claude", true),
            profile("codex-profile", "\t/config/codex\n", "codex", false),
        ],
        ..Default::default()
    };
    let (result, calls) = selected("dsh-profile", &paths);
    assert_eq!(
        result.unwrap(),
        Some(PathBuf::from("/fake/homes/dsh-profile/settings.yaml"))
    );
    assert_eq!(calls, ["custom:dsh-profile"]);
    for agent in ["claude-profile", "codex-profile"] {
        let (result, calls) = selected(agent, &paths);
        assert_eq!(
            result.unwrap(),
            Some(PathBuf::from(if agent == "claude-profile" {
                "/config/claude"
            } else {
                "/config/codex"
            }))
        );
        assert!(calls.is_empty());
    }
}

#[test]
fn unknown_family_and_case_keep_the_legacy_codex_like_fallback() {
    for family in ["", "unknown", "OMP", "Dsh"] {
        for codex_like in [true, false] {
            let paths = AgentConfigPaths {
                custom_agents: vec![profile("custom", " /config/custom ", family, codex_like)],
                ..Default::default()
            };
            assert_eq!(
                selected("custom", &paths),
                (Ok(Some(PathBuf::from("/config/custom"))), vec![])
            );
        }
    }
}

#[test]
fn lookup_is_exact_first_match_and_unknown_ids_are_not_trimmed() {
    let paths = AgentConfigPaths {
        custom_agents: vec![
            profile("duplicate", " ", "claude", false),
            profile("duplicate", "/must/not/select/second", "dsh", false),
            profile(" custom ", "/exact-spaced-id", "codex", false),
        ],
        ..Default::default()
    };
    assert_eq!(selected("duplicate", &paths), (Ok(None), vec![]));
    assert_eq!(
        selected(" custom ", &paths),
        (Ok(Some(PathBuf::from("/exact-spaced-id"))), vec![])
    );
    for agent in ["custom", "", " claude ", "missing"] {
        assert_eq!(
            selected(agent, &paths),
            (Err(format!("Unknown agent: {agent}")), vec![])
        );
    }
}

#[test]
fn failed_selected_home_port_is_none_not_an_alternate_path_or_port() {
    let paths = AgentConfigPaths {
        custom_agents: vec![
            profile("custom-omp", "/not-a-fallback", "omp", false),
            profile("custom-dsh", "/not-a-fallback", "dsh", false),
        ],
        ..Default::default()
    };
    for agent in ["claude", "codex", "dsh", "omp", "custom-omp", "custom-dsh"] {
        let calls = RefCell::new(Vec::new());
        assert_eq!(
            resolve_path(
                agent,
                &paths,
                |id| {
                    calls.borrow_mut().push(format!("builtin:{id}"));
                    Err("synthetic missing home".into())
                },
                |id| {
                    calls.borrow_mut().push(format!("custom:{id}"));
                    Err("synthetic invalid id".into())
                },
            ),
            Ok(None)
        );
        let expected = match agent {
            "custom-omp" => "builtin:omp".to_string(),
            "custom-dsh" => "custom:custom-dsh".to_string(),
            other => format!("builtin:{other}"),
        };
        assert_eq!(calls.into_inner(), [expected]);
    }
}

#[test]
fn selection_does_not_expand_paths_canonicalize_or_sanitize_home_arguments() {
    for path in [
        "~/literal/config",
        "relative/../config",
        "\"quoted path\"",
        "C:\\config\\agent",
    ] {
        let paths = AgentConfigPaths {
            claude_gpt55_config_path: path,
            ..Default::default()
        };
        assert_eq!(
            selected("claude_gpt55", &paths),
            (Ok(Some(PathBuf::from(path))), vec![])
        );
    }
    let paths = AgentConfigPaths {
        custom_agents: vec![profile("../raw id", "/unused", "dsh", false)],
        ..Default::default()
    };
    let (_, calls) = selected("../raw id", &paths);
    assert_eq!(
        calls,
        ["custom:../raw id"],
        "the injected home resolver owns ID policy"
    );
}

#[test]
fn core_settings_projection_borrows_only_the_exact_selection_fields() {
    let settings: AppSettings = serde_json::from_value(json!({
        "claude_config_path": " c ", "claude_gpt55_config_path": " g ",
        "codex_config_path": " x ", "dsh_config_path": " d ",
        "custom_agents": [{
            "id": " raw ", "label": "not in projection", "path": " p ",
            "family": " omp ", "codex_like": false, "api_key": "synthetic-not-in-view"
        }]
    }))
    .unwrap();
    let paths = AgentConfigPaths::from(&settings);
    assert_eq!(
        paths,
        AgentConfigPaths {
            claude_config_path: " c ",
            claude_gpt55_config_path: " g ",
            codex_config_path: " x ",
            dsh_config_path: " d ",
            custom_agents: vec![profile(" raw ", " p ", " omp ", false)],
        }
    );
    assert_eq!(
        paths.claude_config_path.as_ptr(),
        settings.claude_config_path.as_ptr()
    );
    assert_eq!(
        paths.custom_agents[0].path.as_ptr(),
        settings.custom_agents[0].path.as_ptr()
    );
}
