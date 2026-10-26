//! Which `AgentConfig` a task launches from, and its managed home (Q5, Q27).
//!
//! Read-only: this decides, the host issues tokens and spawns. A task pinned to a
//! configuration never falls back to a legacy wrapper. A legacy task (agent = old
//! custom id, no pinned configuration) maps to the configuration the migration
//! report recorded for it, keeping the old agent home so its sessions resume.

use std::collections::BTreeMap;
use std::path::Path;

use super::migration::{self, MigrationReport};
use super::model::{ConnectionsDocument, Harness};
use super::store;

/// Prefix that keeps configuration ids in the native catalog apart from legacy
/// `custom_agents` ids and built-in names.
pub const CATALOG_ID_PREFIX: &str = "agent-config:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLaunch {
    pub config_id: String,
    /// Claude or Codex; other harnesses do not go through the gateway yet (Q32-a).
    pub harness: Harness,
    /// Directory name under `<data dir>/agent-homes/`.
    pub home_dir_name: String,
}

impl ConfigLaunch {
    /// The built-in agent name the harness launches as.
    pub fn agent(&self) -> &'static str {
        harness_agent(self.harness)
    }
}

pub fn harness_agent(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "claude",
        Harness::Codex => "codex",
        Harness::Omp => "omp",
        Harness::Dsh => "dsh",
    }
}

/// Catalog id for a configuration.
pub fn catalog_id(config_id: &str) -> String {
    format!("{CATALOG_ID_PREFIX}{config_id}")
}

/// Configuration id behind a catalog id; `None` for legacy and built-in entries.
pub fn config_id_from_catalog(catalog_id: &str) -> Option<&str> {
    catalog_id
        .strip_prefix(CATALOG_ID_PREFIX)
        .filter(|id| !id.trim().is_empty())
}

/// The configuration a task is pinned to. Missing or non-gateway harness is an
/// error, never a fallback.
pub fn resolve_config(data_dir: &Path, config_id: &str) -> Result<ConfigLaunch, String> {
    let document = store::load(&store::document_path(data_dir))?;
    let report = load_report(data_dir);
    resolve_in(&document, report.as_ref(), config_id)
}

/// The configuration a legacy custom Agent id was migrated to, if it still exists.
pub fn legacy_mapping(data_dir: &Path, agent: &str) -> Option<ConfigLaunch> {
    if agent.trim().is_empty() || crate::agents::families::is_builtin(agent) {
        return None;
    }
    let report = load_report(data_dir)?;
    let document = store::load(&store::document_path(data_dir)).ok()?;
    let config_id = migrated_legacy_ids(&document, &report).remove(agent)?;
    resolve_in(&document, Some(&report), &config_id).ok()
}

/// Legacy custom id → configuration id, for every migrated custom source whose
/// configuration still exists.
pub fn migrated_legacy_ids(
    document: &ConnectionsDocument,
    report: &MigrationReport,
) -> BTreeMap<String, String> {
    report
        .migrated
        .iter()
        .chain(&report.already_present)
        .filter_map(|entry| {
            let id = entry.source.strip_prefix("custom:")?;
            document
                .configs
                .iter()
                .any(|config| config.id == entry.config_id)
                .then(|| (id.to_string(), entry.config_id.clone()))
        })
        .collect()
}

/// Missing or unreadable report: no mapping. Cleanup refuses to run without a
/// valid report, so a legacy profile is never removed while this is the case.
pub fn load_report(data_dir: &Path) -> Option<MigrationReport> {
    let raw = std::fs::read(migration::report_path(data_dir)).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn resolve_in(
    document: &ConnectionsDocument,
    report: Option<&MigrationReport>,
    config_id: &str,
) -> Result<ConfigLaunch, String> {
    let config = document
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .ok_or_else(|| format!("agent configuration `{config_id}` no longer exists"))?;
    if !config.harness.gateway_supported() {
        return Err(format!(
            "agent configuration `{config_id}` uses a harness that cannot launch through the gateway yet"
        ));
    }
    Ok(ConfigLaunch {
        config_id: config.id.clone(),
        harness: config.harness,
        home_dir_name: home_dir_name(report, config_id),
    })
}

/// A migrated configuration reuses the legacy wrapper's home; any other gets its
/// own, hex-encoded so it is one safe path segment.
fn home_dir_name(report: Option<&MigrationReport>, config_id: &str) -> String {
    report
        .into_iter()
        .flat_map(|report| report.migrated.iter().chain(&report.already_present))
        .filter(|entry| entry.config_id == config_id)
        .find_map(|entry| entry.source.strip_prefix("custom:"))
        .and_then(crate::settings::custom_agent_home_dir_name)
        .unwrap_or_else(|| format!("config-{}", hex::encode(config_id.as_bytes())))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::super::migration::MigrationEntry;
    use super::super::model::{AgentConfig, ConfigTarget};
    use super::*;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    struct Root(PathBuf);

    impl Drop for Root {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn root(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-config-launch-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Root(dir)
    }

    fn config(id: &str, harness: Harness) -> AgentConfig {
        AgentConfig {
            id: id.into(),
            label: id.into(),
            harness,
            target: ConfigTarget::Route { pool_id: None },
            default_model: String::new(),
            default_reasoning: String::new(),
            fallback_models: Vec::new(),
        }
    }

    fn write(dir: &Path, configs: Vec<AgentConfig>, migrated: &[(&str, &str)]) {
        let document = ConnectionsDocument {
            configs,
            ..Default::default()
        };
        std::fs::write(
            store::document_path(dir),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        let report = MigrationReport {
            migrated: migrated
                .iter()
                .map(|(source, config_id)| MigrationEntry {
                    source: (*source).into(),
                    connection_id: format!("conn-{config_id}"),
                    config_id: (*config_id).into(),
                    source_fingerprint: Some("0".repeat(64)),
                })
                .collect(),
            ..Default::default()
        };
        std::fs::write(
            migration::report_path(dir),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn resolves_pinned_configs_and_refuses_missing_or_unsupported() {
        let root = root("resolve");
        write(
            &root.0,
            vec![
                config("cfg-claude", Harness::Claude),
                config("cfg-codex", Harness::Codex),
                config("cfg-omp", Harness::Omp),
            ],
            &[],
        );
        let claude = resolve_config(&root.0, "cfg-claude").unwrap();
        assert_eq!(claude.harness, Harness::Claude);
        assert_eq!(claude.agent(), "claude");
        assert_eq!(
            claude.home_dir_name,
            format!("config-{}", hex::encode("cfg-claude"))
        );
        assert_eq!(
            resolve_config(&root.0, "cfg-codex").unwrap().agent(),
            "codex"
        );
        assert!(resolve_config(&root.0, "gone")
            .unwrap_err()
            .contains("no longer exists"));
        assert!(resolve_config(&root.0, "cfg-omp")
            .unwrap_err()
            .contains("cannot launch through the gateway"));
    }

    #[test]
    fn migrated_configs_reuse_the_legacy_home() {
        let root = root("home");
        write(
            &root.0,
            vec![config("legacy-config-custom-6473", Harness::Claude)],
            &[("custom:ds", "legacy-config-custom-6473")],
        );
        let launch = resolve_config(&root.0, "legacy-config-custom-6473").unwrap();
        assert_eq!(launch.home_dir_name, "ds");
    }

    #[test]
    fn legacy_mapping_needs_report_entry_and_live_config() {
        let root = root("legacy");
        write(
            &root.0,
            vec![config("legacy-config-custom-6473", Harness::Codex)],
            &[
                ("custom:ds", "legacy-config-custom-6473"),
                ("custom:deleted", "legacy-config-custom-gone"),
                ("builtin:claude", "legacy-config-builtin-x"),
            ],
        );
        let mapped = legacy_mapping(&root.0, "ds").unwrap();
        assert_eq!(mapped.config_id, "legacy-config-custom-6473");
        assert_eq!(mapped.harness, Harness::Codex);
        assert_eq!(mapped.home_dir_name, "ds");
        assert!(
            legacy_mapping(&root.0, "deleted").is_none(),
            "config removed"
        );
        assert!(legacy_mapping(&root.0, "unknown").is_none(), "not migrated");
        assert!(legacy_mapping(&root.0, "claude").is_none(), "built-in");
        assert!(legacy_mapping(&root.0, "").is_none());

        std::fs::remove_file(migration::report_path(&root.0)).unwrap();
        assert!(legacy_mapping(&root.0, "ds").is_none(), "no report");
        // Without a report a pinned configuration still resolves, with its own home.
        assert_eq!(
            resolve_config(&root.0, "legacy-config-custom-6473")
                .unwrap()
                .home_dir_name,
            format!("config-{}", hex::encode("legacy-config-custom-6473"))
        );
    }

    #[test]
    fn catalog_ids_round_trip_and_reject_legacy_ids() {
        assert_eq!(catalog_id("cfg"), "agent-config:cfg");
        assert_eq!(config_id_from_catalog("agent-config:cfg"), Some("cfg"));
        assert_eq!(config_id_from_catalog("ds"), None);
        assert_eq!(config_id_from_catalog("claude"), None);
        assert_eq!(config_id_from_catalog("agent-config:"), None);
    }
}
