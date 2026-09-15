//! Load/save, atomic write, file fingerprint cache, and settings lock helpers.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::storage::atomic_write_private;

use super::*;

pub(super) static CACHED_CLAUDE_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
pub(super) static CACHED_CODEX_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
pub(super) static CACHED_DSH_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
pub(super) static CACHED_SETTINGS: OnceLock<Mutex<Option<CachedSettings>>> = OnceLock::new();
pub(super) static SETTINGS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(super) static AGENT_UPGRADE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub(super) fn clear_cached_versions() {
    *CACHED_CLAUDE_VERSION
        .get_or_init(|| Mutex::new(None))
        .lock() = None;
    *CACHED_CODEX_VERSION.get_or_init(|| Mutex::new(None)).lock() = None;
    *CACHED_DSH_VERSION.get_or_init(|| Mutex::new(None)).lock() = None;
}

pub(super) fn settings_lock() -> &'static Mutex<()> {
    SETTINGS_LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn agent_upgrade_lock() -> &'static tokio::sync::Mutex<()> {
    AGENT_UPGRADE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(super) fn aeroric_dir() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

pub(super) fn agent_scripts_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("agents"))
}

pub(super) fn agent_api_key_path(id: &str) -> Result<PathBuf, String> {
    let id = sanitize_custom_agent_id(id);
    if id.is_empty() {
        return Err("Invalid custom agent id".to_string());
    }
    Ok(aeroric_dir()?.join("agent-credentials").join(id))
}

pub(super) fn remove_agent_api_key_at_path(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(super) fn write_agent_api_key_at_path(path: &Path, api_key: &str) -> Result<(), String> {
    let api_key = api_key.trim();
    // A cleared key must not leave an old wrapper sidecar behind.  The
    // generated scripts already fail closed for a missing/empty file, so
    // removing the file is both safer and less ambiguous than writing an
    // empty placeholder.
    if api_key.is_empty() {
        return remove_agent_api_key_at_path(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    atomic_write_private(path, api_key)
}

pub(super) fn write_agent_api_key(id: &str, api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    write_agent_api_key_at_path(&path, api_key)
}

pub(super) fn sync_agent_credentials_at_path(
    path: &Path,
    expected_api_key: &str,
) -> Result<(), String> {
    // A blank expected key is an explicit clear operation.  Do not use
    // `read_to_string(...).unwrap_or_default()` here: an existing sidecar that
    // cannot be read (for example because of invalid UTF-8 or a transient
    // permission error) must not be mistaken for an already-empty credential.
    if expected_api_key.trim().is_empty() {
        return remove_agent_api_key_at_path(path);
    }

    // Never treat a symlink as an already-synchronized credential file.  A
    // read through a symlink could report a matching key while the secret is
    // actually stored outside Aeroric's private directory.  Rewriting via
    // `write_agent_api_key_at_path` below replaces the link itself (rename
    // does not follow it), leaving a regular private file at the expected
    // location.  Directories and other non-files are left to the write path
    // to reject with a useful error.
    let force_rewrite = fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        .unwrap_or(false);
    if force_rewrite {
        return write_agent_api_key_at_path(path, expected_api_key);
    }

    match fs::read_to_string(path) {
        Ok(current_key) if current_key.trim() == expected_api_key.trim() => Ok(()),
        Ok(_) | Err(_) => write_agent_api_key_at_path(path, expected_api_key),
    }
}

/// Synchronize the agent credentials file with the API key from settings.
/// This ensures the credentials file is always up to date, even when the
/// agent script itself doesn't need regeneration.
pub(super) fn sync_agent_credentials(id: &str, expected_api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    sync_agent_credentials_at_path(&path, expected_api_key)
}

pub(super) fn remove_agent_api_key(id: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    remove_agent_api_key_at_path(&path)
}

#[derive(Debug)]
pub(super) enum AgentFileState {
    Missing,
    RegularFile {
        content: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Symlink(PathBuf),
    Other,
}

#[derive(Debug)]
pub(super) struct AgentFileSnapshot {
    path: PathBuf,
    state: AgentFileState,
}

/// Filesystem changes made while updating a generated Agent profile.
///
/// Settings are persisted after the wrapper/sidecar has been prepared so the
/// generated path can be stored in the profile.  If that final settings write
/// fails, restoring these snapshots keeps the old settings and old launcher
/// mutually consistent instead of leaving a half-applied update behind.
#[derive(Debug)]
pub(super) struct AgentFileTransaction {
    snapshots: Vec<AgentFileSnapshot>,
}

pub(super) struct GeneratedAgentScriptPlan {
    pub(super) current_path: String,
    pub(super) content: String,
    pub(super) target: PathBuf,
}

impl AgentFileTransaction {
    /// Capture a set of files, run a fallible mutation, and restore the
    /// capture when the mutation itself fails.  The caller receives the
    /// transaction on success so the settings write can still roll the files
    /// back if it fails later.
    pub(super) fn capture_and_apply<F>(
        paths: impl IntoIterator<Item = PathBuf>,
        apply: F,
    ) -> Result<Self, String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let transaction = Self::capture(paths)?;
        match apply() {
            Ok(()) => Ok(transaction),
            Err(error) => match transaction.restore() {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(format!("{error}; {rollback_error}")),
            },
        }
    }

    fn capture(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, String> {
        let mut snapshots = Vec::new();
        let mut seen = HashSet::new();
        for path in paths {
            if !seen.insert(path.clone()) {
                continue;
            }
            let state = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    AgentFileState::Symlink(fs::read_link(&path).map_err(|error| {
                        format!("Cannot snapshot Agent file {}: {error}", path.display())
                    })?)
                }
                Ok(metadata) if metadata.is_file() => AgentFileState::RegularFile {
                    content: fs::read(&path).map_err(|error| {
                        format!("Cannot snapshot Agent file {}: {error}", path.display())
                    })?,
                    #[cfg(unix)]
                    mode: {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode()
                    },
                },
                Ok(_) => AgentFileState::Other,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    AgentFileState::Missing
                }
                Err(error) => {
                    return Err(format!(
                        "Cannot inspect Agent file {}: {error}",
                        path.display()
                    ));
                }
            };
            snapshots.push(AgentFileSnapshot { path, state });
        }
        Ok(Self { snapshots })
    }

    fn remove_current(path: &Path) -> Result<(), String> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(path).map_err(|error| error.to_string())
        } else {
            Err(format!(
                "Refusing to replace non-file Agent snapshot target: {}",
                path.display()
            ))
        }
    }

    pub(super) fn restore(self) -> Result<(), String> {
        let mut failures = Vec::new();
        for snapshot in self.snapshots.into_iter().rev() {
            let result = (|| match snapshot.state {
                AgentFileState::Missing => Self::remove_current(&snapshot.path),
                AgentFileState::RegularFile {
                    content,
                    #[cfg(unix)]
                    mode,
                } => {
                    Self::remove_current(&snapshot.path)?;
                    if let Some(parent) = snapshot.path.parent() {
                        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    crate::storage::atomic_write_private_bytes(&snapshot.path, &content)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&snapshot.path, fs::Permissions::from_mode(mode))
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                }
                AgentFileState::Symlink(target) => {
                    Self::remove_current(&snapshot.path)?;
                    if let Some(parent) = snapshot.path.parent() {
                        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&target, &snapshot.path)
                        .map_err(|error| error.to_string())?;
                    #[cfg(windows)]
                    std::os::windows::fs::symlink_file(&target, &snapshot.path)
                        .map_err(|error| error.to_string())?;
                    Ok(())
                }
                AgentFileState::Other => Ok(()),
            })();
            if let Err(error) = result {
                failures.push(format!("{}: {error}", snapshot.path.display()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Failed to roll back Agent files: {}",
                failures.join("; ")
            ))
        }
    }
}

pub(super) fn settings_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("settings.json"))
}

pub(super) fn normalize_settings(settings: AppSettings) -> AppSettings {
    let proxy_settings = migrate_legacy_proxy_settings(&settings);
    let agent_proxy_enabled = migrate_agent_proxy_enabled(&settings);
    AppSettings {
        claude_path: normalize_agent_configured_path("claude", &settings.claude_path),
        claude_gpt55_path: if settings.claude_gpt55_path.is_empty() {
            String::new()
        } else {
            normalize_agent_configured_path("claude_gpt55", &settings.claude_gpt55_path)
        },
        codex_path: normalize_agent_configured_path("codex", &settings.codex_path),
        dsh_path: if settings.dsh_path.is_empty() {
            String::new()
        } else {
            normalize_agent_configured_path("dsh", &settings.dsh_path)
        },
        omp_path: if settings.omp_path.is_empty() {
            String::new()
        } else {
            normalize_agent_configured_path("omp", &settings.omp_path)
        },
        claude_config_path: normalize_config_path(settings.claude_config_path),
        claude_gpt55_config_path: normalize_config_path(settings.claude_gpt55_config_path),
        codex_config_path: normalize_config_path(settings.codex_config_path),
        dsh_config_path: normalize_config_path(settings.dsh_config_path),
        agent_label_overrides: normalize_agent_label_overrides(settings.agent_label_overrides),
        builtin_agent_credentials: normalize_builtin_agent_credentials(
            settings.builtin_agent_credentials,
        ),
        dsh_reasoning_efforts: normalize_dsh_reasoning_efforts(settings.dsh_reasoning_efforts),
        proxy_settings,
        local_router_settings: normalize_local_router_settings(settings.local_router_settings),
        notebook_embedding_settings: normalize_notebook_embedding_settings(
            settings.notebook_embedding_settings,
        ),
        agent_proxy_enabled,
        agent_proxy_overrides: HashMap::new(),
        custom_agents: normalize_custom_agents(settings.custom_agents),
        send_shortcut: normalize_send_shortcut(settings.send_shortcut),
        terminal_shift_enter_newline: settings.terminal_shift_enter_newline,
        dsh_web_search_enabled: settings.dsh_web_search_enabled,
        dsh_telemetry_enabled: settings.dsh_telemetry_enabled,
        auto_cleanup_settings: normalize_auto_cleanup_settings(settings.auto_cleanup_settings),
        weekly_report_settings: normalize_weekly_report_settings(settings.weekly_report_settings),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SettingsFileFingerprint {
    pub(super) path: PathBuf,
    pub(super) modified: Option<SystemTime>,
    pub(super) len: u64,
    pub(super) content_sha256: Option<[u8; 32]>,
}

#[derive(Clone)]
pub(super) struct CachedSettings {
    fingerprint: SettingsFileFingerprint,
    settings: AppSettings,
}

pub(super) fn settings_file_fingerprint(path: &Path) -> SettingsFileFingerprint {
    let metadata = fs::metadata(path).ok();
    let content_sha256 = fs::read(path)
        .ok()
        .map(|content| Sha256::digest(content).into());
    SettingsFileFingerprint {
        path: path.to_path_buf(),
        modified: metadata.as_ref().and_then(|value| value.modified().ok()),
        len: metadata.map(|value| value.len()).unwrap_or_default(),
        content_sha256,
    }
}

pub(super) fn cache_settings(path: &Path, settings: &AppSettings) {
    *CACHED_SETTINGS.get_or_init(|| Mutex::new(None)).lock() = Some(CachedSettings {
        fingerprint: settings_file_fingerprint(path),
        settings: settings.clone(),
    });
}

pub(super) fn get_cached_settings(path: &Path) -> Option<AppSettings> {
    let fingerprint = settings_file_fingerprint(path);
    CACHED_SETTINGS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .as_ref()
        .filter(|cached| cached.fingerprint == fingerprint)
        .map(|cached| cached.settings.clone())
}

pub(super) fn load_settings_unlocked() -> AppSettings {
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };

    if let Some(cached) = get_cached_settings(&path) {
        return cached;
    }

    if !path.exists() {
        let settings = normalize_settings(AppSettings {
            claude_path: String::new(),
            claude_gpt55_path: String::new(),
            codex_path: String::new(),
            dsh_path: String::new(),
            omp_path: String::new(),
            claude_config_path: String::new(),
            claude_gpt55_config_path: String::new(),
            codex_config_path: String::new(),
            dsh_config_path: String::new(),
            agent_label_overrides: HashMap::new(),
            builtin_agent_credentials: HashMap::new(),
            dsh_reasoning_efforts: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            local_router_settings: LocalRouterSettings::default(),
            notebook_embedding_settings: NotebookEmbeddingSettings::default(),
            agent_proxy_enabled: HashMap::new(),
            agent_proxy_overrides: HashMap::new(),
            custom_agents: Vec::new(),
            send_shortcut: default_send_shortcut(),
            terminal_shift_enter_newline: default_shift_enter_newline(),
            dsh_web_search_enabled: true,
            dsh_telemetry_enabled: false,
            auto_cleanup_settings: AutoCleanupSettings::default(),
            weekly_report_settings: WeeklyReportSettings::default(),
        });
        if let Ok(dir) = aeroric_dir() {
            let _ = fs::create_dir_all(&dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&settings) {
            if atomic_write_private(&path, &raw).is_ok() {
                cache_settings(&path, &settings);
            }
        }
        return settings;
    }

    // Older releases wrote settings.json with the platform default mode even
    // though it contains API keys and proxy passwords. Tighten existing files
    // on read so a user who has not changed settings since upgrading is still
    // protected.
    let _ = crate::storage::ensure_private_file_permissions(&path);
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return AppSettings::default(),
    };
    let settings: AppSettings = serde_json::from_str(&raw).unwrap_or_default();
    let mut normalized = normalize_settings(settings.clone());
    recover_custom_agent_settings(&mut normalized);
    refresh_stale_codex_agent_scripts(&mut normalized);
    refresh_stale_claude_agent_scripts(&mut normalized);
    if normalized != settings {
        if let Ok(raw) = serde_json::to_string_pretty(&normalized) {
            let _ = atomic_write_private(&path, &raw);
        }
    }
    cache_settings(&path, &normalized);
    normalized
}

pub(crate) fn load_settings_internal() -> AppSettings {
    let _guard = settings_lock().lock();
    load_settings_unlocked()
}

pub(super) fn persist_settings_unlocked(settings: AppSettings) -> Result<AppSettings, String> {
    let dir = aeroric_dir()?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = settings_path()?;
    let normalized = normalize_settings(settings);
    let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
    atomic_write_private(&path, &raw)?;
    cache_settings(&path, &normalized);
    Ok(normalized)
}

pub(super) fn update_settings_locked<F>(update: F) -> Result<AppSettings, String>
where
    F: FnOnce(&mut AppSettings) -> Result<(), String>,
{
    let normalized = {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        update(&mut settings)?;
        persist_settings_unlocked(settings)?
    };
    clear_cached_versions();
    Ok(normalized)
}

pub(super) fn update_settings_locked_with_agent_files<F>(update: F) -> Result<AppSettings, String>
where
    F: FnOnce(&mut AppSettings) -> Result<AgentFileTransaction, String>,
{
    let normalized = {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let transaction = update(&mut settings)?;
        match persist_settings_unlocked(settings) {
            Ok(normalized) => {
                // The new settings and the generated files are now durable;
                // the snapshots are no longer needed.
                drop(transaction);
                normalized
            }
            Err(error) => {
                let rollback = transaction.restore();
                return match rollback {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!("{error}; {rollback_error}")),
                };
            }
        }
    };
    clear_cached_versions();
    Ok(normalized)
}

pub(crate) fn save_managed_agent_path(agent: &str, path: &Path) -> Result<(), String> {
    {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let value = path.to_string_lossy().into_owned();
        match agent {
            "claude" => settings.claude_path = value,
            "codex" => settings.codex_path = value,
            _ => return Err(format!("Unknown managed agent: {agent}")),
        }
        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
        atomic_write_private(&settings_path()?, &raw)?;
    }
    clear_cached_versions();
    Ok(())
}
