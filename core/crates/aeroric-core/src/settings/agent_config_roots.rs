//! Explicit agent-config roots and shared path shaping. No settings, filesystem
//! or environment acquisition; legacy adapters retain their lazy root callbacks.

use std::path::{Path, PathBuf};

use crate::paths::AppPaths;

use super::agent_config::AgentConfigPaths;
use super::normalize::sanitize_custom_agent_id;

/// Filename policy shared with the legacy builtin helpers. The caller retains
/// ownership of acquiring `external_home`, including its original error order.
pub fn builtin_path(
    agent: &str,
    external_home: &Path,
    dsh_config: impl FnOnce() -> Result<PathBuf, String>,
    omp_home: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    match agent {
        "claude" => Ok(external_home.join(".claude").join("settings.json")),
        "claude_gpt55" => Ok(external_home.join(".claude").join("start-gpt55.sh")),
        "codex" => Ok(external_home.join(".codex").join("config.toml")),
        "dsh" => dsh_config(),
        "omp" => omp_home().map(|home| home.join("config.yml")),
        _ => Err(format!("Unknown built-in agent: {agent}")),
    }
}

/// Preserve the existing normalization and error order: invalid IDs must not
/// acquire a root. Reserved builtin IDs deliberately become `local_<id>`.
pub fn custom_home(
    id: &str,
    agent_homes_root: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    let normalized = sanitize_custom_agent_id(id);
    if normalized.is_empty() {
        return Err(format!("Invalid custom Agent id: {id}"));
    }
    let root = agent_homes_root()?;
    let home = root.join(&normalized);
    if home.parent() != Some(root.as_path()) {
        return Err(
            "Refusing to resolve an Agent home outside the isolation directory".to_string(),
        );
    }
    Ok(home)
}

/// Hosted defaults follow injected roots, never legacy process-global roots.
/// Configured paths and selection/error->None semantics stay in the selector.
pub fn resolve_path(
    agent: &str,
    settings: &AgentConfigPaths<'_>,
    paths: &AppPaths,
) -> Result<Option<PathBuf>, String> {
    super::agent_config::resolve_path(
        agent,
        settings,
        |id| {
            builtin_path(
                id,
                paths.user_home(),
                || {
                    custom_home("dsh", || Ok(paths.agent_homes_dir()))
                        .map(|home| super::agent_config::dsh_config_path(&home))
                },
                || Ok(paths.agent_homes_dir().join("omp")),
            )
        },
        |id| custom_home(id, || Ok(paths.agent_homes_dir())),
    )
}
