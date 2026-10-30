//! Non-destructive migration from legacy Agent settings into `agent-connections.json`.
//!
//! Legacy settings, generated wrappers and credential sidecars are read-only inputs.
//! The migration only appends objects whose deterministic ids do not exist, and it
//! never overwrites a credential already stored for a new connection.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::context::{CoreContext, SecretKey, SecretStore};
use crate::settings::schema::{AgentFamily, BuiltInAgentCredentials, CustomAgentProfile};
use crate::settings::secret_fields::KEYCHAIN_MARKER;

use super::model::{
    AgentConfig, ConfigTarget, Connection, ConnectionKind, Harness, InputKind, ModelEntry,
    ModelSource, Vendor, WireProtocol,
};
use super::store;

pub const REPORT_VERSION: u32 = 2;
pub const REPORT_FILE_NAME: &str = "agent-connections-migration-report.json";
const FINGERPRINT_DOMAIN: &[u8] = b"aeroric/legacy-agent-fingerprint/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationEntry {
    pub source: String,
    pub connection_id: String,
    pub config_id: String,
    /// One-way digest of the legacy profile (secrets removed) and its credential.
    /// Cleanup refuses entries without it (reports before v2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationIssue {
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationWarning {
    pub source: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub version: u32,
    pub migrated: Vec<MigrationEntry>,
    pub already_present: Vec<MigrationEntry>,
    pub skipped: Vec<MigrationIssue>,
    pub warnings: Vec<MigrationWarning>,
}

impl Default for MigrationReport {
    fn default() -> Self {
        Self {
            version: REPORT_VERSION,
            migrated: Vec::new(),
            already_present: Vec::new(),
            skipped: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

pub(super) struct PreparedMigration {
    pub(super) source: String,
    pub(super) connection: Connection,
    pub(super) config: AgentConfig,
    pub(super) credential: String,
    pub(super) fingerprint: String,
    warnings: Vec<String>,
}

impl PreparedMigration {
    fn entry(&self) -> MigrationEntry {
        MigrationEntry {
            source: self.source.clone(),
            connection_id: self.connection.id.clone(),
            config_id: self.config.id.clone(),
            source_fingerprint: Some(self.fingerprint.clone()),
        }
    }
}

/// `sha256(domain || canonical profile JSON without secrets || 0x00 || credential)`.
///
/// Rotating the credential or editing the profile changes the digest, and the
/// report never holds a reversible fragment of the credential.
pub(super) fn source_fingerprint(profile: &Value, credential: &str) -> String {
    let mut profile = profile.clone();
    if let Some(object) = profile.as_object_mut() {
        object.remove("api_key");
        object.remove("password");
    }
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_DOMAIN);
    hasher.update(profile.to_string().as_bytes());
    hasher.update([0u8]);
    hasher.update(credential.as_bytes());
    hex::encode(hasher.finalize())
}

struct PendingMigration {
    item: PreparedMigration,
    add_connection: bool,
    add_config: bool,
    write_credential: bool,
}

pub fn report_path(data_dir: &Path) -> PathBuf {
    data_dir.join(REPORT_FILE_NAME)
}

/// Copy every fully usable legacy Claude/Codex endpoint into the new document.
///
/// The caller must hold the current data-root writer lease. A missing legacy
/// settings file is a no-op and does not create a report. Per-entry problems are
/// recorded in the report; malformed source/target documents and write failures
/// are returned because proceeding could hide an incomplete migration.
pub fn migrate_legacy_agent_settings(context: &CoreContext) -> Result<MigrationReport, String> {
    context
        .require_write()
        .map_err(|error| format!("agent connection migration: {error}"))?;

    let settings_path = context.paths.settings_file();
    let raw = match std::fs::read(&settings_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MigrationReport::default());
        }
        Err(error) => {
            return Err(format!(
                "cannot read legacy settings {}: {error}",
                settings_path.display()
            ));
        }
    };
    let value: Value = serde_json::from_slice(&raw).map_err(|error| {
        format!(
            "cannot parse legacy settings {}: {error}",
            settings_path.display()
        )
    })?;
    let fields = value.as_object().ok_or_else(|| {
        format!(
            "cannot parse legacy settings {}: root must be an object",
            settings_path.display()
        )
    })?;

    let document_path = store::document_path(context.paths.data_dir());
    let mut document = store::load(&document_path)?;
    store::validate(&document)
        .map_err(|error| format!("existing agent connections document is invalid: {error}"))?;

    let mut report = MigrationReport::default();
    let mut prepared = Vec::new();
    collect_builtin_migrations(fields, context.secrets.as_ref(), &mut prepared, &mut report);
    collect_custom_migrations(context, fields, &mut prepared, &mut report);

    let original_document = document.clone();
    let mut pending = Vec::new();
    let mut seen_sources = HashSet::new();
    for item in prepared {
        if !seen_sources.insert(item.source.clone()) {
            report.skipped.push(MigrationIssue {
                source: item.source,
                reason: "duplicate legacy source id".to_string(),
            });
            continue;
        }

        let add_connection = match document
            .connections
            .iter()
            .find(|connection| connection.id == item.connection.id)
        {
            Some(existing) if existing == &item.connection => false,
            Some(_) => {
                report.skipped.push(MigrationIssue {
                    source: item.source,
                    reason: format!(
                        "connection id `{}` already exists; existing data was not overwritten",
                        item.connection.id
                    ),
                });
                continue;
            }
            None => true,
        };
        let add_config = match document
            .configs
            .iter()
            .find(|config| config.id == item.config.id)
        {
            Some(existing) if existing == &item.config => false,
            Some(_) => {
                report.skipped.push(MigrationIssue {
                    source: item.source,
                    reason: format!(
                        "agent config id `{}` already exists; existing data was not overwritten",
                        item.config.id
                    ),
                });
                continue;
            }
            None => true,
        };

        if add_config
            && document.configs.iter().any(|config| {
                config.id != item.config.id
                    && config.harness == item.config.harness
                    && config.label.trim() == item.config.label.trim()
            })
        {
            report.skipped.push(MigrationIssue {
                source: item.source,
                reason: format!(
                    "an agent config named `{}` already exists for this harness",
                    item.config.label
                ),
            });
            continue;
        }

        let new_key = SecretKey(item.connection.secret_account());
        let write_credential = match context.secrets.get(&new_key) {
            Ok(Some(existing)) if existing == item.credential => false,
            Ok(Some(_)) => {
                report.skipped.push(MigrationIssue {
                    source: item.source,
                    reason: format!(
                        "credential `{}` already exists with a different value; it was not overwritten",
                        new_key.0
                    ),
                });
                continue;
            }
            Ok(None) => true,
            Err(error) => {
                report.skipped.push(MigrationIssue {
                    source: item.source,
                    reason: format!("cannot inspect new credential `{}`: {error}", new_key.0),
                });
                continue;
            }
        };

        if !add_connection && !add_config && !write_credential {
            append_warnings(&mut report, &item);
            report.already_present.push(item.entry());
            continue;
        }
        if add_connection {
            document.connections.push(item.connection.clone());
        }
        if add_config {
            document.configs.push(item.config.clone());
        }
        pending.push(PendingMigration {
            item,
            add_connection,
            add_config,
            write_credential,
        });
    }

    store::validate(&document)
        .map_err(|error| format!("migrated agent connections document is invalid: {error}"))?;

    let mut committed = Vec::new();
    for pending_item in pending {
        if pending_item.write_credential {
            context
                .require_write()
                .map_err(|error| format!("agent connection migration: {error}"))?;
            let key = SecretKey(pending_item.item.connection.secret_account());
            if let Err(error) = context.secrets.set(&key, &pending_item.item.credential) {
                remove_added_objects(&mut document, &pending_item);
                report.skipped.push(MigrationIssue {
                    source: pending_item.item.source,
                    reason: format!("cannot write new credential `{}`: {error}", key.0),
                });
                continue;
            }
        }
        committed.push(pending_item.item);
    }

    store::validate(&document)
        .map_err(|error| format!("migrated agent connections document is invalid: {error}"))?;
    if document != original_document {
        context
            .require_write()
            .map_err(|error| format!("agent connection migration: {error}"))?;
        store::save(&document_path, &document)?;
    }
    for item in &committed {
        append_warnings(&mut report, item);
        report.migrated.push(item.entry());
    }

    let report_raw = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize agent connection migration report: {error}"))?;
    context
        .require_write()
        .map_err(|error| format!("agent connection migration: {error}"))?;
    crate::settings::atomic_write_private(&report_path(context.paths.data_dir()), &report_raw)
        .map_err(|error| format!("cannot write agent connection migration report: {error}"))?;

    Ok(report)
}

fn append_warnings(report: &mut MigrationReport, item: &PreparedMigration) {
    report
        .warnings
        .extend(item.warnings.iter().map(|message| MigrationWarning {
            source: item.source.clone(),
            message: message.clone(),
        }));
}

fn remove_added_objects(
    document: &mut super::model::ConnectionsDocument,
    pending: &PendingMigration,
) {
    if pending.add_connection {
        document
            .connections
            .retain(|connection| connection.id != pending.item.connection.id);
    }
    if pending.add_config {
        document
            .configs
            .retain(|config| config.id != pending.item.config.id);
    }
}

fn collect_builtin_migrations(
    fields: &Map<String, Value>,
    secrets: &dyn SecretStore,
    prepared: &mut Vec<PreparedMigration>,
    report: &mut MigrationReport,
) {
    let Some(credentials) = fields
        .get("builtin_agent_credentials")
        .and_then(Value::as_object)
    else {
        return;
    };

    for agent in ["claude", "codex", "claude_gpt55"] {
        let Some(raw) = credentials.get(agent) else {
            continue;
        };
        let source = format!("builtin:{agent}");
        let parsed: BuiltInAgentCredentials = match serde_json::from_value(raw.clone()) {
            Ok(parsed) => parsed,
            Err(error) => {
                report.skipped.push(MigrationIssue {
                    source,
                    reason: format!("invalid legacy built-in credentials: {error}"),
                });
                continue;
            }
        };
        let credential = match legacy_settings_secret(
            &parsed.api_key,
            &format!("agent-config:builtin:{agent}:api_key"),
            secrets,
        ) {
            Ok(credential) => credential,
            Err(reason) => {
                report.skipped.push(MigrationIssue { source, reason });
                continue;
            }
        };
        let models = model_entries(&parsed.models);
        if let Err(reason) = validate_legacy_endpoint(&parsed.base_url, &models) {
            report.skipped.push(MigrationIssue { source, reason });
            continue;
        }

        let (harness, vendor, protocol) = if agent == "claude" {
            (Harness::Claude, Vendor::Anthropic, WireProtocol::Messages)
        } else {
            (Harness::Codex, Vendor::OpenAi, WireProtocol::Responses)
        };
        let label = fields
            .get("agent_label_overrides")
            .and_then(Value::as_object)
            .and_then(|labels| labels.get(agent))
            .and_then(Value::as_str)
            .filter(|label| !label.trim().is_empty())
            .unwrap_or(agent)
            .trim()
            .to_string();
        let suffix = stable_suffix(agent);
        let connection_id = format!("legacy-connection-builtin-{suffix}");
        let config_id = format!("legacy-config-builtin-{suffix}");
        let default_model = models[0].upstream_id.clone();
        let mut warnings = Vec::new();
        if parsed.enable_1m_context {
            warnings.push(
                "legacy 1M-context preference has no AgentConfig field and was not copied"
                    .to_string(),
            );
        }
        let fingerprint = source_fingerprint(
            &serde_json::to_value(&parsed).unwrap_or(Value::Null),
            &credential,
        );
        prepared.push(PreparedMigration {
            source,
            connection: Connection {
                id: connection_id.clone(),
                label: label.clone(),
                kind: ConnectionKind::ApiKey,
                vendor,
                base_url: parsed.base_url.trim().to_string(),
                protocols: vec![protocol],
                use_global_proxy: proxy_enabled(fields, agent),
                models,
                catalog_revision: 1,
                enabled: true,
                rest_on_limit: true,
            },
            config: AgentConfig {
                id: config_id,
                label,
                harness,
                target: ConfigTarget::Connection { connection_id },
                default_model,
                default_reasoning: String::new(),
                fallback_models: Vec::new(),
            },
            credential,
            fingerprint,
            warnings,
        });
    }
}

fn collect_custom_migrations(
    context: &CoreContext,
    fields: &Map<String, Value>,
    prepared: &mut Vec<PreparedMigration>,
    report: &mut MigrationReport,
) {
    let Some(profiles) = fields.get("custom_agents").and_then(Value::as_array) else {
        if fields.contains_key("custom_agents") {
            report.skipped.push(MigrationIssue {
                source: "custom_agents".to_string(),
                reason: "legacy custom_agents is not an array".to_string(),
            });
        }
        return;
    };

    for (index, raw) in profiles.iter().enumerate() {
        let fallback_source = format!("custom[{index}]");
        let profile: CustomAgentProfile = match serde_json::from_value(raw.clone()) {
            Ok(profile) => profile,
            Err(error) => {
                report.skipped.push(MigrationIssue {
                    source: fallback_source,
                    reason: format!("invalid legacy custom Agent profile: {error}"),
                });
                continue;
            }
        };
        let source = format!("custom:{}", profile.id);
        match prepare_custom(context, fields, profile, source.clone()) {
            Ok(item) => prepared.push(item),
            Err(reason) => report.skipped.push(MigrationIssue { source, reason }),
        }
    }
}

pub(super) fn prepare_custom(
    context: &CoreContext,
    fields: &Map<String, Value>,
    profile: CustomAgentProfile,
    source: String,
) -> Result<PreparedMigration, String> {
    let id = profile.id.trim();
    if id.is_empty() || profile.label.trim().is_empty() || profile.path.trim().is_empty() {
        return Err("legacy profile id, label and path must all be present".to_string());
    }
    ensure_safe_sidecar_id(id)?;

    let harness = match profile.agent_family() {
        AgentFamily::Claude => Harness::Claude,
        AgentFamily::Codex => Harness::Codex,
        AgentFamily::Dsh | AgentFamily::Omp => {
            return Err("this Harness is not supported by the first gateway migration".to_string());
        }
    };
    let wrapper = read_managed_wrapper(context.paths.data_dir(), &profile.path)?;
    if !crate::settings::agent_scripts::is_aeroric_generated_agent_wrapper(&wrapper) {
        return Err("launcher is not an Aeroric-generated wrapper".to_string());
    }
    if !profile.username.trim().is_empty() || !profile.password.trim().is_empty() {
        return Err(
            "per-profile proxy credentials cannot be represented by Connection".to_string(),
        );
    }

    let models = model_entries(&profile.models);
    validate_legacy_endpoint(&profile.base_url, &models)?;
    let credential = custom_legacy_secret(context, id, &profile.api_key)?;
    let fingerprint = source_fingerprint(
        &serde_json::to_value(&profile).map_err(|error| error.to_string())?,
        &credential,
    );
    let suffix = stable_suffix(id);
    let connection_id = format!("legacy-connection-custom-{suffix}");
    let config_id = format!("legacy-config-custom-{suffix}");
    let default_model = models[0].upstream_id.clone();
    let protocol = match harness {
        Harness::Claude => WireProtocol::Messages,
        Harness::Codex if profile.enable_chat_completions_proxy => WireProtocol::ChatCompletions,
        Harness::Codex => WireProtocol::Responses,
        Harness::Omp | Harness::Dsh => unreachable!("filtered above"),
    };
    let mut warnings = Vec::new();
    if profile.enable_1m_context {
        warnings.push(
            "legacy 1M-context preference has no AgentConfig field and was not copied".to_string(),
        );
    }
    if profile.disable_artifact_tool {
        warnings.push(
            "legacy disable-artifact preference has no AgentConfig field and was not copied"
                .to_string(),
        );
    }

    Ok(PreparedMigration {
        source,
        connection: Connection {
            id: connection_id.clone(),
            label: profile.label.trim().to_string(),
            kind: ConnectionKind::ApiKey,
            vendor: Vendor::Other,
            base_url: profile.base_url.trim().to_string(),
            protocols: vec![protocol],
            use_global_proxy: proxy_enabled(fields, id),
            models,
            catalog_revision: 1,
            enabled: true,
            rest_on_limit: true,
        },
        config: AgentConfig {
            id: config_id,
            label: profile.label.trim().to_string(),
            harness,
            target: ConfigTarget::Connection { connection_id },
            default_model,
            default_reasoning: String::new(),
            fallback_models: Vec::new(),
        },
        credential,
        fingerprint,
        warnings,
    })
}

fn model_entries(models: &[String]) -> Vec<ModelEntry> {
    let mut seen = HashSet::new();
    models
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty() && seen.insert((*model).to_string()))
        .map(|model| ModelEntry {
            upstream_id: model.to_string(),
            mapped_name: String::new(),
            display_name: String::new(),
            source: ModelSource::Manual,
            context_window: None,
            max_output: None,
            inputs: Vec::<InputKind>::new(),
            reasoning_tiers: Vec::new(),
            enabled: true,
            protocols: vec![],
        })
        .collect()
}

fn validate_legacy_endpoint(base_url: &str, models: &[ModelEntry]) -> Result<(), String> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err("legacy endpoint has no base URL".to_string());
    }
    let parsed = url::Url::parse(base_url)
        .map_err(|error| format!("legacy endpoint has an invalid base URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("legacy endpoint base URL must be an absolute HTTP(S) URL".to_string());
    }
    if models.is_empty() {
        return Err("legacy endpoint has no model and cannot produce a route".to_string());
    }
    Ok(())
}

fn proxy_enabled(fields: &Map<String, Value>, agent: &str) -> bool {
    fields
        .get("agent_proxy_enabled")
        .and_then(Value::as_object)
        .and_then(|enabled| enabled.get(agent))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn stable_suffix(value: &str) -> String {
    hex::encode(value.trim().as_bytes())
}

fn legacy_settings_secret(
    raw: &str,
    account: &str,
    secrets: &dyn SecretStore,
) -> Result<String, String> {
    if raw == KEYCHAIN_MARKER {
        return secrets
            .get(&SecretKey(account.to_string()))
            .map_err(|error| format!("cannot read legacy credential `{account}`: {error}"))?
            .filter(|secret| !secret.is_empty())
            .ok_or_else(|| {
                format!("legacy credential `{account}` is not in the credential store")
            });
    }
    if raw.is_empty() {
        Err("legacy endpoint has no credential".to_string())
    } else {
        Ok(raw.to_string())
    }
}

fn custom_legacy_secret(context: &CoreContext, id: &str, raw: &str) -> Result<String, String> {
    let sidecar = context.paths.agent_credentials_dir().join(id);
    match std::fs::read_to_string(&sidecar) {
        Ok(secret) if !secret.is_empty() => return Ok(secret),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot read legacy credential sidecar {}: {error}",
                sidecar.display()
            ));
        }
    }
    legacy_settings_secret(
        raw,
        &format!("agent-config:custom:{id}:api_key"),
        context.secrets.as_ref(),
    )
}

fn ensure_safe_sidecar_id(id: &str) -> Result<(), String> {
    let mut components = Path::new(id).components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        Ok(())
    } else {
        Err("legacy profile id is not a safe path component".to_string())
    }
}

pub(super) fn read_managed_wrapper(data_dir: &Path, raw_path: &str) -> Result<String, String> {
    let path = resolve_managed_path(data_dir, raw_path)?;
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("cannot inspect legacy launcher {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("legacy launcher is not a regular non-symlink file".to_string());
    }
    let canonical_root = std::fs::canonicalize(data_dir)
        .map_err(|error| format!("cannot resolve managed data root: {error}"))?;
    let canonical_path = std::fs::canonicalize(&path)
        .map_err(|error| format!("cannot resolve legacy launcher {}: {error}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err("legacy launcher is outside the injected managed data root".to_string());
    }
    std::fs::read_to_string(&canonical_path)
        .map_err(|error| format!("cannot read legacy launcher {}: {error}", path.display()))
}

pub(super) fn resolve_managed_path(data_dir: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return Err("legacy launcher path is empty".to_string());
    }
    if let Some(relative) = raw_path.strip_prefix("~/.aeroric/") {
        return Ok(data_dir.join(relative));
    }
    let path = Path::new(raw_path);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(data_dir.join(path))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::clock::SystemClock;
    use crate::context::MemorySecrets;
    use crate::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use crate::paths::AppPaths;
    use crate::storage_lock::LeaseOwner;

    struct LocalProbe;

    impl StorageLocationProbe for LocalProbe {
        fn probe(&self, data_dir: &Path) -> StorageLocation {
            StorageLocation {
                directory: data_dir.to_path_buf(),
                volume: VolumeKind::Local,
                private_permissions_enforced: true,
            }
        }
    }

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn fixture(tag: &str, secrets: Arc<MemorySecrets>) -> (PathBuf, CoreContext) {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!(
            "aeroric-agent-migration-{tag}-{}-{n}",
            std::process::id()
        ));
        let data_dir = root.join("managed");
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = AppPaths::with_data_dir(root.join("home"), Some(&data_dir));
        let context = CoreContext::acquire(
            paths,
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "agent-migration-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
        )
        .unwrap()
        .with_secrets(secrets);
        (root, context)
    }

    fn generated_claude_wrapper() -> &'static [u8] {
        b"#!/bin/sh\nexport CLAUDE_CONFIG_DIR=x\nCLAUDE_CODE_SESSION_ENV_DIR=y\n"
    }

    #[test]
    fn migration_is_non_destructive_and_idempotent() {
        let secrets = Arc::new(MemorySecrets::default());
        let (root, context) = fixture("non-destructive", Arc::clone(&secrets));
        let data_dir = context.paths.data_dir();
        let wrapper = data_dir.join("agent-scripts/deepseek.sh");
        let sidecar = data_dir.join("agent-credentials/deepseek");
        let orphan = data_dir.join("agent-credentials/orphan");
        std::fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        std::fs::write(&wrapper, generated_claude_wrapper()).unwrap();
        std::fs::write(&sidecar, b"sk-sidecar-exact\n").unwrap();
        std::fs::write(&orphan, b"orphan-exact").unwrap();
        let settings = serde_json::to_vec_pretty(&json!({
            "custom_agents": [{
                "id": "deepseek",
                "label": "DeepSeek",
                "path": wrapper,
                "codex_like": false,
                "family": "claude",
                "config_lang": "shellscript",
                "base_url": "https://api.example/v1",
                "api_key": "stale-settings-key",
                "models": ["deepseek-chat", "deepseek-chat"],
                "enable_1m_context": true
            }],
            "agent_proxy_enabled": {"deepseek": true},
            "unknown": {"keep": true}
        }))
        .unwrap();
        std::fs::write(context.paths.settings_file(), &settings).unwrap();

        let wrapper_before = std::fs::read(&wrapper).unwrap();
        let sidecar_before = std::fs::read(&sidecar).unwrap();
        let orphan_before = std::fs::read(&orphan).unwrap();
        let first = migrate_legacy_agent_settings(&context).unwrap();
        assert_eq!(first.migrated.len(), 1);
        assert_eq!(first.warnings.len(), 1);
        assert!(first.skipped.is_empty());

        let document_path = store::document_path(data_dir);
        let document_before = std::fs::read(&document_path).unwrap();
        let document: super::super::model::ConnectionsDocument =
            serde_json::from_slice(&document_before).unwrap();
        assert_eq!(document.connections.len(), 1);
        assert_eq!(document.configs.len(), 1);
        assert_eq!(document.connections[0].protocols, [WireProtocol::Messages]);
        assert!(document.connections[0].use_global_proxy);
        assert_eq!(document.connections[0].models.len(), 1);
        assert!(!String::from_utf8_lossy(&document_before).contains("sk-sidecar"));
        assert_eq!(
            secrets
                .peek(&document.connections[0].secret_account())
                .as_deref(),
            Some("sk-sidecar-exact\n")
        );

        let second = migrate_legacy_agent_settings(&context).unwrap();
        assert!(second.migrated.is_empty());
        assert_eq!(second.already_present.len(), 1);
        assert_eq!(std::fs::read(&document_path).unwrap(), document_before);
        assert_eq!(
            std::fs::read(context.paths.settings_file()).unwrap(),
            settings
        );
        assert_eq!(std::fs::read(&wrapper).unwrap(), wrapper_before);
        assert_eq!(std::fs::read(&sidecar).unwrap(), sidecar_before);
        assert_eq!(std::fs::read(&orphan).unwrap(), orphan_before);
        drop(context);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn conflicts_and_unrepresentable_profiles_are_reported_without_overwrite() {
        let secrets = Arc::new(MemorySecrets::default());
        let (root, context) = fixture("conflicts", Arc::clone(&secrets));
        let data_dir = context.paths.data_dir();
        let wrapper = data_dir.join("agent-scripts/managed.sh");
        let user_script = data_dir.join("agent-scripts/user.sh");
        std::fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
        std::fs::write(&wrapper, generated_claude_wrapper()).unwrap();
        std::fs::write(&user_script, b"#!/bin/sh\nexec claude \"$@\"\n").unwrap();
        std::fs::create_dir_all(context.paths.agent_credentials_dir()).unwrap();
        std::fs::write(
            context.paths.agent_credentials_dir().join("managed"),
            b"old-key",
        )
        .unwrap();
        std::fs::write(
            context.paths.agent_credentials_dir().join("user"),
            b"user-key",
        )
        .unwrap();
        let settings = json!({
            "custom_agents": [
                {"id":"managed","label":"Managed","path":wrapper,"family":"claude","codex_like":false,"config_lang":"shellscript","base_url":"https://old.example","api_key":"","models":["m"]},
                {"id":"user","label":"User Script","path":user_script,"family":"claude","codex_like":false,"config_lang":"shellscript","base_url":"https://user.example","api_key":"","models":["m"]},
                {"id":"omp","label":"OMP","path":"agent-scripts/omp.yaml","family":"omp","codex_like":false,"config_lang":"yaml","base_url":"https://omp.example","api_key":"k","models":["m"]}
            ]
        });
        std::fs::write(
            context.paths.settings_file(),
            serde_json::to_vec_pretty(&settings).unwrap(),
        )
        .unwrap();

        let suffix = stable_suffix("managed");
        let original = super::super::model::ConnectionsDocument {
            version: super::super::model::DOCUMENT_VERSION,
            connections: vec![Connection {
                id: format!("legacy-connection-custom-{suffix}"),
                label: "User-owned target".to_string(),
                kind: ConnectionKind::ApiKey,
                vendor: Vendor::Other,
                base_url: "https://keep.example".to_string(),
                protocols: vec![WireProtocol::Messages],
                use_global_proxy: false,
                models: model_entries(&["keep".to_string()]),
                catalog_revision: 7,
                enabled: true,
                rest_on_limit: true,
            }],
            pools: Vec::new(),
            configs: Vec::new(),
            login_gateway: Default::default(),
        };
        store::save(&store::document_path(data_dir), &original).unwrap();
        let document_before = std::fs::read(store::document_path(data_dir)).unwrap();

        let report = migrate_legacy_agent_settings(&context).unwrap();
        assert!(report.migrated.is_empty());
        assert_eq!(report.skipped.len(), 3);
        assert!(report
            .skipped
            .iter()
            .any(|issue| issue.reason.contains("was not overwritten")));
        assert!(report
            .skipped
            .iter()
            .any(|issue| issue.reason.contains("not an Aeroric-generated")));
        assert!(report
            .skipped
            .iter()
            .any(|issue| issue.reason.contains("not supported")));
        assert_eq!(
            std::fs::read(store::document_path(data_dir)).unwrap(),
            document_before
        );
        assert!(secrets.keys().is_empty());
        drop(context);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn keychain_markers_are_read_but_old_entries_are_not_deleted() {
        let old_account = "agent-config:builtin:claude:api_key";
        let secrets = Arc::new(MemorySecrets::with(&[(old_account, "sk-old-keychain")]));
        let (root, context) = fixture("marker", Arc::clone(&secrets));
        let settings = json!({
            "builtin_agent_credentials": {
                "claude": {
                    "base_url": "https://anthropic.example",
                    "api_key": KEYCHAIN_MARKER,
                    "models": ["claude-test"]
                }
            }
        });
        let settings_bytes = serde_json::to_vec_pretty(&settings).unwrap();
        std::fs::write(context.paths.settings_file(), &settings_bytes).unwrap();

        let report = migrate_legacy_agent_settings(&context).unwrap();
        assert_eq!(report.migrated.len(), 1);
        let document = store::load(&store::document_path(context.paths.data_dir())).unwrap();
        assert_eq!(
            secrets
                .peek(&document.connections[0].secret_account())
                .as_deref(),
            Some("sk-old-keychain")
        );
        assert_eq!(
            secrets.peek(old_account).as_deref(),
            Some("sk-old-keychain")
        );
        assert_eq!(
            std::fs::read(context.paths.settings_file()).unwrap(),
            settings_bytes
        );
        drop(context);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn report_v2_fingerprints_sources_without_leaking_credentials() {
        let secrets = Arc::new(MemorySecrets::default());
        let (root, context) = fixture("fingerprint", Arc::clone(&secrets));
        let data_dir = context.paths.data_dir();
        let wrapper = data_dir.join("agent-scripts/fp.sh");
        let sidecar = data_dir.join("agent-credentials/fp");
        std::fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        std::fs::write(&wrapper, generated_claude_wrapper()).unwrap();
        std::fs::write(&sidecar, b"sk-fingerprint-secret").unwrap();
        std::fs::write(
            context.paths.settings_file(),
            serde_json::to_vec_pretty(&json!({
                "custom_agents": [{
                    "id": "fp", "label": "FP", "path": wrapper, "family": "claude",
                    "codex_like": false, "config_lang": "shellscript",
                    "base_url": "https://fp.example", "api_key": "", "models": ["m"]
                }],
                "builtin_agent_credentials": {
                    "codex": {"base_url": "https://b.example", "api_key": "sk-builtin-secret", "models": ["b"]}
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let report = migrate_legacy_agent_settings(&context).unwrap();
        assert_eq!(report.version, 2);
        assert_eq!(report.migrated.len(), 2);
        let custom = report
            .migrated
            .iter()
            .find(|entry| entry.source == "custom:fp")
            .unwrap();
        let first = custom.source_fingerprint.clone().unwrap();
        assert_eq!(first.len(), 64);
        assert!(report
            .migrated
            .iter()
            .all(|entry| entry.source_fingerprint.is_some()));
        let raw = std::fs::read_to_string(report_path(data_dir)).unwrap();
        assert!(raw.contains("sourceFingerprint"));
        for needle in [
            "sk-fingerprint-secret",
            "fingerprint-secret",
            "sk-builtin-secret",
        ] {
            assert!(!raw.contains(needle), "report leaked {needle}");
        }

        // Same profile, rotated credential: a different digest.
        let profile = json!({"id": "fp", "api_key": "", "models": ["m"]});
        assert_ne!(
            source_fingerprint(&profile, "sk-fingerprint-secret"),
            source_fingerprint(&profile, "sk-rotated")
        );
        // Secret fields of the profile itself never feed the digest.
        let with_key = json!({"id": "fp", "api_key": KEYCHAIN_MARKER, "models": ["m"]});
        assert_eq!(
            source_fingerprint(&profile, "k"),
            source_fingerprint(&with_key, "k")
        );

        // A v1 report entry (no fingerprint) still parses.
        let v1: MigrationEntry = serde_json::from_value(json!({
            "source": "custom:fp", "connectionId": "c", "configId": "g"
        }))
        .unwrap();
        assert!(v1.source_fingerprint.is_none());

        // Rerun takes the already-present path and keeps the same digest.
        let again = migrate_legacy_agent_settings(&context).unwrap();
        let entry = again
            .already_present
            .iter()
            .find(|entry| entry.source == "custom:fp")
            .unwrap();
        assert_eq!(entry.source_fingerprint.as_deref(), Some(first.as_str()));
        drop(context);
        std::fs::remove_dir_all(root).ok();
    }
}
