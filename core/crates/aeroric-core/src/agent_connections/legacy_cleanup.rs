//! Cleanup of legacy custom Agent artifacts after the non-destructive migration (Q28/Q37).
//!
//! Only `custom:<id>` sources are handled; built-in credentials are never touched.
//! [`plan_legacy_cleanup`] is read-only. [`apply_legacy_cleanup`] processes only the
//! source ids the user confirmed, re-verifies each one, persists a journal before
//! acting, and runs the fixed order: remove profile → quarantine sidecar →
//! quarantine wrapper → delete old keychain entries. A failed step leaves the entry
//! `partial` (no rollback); the next apply resumes it.
//!
//! Not wired to any production entry point yet: the executor still launches custom
//! Agents from `custom_agents` and their wrappers (Q5).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::context::{CoreContext, SecretKey};
use crate::settings::schema::CustomAgentProfile;
use crate::storage::atomic::StorageError;

use super::migration::{self, MigrationEntry, MigrationReport};
use super::store;

pub const JOURNAL_VERSION: u32 = 1;
pub const JOURNAL_FILE_NAME: &str = "agent-legacy-cleanup.json";
pub const QUARANTINE_DIR_NAME: &str = "legacy-agent-quarantine";
pub const SETTINGS_SNAPSHOT_NAME: &str = "settings.snapshot.json";
const REDACTED: &str = "<redacted>";
const PREDATES_FINGERPRINTS: &str = "migration report predates fingerprints; rerun migration";
/// Settings maps keyed by the Agent id that belong to the removed profile.
const PROFILE_KEYED_MAPS: [&str; 4] = [
    "agent_label_overrides",
    "agent_proxy_enabled",
    "agent_proxy_overrides",
    "dsh_reasoning_efforts",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupStep {
    RemoveProfile,
    QuarantineSidecar,
    QuarantineWrapper,
    DeleteOldSecrets,
}

const STEPS: [CleanupStep; 4] = [
    CleanupStep::RemoveProfile,
    CleanupStep::QuarantineSidecar,
    CleanupStep::QuarantineWrapper,
    CleanupStep::DeleteOldSecrets,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CandidateState {
    Eligible,
    Blocked {
        reason: String,
    },
    Partial {
        remaining: Vec<CleanupStep>,
        reason: Option<String>,
    },
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupCandidate {
    pub source: String,
    pub state: CandidateState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub candidates: Vec<CleanupCandidate>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    pub candidates: Vec<CleanupCandidate>,
    /// Batch directory created by this apply, if any new entry was started.
    pub quarantine_dir: Option<PathBuf>,
}

/// Persisted cross-run state. Holds paths, ids and account names only — never
/// a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CleanupJournal {
    version: u32,
    entries: Vec<JournalEntry>,
}

impl Default for CleanupJournal {
    fn default() -> Self {
        Self {
            version: JOURNAL_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JournalEntry {
    source: String,
    profile_id: String,
    connection_id: String,
    config_id: String,
    /// Resolved before the profile is removed; the profile is the only other record.
    sidecar: Option<PathBuf>,
    wrapper: PathBuf,
    old_secret_accounts: Vec<String>,
    quarantine_dir: PathBuf,
    completed: Vec<CleanupStep>,
    last_error: Option<String>,
}

impl JournalEntry {
    fn remaining(&self) -> Vec<CleanupStep> {
        STEPS
            .into_iter()
            .filter(|step| !self.completed.contains(step))
            .collect()
    }
}

struct Draft {
    entry: JournalEntry,
    /// The verified on-disk profile; RemoveProfile refuses to drop a profile
    /// that changed since. `None` once the profile is already gone.
    profile: Option<Value>,
}

struct Row {
    candidate: CleanupCandidate,
    draft: Option<Draft>,
}

struct Evaluation {
    settings_raw: Option<Vec<u8>>,
    fields: Map<String, Value>,
    journal: CleanupJournal,
    rows: Vec<Row>,
}

pub fn journal_path(data_dir: &Path) -> PathBuf {
    data_dir.join(JOURNAL_FILE_NAME)
}

pub fn quarantine_root(data_dir: &Path) -> PathBuf {
    data_dir.join(QUARANTINE_DIR_NAME)
}

/// Dry run: the state every custom source would be in. No side effects.
pub fn plan_legacy_cleanup(context: &CoreContext) -> Result<CleanupPlan, String> {
    Ok(CleanupPlan {
        candidates: evaluate(context)?
            .rows
            .into_iter()
            .map(|row| row.candidate)
            .collect(),
    })
}

/// Clean up the confirmed sources that still pass verification (or resume their
/// partial cleanup). Every other source is reported unchanged and left alone.
///
/// Returns an error only when the inputs cannot be read or the journal cannot be
/// written; per-entry failures are reported as `Partial`.
pub fn apply_legacy_cleanup(
    context: &CoreContext,
    confirmed: &[String],
) -> Result<CleanupOutcome, String> {
    context
        .require_write()
        .map_err(|error| format!("legacy agent cleanup: {error}"))?;
    let confirmed: HashSet<&str> = confirmed.iter().map(String::as_str).collect();
    let Evaluation {
        settings_raw,
        fields,
        mut journal,
        rows,
    } = evaluate(context)?;
    let data_dir = context.paths.data_dir();

    let mut quarantine_dir = None;
    let mut work = Vec::new();
    let mut candidates = Vec::with_capacity(rows.len());
    for row in rows {
        let actionable = confirmed.contains(row.candidate.source.as_str())
            && matches!(
                row.candidate.state,
                CandidateState::Eligible | CandidateState::Partial { .. }
            );
        match (actionable, row.draft) {
            (true, Some(mut draft)) => {
                if row.candidate.state == CandidateState::Eligible {
                    if quarantine_dir.is_none() {
                        let batch = create_batch_dir(data_dir)?;
                        write_settings_snapshot(&batch, settings_raw.as_deref(), &fields)?;
                        quarantine_dir = Some(batch);
                    }
                    draft.entry.quarantine_dir = quarantine_dir.clone().unwrap_or_default();
                    journal
                        .entries
                        .retain(|entry| entry.source != draft.entry.source);
                    journal.entries.push(draft.entry.clone());
                }
                work.push((candidates.len(), draft));
                candidates.push(row.candidate);
            }
            _ => candidates.push(row.candidate),
        }
    }
    if work.is_empty() {
        return Ok(CleanupOutcome {
            candidates,
            quarantine_dir,
        });
    }

    let journal_file = journal_path(data_dir);
    save_journal(&journal_file, &journal)?;
    for (index, draft) in work {
        let position = journal
            .entries
            .iter()
            .position(|entry| entry.source == draft.entry.source)
            .ok_or_else(|| "legacy cleanup journal lost an entry".to_string())?;
        run_entry(
            context,
            &mut journal,
            &journal_file,
            position,
            draft.profile.as_ref(),
        )?;
        let entry = &journal.entries[position];
        let remaining = entry.remaining();
        candidates[index].state = if remaining.is_empty() {
            CandidateState::Done
        } else {
            CandidateState::Partial {
                remaining,
                reason: entry.last_error.clone(),
            }
        };
    }
    Ok(CleanupOutcome {
        candidates,
        quarantine_dir,
    })
}

fn evaluate(context: &CoreContext) -> Result<Evaluation, String> {
    let data_dir = context.paths.data_dir();
    let settings_path = context.paths.settings_file();
    let settings_raw = match std::fs::read(&settings_path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "cannot read legacy settings {}: {error}",
                settings_path.display()
            ))
        }
    };
    let fields = match &settings_raw {
        None => Map::new(),
        Some(raw) => match serde_json::from_slice::<Value>(raw) {
            Ok(Value::Object(fields)) => fields,
            Ok(_) => {
                return Err(format!(
                    "cannot parse legacy settings {}: root must be an object",
                    settings_path.display()
                ))
            }
            Err(error) => {
                return Err(format!(
                    "cannot parse legacy settings {}: {error}",
                    settings_path.display()
                ))
            }
        },
    };
    let journal = load_journal(&journal_path(data_dir))?;
    let report = load_report(data_dir)?;
    let document = store::load(&store::document_path(data_dir))?;

    let profiles: Vec<&Value> = fields
        .get("custom_agents")
        .and_then(Value::as_array)
        .map(|profiles| profiles.iter().collect())
        .unwrap_or_default();
    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for raw in &profiles {
        if let Some(id) = raw.get("id").and_then(Value::as_str) {
            *id_counts.entry(id.trim().to_string()).or_default() += 1;
        }
    }
    let live_profile = |id: &str| {
        profiles
            .iter()
            .find(|raw| raw.get("id").and_then(Value::as_str).map(str::trim) == Some(id))
    };

    let mut rows = Vec::new();
    let mut handled = HashSet::new();
    for entry in &journal.entries {
        let remaining = entry.remaining();
        let live = live_profile(&entry.profile_id);
        if remaining.is_empty() {
            // A profile re-created after a finished cleanup is a new candidate.
            if live.is_none() {
                handled.insert(entry.source.clone());
                rows.push(Row {
                    candidate: candidate(&entry.source, CandidateState::Done),
                    draft: None,
                });
            }
            continue;
        }
        handled.insert(entry.source.clone());
        let verified = match live {
            Some(raw) if remaining.contains(&CleanupStep::RemoveProfile) => verify_profile(
                context,
                &fields,
                report.as_ref(),
                &document,
                raw,
                &entry.source,
            )
            .map(|draft| draft.profile),
            _ => verify_resume(context, &document, entry).map(|()| None),
        };
        rows.push(match verified {
            Ok(profile) => Row {
                candidate: candidate(
                    &entry.source,
                    CandidateState::Partial {
                        remaining,
                        reason: entry.last_error.clone(),
                    },
                ),
                draft: Some(Draft {
                    entry: entry.clone(),
                    profile,
                }),
            },
            Err(reason) => Row {
                candidate: candidate(&entry.source, CandidateState::Blocked { reason }),
                draft: None,
            },
        });
    }

    for (index, raw) in profiles.iter().enumerate() {
        let Some(id) = raw.get("id").and_then(Value::as_str) else {
            rows.push(Row {
                candidate: candidate(
                    &format!("custom[{index}]"),
                    CandidateState::Blocked {
                        reason: "legacy profile has no id".to_string(),
                    },
                ),
                draft: None,
            });
            continue;
        };
        // Same source spelling as the migration report.
        let source = format!("custom:{id}");
        if handled.contains(&source) {
            continue;
        }
        let state_row = if id_counts.get(id.trim()).copied().unwrap_or(0) > 1 {
            Err("duplicate legacy source id".to_string())
        } else {
            verify_profile(context, &fields, report.as_ref(), &document, raw, &source)
        };
        rows.push(match state_row {
            Ok(draft) => Row {
                candidate: candidate(&source, CandidateState::Eligible),
                draft: Some(draft),
            },
            Err(reason) => Row {
                candidate: candidate(&source, CandidateState::Blocked { reason }),
                draft: None,
            },
        });
    }

    Ok(Evaluation {
        settings_raw,
        fields,
        journal,
        rows,
    })
}

fn candidate(source: &str, state: CandidateState) -> CleanupCandidate {
    CleanupCandidate {
        source: source.to_string(),
        state,
    }
}

/// Full Q8 verification of a live profile against the report, the new document
/// and the new credential.
fn verify_profile(
    context: &CoreContext,
    fields: &Map<String, Value>,
    report: Option<&MigrationReport>,
    document: &super::model::ConnectionsDocument,
    raw: &Value,
    source: &str,
) -> Result<Draft, String> {
    let report = report.ok_or("no migration report; run the migration first")?;
    if report.version < 2 {
        return Err(PREDATES_FINGERPRINTS.to_string());
    }
    let recorded: &MigrationEntry = report
        .migrated
        .iter()
        .chain(&report.already_present)
        .find(|entry| entry.source == source)
        .ok_or("source is not recorded as migrated in the migration report")?;
    let recorded_fingerprint = recorded
        .source_fingerprint
        .as_deref()
        .ok_or(PREDATES_FINGERPRINTS)?;

    let profile: CustomAgentProfile = serde_json::from_value(raw.clone())
        .map_err(|error| format!("invalid legacy custom Agent profile: {error}"))?;
    let profile_id = profile.id.trim().to_string();
    let launcher = profile.path.clone();
    let prepared = migration::prepare_custom(context, fields, profile, source.to_string())?;
    if prepared.connection.id != recorded.connection_id || prepared.config.id != recorded.config_id
    {
        return Err("migration report ids do not match the legacy profile".to_string());
    }
    match document
        .connections
        .iter()
        .find(|connection| connection.id == prepared.connection.id)
    {
        None => {
            return Err(format!(
                "new connection `{}` is missing",
                prepared.connection.id
            ))
        }
        Some(existing) if existing != &prepared.connection => {
            return Err(format!(
                "new connection `{}` changed after migration",
                prepared.connection.id
            ))
        }
        Some(_) => {}
    }
    match document
        .configs
        .iter()
        .find(|config| config.id == prepared.config.id)
    {
        None => {
            return Err(format!(
                "new agent config `{}` is missing",
                prepared.config.id
            ))
        }
        Some(existing) if existing != &prepared.config => {
            return Err(format!(
                "new agent config `{}` changed after migration",
                prepared.config.id
            ))
        }
        Some(_) => {}
    }
    let new_account = prepared.connection.secret_account();
    match context.secrets.get(&SecretKey(new_account.clone())) {
        Ok(Some(secret)) if secret.as_bytes() == prepared.credential.as_bytes() => {}
        Ok(Some(_)) => {
            return Err(format!(
                "new credential `{new_account}` does not match the legacy credential"
            ))
        }
        Ok(None) => return Err(format!("new credential `{new_account}` is missing")),
        Err(error) => {
            return Err(format!(
                "cannot read new credential `{new_account}`: {error}"
            ))
        }
    }
    if prepared.fingerprint != recorded_fingerprint {
        return Err(
            "legacy profile or credential changed since migration (fingerprint mismatch)"
                .to_string(),
        );
    }

    let data_dir = context.paths.data_dir();
    Ok(Draft {
        entry: JournalEntry {
            source: source.to_string(),
            sidecar: Some(context.paths.agent_credentials_dir().join(&profile_id)),
            wrapper: migration::resolve_managed_path(data_dir, &launcher)?,
            old_secret_accounts: ["api_key", "password"]
                .iter()
                .map(|name| format!("agent-config:custom:{profile_id}:{name}"))
                .collect(),
            profile_id,
            connection_id: prepared.connection.id,
            config_id: prepared.config.id,
            quarantine_dir: PathBuf::new(),
            completed: Vec::new(),
            last_error: None,
        },
        profile: Some(raw.clone()),
    })
}

/// Resuming after the profile is gone: the new objects must still be usable.
fn verify_resume(
    context: &CoreContext,
    document: &super::model::ConnectionsDocument,
    entry: &JournalEntry,
) -> Result<(), String> {
    let connection = document
        .connections
        .iter()
        .find(|connection| connection.id == entry.connection_id)
        .ok_or_else(|| format!("new connection `{}` is missing", entry.connection_id))?;
    if !document
        .configs
        .iter()
        .any(|config| config.id == entry.config_id)
    {
        return Err(format!("new agent config `{}` is missing", entry.config_id));
    }
    let account = connection.secret_account();
    match context.secrets.get(&SecretKey(account.clone())) {
        Ok(Some(secret)) if !secret.is_empty() => Ok(()),
        Ok(_) => Err(format!("new credential `{account}` is missing")),
        Err(error) => Err(format!("cannot read new credential `{account}`: {error}")),
    }
}

fn run_entry(
    context: &CoreContext,
    journal: &mut CleanupJournal,
    journal_file: &Path,
    position: usize,
    profile: Option<&Value>,
) -> Result<(), String> {
    for step in STEPS {
        if journal.entries[position].completed.contains(&step) {
            continue;
        }
        let result = context
            .require_write()
            .map_err(|error| error.to_string())
            .and_then(|_| fail_point(step))
            .and_then(|()| execute_step(context, &journal.entries[position], step, profile));
        let entry = &mut journal.entries[position];
        match result {
            Ok(()) => {
                entry.completed.push(step);
                entry.last_error = None;
                save_journal(journal_file, journal)?;
            }
            Err(reason) => {
                entry.last_error = Some(format!("{step:?}: {reason}"));
                save_journal(journal_file, journal)?;
                return Ok(());
            }
        }
    }
    Ok(())
}

fn execute_step(
    context: &CoreContext,
    entry: &JournalEntry,
    step: CleanupStep,
    profile: Option<&Value>,
) -> Result<(), String> {
    match step {
        CleanupStep::RemoveProfile => remove_profile(context, &entry.profile_id, profile),
        CleanupStep::QuarantineSidecar => match &entry.sidecar {
            Some(sidecar) => quarantine_file(
                context.paths.data_dir(),
                sidecar,
                &entry_quarantine_dir(entry),
                "credential-sidecar",
                false,
            ),
            None => Ok(()),
        },
        CleanupStep::QuarantineWrapper => {
            let name = entry
                .wrapper
                .file_name()
                .map(|name| format!("wrapper-{}", name.to_string_lossy()))
                .ok_or("legacy launcher path has no file name")?;
            quarantine_file(
                context.paths.data_dir(),
                &entry.wrapper,
                &entry_quarantine_dir(entry),
                &name,
                true,
            )
        }
        CleanupStep::DeleteOldSecrets => {
            for account in &entry.old_secret_accounts {
                context
                    .secrets
                    .delete(&SecretKey(account.clone()))
                    .map_err(|error| format!("cannot delete `{account}`: {error}"))?;
            }
            Ok(())
        }
    }
}

fn entry_quarantine_dir(entry: &JournalEntry) -> PathBuf {
    entry.quarantine_dir.join(format!(
        "custom-{}",
        migration::stable_suffix(&entry.profile_id)
    ))
}

fn without_secrets(profile: &Value) -> Value {
    let mut profile = profile.clone();
    if let Some(object) = profile.as_object_mut() {
        object.remove("api_key");
        object.remove("password");
    }
    profile
}

fn remove_profile(context: &CoreContext, id: &str, expected: Option<&Value>) -> Result<(), String> {
    let matches = |raw: &Value| raw.get("id").and_then(Value::as_str).map(str::trim) == Some(id);
    let current = crate::storage::settings::load_settings(context)
        .map_err(|error| error.to_string())?
        .found()
        .unwrap_or_default();
    let has_profile = current
        .get("custom_agents")
        .and_then(Value::as_array)
        .is_some_and(|profiles| profiles.iter().any(matches));
    let has_keyed = PROFILE_KEYED_MAPS.iter().any(|key| {
        current
            .get(*key)
            .and_then(Value::as_object)
            .is_some_and(|map| map.contains_key(id))
    });
    if !has_profile && !has_keyed {
        return Ok(());
    }
    let settings_path = context.paths.settings_file();
    crate::storage::settings::update_settings(context, |fields| {
        if let Some(Value::Array(profiles)) = fields.get_mut("custom_agents") {
            // The loader may have hydrated secrets from the keychain; compare
            // without them so a marker/plaintext difference is not a change.
            if let (Some(expected), Some(current)) =
                (expected, profiles.iter().find(|raw| matches(raw)))
            {
                if without_secrets(current) != without_secrets(expected) {
                    return Err(StorageError::WriteFailed {
                        path: settings_path.clone(),
                        reason: "legacy profile changed after verification; left in place"
                            .to_string(),
                    });
                }
            }
            profiles.retain(|raw| !matches(raw));
        }
        for key in PROFILE_KEYED_MAPS {
            if let Some(Value::Object(map)) = fields.get_mut(key) {
                map.remove(id);
            }
        }
        Ok(())
    })
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Move one file into the quarantine. Missing is already done; a symlink or
/// non-regular file is refused and left in place.
fn quarantine_file(
    data_dir: &Path,
    source: &Path,
    target_dir: &Path,
    target_name: &str,
    require_generated_wrapper: bool,
) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect {}: {error}", source.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{} is not a regular non-symlink file; left in place",
            source.display()
        ));
    }
    if require_generated_wrapper {
        let canonical_root = std::fs::canonicalize(data_dir)
            .map_err(|error| format!("cannot resolve managed data root: {error}"))?;
        let canonical = std::fs::canonicalize(source)
            .map_err(|error| format!("cannot resolve {}: {error}", source.display()))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(
                "legacy launcher is outside the managed data root; left in place".to_string(),
            );
        }
        let content = std::fs::read_to_string(&canonical)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        if !crate::settings::agent_scripts::is_aeroric_generated_agent_wrapper(&content) {
            return Err(
                "legacy launcher is no longer an Aeroric-generated wrapper; left in place"
                    .to_string(),
            );
        }
    }
    crate::settings::ensure_private_dir(target_dir)?;
    let target = target_dir.join(target_name);
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err(format!(
            "quarantine target {} already exists",
            target.display()
        ));
    }
    std::fs::rename(source, &target)
        .map_err(|error| format!("cannot move {} into quarantine: {error}", source.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot restrict {}: {error}", target.display()))?;
    }
    Ok(())
}

fn create_batch_dir(data_dir: &Path) -> Result<PathBuf, String> {
    let root = quarantine_root(data_dir);
    crate::settings::ensure_private_dir(&root)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    for seq in 0..1000u32 {
        let batch = root.join(format!("{stamp}-{seq}"));
        match std::fs::create_dir(&batch) {
            Ok(()) => {
                crate::settings::ensure_private_dir(&batch)?;
                return Ok(batch);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create quarantine batch {}: {error}",
                    batch.display()
                ))
            }
        }
    }
    Err("cannot allocate a quarantine batch directory".to_string())
}

/// Byte-for-byte copy of settings on disk (markers only in production). A file
/// that still holds plaintext is redacted first.
fn write_settings_snapshot(
    batch: &Path,
    raw: Option<&[u8]>,
    fields: &Map<String, Value>,
) -> Result<(), String> {
    let path = batch.join(SETTINGS_SNAPSHOT_NAME);
    let Some(raw) = raw else {
        return crate::settings::atomic_write_private(&path, "null");
    };
    let mut fields = fields.clone();
    if !crate::settings::secret_fields::has_plaintext_secrets(&mut fields) {
        return crate::settings::atomic_write_private_bytes(&path, raw);
    }
    redact_secrets(&mut fields);
    let redacted = serde_json::to_string_pretty(&Value::Object(fields))
        .map_err(|error| format!("cannot serialize settings snapshot: {error}"))?;
    crate::settings::atomic_write_private(&path, &redacted)
}

/// Same credential fields as `settings::secret_fields`.
fn redact_secrets(fields: &mut Map<String, Value>) {
    fn redact(value: Option<&mut Value>) {
        if let Some(value) = value {
            if matches!(value.as_str(), Some(secret) if !secret.is_empty()
                && secret != crate::settings::secret_fields::KEYCHAIN_MARKER)
            {
                *value = Value::String(REDACTED.to_string());
            }
        }
    }
    if let Some(Value::Array(profiles)) = fields.get_mut("custom_agents") {
        for profile in profiles.iter_mut().filter_map(Value::as_object_mut) {
            redact(profile.get_mut("api_key"));
            redact(profile.get_mut("password"));
        }
    }
    if let Some(Value::Object(builtins)) = fields.get_mut("builtin_agent_credentials") {
        for credentials in builtins.values_mut().filter_map(Value::as_object_mut) {
            redact(credentials.get_mut("api_key"));
        }
    }
    redact(
        fields
            .get_mut("proxy_settings")
            .and_then(Value::as_object_mut)
            .and_then(|object| object.get_mut("password")),
    );
    redact(
        fields
            .get_mut("local_router_settings")
            .and_then(Value::as_object_mut)
            .and_then(|object| object.get_mut("access_token")),
    );
}

fn load_journal(path: &Path) -> Result<CleanupJournal, String> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanupJournal::default())
        }
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let journal: CleanupJournal = serde_json::from_slice(&raw)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    if journal.version > JOURNAL_VERSION {
        return Err(format!(
            "{} was written by a newer cleanup (version {}); refusing to use it",
            path.display(),
            journal.version
        ));
    }
    Ok(journal)
}

fn save_journal(path: &Path, journal: &CleanupJournal) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(journal)
        .map_err(|error| format!("cannot serialize legacy cleanup journal: {error}"))?;
    crate::settings::atomic_write_private(path, &raw)
        .map_err(|error| format!("cannot write legacy cleanup journal: {error}"))
}

fn load_report(data_dir: &Path) -> Result<Option<MigrationReport>, String> {
    let path = migration::report_path(data_dir);
    match std::fs::read(&path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| format!("cannot parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

#[cfg(test)]
thread_local! {
    static FAIL_AT: std::cell::Cell<Option<CleanupStep>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn fail_point(step: CleanupStep) -> Result<(), String> {
    if FAIL_AT.with(|fail| fail.get()) == Some(step) {
        Err("injected failure".to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_point(_step: CleanupStep) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::clock::SystemClock;
    use crate::context::{MemorySecrets, SecretStore};
    use crate::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use crate::paths::AppPaths;
    use crate::settings::secret_fields::KEYCHAIN_MARKER;
    use crate::storage_lock::LeaseOwner;

    const SIDECAR_SECRET: &str = "sk-ds-sidecar-secret\n";
    const SETTINGS_SECRET: &str = "sk-ds-settings-secret";
    const BUILTIN_SECRET: &str = "sk-builtin-keychain-secret";
    const OLD_API_KEY: &str = "agent-config:custom:ds:api_key";
    const OLD_BUILTIN: &str = "agent-config:builtin:claude:api_key";

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

    struct Fixture {
        root: PathBuf,
        context: CoreContext,
        secrets: Arc<MemorySecrets>,
    }

    impl Fixture {
        fn data_dir(&self) -> &Path {
            self.context.paths.data_dir()
        }

        fn wrapper(&self, id: &str) -> PathBuf {
            self.data_dir().join(format!("agent-scripts/{id}.sh"))
        }

        fn sidecar(&self, id: &str) -> PathBuf {
            self.context.paths.agent_credentials_dir().join(id)
        }

        fn new_account(&self, id: &str) -> String {
            format!(
                "connection:legacy-connection-custom-{}:credential",
                migration::stable_suffix(id)
            )
        }

        fn settings(&self) -> Map<String, Value> {
            serde_json::from_slice(&std::fs::read(self.context.paths.settings_file()).unwrap())
                .unwrap()
        }

        fn plan_state(&self, source: &str) -> CandidateState {
            state_of(
                &plan_legacy_cleanup(&self.context).unwrap().candidates,
                source,
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }

    fn generated_wrapper() -> &'static [u8] {
        b"#!/bin/sh\nexport CLAUDE_CONFIG_DIR=x\nCLAUDE_CODE_SESSION_ENV_DIR=y\n"
    }

    fn profile(id: &str, wrapper: &Path, api_key: &str) -> Value {
        json!({
            "id": id, "label": format!("Agent {id}"), "path": wrapper, "codex_like": false,
            "family": "claude", "config_lang": "shellscript",
            "base_url": format!("https://{id}.example/v1"), "api_key": api_key, "models": ["m-1"]
        })
    }

    /// Migrated fixture: custom Agents `ids` (wrapper + sidecar + old keychain
    /// entry) and a built-in Claude credential stored behind a marker.
    fn migrated(tag: &str, ids: &[&str]) -> Fixture {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "aeroric-legacy-cleanup-{tag}-{}-{n}",
            std::process::id()
        ));
        let data_dir = root.join("managed");
        std::fs::create_dir_all(&data_dir).unwrap();
        let secrets = Arc::new(MemorySecrets::with(&[(OLD_BUILTIN, BUILTIN_SECRET)]));
        let context = CoreContext::acquire(
            AppPaths::with_data_dir(root.join("home"), Some(&data_dir)),
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "legacy-cleanup-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
        )
        .unwrap()
        .with_secrets(Arc::clone(&secrets) as Arc<dyn SecretStore>);
        let fixture = Fixture {
            root,
            context,
            secrets,
        };

        let mut profiles = Vec::new();
        let mut labels = Map::new();
        let mut proxies = Map::new();
        for id in ids {
            let wrapper = fixture.wrapper(id);
            std::fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
            std::fs::create_dir_all(fixture.sidecar(id).parent().unwrap()).unwrap();
            std::fs::write(&wrapper, generated_wrapper()).unwrap();
            std::fs::write(fixture.sidecar(id), SIDECAR_SECRET).unwrap();
            fixture
                .secrets
                .set(
                    &SecretKey(format!("agent-config:custom:{id}:api_key")),
                    SETTINGS_SECRET,
                )
                .unwrap();
            profiles.push(profile(id, &wrapper, SETTINGS_SECRET));
            labels.insert((*id).to_string(), json!(format!("Label {id}")));
            proxies.insert((*id).to_string(), json!(true));
        }
        let settings = json!({
            "custom_agents": profiles,
            "agent_label_overrides": labels,
            "agent_proxy_enabled": proxies,
            "builtin_agent_credentials": {
                "claude": {"base_url": "https://anthropic.example", "api_key": KEYCHAIN_MARKER, "models": ["c"]}
            },
            "unknown": {"keep": true}
        });
        std::fs::write(
            fixture.context.paths.settings_file(),
            serde_json::to_vec_pretty(&settings).unwrap(),
        )
        .unwrap();
        let report = migration::migrate_legacy_agent_settings(&fixture.context).unwrap();
        assert_eq!(report.migrated.len(), ids.len() + 1, "{report:?}");
        fixture
    }

    /// Every file and symlink under the root, plus every secret.
    fn snapshot(fixture: &Fixture) -> (BTreeMap<PathBuf, Vec<u8>>, BTreeMap<String, String>) {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains("lock") || name.contains("lease") {
                    continue;
                }
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                if metadata.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).unwrap();
                    out.insert(path, target.to_string_lossy().as_bytes().to_vec());
                } else if metadata.is_dir() {
                    out.insert(path.clone(), b"<dir>".to_vec());
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), std::fs::read(&path).unwrap());
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(&fixture.root, &mut files);
        let secrets = fixture
            .secrets
            .keys()
            .into_iter()
            .map(|key| {
                let value = fixture.secrets.peek(&key).unwrap();
                (key, value)
            })
            .collect();
        (files, secrets)
    }

    fn state_of(candidates: &[CleanupCandidate], source: &str) -> CandidateState {
        candidates
            .iter()
            .find(|candidate| candidate.source == source)
            .unwrap_or_else(|| panic!("{source} missing from {candidates:?}"))
            .state
            .clone()
    }

    fn confirm(sources: &[&str]) -> Vec<String> {
        sources.iter().map(|source| source.to_string()).collect()
    }

    fn assert_blocked(fixture: &Fixture, source: &str, needle: &str) {
        let before = snapshot(fixture);
        let outcome = apply_legacy_cleanup(&fixture.context, &confirm(&[source])).unwrap();
        match state_of(&outcome.candidates, source) {
            CandidateState::Blocked { reason } => {
                assert!(
                    reason.contains(needle),
                    "reason `{reason}` lacks `{needle}`"
                )
            }
            other => panic!("expected blocked, got {other:?}"),
        }
        assert!(outcome.quarantine_dir.is_none());
        assert_eq!(snapshot(fixture), before, "blocked entry was touched");
    }

    #[test]
    fn plan_is_side_effect_free() {
        let fixture = migrated("plan", &["ds"]);
        let before = snapshot(&fixture);
        let plan = plan_legacy_cleanup(&fixture.context).unwrap();
        assert_eq!(
            state_of(&plan.candidates, "custom:ds"),
            CandidateState::Eligible
        );
        assert_eq!(plan.candidates.len(), 1, "built-ins are not candidates");
        let again = plan_legacy_cleanup(&fixture.context).unwrap();
        assert_eq!(again, plan);
        assert_eq!(snapshot(&fixture), before);
        assert!(!journal_path(fixture.data_dir()).exists());
        assert!(!quarantine_root(fixture.data_dir()).exists());
    }

    #[test]
    fn apply_moves_files_removes_profile_and_old_secrets() {
        let fixture = migrated("apply", &["ds"]);
        let data_dir = fixture.data_dir().to_path_buf();
        let document_before = std::fs::read(store::document_path(&data_dir)).unwrap();
        let new_secret_before = fixture.secrets.peek(&fixture.new_account("ds"));
        let wrapper = fixture.wrapper("ds");
        let sidecar = fixture.sidecar("ds");

        let outcome = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        assert_eq!(
            state_of(&outcome.candidates, "custom:ds"),
            CandidateState::Done
        );
        let batch = outcome.quarantine_dir.clone().unwrap();
        assert!(batch.starts_with(quarantine_root(&data_dir)));

        // Profile and its id-keyed settings are gone; everything else is kept.
        let settings = fixture.settings();
        assert_eq!(settings["custom_agents"], json!([]));
        assert!(!settings["agent_label_overrides"]
            .as_object()
            .unwrap()
            .contains_key("ds"));
        assert!(!settings["agent_proxy_enabled"]
            .as_object()
            .unwrap()
            .contains_key("ds"));
        assert_eq!(settings["unknown"], json!({"keep": true}));
        assert!(settings.contains_key("builtin_agent_credentials"));

        // Files moved into the private quarantine, content unchanged.
        let entry_dir = batch.join(format!("custom-{}", migration::stable_suffix("ds")));
        assert!(!wrapper.exists() && !sidecar.exists());
        assert_eq!(
            std::fs::read(entry_dir.join("credential-sidecar")).unwrap(),
            SIDECAR_SECRET.as_bytes()
        );
        assert_eq!(
            std::fs::read(entry_dir.join("wrapper-ds.sh")).unwrap(),
            generated_wrapper()
        );
        assert!(batch.join(SETTINGS_SNAPSHOT_NAME).is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&quarantine_root(&data_dir)), 0o700);
            assert_eq!(mode(&batch), 0o700);
            assert_eq!(mode(&entry_dir), 0o700);
            assert_eq!(mode(&entry_dir.join("credential-sidecar")), 0o600);
            assert_eq!(mode(&entry_dir.join("wrapper-ds.sh")), 0o600);
            assert_eq!(mode(&batch.join(SETTINGS_SNAPSHOT_NAME)), 0o600);
            assert_eq!(mode(&journal_path(&data_dir)), 0o600);
        }

        // Old keychain entry deleted; new objects and secret untouched.
        assert!(fixture.secrets.peek(OLD_API_KEY).is_none());
        assert_eq!(
            fixture.secrets.peek(&fixture.new_account("ds")),
            new_secret_before
        );
        assert_eq!(
            std::fs::read(store::document_path(&data_dir)).unwrap(),
            document_before
        );

        // The legacy loader regenerates wrappers/sidecars only from
        // `custom_agents` (agent_scripts.rs recover/refresh loops); a typed load
        // of the settings on disk no longer has the profile to regenerate from.
        let loaded = crate::storage::settings::load_settings(&fixture.context)
            .unwrap()
            .found()
            .unwrap();
        let typed: crate::settings::schema::AppSettings =
            serde_json::from_value(Value::Object(loaded)).unwrap();
        assert!(typed.custom_agents.iter().all(|profile| profile.id != "ds"));
        assert!(!wrapper.exists() && !sidecar.exists());
        assert_eq!(fixture.plan_state("custom:ds"), CandidateState::Done);
    }

    #[test]
    fn unconfirmed_entries_are_untouched() {
        let fixture = migrated("unconfirmed", &["ds", "other"]);
        let other_wrapper = std::fs::read(fixture.wrapper("other")).unwrap();
        let outcome = apply_legacy_cleanup(
            &fixture.context,
            &confirm(&["custom:ds", "custom:not-there", "builtin:claude"]),
        )
        .unwrap();
        assert_eq!(
            state_of(&outcome.candidates, "custom:ds"),
            CandidateState::Done
        );
        assert_eq!(
            state_of(&outcome.candidates, "custom:other"),
            CandidateState::Eligible
        );
        assert!(outcome
            .candidates
            .iter()
            .all(|candidate| !candidate.source.starts_with("builtin:")));

        let settings = fixture.settings();
        let profiles = settings["custom_agents"].as_array().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0]["id"], "other");
        assert_eq!(settings["agent_label_overrides"]["other"], "Label other");
        assert_eq!(
            std::fs::read(fixture.wrapper("other")).unwrap(),
            other_wrapper
        );
        assert_eq!(
            std::fs::read(fixture.sidecar("other")).unwrap(),
            SIDECAR_SECRET.as_bytes()
        );
        assert_eq!(
            fixture
                .secrets
                .peek("agent-config:custom:other:api_key")
                .as_deref(),
            Some(SETTINGS_SECRET)
        );
        assert_eq!(fixture.plan_state("custom:other"), CandidateState::Eligible);
    }

    #[test]
    fn blocked_when_credential_rotated() {
        let fixture = migrated("rotated", &["ds"]);
        std::fs::write(fixture.sidecar("ds"), b"sk-rotated").unwrap();
        assert_blocked(
            &fixture,
            "custom:ds",
            "does not match the legacy credential",
        );
    }

    #[test]
    fn blocked_when_new_secret_missing_or_different() {
        let fixture = migrated("new-secret", &["ds"]);
        let account = SecretKey(fixture.new_account("ds"));
        fixture.secrets.delete(&account).unwrap();
        assert_blocked(&fixture, "custom:ds", "is missing");
        fixture.secrets.set(&account, "sk-someone-else").unwrap();
        assert_blocked(
            &fixture,
            "custom:ds",
            "does not match the legacy credential",
        );
    }

    #[test]
    fn blocked_when_new_objects_changed() {
        let fixture = migrated("objects", &["ds"]);
        let path = store::document_path(fixture.data_dir());
        let mut document = store::load(&path).unwrap();
        let original = document.clone();
        document
            .connections
            .iter_mut()
            .find(|connection| connection.id.contains("custom"))
            .unwrap()
            .label = "Renamed by user".to_string();
        store::save(&path, &document).unwrap();
        assert_blocked(&fixture, "custom:ds", "changed after migration");

        let mut document = original;
        document
            .configs
            .retain(|config| !config.id.contains("custom"));
        store::save(&path, &document).unwrap();
        assert_blocked(&fixture, "custom:ds", "is missing");
    }

    #[test]
    fn blocked_when_fingerprint_differs_or_report_is_v1() {
        let fixture = migrated("fingerprint", &["ds"]);
        let path = migration::report_path(fixture.data_dir());
        let original: MigrationReport =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        let mut tampered = original.clone();
        for entry in &mut tampered.migrated {
            entry.source_fingerprint = Some("0".repeat(64));
        }
        std::fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert_blocked(&fixture, "custom:ds", "fingerprint mismatch");

        let mut v1 = original.clone();
        v1.version = 1;
        for entry in &mut v1.migrated {
            entry.source_fingerprint = None;
        }
        std::fs::write(&path, serde_json::to_vec(&v1).unwrap()).unwrap();
        assert_blocked(&fixture, "custom:ds", "predates fingerprints");

        // A v2 report that still lacks the digest for this entry.
        let mut missing = original;
        for entry in &mut missing.migrated {
            entry.source_fingerprint = None;
        }
        std::fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
        assert_blocked(&fixture, "custom:ds", "predates fingerprints");

        std::fs::remove_file(&path).unwrap();
        assert_blocked(&fixture, "custom:ds", "no migration report");
    }

    #[cfg(unix)]
    #[test]
    fn blocked_when_wrapper_is_symlink_or_user_script() {
        let fixture = migrated("wrapper", &["ds"]);
        let wrapper = fixture.wrapper("ds");
        let real = fixture.data_dir().join("agent-scripts/real.sh");
        std::fs::write(&real, generated_wrapper()).unwrap();
        std::fs::remove_file(&wrapper).unwrap();
        std::os::unix::fs::symlink(&real, &wrapper).unwrap();
        assert_blocked(&fixture, "custom:ds", "not a regular non-symlink file");

        std::fs::remove_file(&wrapper).unwrap();
        std::fs::write(&wrapper, b"#!/bin/sh\nexec claude \"$@\"\n").unwrap();
        assert_blocked(&fixture, "custom:ds", "not an Aeroric-generated wrapper");
    }

    #[test]
    fn partial_failure_resumes() {
        let fixture = migrated("partial", &["ds"]);
        FAIL_AT.with(|fail| fail.set(Some(CleanupStep::QuarantineWrapper)));
        let outcome = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        FAIL_AT.with(|fail| fail.set(None));
        let expected_remaining = vec![
            CleanupStep::QuarantineWrapper,
            CleanupStep::DeleteOldSecrets,
        ];
        match state_of(&outcome.candidates, "custom:ds") {
            CandidateState::Partial { remaining, reason } => {
                assert_eq!(remaining, expected_remaining);
                assert!(reason.unwrap().contains("injected failure"));
            }
            other => panic!("expected partial, got {other:?}"),
        }
        // Completed steps stay done (no rollback); later steps did not run.
        assert_eq!(fixture.settings()["custom_agents"], json!([]));
        assert!(!fixture.sidecar("ds").exists());
        assert!(fixture.wrapper("ds").exists());
        assert_eq!(
            fixture.secrets.peek(OLD_API_KEY).as_deref(),
            Some(SETTINGS_SECRET)
        );
        assert!(matches!(
            fixture.plan_state("custom:ds"),
            CandidateState::Partial { ref remaining, .. } if remaining == &expected_remaining
        ));

        // Unconfirmed partial entries are not resumed.
        let skipped = apply_legacy_cleanup(&fixture.context, &[]).unwrap();
        assert!(matches!(
            state_of(&skipped.candidates, "custom:ds"),
            CandidateState::Partial { .. }
        ));
        assert!(fixture.wrapper("ds").exists());

        let resumed = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        assert_eq!(
            state_of(&resumed.candidates, "custom:ds"),
            CandidateState::Done
        );
        assert!(
            resumed.quarantine_dir.is_none(),
            "resume reuses the first batch"
        );
        let batch = outcome.quarantine_dir.unwrap();
        let entry_dir = batch.join(format!("custom-{}", migration::stable_suffix("ds")));
        assert!(entry_dir.join("wrapper-ds.sh").is_file());
        assert!(entry_dir.join("credential-sidecar").is_file());
        assert!(!fixture.wrapper("ds").exists());
        assert!(fixture.secrets.peek(OLD_API_KEY).is_none());
    }

    #[test]
    fn partial_resume_blocks_when_new_objects_disappear() {
        let fixture = migrated("partial-blocked", &["ds"]);
        FAIL_AT.with(|fail| fail.set(Some(CleanupStep::QuarantineSidecar)));
        apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        FAIL_AT.with(|fail| fail.set(None));
        fixture
            .secrets
            .delete(&SecretKey(fixture.new_account("ds")))
            .unwrap();
        assert_blocked(&fixture, "custom:ds", "is missing");
        assert!(fixture.sidecar("ds").exists());
    }

    #[test]
    fn apply_is_idempotent() {
        let fixture = migrated("idempotent", &["ds"]);
        apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        let after_first = snapshot(&fixture);
        let second = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        assert_eq!(
            state_of(&second.candidates, "custom:ds"),
            CandidateState::Done
        );
        assert!(second.quarantine_dir.is_none());
        assert_eq!(snapshot(&fixture), after_first);
    }

    #[test]
    fn no_plaintext_in_journal_report_or_snapshot() {
        let fixture = migrated("plaintext", &["ds"]);
        // Mid-cleanup journal (with last_error) and the final one.
        FAIL_AT.with(|fail| fail.set(Some(CleanupStep::DeleteOldSecrets)));
        let outcome = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        FAIL_AT.with(|fail| fail.set(None));
        let partial_journal = std::fs::read_to_string(journal_path(fixture.data_dir())).unwrap();
        apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();

        let batch = outcome.quarantine_dir.unwrap();
        let snapshot_raw = std::fs::read_to_string(batch.join(SETTINGS_SNAPSHOT_NAME)).unwrap();
        assert!(snapshot_raw.contains(REDACTED));
        assert!(snapshot_raw.contains(KEYCHAIN_MARKER), "markers are kept");
        let texts = [
            ("partial journal", partial_journal),
            (
                "journal",
                std::fs::read_to_string(journal_path(fixture.data_dir())).unwrap(),
            ),
            (
                "report",
                std::fs::read_to_string(migration::report_path(fixture.data_dir())).unwrap(),
            ),
            ("snapshot", snapshot_raw),
            (
                "outcome",
                serde_json::to_string(&plan_legacy_cleanup(&fixture.context).unwrap()).unwrap(),
            ),
        ];
        for (name, text) in &texts {
            for secret in [SIDECAR_SECRET.trim(), SETTINGS_SECRET, BUILTIN_SECRET] {
                assert!(!text.contains(secret), "{name} leaked `{secret}`");
            }
        }
    }

    #[test]
    fn markers_only_settings_are_snapshotted_byte_for_byte() {
        let fixture = migrated("marker-snapshot", &["ds"]);
        let path = fixture.context.paths.settings_file();
        let mut settings = fixture.settings();
        settings["custom_agents"][0]["api_key"] = json!(KEYCHAIN_MARKER);
        let bytes = serde_json::to_vec_pretty(&Value::Object(settings)).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // The marker is resolved through the old keychain entry, so the fingerprint
        // (which excludes `api_key`) and the sidecar credential still match.
        let outcome = apply_legacy_cleanup(&fixture.context, &confirm(&["custom:ds"])).unwrap();
        assert_eq!(
            state_of(&outcome.candidates, "custom:ds"),
            CandidateState::Done
        );
        assert_eq!(
            std::fs::read(outcome.quarantine_dir.unwrap().join(SETTINGS_SNAPSHOT_NAME)).unwrap(),
            bytes
        );
    }

    #[test]
    fn builtin_credentials_untouched() {
        let fixture = migrated("builtin", &["ds"]);
        let builtin_before = fixture.settings()["builtin_agent_credentials"].clone();
        let outcome = apply_legacy_cleanup(
            &fixture.context,
            &confirm(&["custom:ds", "builtin:claude", "builtin:codex"]),
        )
        .unwrap();
        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(
            fixture.settings()["builtin_agent_credentials"],
            builtin_before
        );
        assert_eq!(
            fixture.secrets.peek(OLD_BUILTIN).as_deref(),
            Some(BUILTIN_SECRET)
        );
        let builtin_new = format!(
            "connection:legacy-connection-builtin-{}:credential",
            migration::stable_suffix("claude")
        );
        assert_eq!(
            fixture.secrets.peek(&builtin_new).as_deref(),
            Some(BUILTIN_SECRET)
        );
    }
}
