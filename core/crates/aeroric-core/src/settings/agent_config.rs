//! Pure agent-config path selection. Adapters supply paths and lazy home
//! resolvers; this module never loads settings, reads globals or performs IO.

use std::path::{Path, PathBuf};

use super::schema::{AgentFamily, AppSettings};

/// Only the fields needed for selection, not credentials or the full settings DTO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomAgentConfigPath<'a> {
    pub id: &'a str,
    pub path: &'a str,
    pub family: &'a str,
    pub codex_like: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentConfigPaths<'a> {
    pub claude_config_path: &'a str,
    pub claude_gpt55_config_path: &'a str,
    pub codex_config_path: &'a str,
    pub dsh_config_path: &'a str,
    pub custom_agents: Vec<CustomAgentConfigPath<'a>>,
}

impl<'a> From<&'a AppSettings> for AgentConfigPaths<'a> {
    fn from(settings: &'a AppSettings) -> Self {
        Self {
            claude_config_path: &settings.claude_config_path,
            claude_gpt55_config_path: &settings.claude_gpt55_config_path,
            codex_config_path: &settings.codex_config_path,
            dsh_config_path: &settings.dsh_config_path,
            custom_agents: settings
                .custom_agents
                .iter()
                .map(|profile| CustomAgentConfigPath {
                    id: &profile.id,
                    path: &profile.path,
                    family: &profile.family,
                    codex_like: profile.codex_like,
                })
                .collect(),
        }
    }
}

fn configured_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Shared by builtin and custom DSH config paths, including legacy adapters.
pub fn dsh_config_path(home: &Path) -> PathBuf {
    home.join("settings.yaml")
}

/// Preserve the shipping selector's exact IDs, first-match and error->None
/// rules. Callbacks are lazy: a configured path never probes a default home.
pub fn resolve_path(
    agent: &str,
    settings: &AgentConfigPaths<'_>,
    default_builtin: impl Fn(&str) -> Result<PathBuf, String>,
    custom_home: impl Fn(&str) -> Result<PathBuf, String>,
) -> Result<Option<PathBuf>, String> {
    match agent {
        "claude" => {
            Ok(configured_path(settings.claude_config_path)
                .or_else(|| default_builtin("claude").ok()))
        }
        "claude_gpt55" => Ok(configured_path(settings.claude_gpt55_config_path)),
        "codex" => {
            Ok(configured_path(settings.codex_config_path)
                .or_else(|| default_builtin("codex").ok()))
        }
        "dsh" => {
            Ok(configured_path(settings.dsh_config_path).or_else(|| default_builtin("dsh").ok()))
        }
        // OMP profile.path is an executable, never a settings file.
        "omp" => Ok(default_builtin("omp").ok()),
        _ => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
            .map(|profile| {
                let family = AgentFamily::parse(profile.family.trim())
                    .unwrap_or_else(|| AgentFamily::from_codex_like(profile.codex_like));
                match family {
                    AgentFamily::Omp => default_builtin("omp").ok(),
                    AgentFamily::Dsh => custom_home(profile.id)
                        .ok()
                        .map(|home| dsh_config_path(&home)),
                    _ => configured_path(profile.path),
                }
            })
            .ok_or_else(|| format!("Unknown agent: {agent}")),
    }
}
